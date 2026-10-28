//! "Sign in with Google" against a fake token endpoint: the code the fake
//! receives is the email it will vouch for, with optional flags.

mod common;

use std::sync::Arc;

use authorship_registry_web::google::{TokenFetch, decode_jwt_payload};
use axum::http::StatusCode;
use common::{TestApp, test_app};

fn id_token(claims: serde_json::Value) -> String {
    use base64::Engine;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string());
    format!("e30.{payload}.sig")
}

const CLIENT_ID: &str = "cid.apps.googleusercontent.com";

fn fake_google() -> TokenFetch {
    Arc::new(|body: String| {
        Box::pin(async move {
            let form: std::collections::HashMap<String, String> = form_urlencoded::parse(body.as_bytes()).into_owned().collect();
            assert_eq!(form.get("client_secret").map(String::as_str), Some("shh"));
            assert!(form.get("code_verifier").is_some_and(|v| !v.is_empty()));
            let code = form.get("code").cloned().unwrap_or_default();
            if code == "bad" {
                return Ok((false, serde_json::json!({ "error": "invalid_grant" })));
            }
            let (email, flag) = code.split_once('|').unwrap_or((&code, ""));
            let claims = serde_json::json!({
                "iss": "https://accounts.google.com",
                "aud": if flag == "wrongaud" { "other" } else { CLIENT_ID },
                "exp": chrono::Utc::now().timestamp() + 300,
                "email": email,
                "email_verified": flag != "unverified",
                "sub": "123",
            });
            Ok((true, serde_json::json!({ "id_token": id_token(claims) })))
        })
    })
}

/// Drive /auth/google, then hit the callback with the given code and the state Google would echo.
async fn callback(app: &TestApp, code: &str, next: Option<&str>, tamper_state: bool) -> common::Resp {
    let path = match next {
        Some(n) => format!("/auth/google?next={}", form_urlencoded::byte_serialize(n.as_bytes()).collect::<String>()),
        None => "/auth/google".to_string(),
    };
    let start = app.get(&path, None).await;
    assert!(start.status.is_redirection(), "{}", start.body);
    let loc = url::Url::parse(start.location()).unwrap();
    assert_eq!(format!("{}://{}{}", loc.scheme(), loc.host_str().unwrap(), loc.path()), "https://accounts.google.com/o/oauth2/v2/auth");
    let q: std::collections::HashMap<String, String> = loc.query_pairs().into_owned().collect();
    assert_eq!(q.get("redirect_uri").map(String::as_str), Some("http://test.local/auth/google/callback"));
    assert_eq!(q.get("code_challenge_method").map(String::as_str), Some("S256"));
    let cookie = start.set_cookies().into_iter().find(|c| c.starts_with("goauth=")).expect("goauth cookie");
    let state = if tamper_state { "nope".to_string() } else { q.get("state").cloned().unwrap() };
    let enc: String = form_urlencoded::byte_serialize(code.as_bytes()).collect();
    app.get(&format!("/auth/google/callback?code={enc}&state={state}"), Some(&cookie)).await
}

#[tokio::test]
async fn google_sign_in() {
    let app = test_app(&[("GOOGLE_CLIENT_ID", CLIENT_ID), ("GOOGLE_CLIENT_SECRET", "shh")], Some(fake_google())).await;

    // shows the button only when configured
    assert!(app.get("/login", None).await.body.contains("Sign in with Google"));
    let plain = test_app(&[], None).await;
    assert!(!plain.get("/login", None).await.body.contains("Sign in with Google"));
    assert_eq!(plain.get("/auth/google", None).await.status, StatusCode::NOT_FOUND);
    drop(plain);

    // signs in a staff account whose email Google verified
    let r = callback(&app, "admin@test.local|ok", Some("/admin/users"), false).await;
    assert!(r.status.is_redirection(), "{}", r.body);
    assert_eq!(r.location(), "/admin/users");
    let sid = r.session_cookie().expect("session cookie");
    assert_eq!(app.get("/admin/users", Some(&sid)).await.status, StatusCode::OK);
    let last: Option<String> = app.db.query("SELECT detail FROM audit_log WHERE action = 'login' ORDER BY at DESC", &[]).await.unwrap().first().map(|r| r.get("detail"));
    assert_eq!(last.as_deref(), Some("google"));

    // refuses a Google account that is not staff, and logs it
    let r = callback(&app, "stranger@gmail.com|ok", None, false).await;
    assert!(r.status.is_redirection());
    assert_eq!(r.location(), "/login?denied=staff");
    assert!(r.session_cookie().is_none());
    let denied: Vec<String> = app.db.query("SELECT detail FROM audit_log WHERE action = 'login_denied'", &[]).await.unwrap().iter().map(|r| r.get("detail")).collect();
    assert!(denied.iter().any(|d| d == "google: stranger@gmail.com"), "{denied:?}");
    assert!(app.get("/login?denied=staff", None).await.body.contains("not a registered staff account"));

    // refuses unverified emails, wrong audience, bad codes and tampered state
    for code in ["admin@test.local|unverified", "admin@test.local|wrongaud", "bad"] {
        let r = callback(&app, code, None, false).await;
        assert_eq!(r.location(), "/login?denied=google", "{code}");
        assert!(r.session_cookie().is_none(), "{code}");
    }
    assert_eq!(callback(&app, "admin@test.local|ok", None, true).await.location(), "/login?denied=google");

    // decodes a JWT payload
    assert_eq!(decode_jwt_payload(&id_token(serde_json::json!({ "a": 1 }))).unwrap(), serde_json::json!({ "a": 1 }));
}
