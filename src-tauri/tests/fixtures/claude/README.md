# Claude Code CLI transcripts (M2 spike, D26)

Recorded 2026-11-02 against Claude Code **2.1.316** with
`claude -p --input-format stream-json --output-format stream-json --verbose --setting-sources project --strict-mcp-config`
plus the mode flags below, model claude-haiku-5-5. Each line is `{"dir": "in"|"out", "msg": <stream-json message>}`:
`in` = what the host wrote to stdin, `out` = what the CLI printed. Paths are rewritten to `/project` and
`/home/user`; init events are trimmed (tool, plugin, skill lists removed); hook/thinking-token system events dropped.

| File | Flags | What it shows |
|---|---|---|
| host_edit_allow | `--permission-mode manual --permission-prompt-tool stdio` | `control_request` `can_use_tool` (Bash with redirect) answered allow; file written |
| host_bash_allow | same | `can_use_tool` for `mkdir`, allow, `permission_suggestions` shape |
| host_bash_deny | same | deny answer -> `tool_result` is_error with our message; `result.permission_denials` has 1 entry |
| host_plan | `--permission-mode plan --permission-prompt-tool stdio` | plan written to the CLI config dir (outside the project); `ExitPlanMode` arrives as `can_use_tool` with `input.plan`; answering allow lets Claude start coding |
| fallback_accept_edits_write | `--permission-mode acceptEdits --permission-prompts none` | Write allowed without a prompt |
| fallback_accept_edits_bash | same | `touch` in the project ALSO allowed without a prompt (acceptEdits covers file-changing shell commands) |
| no_host_write_denied | `--permission-mode manual`, no prompt tool | nobody answers: Bash redirect and Write are denied automatically and listed in `permission_denials` |

Without `--setting-sources project`, the user's own `~/.claude` allow rules, hooks, plugins and MCP servers apply to the run
(on the recording machine they auto-allowed Bash and Write, so no approval was ever requested).
