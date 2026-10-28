//! Migrations are plain SQL strings, applied once each, one statement per query,
//! recorded in `schema_migrations` with the same ids the Node version used so an
//! existing database is picked up as-is. DSQL requires indexes to be created
//! with CREATE INDEX ASYNC; vanilla Postgres does not know that keyword.

use chrono::Utc;
use uuid::Uuid;

use crate::config::DbDialect;
use crate::db::Db;

struct Migration {
    id: &'static str,
    statements: fn(DbDialect) -> Vec<String>,
}

fn idx(d: DbDialect, name: &str, table: &str, cols: &str, unique: bool) -> String {
    format!("CREATE {}INDEX {}{name} ON {table} ({cols})", if unique { "UNIQUE " } else { "" }, if d == DbDialect::Dsql { "ASYNC " } else { "" })
}

const MIGRATIONS: &[Migration] = &[Migration {
    id: "0001_init",
    statements: |d| {
        vec![
            "CREATE TABLE users (
               id uuid PRIMARY KEY,
               email text NOT NULL,
               name text NOT NULL,
               role text NOT NULL,
               disabled boolean NOT NULL DEFAULT false,
               created_at timestamptz NOT NULL)"
                .into(),
            idx(d, "users_email_idx", "users", "email", true),
            "CREATE TABLE tokens (
               id uuid PRIMARY KEY,
               token_hash text NOT NULL,
               purpose text NOT NULL,
               email text NOT NULL,
               report_id uuid,
               expires_at timestamptz NOT NULL,
               used_at timestamptz)"
                .into(),
            idx(d, "tokens_hash_idx", "tokens", "token_hash", true),
            "CREATE TABLE sessions (
               id uuid PRIMARY KEY,
               token_hash text NOT NULL,
               user_id uuid NOT NULL,
               expires_at timestamptz NOT NULL,
               created_at timestamptz NOT NULL)"
                .into(),
            idx(d, "sessions_hash_idx", "sessions", "token_hash", true),
            "CREATE TABLE reports (
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
               decided_at timestamptz)"
                .into(),
            idx(d, "reports_status_idx", "reports", "status", false),
            idx(d, "reports_name_key_idx", "reports", "subject_name_key", false),
            idx(d, "reports_doi_idx", "reports", "paper_doi", false),
            "CREATE TABLE audit_log (
               id uuid PRIMARY KEY,
               at timestamptz NOT NULL,
               actor_user_id uuid,
               actor_email text,
               action text NOT NULL,
               report_id uuid,
               detail text)"
                .into(),
            idx(d, "audit_log_at_idx", "audit_log", "at", false),
        ]
    },
}];

/// Apply pending migrations and seed the admin; returns what ran.
pub async fn migrate(db: &Db, admin_email: Option<&str>) -> anyhow::Result<Vec<String>> {
    db.execute("CREATE TABLE IF NOT EXISTS schema_migrations (id text PRIMARY KEY, applied_at timestamptz NOT NULL)", &[]).await?;
    let applied: Vec<String> = db.query("SELECT id FROM schema_migrations", &[]).await?.iter().map(|r| r.get("id")).collect();
    let mut ran = Vec::new();
    for m in MIGRATIONS {
        if applied.iter().any(|a| a == m.id) {
            continue;
        }
        for s in (m.statements)(db.dialect) {
            db.execute(&s, &[]).await?;
        }
        db.execute("INSERT INTO schema_migrations (id, applied_at) VALUES ($1, $2)", &[&m.id, &Utc::now()]).await?;
        ran.push(m.id.to_string());
    }
    if let Some(email) = admin_email {
        let existing = db.query("SELECT id FROM users WHERE email = $1", &[&email]).await?;
        if existing.is_empty() {
            db.execute("INSERT INTO users (id, email, name, role, disabled, created_at) VALUES ($1, $2, $3, 'admin', false, $4)", &[&Uuid::new_v4(), &email, &"Administrator", &Utc::now()]).await?;
            ran.push(format!("seed_admin:{email}"));
        }
    }
    Ok(ran)
}
