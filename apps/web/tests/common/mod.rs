//! Shared test harness: a real PostgreSQL (embedded, downloaded once into
//! ~/.theseus, or TEST_DATABASE_URL if set), the router with a console mailer,
//! and tiny request helpers mirroring the old `app.request()` calls.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use authorship_registry_web::app::{AppState, router};
use authorship_registry_web::config::Config;
use authorship_registry_web::db::Db;
use authorship_registry_web::google::TokenFetch;
use authorship_registry_web::mail::Mailer;
use authorship_registry_web::schema::migrate;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, Response, StatusCode, header};
use http_body_util::BodyExt;
use postgresql_embedded::{PostgreSQL, Settings};
use tower::ServiceExt;

pub struct TestDb {
    _pg: Option<PostgreSQL>,
    pub url: String,
}

/// A fresh, empty database. Dropping the value stops the embedded server.
pub async fn test_db() -> TestDb {
    if let Ok(url) = std::env::var("TEST_DATABASE_URL") {
        return TestDb { _pg: None, url };
    }
    let mut pg = PostgreSQL::new(Settings { temporary: true, ..Default::default() });
    pg.setup().await.expect("embedded postgres setup");
    pg.start().await.expect("embedded postgres start");
    let name = format!("t_{}", uuid::Uuid::new_v4().simple());
    pg.create_database(&name).await.expect("create database");
    let url = pg.settings().url(&name);
    TestDb { _pg: Some(pg), url }
}

pub fn base_env(extra: &[(&str, &str)]) -> HashMap<String, String> {
    let mut m: HashMap<String, String> =
        [("MAIL_DRIVER", "console"), ("BASE_URL", "http://test.local"), ("ADMIN_EMAIL", "admin@test.local"), ("TOKEN_TTL_MINUTES", "5")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    for (k, v) in extra {
        m.insert(k.to_string(), v.to_string());
    }
    m
}

pub struct TestApp {
    pub router: Router,
    pub db: Db,
    pub mailer: Mailer,
    pub cfg: Config,
    _db: TestDb,
}

pub async fn test_app(extra: &[(&str, &str)], token_fetch: Option<TokenFetch>) -> TestApp {
    authorship_registry_web::init_crypto();
    let tdb = test_db().await;
    let mut env = base_env(extra);
    env.insert("DB_DIALECT".into(), "postgres".into());
    env.insert("DATABASE_URL".into(), tdb.url.clone());
    let cfg = Config::from_map(&env).expect("config");
    let db = Db::open(&cfg).await.expect("db");
    migrate(&db, cfg.admin_email.as_deref()).await.expect("migrate");
    let mailer = Mailer::console();
    let state = Arc::new(AppState { cfg: cfg.clone(), db: db.clone(), mailer: mailer.clone(), token_fetch: token_fetch.unwrap_or_else(authorship_registry_web::google::reqwest_token_fetch) });
    TestApp { router: router(state), db, mailer, cfg, _db: tdb }
}

pub struct Resp {
    pub status: StatusCode,
    pub headers: axum::http::HeaderMap,
    pub body: String,
}

impl Resp {
    pub fn location(&self) -> &str {
        self.headers.get(header::LOCATION).and_then(|v| v.to_str().ok()).unwrap_or("")
    }
    pub fn set_cookies(&self) -> Vec<String> {
        self.headers.get_all(header::SET_COOKIE).iter().filter_map(|v| v.to_str().ok()).map(|s| s.split(';').next().unwrap_or("").to_string()).collect()
    }
    /// The session cookie this response set, as "sid=..." for reuse.
    pub fn session_cookie(&self) -> Option<String> {
        self.set_cookies().into_iter().find(|c| c.starts_with("sid="))
    }
}

async fn send(app: &Router, req: Request<Body>) -> Resp {
    let res: Response<Body> = app.clone().oneshot(req).await.expect("request");
    let status = res.status();
    let headers = res.headers().clone();
    let body = String::from_utf8(res.into_body().collect().await.expect("body").to_bytes().to_vec()).expect("utf8");
    Resp { status, headers, body }
}

impl TestApp {
    pub async fn get(&self, path: &str, cookie: Option<&str>) -> Resp {
        let mut b = Request::builder().uri(format!("http://test.local{path}"));
        if let Some(c) = cookie {
            b = b.header(header::COOKIE, c);
        }
        send(&self.router, b.body(Body::empty()).unwrap()).await
    }

    pub async fn post(&self, path: &str, data: &[(&str, &str)], cookie: Option<&str>) -> Resp {
        let body: String = form_urlencoded::Serializer::new(String::new()).extend_pairs(data).finish();
        let mut b = Request::builder().method("POST").uri(format!("http://test.local{path}")).header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
        if let Some(c) = cookie {
            b = b.header(header::COOKIE, c);
        }
        send(&self.router, b.body(Body::from(body)).unwrap()).await
    }

    /// Path of the last link in the last console mail, e.g. "/verify/<token>".
    pub fn last_link(&self) -> String {
        let mail = self.mailer.sent().last().cloned().expect("a mail was sent");
        let start = mail.text.find("http://test.local").expect("link in mail");
        let rest = &mail.text[start + "http://test.local".len()..];
        rest.split_whitespace().next().unwrap().to_string()
    }

    /// Request a magic link for `email` and follow it; returns the "sid=..." cookie.
    pub async fn sign_in(&self, email: &str) -> String {
        let r = self.post("/login", &[("email", email)], None).await;
        assert!(r.status.is_redirection(), "login should redirect: {}", r.body);
        let link = self.last_link();
        let auth = self.get(&link, None).await;
        assert!(auth.status.is_redirection(), "auth link should redirect, body: {}", auth.body);
        auth.session_cookie().expect("session cookie")
    }
}
