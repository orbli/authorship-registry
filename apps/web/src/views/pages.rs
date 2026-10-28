//! One struct per template. Each holds exactly the data its template renders;
//! the constructor functions keep the handler call sites short.

use askama::Template;
use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use super::{Page, filters, fmt_date};
use crate::auth::User;

// ---------- rows read from the database ----------

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

    pub fn decided(&self) -> bool {
        self.status == "accepted" || self.status == "rejected"
    }

    /// Date the report was confirmed, falling back to when it was filed.
    pub fn reported_on(&self) -> String {
        fmt_date(self.verified_at.or(Some(self.created_at)))
    }
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

    pub fn percent(&self) -> i64 {
        (self.score * 100.0).round() as i64
    }

    /// First 180 characters of the summary.
    pub fn summary_snippet(&self) -> String {
        const N: usize = 180;
        match &self.summary {
            Some(s) if s.chars().count() > N => format!("{}…", s.chars().take(N - 1).collect::<String>().trim_end()),
            Some(s) => s.clone(),
            None => String::new(),
        }
    }
}

pub struct AuditRow {
    pub at: DateTime<Utc>,
    pub actor_email: Option<String>,
    pub action: String,
    pub report_id: Option<Uuid>,
    pub detail: Option<String>,
}

impl AuditRow {
    pub fn short_report_id(&self) -> String {
        self.report_id.map(|id| id.to_string()[..8].to_string()).unwrap_or_default()
    }
}

/// Field values echoed back into the report form after a validation error.
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

// ---------- templates ----------

#[derive(Template)]
#[template(path = "report.html")]
pub struct ReportPage<'a> {
    pub page: Page<'a>,
    pub form: &'a ReportForm,
    pub error: Option<&'a str>,
}

pub fn report_page<'a>(user: Option<&'a User>, form: &'a ReportForm, error: Option<&'a str>) -> ReportPage<'a> {
    ReportPage { page: Page::new("Report", user), form, error }
}

#[derive(Template)]
#[template(path = "message.html")]
pub struct MessagePage<'a> {
    pub page: Page<'a>,
    pub body: &'a str,
    /// Notice style: `ok` or `err`.
    pub kind: &'a str,
}

pub fn message_page<'a>(user: Option<&'a User>, title: &'a str, body: &'a str, kind: &'a str) -> MessagePage<'a> {
    MessagePage { page: Page::new(title, user), body, kind }
}

#[derive(Template)]
#[template(path = "login.html")]
pub struct LoginPage<'a> {
    pub page: Page<'a>,
    pub next: &'a str,
    pub sent: bool,
    /// Present when Google sign-in is configured.
    pub google_href: Option<String>,
    pub error: Option<&'a str>,
}

pub fn login_page<'a>(next: Option<&'a str>, sent: bool, google: bool, denied: Option<&'a str>) -> LoginPage<'a> {
    let next = next.unwrap_or("");
    let google_href = google.then(|| if next.is_empty() { "/auth/google".to_string() } else { format!("/auth/google?next={}", crate::google::urlencode(next)) });
    LoginPage { page: Page::new("Staff sign in", None), next, sent, google_href, error: denied }
}

#[derive(Template)]
#[template(path = "review_list.html")]
pub struct ReviewListPage<'a> {
    pub page: Page<'a>,
    pub reports: &'a [ReportRow],
    pub status: &'a str,
    pub other_statuses: Vec<&'static str>,
}

pub fn review_list_page<'a>(user: &'a User, reports: &'a [ReportRow], status: &'a str) -> ReviewListPage<'a> {
    let other_statuses = ["submitted", "accepted", "rejected"].into_iter().filter(|s| *s != status).collect();
    ReviewListPage { page: Page::new("Review queue", Some(user)), reports, status, other_statuses }
}

#[derive(Template)]
#[template(path = "review_detail.html")]
pub struct ReviewDetailPage<'a> {
    pub page: Page<'a>,
    pub r: &'a ReportRow,
    pub error: Option<&'a str>,
}

pub fn review_detail_page<'a>(user: &'a User, r: &'a ReportRow, error: Option<&'a str>) -> ReviewDetailPage<'a> {
    ReviewDetailPage { page: Page::new("Review report", Some(user)), r, error }
}

#[derive(Template)]
#[template(path = "incidents.html")]
pub struct IncidentsPage<'a> {
    pub page: Page<'a>,
    pub q: &'a str,
    pub hits: &'a [SearchHit],
    /// "Latest 5" or "3 matches".
    pub heading: String,
}

pub fn incidents_page<'a>(user: Option<&'a User>, q: &'a str, hits: &'a [SearchHit]) -> IncidentsPage<'a> {
    let title = if q.is_empty() { "Reported incidents".to_string() } else { format!("Search: {q}") };
    let heading = if q.is_empty() { format!("Latest {}", hits.len()) } else { format!("{} match{}", hits.len(), if hits.len() == 1 { "" } else { "es" }) };
    IncidentsPage { page: Page::new(title, user), q, hits, heading }
}

#[derive(Template)]
#[template(path = "record.html")]
pub struct RecordPage<'a> {
    pub page: Page<'a>,
    pub r: &'a ReportRow,
    /// Other accepted reports about the same name.
    pub related: i64,
}

pub fn record_page<'a>(user: Option<&'a User>, r: &'a ReportRow, related: i64) -> RecordPage<'a> {
    RecordPage { page: Page::new("Record", user), r, related }
}

#[derive(Template)]
#[template(path = "admin_users.html")]
pub struct AdminUsersPage<'a> {
    pub page: Page<'a>,
    pub me: &'a User,
    pub users: &'a [User],
    pub error: Option<&'a str>,
}

pub fn admin_users_page<'a>(me: &'a User, users: &'a [User], error: Option<&'a str>) -> AdminUsersPage<'a> {
    AdminUsersPage { page: Page::new("Users", Some(me)), me, users, error }
}

#[derive(Template)]
#[template(path = "audit.html")]
pub struct AuditPage<'a> {
    pub page: Page<'a>,
    pub rows: &'a [AuditRow],
}

pub fn audit_page<'a>(user: &'a User, rows: &'a [AuditRow]) -> AuditPage<'a> {
    AuditPage { page: Page::new("Audit log", Some(user)), rows }
}
