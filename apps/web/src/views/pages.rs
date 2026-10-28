//! Every page, as functions returning markup. Ported one to one from the JSX
//! views so the HTML the tests and users see is unchanged.

use chrono::{DateTime, Utc};
use maud::{Markup, html};
use tokio_postgres::Row;
use uuid::Uuid;

use super::layout::{fmt_date, layout};
use crate::auth::{Role, User};

#[derive(Clone, Debug)]
pub struct ReportRow {
    pub id: Uuid,
    pub status: String,
    pub reporter_name: String,
    pub reporter_email: String,
    pub reporter_affiliation: String,
    pub subject_name: String,
    pub subject_name_key: String,
    pub subject_affiliation: Option<String>,
    pub subject_email: Option<String>,
    pub paper_title: String,
    pub paper_doi: Option<String>,
    pub paper_venue: Option<String>,
    pub description: String,
    pub summary: Option<String>,
    pub decision_note: Option<String>,
    pub created_at: DateTime<Utc>,
    pub verified_at: Option<DateTime<Utc>>,
    pub decided_at: Option<DateTime<Utc>>,
}

impl ReportRow {
    pub const COLS: &'static str = "id, status, reporter_name, reporter_email, reporter_affiliation, subject_name, subject_name_key, subject_affiliation, subject_email, paper_title, paper_doi, paper_venue, description, summary, decision_note, decided_by, created_at, verified_at, decided_at";

