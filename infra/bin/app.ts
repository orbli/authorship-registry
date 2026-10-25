import * as cdk from 'aws-cdk-lib';
import { RegistryStack } from '../lib/stack.js';

const app = new cdk.App();
new RegistryStack(app, 'AuthorshipRegistry', {
  env: { account: process.env.CDK_DEFAULT_ACCOUNT, region: process.env.CDK_DEFAULT_REGION ?? 'us-east-1' },
  // Context values: pass with `cdk deploy -c senderEmail=... -c adminEmail=...` or put them in cdk.context.json
  mailDriver: app.node.tryGetContext('mailDriver') ?? 'smtp',
  senderEmail: app.node.tryGetContext('senderEmail') ?? 'registry@example.org',
  smtpUser: app.node.tryGetContext('smtpUser'),
  smtpHost: app.node.tryGetContext('smtpHost'),
  adminEmail: app.node.tryGetContext('adminEmail'),
  baseUrl: app.node.tryGetContext('baseUrl'),
  description: 'Authorship Registry: Lambda + Aurora DSQL + SES, scale-to-zero',
});
