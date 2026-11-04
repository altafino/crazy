//! The one place that knows what a secret looks like: profiles refuse them (D40), session
//! logs redact them (D36), commit scans flag them (M3-D17) and git/gh output is redacted (M5).
use std::{borrow::Cow, sync::LazyLock};

use regex::Regex;

static SECRET_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(_KEY|_TOKEN|_SECRET)$|PASSWORD|API_KEY").unwrap());

/// One secret shape. `scan` is strict (few false positives, for commit warnings) and holds
/// the token in capture group `group`; `redact` is the looser prefix form that also catches
/// truncated tokens in logs. `redact: None` rows are rewritten by the URL rule instead.
pub(crate) struct Pattern {
    pub(crate) kind: &'static str,
    pub(crate) scan: Regex,
    pub(crate) group: usize,
    redact: Option<Regex>,
}

/// The pattern table (R8).
pub(crate) static PATTERNS: LazyLock<Vec<Pattern>> = LazyLock::new(|| {
    // (kind, scan regex, scan group, redact regex)
    let rows: [(&str, &str, usize, Option<&str>); 10] = [
        (
            "private key",
            r"-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY(?: BLOCK)?-----",
            0,
            Some(r"-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY(?: BLOCK)?-----"),
        ),
        (
            "AWS access key",
            r"(?:^|[^A-Za-z0-9])(AKIA[0-9A-Z]{16})(?:[^A-Za-z0-9]|$)",
            1,
            Some(r"\bAKIA[A-Z0-9]{16}\b"),
        ),
        (
            // ghp_ gho_ ghu_ ghs_ ghr_
            "GitHub token",
            r"(?:^|[^A-Za-z0-9_])(gh[pousr]_[A-Za-z0-9]{36,})",
            1,
            Some(r"\bgh[pousr]_[A-Za-z0-9]*"),
        ),
        (
            "GitHub token",
            r"(?:^|[^A-Za-z0-9_])(github_pat_[A-Za-z0-9_]{22,})",
            1,
            Some(r"\bgithub_pat_[A-Za-z0-9_]*"),
        ),
        (
            "Anthropic key",
            r"(?:^|[^A-Za-z0-9])(sk-ant-[A-Za-z0-9_-]{20,})",
            1,
            Some(r"\bsk-ant-[A-Za-z0-9_-]*"),
        ),
        (
            "API key",
            r"(?:^|[^A-Za-z0-9_-])(sk-(?:proj-)?[A-Za-z0-9_]{32,})",
            1,
            Some(r"\bsk-[A-Za-z0-9_-]*"),
        ),
        (
            "Slack token",
            r"(?:^|[^A-Za-z0-9])(xox[baprs]-[A-Za-z0-9-]{10,})",
            1,
            Some(r"\bxox[baprs]-[A-Za-z0-9-]*"),
        ),
        (
            "Google API key",
            r"(?:^|[^A-Za-z0-9_-])(AIza[0-9A-Za-z_-]{35})(?:[^0-9A-Za-z_-]|$)",
            1,
            Some(r"\bAIza[0-9A-Za-z_-]{35}"),
        ),
        // scheme://user:password@host
        ("URL credentials", r"://[^\s/@:]+:([^\s/@]+)@", 1, None),
        // scheme://token@host (a long userinfo; `git@host` is not a secret)
        ("URL credentials", r"://([A-Za-z0-9_.~-]{20,})@", 1, None),
    ];
    rows.into_iter()
        .map(|(kind, scan, group, redact)| Pattern {
            kind,
            scan: Regex::new(scan).expect("static pattern"),
            group,
            redact: redact.map(|r| Regex::new(r).expect("static pattern")),
        })
        .collect()
});

/// `scheme://userinfo@` where userinfo is `user:password` or a bare token.
static URL_CREDENTIALS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([A-Za-z][A-Za-z0-9+.-]*://)[^\s/@]+@").unwrap());

const REDACTED: &str = "[redacted]";

/// True for env names or values that look like credentials.
pub(crate) fn looks_secret(name: &str, value: &str) -> bool {
    SECRET_NAME.is_match(name)
        || PATTERNS
            .iter()
            .any(|p| p.redact.as_ref().unwrap_or(&p.scan).is_match(value))
}

/// Replaces every token-shaped substring of `text` and the credentials of URLs
/// (`scheme://user:token@host` becomes `scheme://[redacted]@host`).
pub(crate) fn redact(text: &str) -> Cow<'_, str> {
    let mut out = Cow::Borrowed(text);
    let url_rule = format!("${{1}}{REDACTED}@");
    let rules = PATTERNS
        .iter()
        .filter_map(|p| p.redact.as_ref().map(|re| (re, REDACTED)))
        .chain([(&*URL_CREDENTIALS, url_rule.as_str())]);
    for (re, with) in rules {
        let Cow::Owned(next) = re.replace_all(&out, with) else {
            continue;
        };
        out = Cow::Owned(next);
    }
    out
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

    #[test]
    fn table_rows_redact_and_scan() {
        let ghs = format!("ghs_{}", "a1B2".repeat(9));
        let ghu = format!("ghu_{}", "a1B2".repeat(9));
        let ghr = format!("ghr_{}", "a1B2".repeat(9));
        let aiza = "AIzaSyA1234567890abcdefghijklmnopqrstuv";
        let pem = "-----BEGIN OPENSSH PRIVATE KEY-----";
        let rows = [
            (format!("t {ghs} x"), "GitHub token"),
            (format!("t {ghu} x"), "GitHub token"),
            (format!("t {ghr} x"), "GitHub token"),
            (format!("k = {aiza} x"), "Google API key"),
            (pem.to_owned(), "private key"),
            (
                "clone https://me:hunter2pw@host/r.git".to_owned(),
                "URL credentials",
            ),
            (
                "clone https://abcdefghij0123456789ABCD@host/r.git".to_owned(),
                "URL credentials",
            ),
        ];
        for (line, kind) in rows {
            let red = redact(&line);
            assert!(red.contains(REDACTED), "{line} -> {red}");
            for secret in [
                &ghs,
                &ghu,
                &ghr,
                aiza,
                "hunter2pw",
                "abcdefghij0123456789ABCD",
            ] {
                assert!(!red.contains(secret), "{line} -> {red}");
            }
            assert!(
                PATTERNS
                    .iter()
                    .any(|p| p.kind == kind && p.scan.is_match(&line)),
                "{line}"
            );
        }
        assert!(!redact(pem).contains("PRIVATE KEY"));
    }

    #[test]
    fn url_credentials_keep_scheme_and_host() {
        assert_eq!(
            redact("fatal: https://user:tok3n@github.com/o/r.git/"),
            "fatal: https://[redacted]@github.com/o/r.git/"
        );
        assert_eq!(
            redact("https://ghp_abc@github.com/o/r"),
            "https://[redacted]@github.com/o/r"
        );
        assert!(matches!(redact("https://github.com/o/r"), Cow::Borrowed(_)));
        assert!(!looks_secret("X", "ssh://git@github.com/o/r"));
        assert!(looks_secret("X", "https://u:p@h/"));
        assert!(looks_secret("X", &format!("ghs_{}", "a".repeat(36))));
    }
}
