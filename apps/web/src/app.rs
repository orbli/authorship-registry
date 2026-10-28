//! HTTP routes. One axum Router built over shared state; the same router serves
//! Kubernetes, local dev and the Lambda entry.

use std::collections::HashMap;
use std::sync::Arc;

use askama::Template;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{FromRequestParts, OriginalUri, Path, Query, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::Utc;
use uuid::Uuid;

use crate::auth::{self, Role, SESSION_COOKIE, TokenPurpose, User, user_by_email, user_from_session};
use crate::config::Config;
use crate::db::Db;
use crate::google::{self, TokenFetch};
use crate::mail::{Mail, Mailer};
use crate::names::{looks_like_doi, name_key, name_score, normalize_doi};
use crate::views::{
    self,
    pages::{self, AuditRow, ReportForm, ReportRow, SearchHit},
};

pub struct AppState {
    pub cfg: Config,
    pub db: Db,
    pub mailer: Mailer,
    pub token_fetch: TokenFetch,
}

type St = State<Arc<AppState>>;

/// Any failure inside a handler: logged, answered with a plain 500.
pub struct AppError(anyhow::Error);

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(e: E) -> Self {
        AppError(e.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        tracing::error!("request failed: {:#}", self.0);
        (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response()
    }
}

type Res = Result<Response, AppError>;

/// The signed-in staff member, if the session cookie is valid.
pub struct CurrentUser(pub Option<User>);

impl FromRequestParts<Arc<AppState>> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<AppState>) -> Result<Self, AppError> {
        let sid = CookieJar::from_headers(&parts.headers).get(SESSION_COOKIE).map(|c| c.value().to_string());
        Ok(CurrentUser(user_from_session(&state.db, sid.as_deref()).await?))
    }
}

/// Form bodies as a plain map, whatever the content type (browsers and tests both send urlencoded).
pub struct FormMap(pub HashMap<String, String>);

impl axum::extract::FromRequest<Arc<AppState>> for FormMap {
    type Rejection = AppError;

    async fn from_request(req: axum::extract::Request, state: &Arc<AppState>) -> Result<Self, AppError> {
        let bytes = Bytes::from_request(req, state).await.map_err(|e| anyhow::anyhow!("body: {e}"))?;
        Ok(FormMap(form_urlencoded::parse(&bytes).into_owned().collect()))
    }
}

impl FormMap {
    fn get(&self, k: &str) -> String {
        self.0.get(k).map(|v| v.trim().to_string()).unwrap_or_default()
    }
}

const EMAIL_OK: fn(&str) -> bool = |s| {
    let (local, domain) = match s.split_once('@') {
        Some(p) => p,
        None => return false,
    };
    !local.is_empty()
        && !local.contains(char::is_whitespace)
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains(char::is_whitespace)
        && !domain.contains('@')
};

impl AppState {
    /// Origin for links in emails: configured BASE_URL, else the request's own origin.
    fn base_url(&self, headers: &HeaderMap) -> String {
        if !self.cfg.base_url.is_empty() {
            return self.cfg.base_url.clone();
        }
        let host = headers.get("host").and_then(|v| v.to_str().ok()).unwrap_or("localhost");
        let proto = headers.get("x-forwarded-proto").and_then(|v| v.to_str().ok()).unwrap_or(if std::env::var_os("AWS_LAMBDA_FUNCTION_NAME").is_some() { "https" } else { "http" });
        format!("{proto}://{host}")
    }

    fn secure_cookie(&self, headers: &HeaderMap) -> bool {
        self.base_url(headers).starts_with("https://")
    }

    async fn audit(&self, actor: Option<&User>, action: &str, report_id: Option<Uuid>, detail: Option<&str>) -> anyhow::Result<()> {
        self.db
            .execute(
                "INSERT INTO audit_log (id, at, actor_user_id, actor_email, action, report_id, detail) VALUES ($1, $2, $3, $4, $5, $6, $7)",
                &[&Uuid::new_v4(), &Utc::now(), &actor.map(|u| u.id), &actor.map(|u| u.email.as_str()), &action, &report_id, &detail],
            )
            .await?;
        Ok(())
    }
}

