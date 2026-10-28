//! Plain HTTP server (Kubernetes, local dev) plus two subcommands:
//!   web                  serve (default)
//!   web migrate          apply migrations and seed the admin, then exit
//!   web mail-test <to>   send one test message with the configured mailer

use authorship_registry_web::{
    boot,
    config::Config,
    init_crypto, init_tracing,
    mail::{Mail, Mailer},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("migrate") => {
            // boot() migrates when MIGRATE_ON_START is set; force it here regardless.
            init_crypto();
            let cfg = Config::from_env()?;
            let db = authorship_registry_web::db::Db::open(&cfg).await?;
            let ran = authorship_registry_web::schema::migrate(&db, cfg.admin_email.as_deref()).await?;
            println!("{}", if ran.is_empty() { "nothing to apply".to_string() } else { format!("applied: {}", ran.join(", ")) });
            Ok(())
        }
        Some("mail-test") => {
            init_crypto();
            let to = args.get(1).cloned().ok_or_else(|| anyhow::anyhow!("recipient required"))?;
            let cfg = Config::from_env()?;
            println!("driver={:?} host={}:{} user={} from={} → {to}", cfg.mail_driver, cfg.smtp.host, cfg.smtp.port, cfg.smtp.user, cfg.sender_email);
            Mailer::new(&cfg)
                .send(Mail {
                    to,
                    subject: "Authorship Registry: SMTP test".into(),
                    text: format!("This is a test message from the Authorship Registry mailer.\n\nSent {} via {}.\n", chrono::Utc::now().to_rfc3339(), cfg.smtp.host),
                })
                .await?;
            println!("sent");
            Ok(())
        }
        Some(other) => anyhow::bail!("unknown subcommand {other}; use: migrate | mail-test <to>"),
        None => {
            let b = boot().await?;
            let listener = tokio::net::TcpListener::bind(("0.0.0.0", b.cfg.port)).await?;
            println!("authorship-registry listening on http://localhost:{} (db={:?}, mail={:?})", b.cfg.port, b.cfg.db_dialect, b.cfg.mail_driver);
            axum::serve(listener, b.router).await?;
            Ok(())
        }
    }
}
