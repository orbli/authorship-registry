import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomBytes } from 'node:crypto';
import * as cdk from 'aws-cdk-lib';
import * as acm from 'aws-cdk-lib/aws-certificatemanager';
import * as cloudfront from 'aws-cdk-lib/aws-cloudfront';
import * as origins from 'aws-cdk-lib/aws-cloudfront-origins';
import * as dsql from 'aws-cdk-lib/aws-dsql';
import * as iam from 'aws-cdk-lib/aws-iam';
import * as lambda from 'aws-cdk-lib/aws-lambda';
import { NodejsFunction, OutputFormat } from 'aws-cdk-lib/aws-lambda-nodejs';
import * as logs from 'aws-cdk-lib/aws-logs';
import * as ses from 'aws-cdk-lib/aws-ses';
import type { Construct } from 'constructs';

const here = path.dirname(fileURLToPath(import.meta.url));
const appRoot = path.resolve(here, '..', '..');

export interface RegistryStackProps extends cdk.StackProps {
  /** 'smtp' (default): any SMTP relay such as Google Workspace with an app password stored in
   *  SSM SecureString `/authorship-registry/smtp-password`. 'ses': Amazon SES with a verified identity. */
  mailDriver: 'smtp' | 'ses';
  /** From address. For SES it becomes a verified identity (one-time click in the verification mail). */
  senderEmail: string;
  /** SMTP login, defaults to senderEmail. */
  smtpUser?: string;
  /** SMTP host, default smtp.gmail.com (port 465). */
  smtpHost?: string;
  /** Seeded as the first admin so someone can sign in. */
  adminEmail?: string;
  /** Public origin for links in emails. Omit to use the custom domain, else the Function URL origin. */
  baseUrl?: string;
  /** Custom hostname, e.g. ada.orbb.li. Adds CloudFront in front of the Function URL; needs certificateArn. */
  domainName?: string;
  /** ACM certificate ARN for domainName. Must live in us-east-1 (CloudFront requirement). */
  certificateArn?: string;
}

