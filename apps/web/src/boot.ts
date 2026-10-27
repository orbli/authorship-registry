import { createApp } from './app.js';
import { loadConfig } from './config.js';
import { openDb } from './db/index.js';
import { migrate } from './db/schema.js';
import { createMailer } from './mail.js';

/** Shared start-up for every entry point: config → db → migrate → app. */
export async function boot() {
  const cfg = loadConfig();
  const db = await openDb(cfg);
  if (cfg.migrateOnStart) {
    const ran = await migrate(db, { adminEmail: cfg.adminEmail });
    if (ran.length) console.log(`migrations applied: ${ran.join(', ')}`);
  }
  const app = createApp({ db, mailer: createMailer(cfg), cfg });
  return { cfg, db, app };
}
