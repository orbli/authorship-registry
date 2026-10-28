export type DbDialect = 'pglite' | 'postgres' | 'dsql';
export type MailDriver = 'console' | 'ses' | 'smtp';

export interface Config {
  dbDialect: DbDialect;
  /** postgres:// URL, used when dbDialect === 'postgres' */
  databaseUrl?: string;
  /** <cluster-id>.dsql.<region>.on.aws, used when dbDialect === 'dsql' */
  dsqlEndpoint?: string;
  awsRegion: string;
  mailDriver: MailDriver;
  senderEmail: string;
  /** SMTP settings, used when mailDriver === 'smtp'. Password comes from SMTP_PASSWORD, or is
   *  fetched once from the SSM SecureString parameter named by SMTP_PASSWORD_PARAM. */
  smtp: { host: string; port: number; user: string; password?: string; passwordParam?: string };
  /** Public origin used to build links in emails, e.g. https://registry.example.org.
   *  Empty means "derive from the incoming request", which is what a Lambda Function URL needs. */
  baseUrl: string;
  /** Seeded as admin on migration if no such user exists */
  adminEmail?: string;
  /** "Sign in with Google" for staff; off when unset. Secret from GOOGLE_CLIENT_SECRET or the
   *  SSM SecureString named by GOOGLE_CLIENT_SECRET_PARAM. */
  google?: { clientId: string; clientSecret?: string; clientSecretParam?: string };
  sessionTtlHours: number;
  tokenTtlMinutes: number;
  port: number;
  migrateOnStart: boolean;
}

export function loadConfig(env: NodeJS.ProcessEnv = process.env): Config {
  const dialect = (env.DB_DIALECT ?? (env.DSQL_ENDPOINT ? 'dsql' : env.DATABASE_URL ? 'postgres' : 'pglite')) as DbDialect;
  if (!['pglite', 'postgres', 'dsql'].includes(dialect)) throw new Error(`Unknown DB_DIALECT ${dialect}`);
  if (dialect === 'postgres' && !env.DATABASE_URL) throw new Error('DATABASE_URL required for DB_DIALECT=postgres');
  if (dialect === 'dsql' && !env.DSQL_ENDPOINT) throw new Error('DSQL_ENDPOINT required for DB_DIALECT=dsql');
  const mailDriver = (env.MAIL_DRIVER ?? (env.SMTP_USER ?? env.SMTP_USERNAME ? 'smtp' : env.SENDER_EMAIL ? 'ses' : 'console')) as MailDriver;
  if (!['console', 'ses', 'smtp'].includes(mailDriver)) throw new Error(`Unknown MAIL_DRIVER ${mailDriver}`);
  if (mailDriver === 'smtp' && !(env.SMTP_USER ?? env.SMTP_USERNAME)) throw new Error('SMTP_USER required for MAIL_DRIVER=smtp');
  if (mailDriver === 'smtp' && !env.SMTP_PASSWORD && !env.SMTP_PASSWORD_PARAM) throw new Error('SMTP_PASSWORD or SMTP_PASSWORD_PARAM required for MAIL_DRIVER=smtp');
  if (env.GOOGLE_CLIENT_ID && !env.GOOGLE_CLIENT_SECRET && !env.GOOGLE_CLIENT_SECRET_PARAM) throw new Error('GOOGLE_CLIENT_SECRET or GOOGLE_CLIENT_SECRET_PARAM required with GOOGLE_CLIENT_ID');
  return {
    dbDialect: dialect,
    databaseUrl: env.DATABASE_URL,
    dsqlEndpoint: env.DSQL_ENDPOINT,
    awsRegion: env.AWS_REGION ?? env.AWS_DEFAULT_REGION ?? 'us-east-1',
    mailDriver,
    senderEmail: env.SENDER_EMAIL ?? env.SMTP_USER ?? env.SMTP_USERNAME ?? 'registry@localhost',
    smtp: {
      host: env.SMTP_HOST ?? 'smtp.gmail.com',
      port: Number(env.SMTP_PORT ?? 465),
      user: env.SMTP_USER ?? env.SMTP_USERNAME ?? '',
      password: env.SMTP_PASSWORD,
      passwordParam: env.SMTP_PASSWORD_PARAM,
    },
    baseUrl: (env.BASE_URL ?? (env.AWS_LAMBDA_FUNCTION_NAME ? '' : `http://localhost:${env.PORT ?? 3000}`)).replace(/\/$/, ''),
    adminEmail: env.ADMIN_EMAIL?.toLowerCase(),
    google: env.GOOGLE_CLIENT_ID ? { clientId: env.GOOGLE_CLIENT_ID, clientSecret: env.GOOGLE_CLIENT_SECRET, clientSecretParam: env.GOOGLE_CLIENT_SECRET_PARAM } : undefined,
    sessionTtlHours: Number(env.SESSION_TTL_HOURS ?? 12),
    tokenTtlMinutes: Number(env.TOKEN_TTL_MINUTES ?? 30),
    port: Number(env.PORT ?? 3000),
    migrateOnStart: (env.MIGRATE_ON_START ?? 'true') !== 'false',
  };
}