fn session_cookie(value: String, secure: bool, hours: i64) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, value)).http_only(true).secure(secure).same_site(SameSite::Lax).path("/").max_age(time::Duration::hours(hours)).build()
}

fn html(t: impl Template) -> Response {
    html_status(StatusCode::OK, t)
}

fn html_status(status: StatusCode, t: impl Template) -> Response {
    match t.render() {
        Ok(body) => (status, Html(body)).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "template render failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "template error").into_response()
        }
    }
}

async fn stylesheet() -> Response {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8"), (header::CACHE_CONTROL, "public, max-age=31536000, immutable")], views::CSS).into_response()
}

/// Redirect anonymous visitors to the login page; refuse the wrong role. Admins pass everything.
#[allow(clippy::result_large_err)] // the Err is a ready response, not an error value
fn require(user: &CurrentUser, roles: &[Role], path: &str) -> Result<User, Response> {
    match &user.0 {
        None => Err(Redirect::to(&format!("/login?next={}", google::urlencode(path).replace('+', "%20"))).into_response()),
        Some(u) if u.role != Role::Admin && !roles.contains(&u.role) => Err((StatusCode::FORBIDDEN, "Forbidden").into_response()),
        Some(u) => Ok(u.clone()),
    }
}

fn safe_next(n: Option<&str>) -> Option<String> {
    n.filter(|n| n.starts_with('/') && !n.starts_with("//")).map(|s| s.to_string())
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/static/app.css", get(stylesheet))
        .route("/", get(incidents))
        .route("/search", get(search_redirect))
        .route("/record/{id}", get(record))
        .route("/report", get(report_form).post(report_submit))
        .route("/report/sent", get(report_sent))
        .route("/verify/{token}", get(verify))
        .route("/login", get(login_form).post(login_submit))
        .route("/auth/google", get(google_start))
        .route("/auth/google/callback", get(google_callback))
        .route("/auth/{token}", get(auth_link))
        .route("/logout", post(logout))
        .route("/review", get(review_list))
        .route("/review/{id}", get(review_detail))
        .route("/review/{id}/decision", post(review_decide))
        .route("/admin/users", get(admin_users).post(admin_add_user))
        .route("/admin/users/{id}/{op}", post(admin_toggle_user))
        .route("/admin/audit", get(admin_audit))
        .fallback(not_found)
        .with_state(state)
}

// ---------- health ----------

async fn healthz(State(st): St) -> Res {
    st.db.query("SELECT 1", &[]).await?;
    Ok("ok".into_response())
}

async fn not_found(user: CurrentUser) -> Response {
    html_status(StatusCode::NOT_FOUND, pages::message_page(user.0.as_ref(), "Not found", "There is nothing at this address.", "err"))
}

// ---------- public: incidents (accepted records, reporter stripped) ----------

async fn latest(st: &AppState) -> anyhow::Result<Vec<SearchHit>> {
    let rows = st.db.query(&format!("SELECT {} FROM reports WHERE status = 'accepted' ORDER BY decided_at DESC LIMIT 50", SearchHit::COLS), &[]).await?;
    Ok(rows.iter().map(|r| SearchHit::from_row(r, 1.0)).collect())
}

async fn search(st: &AppState, q: &str) -> anyhow::Result<Vec<SearchHit>> {
    if looks_like_doi(q) {
        let rows = st.db.query(&format!("SELECT {} FROM reports WHERE status = 'accepted' AND paper_doi = $1", SearchHit::COLS), &[&normalize_doi(q)]).await?;
        return Ok(rows.iter().map(|r| SearchHit::from_row(r, 1.0)).collect());
    }
    let rows = st.db.query(&format!("SELECT {} FROM reports WHERE status = 'accepted' LIMIT 5000", SearchHit::COLS), &[]).await?;
    let ql = q.to_lowercase();
    let mut hits: Vec<SearchHit> = rows
        .iter()
        .map(|r| {
            let h = SearchHit::from_row(r, 0.0);
            let by_name = name_score(q, &h.subject_name_key);
            let by_title = if ql.chars().count() >= 4 && h.paper_title.to_lowercase().contains(&ql) { 0.8 } else { 0.0 };
            SearchHit { score: by_name.max(by_title), ..h }
        })
        .filter(|h| h.score >= 0.35)
        .collect();
    hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal).then_with(|| b.decided_at.cmp(&a.decided_at)));
    hits.truncate(50);
    Ok(hits)
}

