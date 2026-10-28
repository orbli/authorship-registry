//! Minimal database layer. Both backends speak PostgreSQL with $1-style
//! parameters, and every statement stays inside the Aurora DSQL subset: UUID
//! keys generated in the app, no foreign keys, no triggers, no extensions, no
//! sequences, one statement per query.
//!
//! DSQL authenticates with a short-lived IAM token used as the password, so the
//! pool mints a fresh token for every new connection instead of holding one.

use std::sync::Arc;

use bb8::{ManageConnection, Pool};
use tokio_postgres::types::ToSql;
use tokio_postgres::{Client, NoTls, Row};

use crate::config::{Config, DbDialect};

#[derive(Clone)]
pub struct Db {
    pub dialect: DbDialect,
    pool: Pool<Manager>,
}

pub type Params<'a> = &'a [&'a (dyn ToSql + Sync)];

impl Db {
    pub async fn open(cfg: &Config) -> anyhow::Result<Db> {
        let manager = match cfg.db_dialect {
            DbDialect::Postgres => Manager::Url(cfg.database_url.clone().expect("DATABASE_URL")),
            DbDialect::Dsql => {
                let endpoint = cfg.dsql_endpoint.clone().expect("DSQL_ENDPOINT");
                let sdk = aws_config::defaults(aws_config::BehaviorVersion::latest()).region(aws_config::Region::new(cfg.aws_region.clone())).load().await;
                let generator =
                    aws_sdk_dsql::auth_token::AuthTokenGenerator::new(aws_sdk_dsql::auth_token::Config::builder().hostname(&endpoint).build().map_err(|e| anyhow::anyhow!("dsql token config: {e}"))?);
                Manager::Dsql { endpoint, sdk: Arc::new(sdk), generator: Arc::new(generator), tls: tls_connector() }
            }
        };
        let pool = Pool::builder().max_size(4).idle_timeout(Some(std::time::Duration::from_secs(30))).build(manager).await?;
        Ok(Db { dialect: cfg.db_dialect, pool })
    }

    /// Dev/tests: wrap an existing URL without going through Config.
    pub async fn open_url(url: &str) -> anyhow::Result<Db> {
        let pool = Pool::builder().max_size(4).build(Manager::Url(url.to_string())).await?;
        Ok(Db { dialect: DbDialect::Postgres, pool })
    }

    pub async fn query(&self, sql: &str, params: Params<'_>) -> anyhow::Result<Vec<Row>> {
        let conn = self.pool.get().await?;
        Ok(conn.query(sql, params).await?)
    }

    pub async fn execute(&self, sql: &str, params: Params<'_>) -> anyhow::Result<u64> {
        let conn = self.pool.get().await?;
        Ok(conn.execute(sql, params).await?)
    }

    /// First row or None.
    pub async fn one(&self, sql: &str, params: Params<'_>) -> anyhow::Result<Option<Row>> {
        Ok(self.query(sql, params).await?.into_iter().next())
    }
}

fn tls_connector() -> tokio_postgres_rustls::MakeRustlsConnect {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder().with_root_certificates(roots).with_no_client_auth();
    tokio_postgres_rustls::MakeRustlsConnect::new(config)
}

#[derive(Clone)]
pub enum Manager {
    Url(String),
    Dsql { endpoint: String, sdk: Arc<aws_config::SdkConfig>, generator: Arc<aws_sdk_dsql::auth_token::AuthTokenGenerator>, tls: tokio_postgres_rustls::MakeRustlsConnect },
}

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error(transparent)]
    Pg(#[from] tokio_postgres::Error),
    #[error("DSQL auth token: {0}")]
    Token(String),
}

impl ManageConnection for Manager {
    type Connection = Client;
    type Error = ConnectError;

    async fn connect(&self) -> Result<Client, ConnectError> {
        match self {
            Manager::Url(url) => {
                let (client, connection) = tokio_postgres::connect(url, NoTls).await?;
                tokio::spawn(async move {
                    if let Err(e) = connection.await {
                        tracing::error!("postgres connection: {e}");
                    }
                });
                Ok(client)
            }
            Manager::Dsql { endpoint, sdk, generator, tls } => {
                let token = generator.db_connect_admin_auth_token(sdk).await.map_err(|e| ConnectError::Token(e.to_string()))?;
                let mut pg = tokio_postgres::Config::new();
                pg.host(endpoint).port(5432).user("admin").dbname("postgres").password(token.as_str()).ssl_mode(tokio_postgres::config::SslMode::Require);
                let (client, connection) = pg.connect(tls.clone()).await?;
                tokio::spawn(async move {
                    if let Err(e) = connection.await {
                        tracing::error!("dsql connection: {e}");
                    }
                });
                Ok(client)
            }
        }
    }

    async fn is_valid(&self, conn: &mut Client) -> Result<(), ConnectError> {
        conn.simple_query("SELECT 1").await?;
        Ok(())
    }

    fn has_broken(&self, conn: &mut Client) -> bool {
        conn.is_closed()
    }
}
