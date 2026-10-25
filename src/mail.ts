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
  return cfg.mailDriver === 'ses' ? sesMailer(cfg) : consoleMailer();
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
