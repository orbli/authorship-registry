//! "Sign in with Google" for staff, as a second door next to the magic link.
//!
//! Plain OpenID Connect authorization-code flow with PKCE, no library: the app
//! sends the browser to Google, Google sends back a one-time code, the server
//! swaps it for an ID token over TLS and reads the verified email out of it.
//! That email is then looked up in the users table exactly like a magic-link
//! login; an address that is not a staff account is refused. Google never
//! creates accounts here.
//!
//! The ID token's signature is not re-verified: it arrives straight from
//! Google's token endpoint on a TLS connection we opened, which Google's own
//! docs list as sufficient. Issuer, audience, expiry and email_verified are
//! still checked.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tokio::sync::OnceCell;

use crate::auth::base64_url;
use crate::config::Config;
use crate::mail::ssm_parameter;

pub const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

/// POST a form body to Google's token endpoint; returns (ok, json). Tests swap in a fake.
pub type TokenFetch = Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = anyhow::Result<(bool, serde_json::Value)>> + Send>> + Send + Sync>;

pub fn reqwest_token_fetch() -> TokenFetch {
    Arc::new(|body: String| {
        Box::pin(async move {
            let res = reqwest::Client::new().post(GOOGLE_TOKEN_URL).header("content-type", "application/x-www-form-urlencoded").body(body).send().await?;
            let ok = res.status().is_success();
            Ok((ok, res.json::<serde_json::Value>().await.unwrap_or(serde_json::Value::Null)))
        })
    })
}

pub struct Login {
    pub state: String,
    pub verifier: String,
    pub challenge: String,
}

/// Fresh state + PKCE pair for one sign-in attempt; keep them in a short-lived cookie.
pub fn begin_login() -> Login {
    let verifier = base64_url(&rand::random::<[u8; 32]>());
    let challenge = base64_url(&Sha256::digest(verifier.as_bytes()));
    Login { state: base64_url(&rand::random::<[u8; 16]>()), verifier, challenge }
}

pub fn auth_url(client_id: &str, redirect_uri: &str, state: &str, challenge: &str) -> String {
    let q = [
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("response_type", "code"),
        ("scope", "openid email"),
        ("state", state),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("prompt", "select_account"),
    ];
    format!("{GOOGLE_AUTH_URL}?{}", form_encode(&q))
}

pub fn form_encode(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v))).collect::<Vec<_>>().join("&")
}

/// application/x-www-form-urlencoded escaping (space becomes '+').
pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'*' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

static SECRET: OnceCell<String> = OnceCell::const_new();

async fn client_secret(cfg: &Config) -> anyhow::Result<String> {
    let g = cfg.google.as_ref().ok_or_else(|| anyhow::anyhow!("Google sign-in not configured"))?;
    if let Some(s) = &g.client_secret {
        return Ok(s.clone());
    }
    let param = g.client_secret_param.clone().ok_or_else(|| anyhow::anyhow!("Google client secret not configured"))?;
    Ok(SECRET.get_or_try_init(|| ssm_parameter(&cfg.aws_region, &param)).await?.clone())
}

pub struct Identity {
    pub email: String,
    pub sub: String,
}

#[derive(serde::Deserialize)]
struct Claims {
    iss: Option<String>,
    aud: Option<String>,
    exp: Option<i64>,
    email: Option<String>,
    email_verified: Option<bool>,
    sub: Option<String>,
}

/// Swap the callback code for an ID token and return the verified email, or fail.
pub async fn finish_login(cfg: &Config, code: &str, verifier: &str, redirect_uri: &str, fetch: &TokenFetch) -> anyhow::Result<Identity> {
    let client_id = cfg.google.as_ref().map(|g| g.client_id.clone()).ok_or_else(|| anyhow::anyhow!("Google sign-in not configured"))?;
    let secret = client_secret(cfg).await?;
    let body = form_encode(&[("code", code), ("client_id", &client_id), ("client_secret", &secret), ("redirect_uri", redirect_uri), ("grant_type", "authorization_code"), ("code_verifier", verifier)]);
    let (ok, json) = fetch(body).await?;
    let id_token = json.get("id_token").and_then(|v| v.as_str());
    let (Some(id_token), true) = (id_token, ok) else {
        anyhow::bail!("Google token exchange failed: {} {}", json.get("error").and_then(|v| v.as_str()).unwrap_or("?"), json.get("error_description").and_then(|v| v.as_str()).unwrap_or(""));
    };
    let claims: Claims = serde_json::from_value(decode_jwt_payload(id_token)?)?;
    match claims.iss.as_deref() {
        Some("accounts.google.com") | Some("https://accounts.google.com") => {}
        _ => anyhow::bail!("Google ID token: bad issuer"),
    }
    if claims.aud.as_deref() != Some(client_id.as_str()) {
        anyhow::bail!("Google ID token: bad audience");
    }
    if claims.exp.is_none_or(|e| e * 1000 < chrono::Utc::now().timestamp_millis()) {
        anyhow::bail!("Google ID token: expired");
    }
    let (Some(email), Some(true)) = (claims.email, claims.email_verified) else {
        anyhow::bail!("Google ID token: email not verified");
    };
    Ok(Identity { email: email.to_lowercase(), sub: claims.sub.unwrap_or_default() })
}

pub fn decode_jwt_payload(jwt: &str) -> anyhow::Result<serde_json::Value> {
    use base64::Engine;
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() != 3 {
        anyhow::bail!("not a JWT");
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(parts[1].trim_end_matches('='))?;
    Ok(serde_json::from_slice(&bytes)?)
}