export class RegistryStack extends cdk.Stack {
  constructor(scope: Construct, id: string, props: RegistryStackProps) {
    super(scope, id, props);

    // --- Database: Aurora DSQL, serverless, free tier 100k DPU + 1 GB/month, true zero when idle.
    const cluster = new dsql.CfnCluster(this, 'Db', {
      deletionProtectionEnabled: true,
      tags: [{ key: 'app', value: 'authorship-registry' }],
    });
    cluster.applyRemovalPolicy(cdk.RemovalPolicy.RETAIN);
    const dsqlEndpoint = `${cluster.attrIdentifier}.dsql.${this.region}.on.aws`;

    // --- Outbound mail: SES identity, or an SMTP relay whose password lives in SSM.
    const smtpPasswordParam = '/authorship-registry/smtp-password';
    const sender = props.mailDriver === 'ses' ? new ses.EmailIdentity(this, 'Sender', { identity: ses.Identity.email(props.senderEmail) }) : undefined;
    const mailEnv: Record<string, string> =
      props.mailDriver === 'ses'
        ? { MAIL_DRIVER: 'ses', SENDER_EMAIL: props.senderEmail }
        : {
            MAIL_DRIVER: 'smtp',
            SENDER_EMAIL: props.senderEmail,
            SMTP_USER: props.smtpUser ?? props.senderEmail,
            SMTP_HOST: props.smtpHost ?? 'smtp.gmail.com',
            SMTP_PORT: '465',
            SMTP_PASSWORD_PARAM: smtpPasswordParam,
          };

    // --- Custom domain (optional): CloudFront terminates TLS for the hostname and relays to the
    // Function URL. Function URLs cannot carry a custom hostname themselves, and Cloudflare's
    // free plan cannot rewrite the Host header, so CloudFront is the cheapest correct front door
    // (free tier: 1 TB + 10M requests/month). A per-deploy shared secret header lets the function
    // refuse requests that bypass CloudFront and hit the raw Function URL.
    if (props.domainName && !props.certificateArn) throw new Error('domainName requires certificateArn (ACM, us-east-1)');
    const originSecret = props.domainName ? randomBytes(24).toString('hex') : undefined;
    const publicUrl = props.baseUrl ?? (props.domainName ? `https://${props.domainName}` : undefined);

    // --- Web app: one Node.js function, zip-bundled with esbuild (no Docker), arm64 for price.
    const fn = new NodejsFunction(this, 'Web', {
      entry: path.join(appRoot, 'apps', 'web', 'src', 'lambda.ts'),
      projectRoot: appRoot,
      depsLockFilePath: path.join(appRoot, 'package-lock.json'),
      runtime: lambda.Runtime.NODEJS_22_X,
      architecture: lambda.Architecture.ARM_64,
      memorySize: 512,
      timeout: cdk.Duration.seconds(20),
      logGroup: new logs.LogGroup(this, 'WebLogs', { retention: logs.RetentionDays.THREE_MONTHS, removalPolicy: cdk.RemovalPolicy.DESTROY }),
      bundling: {
        format: OutputFormat.ESM,
        target: 'node22',
        tsconfig: path.join(appRoot, 'tsconfig.json'),
        externalModules: ['@electric-sql/pglite'],
        // pg and the SDK use require() internally; give ESM output a require shim.
        banner: "import { createRequire } from 'module'; const require = createRequire(import.meta.url);",
        sourceMap: true,
        minify: false,
      },
      environment: {
        NODE_OPTIONS: '--enable-source-maps',
        DB_DIALECT: 'dsql',
        DSQL_ENDPOINT: dsqlEndpoint,
        ...mailEnv,
        ...(props.adminEmail ? { ADMIN_EMAIL: props.adminEmail } : {}),
        ...(publicUrl ? { BASE_URL: publicUrl } : {}),
        ...(originSecret ? { ORIGIN_SECRET: originSecret } : {}),
      },
    });

    fn.addToRolePolicy(new iam.PolicyStatement({ actions: ['dsql:DbConnectAdmin'], resources: [cluster.attrResourceArn] }));
    if (sender) sender.grantSendEmail(fn);
    if (props.mailDriver === 'smtp') {
      fn.addToRolePolicy(new iam.PolicyStatement({ actions: ['ssm:GetParameter'], resources: [this.formatArn({ service: 'ssm', resource: 'parameter', resourceName: smtpPasswordParam.replace(/^\//, '') })] }));
      // SecureString parameters are encrypted with the AWS-managed SSM key.
      fn.addToRolePolicy(new iam.PolicyStatement({ actions: ['kms:Decrypt'], resources: ['*'], conditions: { StringEquals: { 'kms:ViaService': `ssm.${this.region}.amazonaws.com` } } }));
    }

    // Public HTTPS endpoint. This is a public web form by design; put CloudFront + WAF in front
    // before real traffic if rate limiting is needed.
    const url = fn.addFunctionUrl({ authType: lambda.FunctionUrlAuthType.NONE });

    let distribution: cloudfront.Distribution | undefined;
    if (props.domainName && props.certificateArn && originSecret) {
      distribution = new cloudfront.Distribution(this, 'Cdn', {
        comment: `authorship-registry ${props.domainName}`,
        domainNames: [props.domainName],
        certificate: acm.Certificate.fromCertificateArn(this, 'Cert', props.certificateArn),
        minimumProtocolVersion: cloudfront.SecurityPolicyProtocol.TLS_V1_2_2021,
        httpVersion: cloudfront.HttpVersion.HTTP2_AND_3,
        priceClass: cloudfront.PriceClass.PRICE_CLASS_ALL, // users are in Hong Kong
        defaultBehavior: {
          origin: new origins.FunctionUrlOrigin(url, { customHeaders: { 'x-origin-verify': originSecret } }),
          allowedMethods: cloudfront.AllowedMethods.ALLOW_ALL,
          cachePolicy: cloudfront.CachePolicy.CACHING_DISABLED,
          originRequestPolicy: cloudfront.OriginRequestPolicy.ALL_VIEWER_EXCEPT_HOST_HEADER,
          viewerProtocolPolicy: cloudfront.ViewerProtocolPolicy.REDIRECT_TO_HTTPS,
        },
      });
    }

    new cdk.CfnOutput(this, 'Url', { value: publicUrl ?? url.url });
    new cdk.CfnOutput(this, 'FunctionUrl', { value: url.url });
    if (distribution) new cdk.CfnOutput(this, 'CdnDomain', { value: distribution.distributionDomainName, description: `CNAME target for ${props.domainName}` });
    new cdk.CfnOutput(this, 'DsqlEndpoint', { value: dsqlEndpoint });
    new cdk.CfnOutput(this, 'DsqlClusterArn', { value: cluster.attrResourceArn });
    new cdk.CfnOutput(this, 'Sender', { value: `${props.mailDriver}:${props.senderEmail}` });
    if (props.mailDriver === 'smtp') new cdk.CfnOutput(this, 'SmtpPasswordParameter', { value: smtpPasswordParam });
  }
}