async fn incidents(State(st): St, user: CurrentUser, Query(qs): Query<HashMap<String, String>>) -> Res {
    let q = qs.get("q").map(|s| s.trim().to_string()).unwrap_or_default();
    let hits = if q.is_empty() { latest(&st).await? } else { search(&st, &q).await? };
    if !q.is_empty() && user.0.is_some() {
        st.audit(user.0.as_ref(), "search", None, Some(&format!("{q} → {}", hits.len()))).await?;
    }
    Ok(html(pages::incidents_page(user.0.as_ref(), &q, &hits)))
}

/// Old editor-only search URL.
async fn search_redirect(Query(qs): Query<HashMap<String, String>>) -> Response {
    match qs.get("q").map(|s| s.trim()).filter(|s| !s.is_empty()) {
        Some(q) => Redirect::to(&format!("/?q={}", google::urlencode(q).replace('+', "%20"))).into_response(),
        None => Redirect::to("/").into_response(),
    }
}

async fn record(State(st): St, user: CurrentUser, Path(id): Path<String>) -> Res {
    let Ok(id) = Uuid::parse_str(&id) else { return Ok(not_found(user).await) };
    let Some(row) = st.db.one(&format!("SELECT {} FROM reports WHERE id = $1 AND status = 'accepted'", ReportRow::COLS), &[&id]).await? else {
        return Ok(not_found(user).await);
    };
    let r = ReportRow::from_row(&row);
    let related: i64 = st
        .db
        .one("SELECT COUNT(*) AS n FROM reports WHERE status = 'accepted' AND subject_name_key = $1 AND id <> $2", &[&r.subject_name_key, &r.id])
        .await?
        .map(|x| x.get::<_, i64>("n"))
        .unwrap_or(0);
    if user.0.is_some() {
        st.audit(user.0.as_ref(), "record_view", Some(r.id), None).await?;
    }
    Ok(html(pages::record_page(user.0.as_ref(), &r, related)))
}

// ---------- public: reporting ----------

async fn report_form(user: CurrentUser) -> Response {
    html(pages::report_page(user.0.as_ref(), &ReportForm::default(), None))
}

