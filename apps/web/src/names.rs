//! Name normalisation and fuzzy matching done in the application, not the
//! database. Aurora DSQL has no pg_trgm, and at the scale of this registry
//! (thousands of records) scoring candidates in memory is instant anyway.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use unicode_normalization::UnicodeNormalization;

fn is_cjk(c: char) -> bool {
    matches!(c, '\u{3040}'..='\u{30FF}' | '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}')
}

fn has_cjk(s: &str) -> bool {
    s.chars().any(is_cjk)
}

/// Lowercase, strip diacritics and punctuation, sort tokens so word order is irrelevant.
pub fn name_key(input: &str) -> String {
    let stripped: String = input
        .nfkd()
        .filter(|c| !('\u{0300}'..='\u{036F}').contains(c)) // combining diacritical marks
        .collect::<String>()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c.is_whitespace() { c } else { ' ' })
        .collect();
    let base = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    if has_cjk(&base) {
        return base.chars().filter(|c| !c.is_whitespace()).collect();
    }
    let mut tokens: Vec<&str> = base.split(' ').filter(|t| !t.is_empty()).collect();
    tokens.sort_unstable();
    tokens.join(" ")
}

pub fn trigrams(s: &str) -> HashSet<String> {
    let padded: Vec<char> = format!("  {s} ").chars().collect();
    let mut out = HashSet::new();
    for w in padded.windows(3) {
        out.insert(w.iter().collect::<String>());
    }
    out
}

/// Jaccard similarity over trigrams, same idea as pg_trgm's similarity().
pub fn trigram_similarity(a: &str, b: &str) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    let ta = trigrams(a);
    let tb = trigrams(b);
    let inter = ta.iter().filter(|t| tb.contains(*t)).count();
    let union = ta.len() + tb.len() - inter;
    if union == 0 { 0.0 } else { inter as f64 / union as f64 }
}

/// Fraction of query tokens that match a candidate token exactly or as a prefix.
/// Catches "K. M. Wong" vs "Wong Ka Ming" where trigram similarity is poor.
pub fn token_match(query_key: &str, candidate_key: &str) -> f64 {
    let q: Vec<&str> = query_key.split(' ').filter(|t| !t.is_empty()).collect();
    let c: Vec<&str> = candidate_key.split(' ').filter(|t| !t.is_empty()).collect();
    if q.is_empty() || c.is_empty() {
        return 0.0;
    }
    let hits = q.iter().filter(|qt| c.iter().any(|ct| ct == *qt || if qt.chars().count() == 1 { ct.starts_with(*qt) } else { ct.starts_with(*qt) || qt.starts_with(ct) })).count();
    hits as f64 / q.len() as f64
}

pub fn name_score(query: &str, candidate_key: &str) -> f64 {
    let qk = name_key(query);
    if qk.is_empty() {
        return 0.0;
    }
    if qk == candidate_key {
        return 1.0;
    }
    if has_cjk(&qk) || has_cjk(candidate_key) {
        return if candidate_key.contains(&qk) || qk.contains(candidate_key) { 0.9 } else { trigram_similarity(&qk, candidate_key) };
    }
    trigram_similarity(&qk, candidate_key).max(token_match(&qk, candidate_key) * 0.85)
}

static DOI_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b10\.\d{4,9}/\S+").unwrap());
static DOI_PREFIX_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^https?://(dx\.)?doi\.org/").unwrap());
static DOI_LABEL_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^doi:\s*").unwrap());

pub fn looks_like_doi(q: &str) -> bool {
    DOI_RE.is_match(q.trim())
}

pub fn normalize_doi(q: &str) -> String {
    let s = q.trim().to_lowercase();
    let s = DOI_PREFIX_RE.replace(&s, "");
    DOI_LABEL_RE.replace(&s, "").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_key_ignores_case_punctuation_diacritics_and_order() {
        assert_eq!(name_key("Wong, Ka-Ming"), "ka ming wong");
        assert_eq!(name_key("ka ming WONG"), "ka ming wong");
        assert_eq!(name_key("José Núñez"), "jose nunez");
        assert_eq!(name_key("María José García-López"), "garcia jose lopez maria");
    }

    #[test]
    fn name_key_keeps_cjk_whole() {
        assert_eq!(name_key("黃 家明"), "黃家明");
    }

    #[test]
    fn name_score_behaves_like_the_original() {
        let key = name_key("Wong Ka Ming");
        assert_eq!(name_score("ka-ming wong", &key), 1.0);
        assert!(name_score("K. M. Wong", &key) > 0.5, "initials match tokens by prefix");
        assert!(name_score("Wong Ka Mng", &key) > 0.4, "typos still score");
        assert!(name_score("Chan Tai Man", &key) < 0.35, "unrelated names score low");
        assert!(name_score("家明", &name_key("黃家明")) > 0.8, "CJK substring");
    }

    #[test]
    fn doi_detection_and_normalisation() {
        assert!(looks_like_doi("https://doi.org/10.1000/XYZ.123"));
        assert!(!looks_like_doi("Wong"));
        assert_eq!(normalize_doi("https://doi.org/10.1000/XYZ.123"), "10.1000/xyz.123");
        assert_eq!(normalize_doi("doi: 10.1000/XYZ"), "10.1000/xyz");
    }
}
