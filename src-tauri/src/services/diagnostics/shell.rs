//! Login-shell capture of PATH and tool aliases (D9).
use super::{
    AliasInfo, ShellEnv, ShellStatus,
    spawn::{RunError, run_capped},
};
use std::{path::Path, time::Duration};

const PATH_BEGIN: &str = "__AAS_PATH_BEGIN__";
const PATH_END: &str = "__AAS_PATH_END__";
const ALIAS_END: &str = "__AAS_ALIAS_END__";
/// Tools whose aliases we look for; keep in sync with the `alias` line in `SCRIPT`.
const ALIAS_TOOLS: &[&str] = &["claude", "git", "node", "pnpm"];
// POSIX printf + alias: works in zsh and bash; stderr of `alias` is hidden when nothing is defined.
const SCRIPT: &str = "printf '\\n__AAS_PATH_BEGIN__%s__AAS_PATH_END__\\n' \"$PATH\"; \
    alias claude git node pnpm 2>/dev/null; printf '__AAS_ALIAS_END__\\n'";

pub(super) async fn capture(shell: &Path, env: &[(String, String)], timeout: Duration) -> ShellEnv {
    let failed = |reason: String| ShellEnv {
        path: None,
        aliases: vec![],
        status: ShellStatus::Failed { reason },
    };
    match run_capped(shell, &["-lic", SCRIPT], env, timeout).await {
        Err(RunError::TimedOut(_)) => ShellEnv {
            path: None,
            aliases: vec![],
            status: ShellStatus::TimedOut,
        },
        Err(e) => failed(e.to_string()),
        // Interactive shells often exit non-zero from rc files; the markers are what counts.
        Ok(out) => parse(&out.stdout).unwrap_or_else(|| {
            failed(format!(
                "no PATH markers in shell output (exit {})",
                out.status
            ))
        }),
    }
}

fn between<'a>(s: &'a str, begin: &str, end: &str) -> Option<&'a str> {
    let from = s.find(begin)? + begin.len();
    let len = s[from..].find(end)?;
    Some(&s[from..from + len])
}

fn parse(stdout: &str) -> Option<ShellEnv> {
    let path = between(stdout, PATH_BEGIN, PATH_END)?;
    let after = &stdout[stdout.find(PATH_END)? + PATH_END.len()..];
    let aliases = after
        .split(ALIAS_END)
        .next()
        .unwrap_or("")
        .lines()
        .filter_map(parse_alias)
        .collect();
    Some(ShellEnv {
        path: Some(path.to_string()),
        aliases,
        status: ShellStatus::Ok,
    })
}

/// Accepts zsh (`name=value`) and bash (`alias name='value'`) output.
fn parse_alias(line: &str) -> Option<AliasInfo> {
    let line = line.trim();
    let line = line.strip_prefix("alias ").unwrap_or(line);
    let (tool, value) = line.split_once('=')?;
    if !ALIAS_TOOLS.contains(&tool) {
        return None;
    }
    let definition = ['\'', '"']
        .iter()
        .find_map(|q| value.strip_prefix(*q)?.strip_suffix(*q))
        .unwrap_or(value)
        .to_string();
    let env_assignments = definition
        .split_whitespace()
        .take_while(|w| is_assignment(w))
        .map(str::to_string)
        .collect();
    Some(AliasInfo {
        tool: tool.to_string(),
        definition,
        env_assignments,
    })
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        name.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_between_markers_ignoring_noise() {
        let out = "p10k instant prompt\n\n__AAS_PATH_BEGIN__/a:/b__AAS_PATH_END__\nclaude='CLAUDE_CONFIG_DIR=~/.claude-a claude'\nfoo=bar\n__AAS_ALIAS_END__\nbye";
        let env = parse(out).unwrap();
        assert_eq!(env.path.as_deref(), Some("/a:/b"));
        assert_eq!(env.aliases.len(), 1);
        assert_eq!(
            env.aliases[0].env_assignments,
            ["CLAUDE_CONFIG_DIR=~/.claude-a"]
        );
    }

    #[test]
    fn parses_bash_style_and_plain_aliases() {
        let a = parse_alias("alias git='git -c color.ui=always'").unwrap();
        assert_eq!(a.definition, "git -c color.ui=always");
        assert!(a.env_assignments.is_empty());
        assert!(parse_alias("alias ls='ls --color'").is_none());
    }

    #[test]
    fn no_markers_is_none() {
        assert!(parse("nothing here").is_none());
    }
}
