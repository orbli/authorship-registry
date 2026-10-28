//! CloudFront-to-origin shared secret. Lets the Lambda Function URL stay public
//! (browser form POSTs work) while refusing requests that bypass the public
//! hostname and hit the raw Function URL.

use subtle::ConstantTimeEq;

pub const ORIGIN_HEADER: &str = "x-origin-verify";

/// True when the request carries the secret CloudFront adds on the way to the origin.
pub fn origin_allowed(header_value: Option<&str>, secret: &str) -> bool {
    match header_value {
        Some(got) if got.len() == secret.len() => got.as_bytes().ct_eq(secret.as_bytes()).into(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_header_passes() {
        assert!(origin_allowed(Some("abc123"), "abc123"));
    }

    #[test]
    fn missing_wrong_and_prefix_fail() {
        assert!(!origin_allowed(None, "abc123"));
        assert!(!origin_allowed(Some("abc124"), "abc123"));
        assert!(!origin_allowed(Some("abc12"), "abc123"));
        assert!(!origin_allowed(Some("abc1234"), "abc123"));
    }
}