async fn report_submit(State(st): St, user: CurrentUser, headers: HeaderMap, f: FormMap) -> Res {
    let v = ReportForm {
        reporter_name: f.get("reporter_name"),
        reporter_email: f.get("reporter_email"),
        reporter_affiliation: f.get("reporter_affiliation"),
        subject_name: f.get("subject_name"),
        subject_affiliation: f.get("subject_affiliation"),
        subject_email: f.get("subject_email"),
        paper_title: f.get("paper_title"),
        paper_doi: f.get("paper_doi"),
        paper_venue: f.get("paper_venue"),
        description: f.get("description"),
    };
    let required = [
        ("reporter name", &v.reporter_name),
        ("reporter email", &v.reporter_email),
        ("reporter affiliation", &v.reporter_affiliation),
        ("subject name", &v.subject_name),
        ("paper title", &v.paper_title),
        ("description", &v.description),
    ];
    let missing: Vec<&str> = required.iter().filter(|(_, val)| val.is_empty()).map(|(k, _)| *k).collect();
    let fail = |error: String| html_status(StatusCode::BAD_REQUEST, pages::report_page(user.0.as_ref(), &v, Some(&error)));
    if !missing.is_empty() {
        return Ok(fail(format!("Please fill in: {}.", missing.join(", "))));
    }
    if !EMAIL_OK(&v.reporter_email) {
        return Ok(fail("That email address does not look valid.".into()));
    }
    let email = v.reporter_email.to_lowercase();
    let id = Uuid::new_v4();
    let opt = |s: &String| if s.is_empty() { None } else { Some(s.clone()) };
    let doi = if v.paper_doi.is_empty() { None } else { Some(normalize_doi(&v.paper_doi)) };
    st.db
        .execute(
            "INSERT INTO reports (id, status, reporter_name, reporter_email, reporter_affiliation, subject_name, subject_name_key, subject_affiliation, subject_email, paper_title, paper_doi, paper_venue, description, created_at)
             VALUES ($1, 'pending_email', $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
            &[&id, &v.reporter_name, &email, &v.reporter_affiliation, &v.subject_name, &name_key(&v.subject_name), &opt(&v.subject_affiliation), &opt(&v.subject_email), &v.paper_title, &doi, &opt(&v.paper_venue), &v.description, &Utc::now()],
        )
        .await?;
    let ttl = st.cfg.token_ttl_minutes * 4;
    let token = auth::issue_token(&st.db, TokenPurpose::VerifyReport, &email, Some(id), ttl).await?;
    st.mailer
        .send(Mail {
            to: email.clone(),
            subject: "Confirm your report to the Authorship Registry".into(),
            text: format!(
                "Hello {},\n\nSomeone used this email address to file a report about \"{}\".\n\nIf that was you, confirm it here (the link works once and expires in {ttl} minutes):\n\n{}/verify/{token}\n\nIf it was not you, ignore this message and nothing will be filed.\n",
                v.reporter_name,
                v.paper_title,
                st.base_url(&headers)
            ),
        })
        .await?;
    Ok(Redirect::to("/report/sent").into_response())
}

async fn report_sent(user: CurrentUser) -> Response {
    html(pages::message_page(user.0.as_ref(), "Check your email", "We sent you a confirmation link. Your report is not filed until you click it.", "ok"))
}

async fn verify(State(st): St, user: CurrentUser, Path(token): Path<String>) -> Res {
    let t = auth::consume_token(&st.db, &token, TokenPurpose::VerifyReport).await?;
    let Some((report_id, email)) = t.and_then(|t| t.report_id.map(|r| (r, t.email))) else {
        return Ok(html_status(StatusCode::BAD_REQUEST, pages::message_page(user.0.as_ref(), "Link not valid", "This confirmation link is invalid, expired or already used.", "err")));
    };
    st.db.execute("UPDATE reports SET status = 'submitted', verified_at = $1 WHERE id = $2 AND status = 'pending_email'", &[&Utc::now(), &report_id]).await?;
    st.audit(None, "report_verified", Some(report_id), Some(&email)).await?;
    Ok(html(pages::message_page(user.0.as_ref(), "Report filed", "Thank you. Your report is now with the review team. You will not be contacted unless they need clarification.", "ok")))
}

// ---------- staff auth ----------

fn denied_message(code: &str) -> Option<&'static str> {
    match code {
        "staff" => Some("That Google account is not a registered staff account. Ask an administrator to add your address, or use the email link below."),
        "google" => Some("Google sign-in did not complete. Try again, or use the email link below."),
        _ => None,
    }
}

async fn login_form(State(st): St, Query(qs): Query<HashMap<String, String>>) -> Response {
    html(pages::login_page(qs.get("next").map(String::as_str), qs.get("sent").map(String::as_str) == Some("1"), st.cfg.google.is_some(), qs.get("denied").and_then(|d| denied_message(d))))
}

async fn login_submit(State(st): St, headers: HeaderMap, f: FormMap) -> Res {
    let email = f.get("email").to_lowercase();
    let next = f.get("next");
    if let Some(user) = user_by_email(&st.db, &email).await?.filter(|u| !u.disabled) {
        let token = auth::issue_token(&st.db, TokenPurpose::Login, &email, None, st.cfg.token_ttl_minutes).await?;
        let url = format!("{}/auth/{token}{}", st.base_url(&headers), if next.is_empty() { String::new() } else { format!("?next={}", google::urlencode(&next).replace('+', "%20")) });
        st.mailer
            .send(Mail {
                to: user.email,
                subject: "Your Authorship Registry sign-in link".into(),
                text: format!("Sign in here (expires in {} minutes, works once):\n\n{url}\n", st.cfg.token_ttl_minutes),
            })
            .await?;
    }
    // Same response whether or not the address is registered: no account enumeration.
    Ok(Redirect::to("/login?sent=1").into_response())
}