    pub fn from_row(r: &Row) -> ReportRow {
        ReportRow {
            id: r.get("id"),
            status: r.get("status"),
            reporter_name: r.get("reporter_name"),
            reporter_email: r.get("reporter_email"),
            reporter_affiliation: r.get("reporter_affiliation"),
            subject_name: r.get("subject_name"),
            subject_name_key: r.get("subject_name_key"),
            subject_affiliation: r.get("subject_affiliation"),
            subject_email: r.get("subject_email"),
            paper_title: r.get("paper_title"),
            paper_doi: r.get("paper_doi"),
            paper_venue: r.get("paper_venue"),
            description: r.get("description"),
            summary: r.get("summary"),
            decision_note: r.get("decision_note"),
            created_at: r.get("created_at"),
            verified_at: r.get("verified_at"),
            decided_at: r.get("decided_at"),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ReportForm {
    pub reporter_name: String,
    pub reporter_email: String,
    pub reporter_affiliation: String,
    pub subject_name: String,
    pub subject_affiliation: String,
    pub subject_email: String,
    pub paper_title: String,
    pub paper_doi: String,
    pub paper_venue: String,
    pub description: String,
}

fn err_box(error: Option<&str>) -> Markup {
    html! { @if let Some(e) = error { div class="notice err" { (e) } } }
}

fn dash(s: &Option<String>) -> &str {
    s.as_deref().unwrap_or("—")
}

pub fn report_page(user: Option<&User>, v: &ReportForm, error: Option<&str>) -> Markup {
    layout(
        "Report",
        user,
        html! {
            h1 { "Report undisclosed AI-generated authorship" }
            p class="lead" {
                "Use this form if you have first-hand knowledge that a manuscript or published paper was substantially produced by AI without disclosure. Reports are reviewed by a human team before anything becomes visible to journal editors. You must give your real name and a working email address; we will send you a confirmation link and nothing is filed until you click it. Your identity is never shown to editors."
            }
            (err_box(error))
            form method="post" action="/report" {
                h2 { "About you" }
                label { span class="req" { "Your full name" } input name="reporter_name" required maxlength="200" value=(v.reporter_name); }
                label { span class="req" { "Your email" } input name="reporter_email" type="email" required maxlength="200" value=(v.reporter_email); }
                label { span class="req" { "Your affiliation" } input name="reporter_affiliation" required maxlength="200" value=(v.reporter_affiliation); }
                h2 { "About the author being reported" }
                label { span class="req" { "Author name" } input name="subject_name" required maxlength="200" value=(v.subject_name); }
                label { span { "Author affiliation" } input name="subject_affiliation" maxlength="200" value=(v.subject_affiliation); }
                label { span { "Author email, if known" } input name="subject_email" maxlength="200" value=(v.subject_email); }
                h2 { "About the paper" }
                label { span class="req" { "Paper title" } input name="paper_title" required maxlength="500" value=(v.paper_title); }
                label { span { "DOI or preprint identifier" } input name="paper_doi" maxlength="200" placeholder="10.1234/abcd.5678" value=(v.paper_doi); }
                label { span { "Journal, conference or venue" } input name="paper_venue" maxlength="200" value=(v.paper_venue); }
                label { span class="req" { "What you observed, and how you know" } textarea name="description" required maxlength="8000" { (v.description) } }
                button { "Send confirmation email" }
            }
        },
    )
}

pub fn message_page(user: Option<&User>, title: &str, body: &str, kind: &str) -> Markup {
    layout(
        title,
        user,
        html! {
            h1 { (title) }
            div class=(format!("notice {kind}")) { (body) }
        },
    )
}

pub fn login_page(next: Option<&str>, sent: bool, google: bool, denied: Option<&str>) -> Markup {
    let next = next.unwrap_or("");
    layout(
        "Staff sign in",
        None,
        html! {
            h1 { "Staff sign in" }
            p class="lead" { "For reviewers, editors and administrators. Your address must already be registered by an administrator." }
            (err_box(denied))
            @if sent {
                div class="notice ok" { "If that address belongs to a registered reviewer or editor, a sign-in link is on its way. It expires shortly." }
            } @else {
                @if google {
                    p {
                        a href=(if next.is_empty() { "/auth/google".to_string() } else { format!("/auth/google?next={}", crate::google::urlencode(next)) }) class="button" { "Sign in with Google" }
                        span class="meta" style="margin-left:10px" { "uses the Google account's email; it must match a staff account" }
                    }
                }
                form method="post" action="/login" {
                    input type="hidden" name="next" value=(next);
                    label { span class="req" { "Email" } input name="email" type="email" required autofocus[!google]; }
                    button class=(if google { "secondary" } else { "" }) { "Email me a sign-in link" }
                }
            }
        },
    )
}

fn status_badge(status: &str) -> Markup {
    html! { span class=(format!("badge {status}")) { (status.replace('_', " ")) } }
}

pub fn review_list_page(user: &User, reports: &[ReportRow], status: &str) -> Markup {
    layout(
        "Review queue",
        Some(user),
        html! {
            h1 { "Review queue" }
            p class="lead" {
                "Showing " strong { (status) } " reports. "
                @for s in ["submitted", "accepted", "rejected"].iter().filter(|s| **s != status) {
                    " · " a href=(format!("/review?status={s}")) { (s) }
                }
            }
            @if reports.is_empty() { div class="notice" { "Nothing here." } }
            @for r in reports {
                div class="card" {
                    h3 { a href=(format!("/review/{}", r.id)) { (r.subject_name) } " " (status_badge(&r.status)) }
                    div { (r.paper_title) }
                    div class="meta" {
                        (r.paper_venue.clone().unwrap_or_default())
                        (r.paper_doi.as_ref().map(|d| format!(" · {d}")).unwrap_or_default())
                        " · reported " (fmt_date(r.verified_at.or(Some(r.created_at)))) " by " (r.reporter_name) " (" (r.reporter_affiliation) ")"
                    }
                }
            }
        },
    )
}

pub fn review_detail_page(user: &User, r: &ReportRow, error: Option<&str>) -> Markup {
    let decided = r.status == "accepted" || r.status == "rejected";
    layout(
        "Review report",
        Some(user),
        html! {
            h1 { (r.subject_name) " " (status_badge(&r.status)) }
            (err_box(error))
            div class="card" {
                h3 { "Paper" }
                dl {
                    dt { "Title" } dd { (r.paper_title) }
                    dt { "DOI" } dd { (dash(&r.paper_doi)) }
                    dt { "Venue" } dd { (dash(&r.paper_venue)) }
                    dt { "Author affiliation" } dd { (dash(&r.subject_affiliation)) }
                    dt { "Author email" } dd { (dash(&r.subject_email)) }
                }
            }
            div class="card" {
                h3 { "Reporter (confidential)" }
                dl {
                    dt { "Name" } dd { (r.reporter_name) }
                    dt { "Email" } dd { (r.reporter_email) }
                    dt { "Affiliation" } dd { (r.reporter_affiliation) }
                    dt { "Email verified" } dd { (if r.verified_at.is_some() { fmt_date(r.verified_at) } else { "not yet".into() }) }
                }
            }
            div class="card" {
                h3 { "Report" }
                dd { (r.description) }
            }
            @if decided {
                div class="card" {
                    h3 { "Decision · " (fmt_date(r.decided_at)) }
                    dl {
                        dt { "Editor-facing summary" } dd { (dash(&r.summary)) }
                        dt { "Internal note" } dd { (dash(&r.decision_note)) }
                    }
                }
            }
            h2 { (if r.status == "submitted" { "Decide" } else { "Revise decision" }) }
            form method="post" action=(format!("/review/{}/decision", r.id)) {
                label { span class="req" { "Decision" }
                    select name="status" {
                        option value="accepted" selected[r.status == "accepted"] { "Accept: visible to editors" }
                        option value="rejected" selected[r.status == "rejected"] { "Reject: never shown" }
                    }
                }
                label { span { "Summary shown to editors (required when accepting; never include the reporter)" }
                    textarea name="summary" maxlength="4000" { (r.summary.clone().unwrap_or_default()) }
                }
                label { span { "Internal note (review team only)" } textarea name="decision_note" maxlength="4000" { (r.decision_note.clone().unwrap_or_default()) } }
                button { "Save decision" }
            }
        },
    )
}

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub id: Uuid,
    pub subject_name: String,
    pub subject_name_key: String,
    pub subject_affiliation: Option<String>,
    pub paper_title: String,
    pub paper_doi: Option<String>,
    pub paper_venue: Option<String>,
    pub summary: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
    pub score: f64,
}

impl SearchHit {
    pub const COLS: &'static str = "id, subject_name, subject_name_key, subject_affiliation, paper_title, paper_doi, paper_venue, summary, decided_at";

    pub fn from_row(r: &Row, score: f64) -> SearchHit {
        SearchHit {
            id: r.get("id"),
            subject_name: r.get("subject_name"),
            subject_name_key: r.get("subject_name_key"),
            subject_affiliation: r.get("subject_affiliation"),
            paper_title: r.get("paper_title"),
            paper_doi: r.get("paper_doi"),
            paper_venue: r.get("paper_venue"),
            summary: r.get("summary"),
            decided_at: r.get("decided_at"),
            score,
        }
    }
}

fn snippet(s: &Option<String>, n: usize) -> String {
    match s {
        Some(s) if s.chars().count() > n => format!("{}…", s.chars().take(n - 1).collect::<String>().trim_end()),
        Some(s) => s.clone(),
        None => String::new(),
    }
}

pub fn incidents_page(user: Option<&User>, q: &str, hits: &[SearchHit]) -> Markup {
    let title = if q.is_empty() { "Reported incidents".to_string() } else { format!("Search: {q}") };
    layout(
        &title,
        user,
        html! {
            h1 { "Reported incidents" }
            p class="lead" {
                "Accepted reports of undisclosed AI-generated authorship, vetted by the review team. Search by author name, DOI or paper title. Name matching is fuzzy and ignores word order, so \"Wong Ka Ming\" also finds \"K. M. Wong\". Reporter identities are never shown."
            }
            form method="get" action="/" class="row" {
                label { span { "Author name, DOI or paper title" } input name="q" value=(q) autofocus placeholder="e.g. K. M. Wong, 10.1234/abcd.5678"; }
                button { "Search" }
                @if !q.is_empty() { a href="/" style="align-self:center" { "Clear" } }
            }
            @if !q.is_empty() && hits.is_empty() { div class="notice ok" style="margin-top:16px" { "No accepted reports match \"" (q) "\"." } }
            @if q.is_empty() && hits.is_empty() { div class="notice" style="margin-top:16px" { "No accepted reports yet." } }
            @if !hits.is_empty() {
                h2 { (if q.is_empty() { format!("Latest {}", hits.len()) } else { format!("{} match{}", hits.len(), if hits.len() == 1 { "" } else { "es" }) }) }
                table {
                    thead { tr { th { "Author" } th { "Paper" } th { "Venue" } th { "Accepted" } @if !q.is_empty() { th { "Match" } } } }
                    tbody {
                        @for h in hits {
                            tr {
                                td { a href=(format!("/record/{}", h.id)) { (h.subject_name) } div class="meta" { (h.subject_affiliation.clone().unwrap_or_default()) } }
                                td { (h.paper_title) div class="meta" { (h.paper_doi.clone().unwrap_or_default()) } div class="meta" { (snippet(&h.summary, 180)) } }
                                td { (h.paper_venue.clone().unwrap_or_default()) }
                                td { (fmt_date(h.decided_at)) }
                                @if !q.is_empty() { td { (format!("{}%", (h.score * 100.0).round() as i64)) } }
                            }
                        }
                    }
                }
            }
        },
    )
}

pub fn record_page(user: Option<&User>, r: &ReportRow, related: i64) -> Markup {
    layout(
        "Record",
        user,
        html! {
            h1 { (r.subject_name) }
            p class="meta" {
                "Accepted " (fmt_date(r.decided_at))
                @if related > 0 { " · " (related) " other accepted report" (if related > 1 { "s" } else { "" }) " about an author with this name" }
            }
            div class="card" {
                dl {
                    dt { "Affiliation" } dd { (dash(&r.subject_affiliation)) }
                    dt { "Paper" } dd { (r.paper_title) }
                    dt { "DOI" } dd { (dash(&r.paper_doi)) }
                    dt { "Venue" } dd { (dash(&r.paper_venue)) }
                }
            }
            div class="card" {
                h3 { "Summary from the review team" }
                dd { (r.summary.clone().unwrap_or_default()) }
            }
            @if user.is_some() { p class="meta" { "This view has been recorded in the audit log." } }
            p class="meta" { a href="/" { "Back to incidents" } " · " a href="/report" { "File a report" } }
        },
    )
}

pub fn admin_users_page(user: &User, users: &[User], error: Option<&str>) -> Markup {
    layout(
        "Users",
        Some(user),
        html! {
            h1 { "Staff accounts" }
            (err_box(error))
            table {
                thead { tr { th { "Email" } th { "Name" } th { "Role" } th { "Status" } th {} } }
                tbody {
                    @for u in users {
                        tr {
                            td { (u.email) } td { (u.name) } td { (u.role.as_str()) } td { (if u.disabled { "disabled" } else { "active" }) }
                            td {
                                @if u.id != user.id {
                                    form method="post" action=(format!("/admin/users/{}/{}", u.id, if u.disabled { "enable" } else { "disable" })) {
                                        button class="secondary inline" { (if u.disabled { "Enable" } else { "Disable" }) }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            h2 { "Add staff" }
            form method="post" action="/admin/users" {
                label { span class="req" { "Email" } input name="email" type="email" required; }
                label { span class="req" { "Name" } input name="name" required maxlength="200"; }
                label { span class="req" { "Role" }
                    select name="role" {
                        option value=(Role::Editor.as_str()) { "editor: search accepted records" }
                        option value=(Role::Reviewer.as_str()) { "reviewer: vet reports, see reporters" }
                        option value=(Role::Admin.as_str()) { "admin: everything" }
                    }
                }
                button { "Add" }
            }
        },
    )
}

pub struct AuditRow {
    pub at: DateTime<Utc>,
    pub actor_email: Option<String>,
    pub action: String,
    pub report_id: Option<Uuid>,
    pub detail: Option<String>,
}

pub fn audit_page(user: &User, rows: &[AuditRow]) -> Markup {
    layout(
        "Audit log",
        Some(user),
        html! {
            h1 { "Audit log" }
            table {
                thead { tr { th { "When (UTC)" } th { "Who" } th { "Action" } th { "Report" } th { "Detail" } } }
                tbody {
                    @for r in rows {
                        tr {
                            td { (r.at.format("%Y-%m-%d %H:%M:%S")) }
                            td { (r.actor_email.clone().unwrap_or_default()) }
                            td { (r.action) }
                            td { @if let Some(id) = r.report_id { a href=(format!("/review/{id}")) { (id.to_string()[..8]) } } }
                            td { (r.detail.clone().unwrap_or_default()) }
                        }
                    }
                }
            }
        },
    )
}
