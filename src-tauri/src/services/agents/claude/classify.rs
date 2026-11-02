//! The run-outcome classifier (PRD F05): one user-facing message per error kind.
use std::sync::LazyLock;

use regex::Regex;

use super::stream::{ResultInfo, truncate};
use crate::services::agents::Outcome;

/// Everything known about how a run ended.
#[derive(Default)]
pub(super) struct Facts<'a> {
    pub not_installed: bool,
    pub cancelled: bool,
    pub timed_out: bool,
    pub handshake_failed: bool,
    pub exit_code: Option<i32>,
    pub result: Option<&'a ResultInfo>,
    pub stderr: &'a str,
}

/// Text patterns found in stderr or the result text, first match wins. Expired comes
/// before unauthenticated because both messages end in "run /login".
static PATTERNS: LazyLock<Vec<(Regex, Outcome, &'static str)>> = LazyLock::new(|| {
    let row = |re: &str, o, msg| (Regex::new(re).unwrap(), o, msg);
    vec![
        row(
            r"(?i)oauth token (has )?expired|(login|session|token) (has )?expired",
            Outcome::ExpiredLogin,
            "Your Claude login has expired. Run `claude /login` in a terminal, then try again.",
        ),
        row(
            r"(?i)invalid api key|please run /login|not logged in|invalid x-api-key|authentication_error",
            Outcome::Unauthenticated,
            "Claude is not logged in. Run `claude /login` in a terminal, then try again.",
        ),
        row(
            r"(?i)command not found|error while loading shared libraries|cannot find module",
            Outcome::MissingDependency,
            "Claude could not start a tool it needs (such as Node or git). Run the environment doctor and fix what it reports.",
        ),
    ]
});

pub(super) fn classify(f: &Facts) -> (Outcome, Option<String>) {
    let said = |o: Outcome, m: &str| (o, Some(m.to_owned()));
    if f.not_installed {
        return said(
            Outcome::NotInstalled,
            "The Claude CLI was not found. Install it or fix the executable in Settings > AI Agents.",
        );
    }
    if f.cancelled {
        return said(Outcome::Cancelled, "The run was cancelled.");
    }
    if f.timed_out {
        return said(
            Outcome::Timeout,
            "The agent produced no output for too long and was stopped.",
        );
    }
    if f.handshake_failed {
        return said(
            Outcome::IncompatibleCli,
            "This Claude CLI did not answer Studio's live protocol. Update Studio or the CLI, then try again.",
        );
    }
    let result_ok = f
        .result
        .is_some_and(|r| !r.is_error && r.subtype == "success");
    if result_ok && f.exit_code == Some(0) {
        return (Outcome::Success, None);
    }

    let result_text = f.result.map_or("", |r| r.text.as_str());
    let haystack = format!("{result_text}\n{}", f.stderr);
    if let Some((_, o, msg)) = PATTERNS.iter().find(|(re, ..)| re.is_match(&haystack)) {
        return said(*o, msg);
    }
    if f.result
        .is_some_and(|r| r.subtype == "error_max_budget_usd")
    {
        return said(
            Outcome::BudgetExceeded,
            "The run stopped because it reached its budget limit.",
        );
    }
    let detail = if !result_text.trim().is_empty() {
        result_text.trim().to_owned()
    } else if let Some(line) = f.stderr.lines().rev().find(|l| !l.trim().is_empty()) {
        line.trim().to_owned()
    } else if f.result.is_none() {
        "The agent ended without a result.".to_owned()
    } else {
        "The agent reported an error.".to_owned()
    };
    let code = f.exit_code.map_or_else(
        || "killed by a signal".to_owned(),
        |c| format!("exit code {c}"),
    );
    (
        Outcome::Failed,
        Some(truncate(&format!("{detail} ({code})"), 1000)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn res(is_error: bool, subtype: &str, text: &str) -> ResultInfo {
        ResultInfo {
            is_error,
            subtype: subtype.into(),
            text: text.into(),
            session_id: None,
            denials: Vec::new(),
        }
    }

    #[test]
    fn one_row_per_outcome() {
        let ok = res(false, "success", "done");
        let bad = res(true, "success", "it broke");
        let budget = res(true, "error_max_budget_usd", "");
        let login = res(true, "success", "Invalid API key · Please run /login");
        let expired = res(
            true,
            "success",
            "OAuth token has expired. Please run /login",
        );
        let cases: Vec<(&str, Facts, Outcome)> = vec![
            (
                "success",
                Facts {
                    exit_code: Some(0),
                    result: Some(&ok),
                    ..Default::default()
                },
                Outcome::Success,
            ),
            (
                "not installed",
                Facts {
                    not_installed: true,
                    ..Default::default()
                },
                Outcome::NotInstalled,
            ),
            (
                "unauthenticated (result)",
                Facts {
                    exit_code: Some(1),
                    result: Some(&login),
                    ..Default::default()
                },
                Outcome::Unauthenticated,
            ),
            (
                "unauthenticated (stderr)",
                Facts {
                    exit_code: Some(1),
                    stderr: "Invalid API key",
                    ..Default::default()
                },
                Outcome::Unauthenticated,
            ),
            (
                "expired login",
                Facts {
                    exit_code: Some(1),
                    result: Some(&expired),
                    ..Default::default()
                },
                Outcome::ExpiredLogin,
            ),
            (
                "incompatible",
                Facts {
                    handshake_failed: true,
                    ..Default::default()
                },
                Outcome::IncompatibleCli,
            ),
            (
                "missing dependency",
                Facts {
                    exit_code: Some(127),
                    stderr: "sh: node: command not found",
                    ..Default::default()
                },
                Outcome::MissingDependency,
            ),
            (
                "timeout",
                Facts {
                    timed_out: true,
                    ..Default::default()
                },
                Outcome::Timeout,
            ),
            (
                "cancelled",
                Facts {
                    cancelled: true,
                    exit_code: None,
                    ..Default::default()
                },
                Outcome::Cancelled,
            ),
            (
                "budget",
                Facts {
                    exit_code: Some(1),
                    result: Some(&budget),
                    ..Default::default()
                },
                Outcome::BudgetExceeded,
            ),
            (
                "is_error",
                Facts {
                    exit_code: Some(0),
                    result: Some(&bad),
                    ..Default::default()
                },
                Outcome::Failed,
            ),
            (
                "non-zero exit",
                Facts {
                    exit_code: Some(3),
                    stderr: "boom\n",
                    ..Default::default()
                },
                Outcome::Failed,
            ),
            (
                "success result but non-zero exit",
                Facts {
                    exit_code: Some(2),
                    result: Some(&ok),
                    ..Default::default()
                },
                Outcome::Failed,
            ),
            (
                "no result at all",
                Facts {
                    exit_code: Some(0),
                    ..Default::default()
                },
                Outcome::Failed,
            ),
        ];
        for (name, facts, want) in cases {
            let (got, msg) = classify(&facts);
            assert_eq!(got, want, "{name}");
            assert_eq!(msg.is_none(), want == Outcome::Success, "{name}: message");
        }
    }

    #[test]
    fn failure_message_carries_the_reason() {
        let (_, msg) = classify(&Facts {
            exit_code: Some(3),
            stderr: "first\nboom\n",
            ..Default::default()
        });
        assert_eq!(msg.unwrap(), "boom (exit code 3)");
    }
}