fn home_for(user: &User) -> &'static str {
    if user.role == Role::Editor { "/" } else { "/review" }
}

async fn auth_link(State(st): St, headers: HeaderMap, jar: CookieJar, Path(token): Path<String>, Query(qs): Query<HashMap<String, String>>) -> Res {
    let t = auth::consume_token(&st.db, &token, TokenPurpose::Login).await?;
    let user = match t {
        Some(t) => user_by_email(&st.db, &t.email).await?,
        None => None,
    };
    let Some(user) = user.filter(|u| !u.disabled) else {
        return Ok(html_status(StatusCode::BAD_REQUEST, pages::message_page(None, "Link not valid", "This sign-in link is invalid, expired or already used.", "err")));
    };
    let sid = auth::create_session(&st.db, user.id, st.cfg.session_ttl_hours).await?;
    let jar = jar.add(session_cookie(sid, st.secure_cookie(&headers), st.cfg.session_ttl_hours));
    st.audit(Some(&user), "login", None, None).await?;
    let next = safe_next(qs.get("next").map(String::as_str)).unwrap_or_else(|| home_for(&user).to_string());
    Ok((jar, Redirect::to(&next)).into_response())
}

async fn logout(State(st): St, jar: CookieJar) -> Res {
    if let Some(c) = jar.get(SESSION_COOKIE) {
        auth::destroy_session(&st.db, c.value()).await?;
    }
    let jar = jar.remove(Cookie::build(SESSION_COOKIE).path("/").build());
    Ok((jar, Redirect::to("/")).into_response())
}

// ---------- staff auth: Google (optional) ----------

const GOOGLE_COOKIE: &str = "goauth";

fn google_redirect_uri(st: &AppState, headers: &HeaderMap) -> String {
    format!("{}/auth/google/callback", st.base_url(headers))
}

async fn google_start(State(st): St, headers: HeaderMap, jar: CookieJar, user: CurrentUser, Query(qs): Query<HashMap<String, String>>) -> Res {
    let Some(g) = &st.cfg.google else { return Ok(not_found(user).await) };
    let login = google::begin_login();
    let next = safe_next(qs.get("next").map(String::as_str)).unwrap_or_default();
    let cookie = Cookie::build((GOOGLE_COOKIE, format!("{}.{}.{}", login.state, login.verifier, google::urlencode(&next))))
        .http_only(true)
        .secure(st.secure_cookie(&headers))
        .same_site(SameSite::Lax)
        .path("/auth/google")
        .max_age(time::Duration::minutes(10))
        .build();
    let url = google::auth_url(&g.client_id, &google_redirect_uri(&st, &headers), &login.state, &login.challenge);
    Ok((jar.add(cookie), Redirect::to(&url)).into_response())
}

async fn google_callback(State(st): St, headers: HeaderMap, jar: CookieJar, user: CurrentUser, Query(qs): Query<HashMap<String, String>>) -> Res {
    if st.cfg.google.is_none() {
        return Ok(not_found(user).await);
    }
    let stored = jar.get(GOOGLE_COOKIE).map(|c| c.value().to_string()).unwrap_or_default();
    let jar = jar.remove(Cookie::build(GOOGLE_COOKIE).path("/auth/google").build());
    let mut parts = stored.splitn(3, '.');
    let (state, verifier, next_enc) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let code = qs.get("code").map(String::as_str).unwrap_or("");
    if code.is_empty() || state.is_empty() || verifier.is_empty() || qs.get("state").map(String::as_str) != Some(state) {
        return Ok((jar, Redirect::to("/login?denied=google")).into_response());
    }
    let identity = match google::finish_login(&st.cfg, code, verifier, &google_redirect_uri(&st, &headers), &st.token_fetch).await {
        Ok(i) => i,
        Err(e) => {
            tracing::error!("google sign-in failed: {e:#}");
            return Ok((jar, Redirect::to("/login?denied=google")).into_response());
        }
    };
    let Some(user) = user_by_email(&st.db, &identity.email).await?.filter(|u| !u.disabled) else {
        st.audit(None, "login_denied", None, Some(&format!("google: {}", identity.email))).await?;
        return Ok((jar, Redirect::to("/login?denied=staff")).into_response());
    };
    let sid = auth::create_session(&st.db, user.id, st.cfg.session_ttl_hours).await?;
    let jar = jar.add(session_cookie(sid, st.secure_cookie(&headers), st.cfg.session_ttl_hours));
    st.audit(Some(&user), "login", None, Some("google")).await?;
    let next_decoded: String = form_urlencoded::parse(format!("n={next_enc}").as_bytes()).find(|(k, _)| k == "n").map(|(_, v)| v.into_owned()).unwrap_or_default();
    let next = safe_next(Some(&next_decoded)).unwrap_or_else(|| home_for(&user).to_string());
    Ok((jar, Redirect::to(&next)).into_response())
}

