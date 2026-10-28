//! Authorship Registry: report intake, review and public incident search.
//! `boot()` is the shared start-up used by the HTTP server, the Lambda entry
//! and the CLI subcommands: config → database → migrations → router.

pub mod app;
pub mod auth;
pub mod config;
pub mod db;
pub mod google;
pub mod mail;
pub mod names;
pub mod origin;
pub mod schema;
pub mod views;

use std::sync::Arc;

use app::AppState;
use config::Config;
use db::Db;
use mail::Mailer;

pub struct Booted {
    pub cfg: Config,
    pub db: Db,
    pub router: axum::Router,
}

/// Make rustls usable when more than one crypto backend is linked (the AWS SDK pulls ring, we use aws-lc).
pub fn init_crypto() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

pub fn init_tracing() {
    let _ = tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into())).try_init();
}

pub async fn boot() -> anyhow::Result<Booted> {
    init_crypto();
    let cfg = Config::from_env()?;
    let db = Db::open(&cfg).await?;
    if cfg.migrate_on_start {
        let ran = schema::migrate(&db, cfg.admin_email.as_deref()).await?;
        if !ran.is_empty() {
            println!("migrations applied: {}", ran.join(", "));
        }
    }
    let state = Arc::new(AppState { db: db.clone(), mailer: Mailer::new(&cfg), token_fetch: google::reqwest_token_fetch(), cfg: cfg.clone() });
    Ok(Booted { cfg, db, router: app::router(state) })
}
