import { loadConfig } from '../config.js';
import { openDb } from '../db/index.js';
import { migrate } from '../db/schema.js';

const cfg = loadConfig();
const db = await openDb(cfg);
const ran = await migrate(db, { adminEmail: cfg.adminEmail });
console.log(ran.length ? `applied: ${ran.join(', ')}` : 'nothing to apply');
await db.close();