// ---------- reviewers ----------

async fn review_list(State(st): St, user: CurrentUser, OriginalUri(uri): OriginalUri, Query(qs): Query<HashMap<String, String>>) -> Res {
    let me = match require(&user, &[Role::Reviewer], uri.path()) {
        Ok(u) => u,
        Err(r) => return Ok(r),
    };
    let status = match qs.get("status").map(String::as_str) {
        Some(s @ ("submitted" | "accepted" | "rejected")) => s.to_string(),
        _ => "submitted".to_string(),
    };
    let rows = st.db.query(&format!("SELECT {} FROM reports WHERE status = $1 ORDER BY verified_at DESC LIMIT 200", ReportRow::COLS), &[&status]).await?;
    let reports: Vec<ReportRow> = rows.iter().map(ReportRow::from_row).collect();
    Ok(html(pages::review_list_page(&me, &reports, &status)))
}

async fn load_report(st: &AppState, id: &str) -> anyhow::Result<Option<ReportRow>> {
    let Ok(id) = Uuid::parse_str(id) else { return Ok(None) };
    Ok(st.db.one(&format!("SELECT {} FROM reports WHERE id = $1 AND status <> 'pending_email'", ReportRow::COLS), &[&id]).await?.map(|r| ReportRow::from_row(&r)))
}

async fn review_detail(State(st): St, user: CurrentUser, OriginalUri(uri): OriginalUri, Path(id): Path<String>) -> Res {
    let me = match require(&user, &[Role::Reviewer], uri.path()) {
        Ok(u) => u,
        Err(r) => return Ok(r),
    };
    let Some(r) = load_report(&st, &id).await? else { return Ok(not_found(user).await) };
    st.audit(Some(&me), "review_view", Some(r.id), None).await?;
    Ok(html(pages::review_detail_page(&me, &r, None)))
}

async fn review_decide(State(st): St, user: CurrentUser, OriginalUri(uri): OriginalUri, Path(id): Path<String>, f: FormMap) -> Res {
    let me = match require(&user, &[Role::Reviewer], uri.path()) {
        Ok(u) => u,
        Err(r) => return Ok(r),
    };
    let Some(r) = load_report(&st, &id).await? else { return Ok(not_found(user).await) };
    let status = f.get("status");
    let summary = f.get("summary");
    let note = f.get("decision_note");
    if status != "accepted" && status != "rejected" {
        return Ok((StatusCode::BAD_REQUEST, "Bad status").into_response());
    }
    if status == "accepted" && summary.is_empty() {
        return Ok(html_status(StatusCode::BAD_REQUEST, pages::review_detail_page(&me, &r, Some("An editor-facing summary is required to accept a report."))));
    }
    let opt = |s: &String| if s.is_empty() { None } else { Some(s.clone()) };
    st.db
        .execute(
            "UPDATE reports SET status = $1, summary = $2, decision_note = $3, decided_by = $4, decided_at = $5 WHERE id = $6",
            &[&status, &opt(&summary), &opt(&note), &me.id, &Utc::now(), &r.id],
        )
        .await?;
    let detail = if r.status != "submitted" { Some(format!("was {}", r.status)) } else { None };
    st.audit(Some(&me), &format!("decision_{status}"), Some(r.id), detail.as_deref()).await?;
    Ok(Redirect::to(&format!("/review/{}", r.id)).into_response())
}

