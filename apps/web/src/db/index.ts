import type { Config, DbDialect } from '../config.js';

/**
 * Minimal database abstraction. Every backend speaks PostgreSQL SQL with $1-style
 * parameters. We deliberately stay inside the Aurora DSQL subset everywhere:
 * UUID keys generated in the app, no foreign keys, no triggers, no extensions,
 * no sequences, one statement per query.
 */
export interface Db {
  readonly dialect: DbDialect;
  query<T = Record<string, unknown>>(sql: string, params?: unknown[]): Promise<T[]>;
  close(): Promise<void>;
}

export async function openDb(cfg: Config): Promise<Db> {
  switch (cfg.dbDialect) {
    case 'pglite':
      return openPglite();
    case 'postgres':
      return openPg({ dialect: 'postgres', connectionString: cfg.databaseUrl! });
    case 'dsql':
      return openDsql(cfg.dsqlEndpoint!, cfg.awsRegion);
  }
}

async function openPglite(): Promise<Db> {
  // Dev/test only: real Postgres compiled to WASM, in-memory, no server needed.
  const { PGlite } = await import('@electric-sql/pglite');
  const pg = new PGlite();
  await pg.waitReady;
  return {
    dialect: 'pglite',
    async query<T>(sql: string, params: unknown[] = []) {
      const r = await pg.query<T>(sql, params as any[]);
      return r.rows;
    },
    async close() {
      await pg.close();
    },
  };
}

async function openPg(opts: { dialect: DbDialect; connectionString?: string; host?: string; user?: string; database?: string; ssl?: object; password?: () => Promise<string> }): Promise<Db> {
  const { default: pg } = await import('pg');
  const { dialect, ...poolOpts } = opts;
  const pool = new pg.Pool({ ...poolOpts, max: 4, idleTimeoutMillis: 30_000 } as any);
  return {
    dialect,
    async query<T>(sql: string, params: unknown[] = []) {
      const r = await pool.query(sql, params as any[]);
      return r.rows as T[];
    },
    async close() {
      await pool.end();
    },
  };
}

async function openDsql(endpoint: string, region: string): Promise<Db> {
  // DSQL authenticates with a short-lived IAM token used as the password.
  // pg accepts a password *function*, so each new pool connection mints a fresh token.
  const { DsqlSigner } = await import('@aws-sdk/dsql-signer');
  const signer = new DsqlSigner({ hostname: endpoint, region });
  return openPg({
    dialect: 'dsql',
    host: endpoint,
    user: 'admin',
    database: 'postgres',
    ssl: { rejectUnauthorized: true },
    password: () => signer.getDbConnectAdminAuthToken(),
  });
}

/** Convenience: first row or undefined. */
export async function one<T>(db: Db, sql: string, params?: unknown[]): Promise<T | undefined> {
  const rows = await db.query<T>(sql, params);
  return rows[0];
}
