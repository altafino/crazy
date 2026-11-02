//! The one place that knows what a secret looks like: profiles refuse them (D40) and
//! session logs redact them (D36).
use std::{borrow::Cow, sync::LazyLock};

use regex::Regex;

static SECRET_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(_KEY|_TOKEN|_SECRET)$|PASSWORD|API_KEY").unwrap());
static TOKEN_SHAPE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(sk-|ghp_|gho_|github_pat_|xox[bap]-)[A-Za-z0-9_-]*|\bAKIA[A-Z0-9]{16}\b")
        .unwrap()
});

const REDACTED: &str = "[redacted]";

/// True for env names or values that look like credentials.
pub(crate) fn looks_secret(name: &str, value: &str) -> bool {
    SECRET_NAME.is_match(name) || TOKEN_SHAPE.is_match(value)
}

/// Replaces every token-shaped substring of `text`.
pub(crate) fn redact(text: &str) -> Cow<'_, str> {
    TOKEN_SHAPE.replace_all(text, REDACTED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_known_token_shapes_and_leaves_the_rest() {
        let line =
            r#"{"text":"key sk-ant-api03-AbC_123 and ghp_abcDEF123 AKIAABCDEFGHIJKLMNOP ok"}"#;
        assert_eq!(
            redact(line),
            r#"{"text":"key [redacted] and [redacted] [redacted] ok"}"#
        );
        assert!(matches!(redact("plain text"), Cow::Borrowed(_)));
    }

    #[test]
    fn names_and_values() {
        assert!(looks_secret("MY_API_KEY", ""));
        assert!(looks_secret("X", "xoxb-123"));
        assert!(!looks_secret("CLAUDE_CONFIG_DIR", "/home/u/.claude"));
    }
}
