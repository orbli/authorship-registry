//! Runtime configuration from environment variables. Same names and defaults as
//! the original Node version so the Kubernetes and CDK manifests keep working.

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DbDialect {
    Postgres,
    Dsql,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MailDriver {
    Console,
    Ses,
    Smtp,
}

#[derive(Clone, Debug)]
pub struct Smtp {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Option<String>,
    /// SSM SecureString parameter holding the password, read once at first send.
    pub password_param: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Google {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub client_secret_param: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub db_dialect: DbDialect,
    /// postgres:// URL, used when db_dialect == Postgres
    pub database_url: Option<String>,
    /// <cluster-id>.dsql.<region>.on.aws, used when db_dialect == Dsql
    pub dsql_endpoint: Option<String>,
    pub aws_region: String,
    pub mail_driver: MailDriver,
    pub sender_email: String,
    pub smtp: Smtp,
    /// Public origin for links in emails. Empty = derive from the incoming request.
    pub base_url: String,
    /// Seeded as admin on migration if no such user exists
    pub admin_email: Option<String>,
    /// "Sign in with Google" for staff; None = button hidden.
    pub google: Option<Google>,
    pub session_ttl_hours: i64,
    pub token_ttl_minutes: i64,
    pub port: u16,
    pub migrate_on_start: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("config: {0}")]
pub struct ConfigError(String);

impl Config {
    pub fn from_env() -> Result<Config, ConfigError> {
        let env: HashMap<String, String> = std::env::vars().collect();
        Config::from_map(&env)
    }

    pub fn from_map(env: &HashMap<String, String>) -> Result<Config, ConfigError> {
        let get = |k: &str| env.get(k).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let err = |m: &str| ConfigError(m.to_string());

        let db_dialect = match get("DB_DIALECT").as_deref() {
            Some("postgres") => DbDialect::Postgres,
            Some("dsql") => DbDialect::Dsql,
            Some(other) => return Err(ConfigError(format!("Unknown DB_DIALECT {other}"))),
            None if get("DSQL_ENDPOINT").is_some() => DbDialect::Dsql,
            None => DbDialect::Postgres,
        };
        if db_dialect == DbDialect::Postgres && get("DATABASE_URL").is_none() {
            return Err(err("DATABASE_URL required for DB_DIALECT=postgres"));
        }
        if db_dialect == DbDialect::Dsql && get("DSQL_ENDPOINT").is_none() {
            return Err(err("DSQL_ENDPOINT required for DB_DIALECT=dsql"));
        }

        let smtp_user = get("SMTP_USER").or_else(|| get("SMTP_USERNAME"));
        let mail_driver = match get("MAIL_DRIVER").as_deref() {
            Some("console") => MailDriver::Console,
            Some("ses") => MailDriver::Ses,
            Some("smtp") => MailDriver::Smtp,
            Some(other) => return Err(ConfigError(format!("Unknown MAIL_DRIVER {other}"))),
            None if smtp_user.is_some() => MailDriver::Smtp,
            None if get("SENDER_EMAIL").is_some() => MailDriver::Ses,
            None => MailDriver::Console,
        };
        if mail_driver == MailDriver::Smtp && smtp_user.is_none() {
            return Err(err("SMTP_USER required for MAIL_DRIVER=smtp"));
        }
        if mail_driver == MailDriver::Smtp && get("SMTP_PASSWORD").is_none() && get("SMTP_PASSWORD_PARAM").is_none() {
            return Err(err("SMTP_PASSWORD or SMTP_PASSWORD_PARAM required for MAIL_DRIVER=smtp"));
        }
        if get("GOOGLE_CLIENT_ID").is_some() && get("GOOGLE_CLIENT_SECRET").is_none() && get("GOOGLE_CLIENT_SECRET_PARAM").is_none() {
            return Err(err("GOOGLE_CLIENT_SECRET or GOOGLE_CLIENT_SECRET_PARAM required with GOOGLE_CLIENT_ID"));
        }

        let port: u16 = get("PORT").unwrap_or_else(|| "3000".into()).parse().map_err(|_| err("PORT must be a number"))?;
        let num = |k: &str, d: i64| -> Result<i64, ConfigError> { get(k).map(|v| v.parse::<i64>().map_err(|_| ConfigError(format!("{k} must be a number")))).unwrap_or(Ok(d)) };

        Ok(Config {
            db_dialect,
            database_url: get("DATABASE_URL"),
            dsql_endpoint: get("DSQL_ENDPOINT"),
            aws_region: get("AWS_REGION").or_else(|| get("AWS_DEFAULT_REGION")).unwrap_or_else(|| "eu-north-1".into()),
            mail_driver,
            sender_email: get("SENDER_EMAIL").or_else(|| smtp_user.clone()).unwrap_or_else(|| "registry@localhost".into()),
            smtp: Smtp {
                host: get("SMTP_HOST").unwrap_or_else(|| "smtp.gmail.com".into()),
                port: get("SMTP_PORT").unwrap_or_else(|| "465".into()).parse().map_err(|_| err("SMTP_PORT must be a number"))?,
                user: smtp_user.unwrap_or_default(),
                password: get("SMTP_PASSWORD"),
                password_param: get("SMTP_PASSWORD_PARAM"),
            },
            base_url: get("BASE_URL").unwrap_or_else(|| if get("AWS_LAMBDA_FUNCTION_NAME").is_some() { String::new() } else { format!("http://localhost:{port}") }).trim_end_matches('/').to_string(),
            admin_email: get("ADMIN_EMAIL").map(|s| s.to_lowercase()),
            google: get("GOOGLE_CLIENT_ID").map(|client_id| Google { client_id, client_secret: get("GOOGLE_CLIENT_SECRET"), client_secret_param: get("GOOGLE_CLIENT_SECRET_PARAM") }),
            session_ttl_hours: num("SESSION_TTL_HOURS", 12)?,
            token_ttl_minutes: num("TOKEN_TTL_MINUTES", 30)?,
            port,
            migrate_on_start: get("MIGRATE_ON_START").as_deref() != Some("false"),
        })
    }
}