// ---------- admin ----------

async fn list_users(st: &AppState) -> anyhow::Result<Vec<User>> {
    Ok(st.db.query(&format!("SELECT {} FROM users ORDER BY email", User::COLS), &[]).await?.iter().map(User::from_row).collect())
}

async fn admin_users(State(st): St, user: CurrentUser, OriginalUri(uri): OriginalUri) -> Res {
    let me = match require(&user, &[], uri.path()) {
        Ok(u) => u,
        Err(r) => return Ok(r),
    };
    Ok(html(pages::admin_users_page(&me, &list_users(&st).await?, None)))
}

async fn admin_add_user(State(st): St, user: CurrentUser, OriginalUri(uri): OriginalUri, f: FormMap) -> Res {
    let me = match require(&user, &[], uri.path()) {
        Ok(u) => u,
        Err(r) => return Ok(r),
    };
    let email = f.get("email").to_lowercase();
    let name = f.get("name");
    let role = Role::parse(&f.get("role"));
    let (Some(role), true) = (role, EMAIL_OK(&email) && !name.is_empty()) else {
        return Ok(html_status(StatusCode::BAD_REQUEST, pages::admin_users_page(&me, &list_users(&st).await?, Some("Email, name and a valid role are required."))));
    };
    if user_by_email(&st.db, &email).await?.is_some() {
        return Ok(html_status(StatusCode::BAD_REQUEST, pages::admin_users_page(&me, &list_users(&st).await?, Some("That email is already registered."))));
    }
    st.db.execute("INSERT INTO users (id, email, name, role, disabled, created_at) VALUES ($1, $2, $3, $4, false, $5)", &[&Uuid::new_v4(), &email, &name, &role.as_str(), &Utc::now()]).await?;
    st.audit(Some(&me), "user_added", None, Some(&format!("{email} as {}", role.as_str()))).await?;
    Ok(Redirect::to("/admin/users").into_response())
}

async fn admin_toggle_user(State(st): St, user: CurrentUser, OriginalUri(uri): OriginalUri, Path((id, op)): Path<(String, String)>) -> Res {
    let me = match require(&user, &[], uri.path()) {
        Ok(u) => u,
        Err(r) => return Ok(r),
    };
    let disabled = match op.as_str() {
        "disable" => true,
        "enable" => false,
        _ => return Ok(not_found(user).await),
    };
    let Ok(id) = Uuid::parse_str(&id) else { return Ok(not_found(user).await) };
    if id == me.id {
        return Ok((StatusCode::BAD_REQUEST, "Cannot change your own account").into_response());
    }
    let rows = st.db.query("UPDATE users SET disabled = $1 WHERE id = $2 RETURNING email", &[&disabled, &id]).await?;
    let Some(row) = rows.first() else { return Ok(not_found(user).await) };
    if disabled {
        st.db.execute("DELETE FROM sessions WHERE user_id = $1", &[&id]).await?;
    }
    let email: String = row.get("email");
    st.audit(Some(&me), if disabled { "user_disabled" } else { "user_enabled" }, None, Some(&email)).await?;
    Ok(Redirect::to("/admin/users").into_response())
}

async fn admin_audit(State(st): St, user: CurrentUser, OriginalUri(uri): OriginalUri) -> Res {
    let me = match require(&user, &[], uri.path()) {
        Ok(u) => u,
        Err(r) => return Ok(r),
    };
    let rows: Vec<AuditRow> = st
        .db
        .query("SELECT at, actor_email, action, report_id, detail FROM audit_log ORDER BY at DESC LIMIT 200", &[])
        .await?
        .iter()
        .map(|r| AuditRow { at: r.get("at"), actor_email: r.get("actor_email"), action: r.get("action"), report_id: r.get("report_id"), detail: r.get("detail") })
        .collect();
    Ok(html(pages::audit_page(&me, &rows)))
}
