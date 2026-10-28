//! End-to-end flow, in order, against a real PostgreSQL: public pages, a
//! report with email confirmation, staff accounts, review, public search and
//! record pages, audit trail, and disabling a user.

mod common;

use axum::http::StatusCode;
use common::test_app;

#[tokio::test]
async fn end_to_end() {
    let app = test_app(&[], None).await;

    // serves the incident list, the report form and the health check
    assert_eq!(app.get("/healthz", None).await.status, StatusCode::OK);
    let home = app.get("/", None).await;
    assert_eq!(home.status, StatusCode::OK);
    assert!(home.body.contains("No accepted reports yet"));
    let form = app.get("/report", None).await;
    assert_eq!(form.status, StatusCode::OK);
    assert!(form.body.contains("Report undisclosed AI-generated authorship"));

    // rejects an incomplete report
    assert_eq!(app.post("/report", &[("reporter_name", "x")], None).await.status, StatusCode::BAD_REQUEST);

    // files a report and emails a verification link
    let r = app
        .post(
            "/report",
            &[
                ("reporter_name", "Alice Example"),
                ("reporter_email", "Alice@uni.edu"),
                ("reporter_affiliation", "Example University"),
                ("subject_name", "Wong, Ka-Ming"),
                ("paper_title", "On the spontaneous generation of related work"),
                ("paper_doi", "https://doi.org/10.1000/SPONT.1"),
                ("paper_venue", "J. Demo"),
                ("description", "The related work section matches ChatGPT output verbatim."),
            ],
            None,
        )
        .await;
    assert!(r.status.is_redirection(), "{}", r.body);
    let pending = app.db.query("SELECT status, reporter_email, paper_doi, subject_name_key, id FROM reports", &[]).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].get::<_, String>("status"), "pending_email");
    assert_eq!(pending[0].get::<_, String>("reporter_email"), "alice@uni.edu");
    assert_eq!(pending[0].get::<_, Option<String>>("paper_doi").as_deref(), Some("10.1000/spont.1"));
    assert_eq!(pending[0].get::<_, String>("subject_name_key"), "ka ming wong");
    let report_id: uuid::Uuid = pending[0].get("id");

    // the verification link works once
    let link = app.last_link();
    assert!(link.starts_with("/verify/"), "{link}");
    let v = app.get(&link, None).await;
    assert_eq!(v.status, StatusCode::OK);
    assert!(v.body.contains("Report filed"));
    let status: String = app.db.one("SELECT status FROM reports", &[]).await.unwrap().unwrap().get("status");
    assert_eq!(status, "submitted");
    assert_eq!(app.get(&link, None).await.status, StatusCode::BAD_REQUEST);

    // admin signs in by magic link and adds a reviewer and an editor
    let admin = app.sign_in("admin@test.local").await;
    assert_eq!(app.get("/admin/users", Some(&admin)).await.status, StatusCode::OK);
    assert!(app.post("/admin/users", &[("email", "rev@test.local"), ("name", "Rhea"), ("role", "reviewer")], Some(&admin)).await.status.is_redirection());
    assert!(app.post("/admin/users", &[("email", "ed@test.local"), ("name", "Eddy"), ("role", "editor")], Some(&admin)).await.status.is_redirection());
    assert_eq!(app.post("/admin/users", &[("email", "ed@test.local"), ("name", "Dup"), ("role", "editor")], Some(&admin)).await.status, StatusCode::BAD_REQUEST);

    // unknown email gets the same response and no mail
    let before = app.mailer.sent().len();
    assert!(app.post("/login", &[("email", "nobody@test.local")], None).await.status.is_redirection());
    assert_eq!(app.mailer.sent().len(), before);

    // editor cannot see the review queue; the old /search URL redirects to /
    let reviewer = app.sign_in("rev@test.local").await;
    let editor = app.sign_in("ed@test.local").await;
    assert_eq!(app.get("/review", Some(&editor)).await.status, StatusCode::FORBIDDEN);
    assert!(app.get("/review", None).await.status.is_redirection()); // anonymous → login
    let old = app.get("/search?q=wong", None).await;
    assert!(old.status.is_redirection());
    assert_eq!(old.location(), "/?q=wong");

    // nothing is listed or found before the report is accepted
    let r = app.get("/?q=Wong%20Ka%20Ming", None).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.contains("No accepted reports match"));
    assert!(app.get("/", None).await.body.contains("No accepted reports yet"));
    assert_eq!(app.get(&format!("/record/{report_id}"), None).await.status, StatusCode::NOT_FOUND);

    // reviewer sees the reporter and must give a summary to accept
    let detail = app.get(&format!("/review/{report_id}"), Some(&reviewer)).await;
    assert_eq!(detail.status, StatusCode::OK);
    assert!(detail.body.contains("alice@uni.edu"));
    assert_eq!(app.post(&format!("/review/{report_id}/decision"), &[("status", "accepted"), ("summary", "")], Some(&reviewer)).await.status, StatusCode::BAD_REQUEST);
    assert!(
        app.post(
            &format!("/review/{report_id}/decision"),
            &[("status", "accepted"), ("summary", "Confirmed by two independent sources."), ("decision_note", "Reporter is credible.")],
            Some(&reviewer)
        )
        .await
        .status
        .is_redirection()
    );

    // the accepted report is listed and found by fuzzy name, DOI and title, never the reporter
    let list = app.get("/", None).await.body;
    assert!(list.contains("Wong, Ka-Ming"));
    assert!(list.contains("Confirmed by two independent sources."));
    assert!(!list.contains("alice"));
    for q in ["K. M. Wong", "wong ka ming", "10.1000/SPONT.1", "spontaneous generation"] {
        let enc: String = form_urlencoded::byte_serialize(q.as_bytes()).collect();
        let body = app.get(&format!("/?q={enc}"), Some(&editor)).await.body;
        assert!(body.contains("Wong, Ka-Ming"), "query {q:?} should find the record");
        assert!(!body.contains("alice"), "query {q:?} must not leak the reporter");
    }
    assert!(app.get("/?q=Chan%20Tai%20Man", None).await.body.contains("No accepted reports match"));
    let anon = app.get(&format!("/record/{report_id}"), None).await.body;
    assert!(anon.contains("Confirmed by two independent sources."));
    assert!(!anon.contains("audit log"));
    let rec = app.get(&format!("/record/{report_id}"), Some(&editor)).await.body; // signed-in reads are audit-logged
    assert!(rec.contains("Confirmed by two independent sources."));
    assert!(!rec.contains("Alice"));
    assert!(!rec.contains("Reporter is credible"));

    // writes an audit trail
    let actions: Vec<String> = app.db.query("SELECT action FROM audit_log ORDER BY at", &[]).await.unwrap().iter().map(|r| r.get("action")).collect();
    for a in ["report_verified", "login", "user_added", "review_view", "decision_accepted", "search", "record_view"] {
        assert!(actions.iter().any(|x| x == a), "audit should contain {a}: {actions:?}");
    }
    assert_eq!(app.get("/admin/audit", Some(&admin)).await.status, StatusCode::OK);
    assert_eq!(app.get("/admin/audit", Some(&editor)).await.status, StatusCode::FORBIDDEN);

    // disabling a user kills their session
    let id: uuid::Uuid = app.db.one("SELECT id FROM users WHERE email = 'ed@test.local'", &[]).await.unwrap().unwrap().get("id");
    assert!(app.post(&format!("/admin/users/{id}/disable"), &[], Some(&admin)).await.status.is_redirection());
    assert!(app.get("/admin/audit", Some(&editor)).await.status.is_redirection()); // anonymous now → login
}
