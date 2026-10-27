import { loadConfig } from '../config.js';
import { createMailer } from '../mail.js';

// Usage: node --env-file=.env --import tsx src/cli/mail-test.ts someone@example.org
const to = process.argv[2];
if (!to) throw new Error('recipient required');
const cfg = loadConfig();
console.log(`driver=${cfg.mailDriver} host=${cfg.smtp.host}:${cfg.smtp.port} user=${cfg.smtp.user} from=${cfg.senderEmail} → ${to}`);
await createMailer(cfg).send({
  to,
  subject: 'Authorship Registry: SMTP test',
  text: `This is a test message from the Authorship Registry mailer.\n\nSent ${new Date().toISOString()} via ${cfg.smtp.host}.\n`,
});
console.log('sent');
