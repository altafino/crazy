//! How network git commands run without a terminal (M5-D5, M5-D11). Shared by the git remote
//! operations (RepoService) and the gh service (create, clone): one place decides how Studio
//! authenticates and when a stalled transfer gives up.
//!
//! Nothing here writes the user's git config: everything is per command, through the
//! environment and `-c` arguments.
#![allow(dead_code)] // M5 lanes A and B call these; remove when they do

use std::path::Path;

/// ssh never prompts (no passphrase or host-key question nobody can see), gives up on an
/// unreachable host after 15 s, and on a silent connection after 4 × 15 s.
pub(crate) const SSH_COMMAND: &str =
    "ssh -o BatchMode=yes -o ConnectTimeout=15 -o ServerAliveInterval=15 -o ServerAliveCountMax=4";

/// The environment for a network git command: `base` (already the doctor's host env) plus
/// no terminal prompts and the batch-mode ssh command. A `GIT_SSH_COMMAND` the user set is
/// kept, and so is their `core.sshCommand` when the caller found one (`user_ssh_config`).
pub(crate) fn network_env(
    base: &[(String, String)],
    user_ssh_config: bool,
) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = base
        .iter()
        .filter(|(k, _)| k != "GIT_TERMINAL_PROMPT")
        .cloned()
        .collect();
    env.push(("GIT_TERMINAL_PROMPT".into(), "0".into()));
    let user_env = env
        .iter()
        .any(|(k, v)| k == "GIT_SSH_COMMAND" && !v.is_empty());
    if !user_env && !user_ssh_config {
        env.push(("GIT_SSH_COMMAND".into(), SSH_COMMAND.into()));
    }
    env
}

/// `-c` arguments that go before the git subcommand: GitHub HTTPS credentials come from
/// `gh auth git-credential` (added after any helper the user configured), and an HTTPS
/// transfer slower than 1 KB/s for 60 s is aborted. Without a resolved `gh`, only the stall
/// settings are added.
pub(crate) fn network_config_args(gh: Option<&Path>) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(gh) = gh {
        args.push("-c".into());
        args.push(format!(
            "credential.https://github.com.helper=!'{}' auth git-credential",
            gh.display().to_string().replace('\'', r"'\''")
        ));
    }
    for kv in ["http.lowSpeedLimit=1000", "http.lowSpeedTime=60"] {
        args.push("-c".into());
        args.push(kv.into());
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect()
    }

    #[test]
    fn adds_batch_ssh_and_no_prompt() {
        let e = network_env(
            &env(&[("PATH", "/usr/bin"), ("GIT_TERMINAL_PROMPT", "1")]),
            false,
        );
        assert!(e.contains(&("GIT_TERMINAL_PROMPT".into(), "0".into())));
        assert_eq!(
            e.iter().filter(|(k, _)| k == "GIT_TERMINAL_PROMPT").count(),
            1
        );
        assert!(e.contains(&("GIT_SSH_COMMAND".into(), SSH_COMMAND.into())));
        assert!(SSH_COMMAND.contains("BatchMode=yes"));
    }

    #[test]
    fn keeps_the_users_ssh_command() {
        let mine = env(&[("GIT_SSH_COMMAND", "ssh -i ~/.ssh/work")]);
        let e = network_env(&mine, false);
        assert_eq!(
            e.iter()
                .filter(|(k, _)| k == "GIT_SSH_COMMAND")
                .collect::<Vec<_>>(),
            vec![&(
                "GIT_SSH_COMMAND".to_string(),
                "ssh -i ~/.ssh/work".to_string()
            )]
        );
        assert!(
            !network_env(&env(&[]), true)
                .iter()
                .any(|(k, _)| k == "GIT_SSH_COMMAND")
        );
    }

    #[test]
    fn config_args_add_gh_helper_and_stall_limits() {
        let a = network_config_args(Some(Path::new("/usr/bin/gh")));
        assert_eq!(a[0], "-c");
        assert_eq!(
            a[1],
            "credential.https://github.com.helper=!'/usr/bin/gh' auth git-credential"
        );
        assert!(a.contains(&"http.lowSpeedTime=60".to_string()));
        assert_eq!(network_config_args(None).len(), 4);
    }
}
