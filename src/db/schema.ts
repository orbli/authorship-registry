import { randomUUID } from 'node:crypto';
import type { Db } from './index.js';

/**
 * Migrations are plain SQL strings, applied once each, one statement per query.
 * DSQL requires indexes to be created with CREATE INDEX ASYNC; vanilla Postgres
 * does not know that keyword, so index statements go through idx().
 */
type Migration = { id: string; statements: (dialect: Db['dialect']) => string[] };

const idx = (d: Db['dialect'], name: string, table: string, cols: string, unique = false) =>
  `CREATE ${unique ? 'UNIQUE ' : ''}INDEX ${d === 'dsql' ? 'ASYNC ' : ''}${name} ON ${table} (${cols})`;

const migrations: Migration[] = [
  {
    id: '0001_init',
    statements: (d) => [
      `CREATE TABLE users (
         id uuid PRIMARY KEY,
         email text NOT NULL,
         name text NOT NULL,
         role text NOT NULL,
         disabled boolean NOT NULL DEFAULT false,
         created_at timestamptz NOT NULL)`,
      idx(d, 'users_email_idx', 'users', 'email', true),
      `CREATE TABLE tokens (
         id uuid PRIMARY KEY,
         token_hash text NOT NULL,
         purpose text NOT NULL,
         email text NOT NULL,
         report_id uuid,
         expires_at timestamptz NOT NULL,
         used_at timestamptz)`,
      idx(d, 'tokens_hash_idx', 'tokens', 'token_hash', true),
      `CREATE TABLE sessions (
         id uuid PRIMARY KEY,
         token_hash text NOT NULL,
         user_id uuid NOT NULL,
         expires_at timestamptz NOT NULL,
         created_at timestamptz NOT NULL)`,
      idx(d, 'sessions_hash_idx', 'sessions', 'token_hash', true),
      `CREATE TABLE reports (
         id uuid PRIMARY KEY,
         status text NOT NULL,
         reporter_name text NOT NULL,
         reporter_email text NOT NULL,
         reporter_affiliation text NOT NULL,
         subject_name text NOT NULL,
         subject_name_key text NOT NULL,
         subject_affiliation text,
         subject_email text,
         paper_title text NOT NULL,
         paper_doi text,
         paper_venue text,
         description text NOT NULL,
         summary text,
         decision_note text,
         decided_by uuid,
         created_at timestamptz NOT NULL,
         verified_at timestamptz,
         decided_at timestamptz)`,
      idx(d, 'reports_status_idx', 'reports', 'status'),
      idx(d, 'reports_name_key_idx', 'reports', 'subject_name_key'),
      idx(d, 'reports_doi_idx', 'reports', 'paper_doi'),
      `CREATE TABLE audit_log (
         id uuid PRIMARY KEY,
         at timestamptz NOT NULL,
         actor_user_id uuid,
         actor_email text,
         action text NOT NULL,
         report_id uuid,
         detail text)`,
      idx(d, 'audit_log_at_idx', 'audit_log', 'at'),
    ],
  },
];

export async function migrate(db: Db, opts: { adminEmail?: string } = {}): Promise<string[]> {
  await db.query(`CREATE TABLE IF NOT EXISTS schema_migrations (id text PRIMARY KEY, applied_at timestamptz NOT NULL)`);
  const applied = new Set((await db.query<{ id: string }>(`SELECT id FROM schema_migrations`)).map((r) => r.id));
  const ran: string[] = [];
  for (const m of migrations) {
    if (applied.has(m.id)) continue;
    for (const s of m.statements(db.dialect)) await db.query(s);
    await db.query(`INSERT INTO schema_migrations (id, applied_at) VALUES ($1, $2)`, [m.id, new Date()]);
    ran.push(m.id);
  }
  if (opts.adminEmail) {
    const existing = await db.query(`SELECT id FROM users WHERE email = $1`, [opts.adminEmail]);
    if (existing.length === 0) {
      await db.query(`INSERT INTO users (id, email, name, role, disabled, created_at) VALUES ($1, $2, $3, 'admin', false, $4)`, [
        randomUUID(), opts.adminEmail, 'Administrator', new Date(),
      ]);
      ran.push(`seed_admin:${opts.adminEmail}`);
    }
  }
  return ran;
}
