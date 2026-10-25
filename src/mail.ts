import type { Config } from './config.js';

export interface Mail {
  to: string;
  subject: string;
  text: string;
}

export interface Mailer {
  send(mail: Mail): Promise<void>;
  /** For tests: the console mailer records what it would have sent. */
  readonly sent?: Mail[];
}

export function createMailer(cfg: Config): Mailer {
  switch (cfg.mailDriver) {
    case 'ses':
      return sesMailer(cfg);
    case 'smtp':
      return smtpMailer(cfg);
    default:
      return consoleMailer();
  }
}

export function consoleMailer(): Mailer {
  const sent: Mail[] = [];
  return {
    sent,
    async send(mail) {
      sent.push(mail);
      // eslint-disable-next-line no-console
      console.log(`\n--- mail to ${mail.to} ---\n${mail.subject}\n\n${mail.text}\n---\n`);
    },
  };
}

function sesMailer(cfg: Config): Mailer {
  let clientPromise: Promise<any> | undefined;
  const client = () =>
    (clientPromise ??= import('@aws-sdk/client-sesv2').then((m) => ({ m, c: new m.SESv2Client({ region: cfg.awsRegion }) })));
  return {
    async send(mail) {
      const { m, c } = await client();
      await c.send(
        new m.SendEmailCommand({
          FromEmailAddress: cfg.senderEmail,
          Destination: { ToAddresses: [mail.to] },
          Content: {
            Simple: {
              Subject: { Data: mail.subject, Charset: 'UTF-8' },
              Body: { Text: { Data: mail.text, Charset: 'UTF-8' } },
            },
          },
        }),
      );
    },
  };
}

/**
 * Any SMTP relay, e.g. Google Workspace with an app password (smtp.gmail.com:465).
 * The password is read lazily: from env, or once from an SSM SecureString so it
 * never has to live in a Lambda environment variable or a template.
 */
function smtpMailer(cfg: Config): Mailer {
  let transportPromise: Promise<any> | undefined;
  const transport = () =>
    (transportPromise ??= (async () => {
      let pass = cfg.smtp.password;
      if (!pass && cfg.smtp.passwordParam) {
        const { SSMClient, GetParameterCommand } = await import('@aws-sdk/client-ssm');
        const r = await new SSMClient({ region: cfg.awsRegion }).send(new GetParameterCommand({ Name: cfg.smtp.passwordParam, WithDecryption: true }));
        pass = r.Parameter?.Value;
      }
      if (!pass) throw new Error('SMTP password unavailable');
      const { default: nodemailer } = await import('nodemailer');
      return nodemailer.createTransport({ host: cfg.smtp.host, port: cfg.smtp.port, secure: cfg.smtp.port === 465, auth: { user: cfg.smtp.user, pass } });
    })());
  return {
    async send(mail) {
      const t = await transport();
      await t.sendMail({ from: cfg.senderEmail, to: mail.to, subject: mail.subject, text: mail.text });
    },
  };
}
