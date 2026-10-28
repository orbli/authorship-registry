//! View models. The markup itself lives in `templates/*.html` (Askama, Jinja
//! syntax, compiled and type-checked at build time) and the stylesheet in
//! `static/app.css`. This module only carries data into those templates and
//! defines the few filters they use.

pub mod pages;

use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

use crate::auth::{Role, User};

/// The stylesheet, embedded so the binary stays self-contained (Lambda, k8s).
pub const CSS: &str = include_str!("../../static/app.css");

/// Short content hash appended to the stylesheet URL, so browsers may cache it
/// for a long time yet always fetch a new one after a deploy.
pub static CSS_VERSION: LazyLock<String> = LazyLock::new(|| hex::encode(&Sha256::digest(CSS.as_bytes())[..6]));

/// What `layout.html` needs on every page.
pub struct Page<'a> {
    pub title: String,
    pub user: Option<&'a User>,
    pub css_version: &'static str,
}

impl<'a> Page<'a> {
    pub fn new(title: impl Into<String>, user: Option<&'a User>) -> Page<'a> {
        Page { title: title.into(), user, css_version: &CSS_VERSION }
    }

    fn has(&self, role: Role) -> bool {
        self.user.is_some_and(|u| u.role == role || u.role == Role::Admin)
    }

    pub fn can_review(&self) -> bool {
        self.has(Role::Reviewer)
    }

    pub fn is_admin(&self) -> bool {
        self.has(Role::Admin)
    }
}

pub fn fmt_date(d: Option<DateTime<Utc>>) -> String {
    d.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default()
}

/// Filters available in templates as `{{ value|name }}`.
pub mod filters {
    use askama::Values;
    use chrono::{DateTime, Utc};

    #[askama::filter_fn]
    pub fn date(d: &Option<DateTime<Utc>>, _: &dyn Values) -> askama::Result<String> {
        Ok(super::fmt_date(*d))
    }

    /// `Some(s)` → s, `None` → em dash.
    #[askama::filter_fn]
    pub fn or_dash(s: &Option<String>, _: &dyn Values) -> askama::Result<String> {
        Ok(s.clone().unwrap_or_else(|| "—".to_string()))
    }

    /// `Some(s)` → s, `None` → empty.
    #[askama::filter_fn]
    pub fn or_empty(s: &Option<String>, _: &dyn Values) -> askama::Result<String> {
        Ok(s.clone().unwrap_or_default())
    }

    /// `pending_email` → `pending email`.
    #[askama::filter_fn]
    pub fn status_label(s: &str, _: &dyn Values) -> askama::Result<String> {
        Ok(s.replace('_', " "))
    }
}
