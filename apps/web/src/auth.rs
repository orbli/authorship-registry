//! Staff accounts, one-time tokens (magic links, report confirmation) and
//! sessions. Only hashes of tokens and session ids are stored.

use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::db::Db;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Reviewer,
    Editor,
}

impl Role {
    pub const ALL: [Role; 3] = [Role::Admin, Role::Reviewer, Role::Editor];

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Reviewer => "reviewer",
            Role::Editor => "editor",
        }
    }

    pub fn parse(s: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|r| r.as_str() == s)
    }
}

#[derive(Clone, Debug)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub role: Role,
    pub disabled: bool,
}

impl User {
    pub fn from_row(r: &Row) -> User {
        User { id: r.get("id"), email: r.get("email"), name: r.get("name"), role: Role::parse(r.get::<_, String>("role").as_str()).unwrap_or(Role::Editor), disabled: r.get("disabled") }
    }

    pub const COLS: &'static str = "id, email, name, role, disabled";
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenPurpose {
    Login,
    VerifyReport,
}

impl TokenPurpose {
    fn as_str(self) -> &'static str {
        match self {
            TokenPurpose::Login => "login",
            TokenPurpose::VerifyReport => "verify_report",
        }
    }
}

pub const SESSION_COOKIE: &str = "sid";

pub fn random_token() -> String {
    base64_url(&rand::random::<[u8; 32]>())
}

pub fn base64_url(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub fn hash_token(raw: &str) -> String {
    hex::encode(Sha256::digest(raw.as_bytes()))
}

/// Create a single-use token; returns the raw token to put in a link. Only the hash is stored.
pub async fn issue_token(db: &Db, purpose: TokenPurpose, email: &str, report_id: Option<Uuid>, ttl_minutes: i64) -> anyhow::Result<String> {
    let raw = random_token();
    db.execute(
        "INSERT INTO tokens (id, token_hash, purpose, email, report_id, expires_at, used_at) VALUES ($1, $2, $3, $4, $5, $6, NULL)",
        &[&Uuid::new_v4(), &hash_token(&raw), &purpose.as_str(), &email, &report_id, &(Utc::now() + Duration::minutes(ttl_minutes))],
    )
    .await?;
    Ok(raw)
}

pub struct TokenRow {
    pub id: Uuid,
    pub email: String,
    pub report_id: Option<Uuid>,
}

/// Validate and burn a token. None when unknown, expired, wrong purpose or already used.
pub async fn consume_token(db: &Db, raw: &str, purpose: TokenPurpose) -> anyhow::Result<Option<TokenRow>> {
    let Some(row) = db.one("SELECT id, email, purpose, report_id, expires_at, used_at FROM tokens WHERE token_hash = $1", &[&hash_token(raw)]).await? else {
        return Ok(None);
    };
    let id: Uuid = row.get("id");
    let expires_at: DateTime<Utc> = row.get("expires_at");
    let used_at: Option<DateTime<Utc>> = row.get("used_at");
    if row.get::<_, String>("purpose") != purpose.as_str() || used_at.is_some() || expires_at < Utc::now() {
        return Ok(None);
    }
    // Guard against a double click: only the first UPDATE wins.
    let updated = db.query("UPDATE tokens SET used_at = $1 WHERE id = $2 AND used_at IS NULL RETURNING id", &[&Utc::now(), &id]).await?;
    if updated.is_empty() {
        return Ok(None);
    }
    Ok(Some(TokenRow { id, email: row.get("email"), report_id: row.get("report_id") }))
}

pub async fn create_session(db: &Db, user_id: Uuid, ttl_hours: i64) -> anyhow::Result<String> {
    let raw = random_token();
    db.execute(
        "INSERT INTO sessions (id, token_hash, user_id, expires_at, created_at) VALUES ($1, $2, $3, $4, $5)",
        &[&Uuid::new_v4(), &hash_token(&raw), &user_id, &(Utc::now() + Duration::hours(ttl_hours)), &Utc::now()],
    )
    .await?;
    Ok(raw)
}

pub async fn destroy_session(db: &Db, raw: &str) -> anyhow::Result<()> {
    db.execute("DELETE FROM sessions WHERE token_hash = $1", &[&hash_token(raw)]).await?;
    Ok(())
}

pub async fn user_from_session(db: &Db, raw: Option<&str>) -> anyhow::Result<Option<User>> {
    let Some(raw) = raw else { return Ok(None) };
    let Some(s) = db.one("SELECT user_id, expires_at FROM sessions WHERE token_hash = $1", &[&hash_token(raw)]).await? else {
        return Ok(None);
    };
    if s.get::<_, DateTime<Utc>>("expires_at") < Utc::now() {
        return Ok(None);
    }
    let user_id: Uuid = s.get("user_id");
    let u = db.one(&format!("SELECT {} FROM users WHERE id = $1", User::COLS), &[&user_id]).await?.map(|r| User::from_row(&r));
    Ok(u.filter(|u| !u.disabled))
}

pub async fn user_by_email(db: &Db, email: &str) -> anyhow::Result<Option<User>> {
    Ok(db.one(&format!("SELECT {} FROM users WHERE email = $1", User::COLS), &[&email]).await?.map(|r| User::from_row(&r)))
}
