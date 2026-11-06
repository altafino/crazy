//! The project plan: `.ai-app-studio/plan.md`, a Markdown file with `- [ ]` checklist lines.
//! `parse` is the only place that understands the format; the UI never parses tasks itself.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use specta::Type;

use super::ProjectError;
use crate::platform::time::rfc3339;

pub(crate) const PLAN_FILE: &str = ".ai-app-studio/plan.md";
const NO_TASKS: &str = "This plan has no checklist lines (- [ ]).";
const GONE: &str = "This task is no longer in the plan.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanTask {
    pub text: String,
    /// Lines indented under the task, "" when there are none.
    pub detail: String,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Plan {
    /// Markdown before the first task.
    pub summary: String,
    pub tasks: Vec<PlanTask>,
    /// The file's modification time (RFC 3339 UTC); None for a plan that was only parsed.
    pub saved_at: Option<String>,
}

/// A task plus where its `[ ]` character sits in the text.
struct Found {
    task: PlanTask,
    mark: usize,
}

/// `- [ ] title` with 0-3 leading spaces. Returns (mark offset in `line`, done, title).
fn task_line(line: &str) -> Option<(usize, bool, &str)> {
    let b = line.as_bytes();
    let indent = b.iter().take_while(|&&c| c == b' ').count();
    if indent > 3 || b.len() < indent + 6 {
        return None;
    }
    let r = &b[indent..];
    if !matches!(r[0], b'-' | b'*' | b'+')
        || r[1] != b' '
        || r[2] != b'['
        || !matches!(r[3], b' ' | b'x' | b'X')
        || r[4] != b']'
        || r[5] != b' '
    {
        return None;
    }
    let title = line[indent + 6..].trim();
    (!title.is_empty()).then_some((indent + 3, r[3] != b' ', title))
}

fn fence(line: &str) -> Option<u8> {
    let t = line.trim_start().as_bytes();
    (t.len() >= 3 && (t.starts_with(b"```") || t.starts_with(b"~~~"))).then(|| t[0])
}

fn scan(text: &str) -> (String, Vec<Found>) {
    let mut summary: Vec<&str> = Vec::new();
    let mut found: Vec<Found> = Vec::new();
    let mut detail: Vec<&str> = Vec::new();
    let mut open = false;
    let mut in_fence: Option<u8> = None;
    let mut offset = 0;
    for raw in text.split_inclusive('\n') {
        let start = offset;
        offset += raw.len();
        let line = raw.trim_end_matches(['\n', '\r']);
        let indented = line.starts_with("  ") || line.starts_with('\t');
        let was_fenced = in_fence.is_some();
        if let Some(c) = fence(line) {
            match in_fence {
                None => in_fence = Some(c),
                Some(open_c) if open_c == c => in_fence = None,
                Some(_) => {}
            }
        }
        if open && (indented || line.trim().is_empty()) {
            if !line.trim().is_empty() {
                let cut = if line.starts_with('\t') { 1 } else { 2 };
                detail.push(line[cut..].trim_end());
            }
            continue;
        }
        if open {
            open = false;
            flush(&mut found, &mut detail);
        }
        if !was_fenced && let Some((mark, done, title)) = task_line(line) {
            found.push(Found {
                task: PlanTask {
                    text: title.into(),
                    detail: String::new(),
                    done,
                },
                mark: start + mark,
            });
            open = true;
        } else if found.is_empty() {
            summary.push(line);
        }
    }
    flush(&mut found, &mut detail);
    (summary.join("\n").trim().into(), found)
}

fn flush(found: &mut [Found], detail: &mut Vec<&str>) {
    if let Some(last) = found.last_mut()
        && last.task.detail.is_empty()
    {
        last.task.detail = detail.join("\n").trim().into();
    }
    detail.clear();
}

/// The plan in `text`. `saved_at` is None.
pub(crate) fn parse(text: &str) -> Plan {
    let (summary, found) = scan(text);
    Plan {
        summary,
        tasks: found.into_iter().map(|f| f.task).collect(),
        saved_at: None,
    }
}

fn plan_error(e: &io::Error) -> ProjectError {
    ProjectError::Plan(e.to_string())
}

fn with_mtime(dir: &Path, text: &str) -> Plan {
    let mut plan = parse(text);
    plan.saved_at = fs::metadata(dir.join(PLAN_FILE))
        .and_then(|m| m.modified())
        .ok()
        .and_then(rfc3339);
    plan
}

/// The saved plan; None when the project has none. An unreadable file is an error with the reason.
pub(crate) fn read(dir: &Path) -> Result<Option<Plan>, ProjectError> {
    match fs::read_to_string(dir.join(PLAN_FILE)) {
        Ok(text) => Ok(Some(with_mtime(dir, &text))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(plan_error(&e)),
    }
}

/// Temp file then rename, so a failed write leaves the old plan.
fn write(dir: &Path, text: &str) -> Result<(), ProjectError> {
    let path = dir.join(PLAN_FILE);
    let tmp = path.with_file_name(".plan.md.tmp");
    let res = fs::create_dir_all(dir.join(".ai-app-studio"))
        .and_then(|()| fs::write(&tmp, text))
        .and_then(|()| fs::rename(&tmp, &path));
    if let Err(e) = res {
        let _ = fs::remove_file(&tmp);
        return Err(plan_error(&e));
    }
    Ok(())
}

/// Replaces the plan with `text`. A text without checklist lines is refused.
pub(crate) fn save(dir: &Path, text: &str) -> Result<Plan, ProjectError> {
    if parse(text).tasks.is_empty() {
        return Err(ProjectError::Plan(NO_TASKS.into()));
    }
    write(dir, text)?;
    Ok(with_mtime(dir, text))
}

/// Ticks or unticks the first task titled `text` that isn't in that state yet; every other byte
/// of the file stays as it was.
pub(crate) fn set_task(dir: &Path, text: &str, done: bool) -> Result<Plan, ProjectError> {
    let mut file = fs::read_to_string(dir.join(PLAN_FILE)).map_err(|e| plan_error(&e))?;
    let want = text.trim();
    let (_, found) = scan(&file);
    let hit = found
        .iter()
        .find(|f| f.task.text == want && f.task.done != done)
        .ok_or_else(|| ProjectError::Plan(GONE.into()))?;
    file.replace_range(hit.mark..=hit.mark, if done { "x" } else { " " });
    write(dir, &file)?;
    Ok(with_mtime(dir, &file))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(plan: &Plan, i: usize) -> (&str, &str, bool) {
        let k = &plan.tasks[i];
        (&k.text, &k.detail, k.done)
    }

    fn dir_with(text: &str) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join(".ai-app-studio")).unwrap();
        fs::write(d.path().join(PLAN_FILE), text).unwrap();
        d
    }

    fn file(d: &tempfile::TempDir) -> String {
        fs::read_to_string(d.path().join(PLAN_FILE)).unwrap()
    }

    #[test]
    fn bullets_and_states() {
        let p = parse("- [ ] a\n* [x] b\n+ [X] c\n");
        assert_eq!(p.tasks.len(), 3);
        assert_eq!(t(&p, 0), ("a", "", false));
        assert_eq!(t(&p, 1), ("b", "", true));
        assert_eq!(t(&p, 2), ("c", "", true));
        assert_eq!(p.saved_at, None);
    }

    #[test]
    fn indent_up_to_three_is_a_task_four_is_not() {
        let p =
            parse("   - [ ] three\n    - [ ] four\n-[ ] no space\n- [] no mark\n- [ ]\n- [ ]   \n");
        assert_eq!(p.tasks.len(), 1);
        assert_eq!(p.tasks[0].text, "three");
    }

    #[test]
    fn detail_lines_nested_items_and_blanks() {
        let p = parse("- [ ] a\n  first\n\n  - [ ] nested\n\tfoo\n- [x] b\nnot detail\n  stray\n");
        assert_eq!(p.tasks.len(), 2);
        assert_eq!(t(&p, 0), ("a", "first\n- [ ] nested\nfoo", false));
        assert_eq!(t(&p, 1), ("b", "", true));
    }

    #[test]
    fn fenced_code_is_never_a_task() {
        let p = parse("# S\n```\n- [ ] x\n```\n~~~\n- [ ] y\n~~~\n- [ ] real\n");
        assert_eq!(p.tasks.len(), 1);
        assert_eq!(p.tasks[0].text, "real");
        assert!(p.summary.contains("- [ ] x"));
        // A ~~~ inside a ``` fence does not close it.
        assert_eq!(parse("```\n~~~\n- [ ] x\n```\n").tasks.len(), 0);
    }

    #[test]
    fn summary_is_markdown_before_first_task_trimmed() {
        let p = parse("\n# Plan\n\nIntro text.\n\n- [ ] a\nafter\n");
        assert_eq!(p.summary, "# Plan\n\nIntro text.");
        assert!(parse("").tasks.is_empty());
        assert_eq!(parse("just text").summary, "just text");
    }

    #[test]
    fn crlf_input() {
        let p = parse("# S\r\n- [x] a\r\n  more\r\n- [ ] b \r\n");
        assert_eq!(p.summary, "# S");
        assert_eq!(t(&p, 0), ("a", "more", true));
        assert_eq!(t(&p, 1), ("b", "", false));
    }

    #[test]
    fn read_missing_is_none_and_bad_utf8_is_err() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(read(d.path()).unwrap(), None);
        let d = dir_with("");
        fs::write(d.path().join(PLAN_FILE), [0xff, 0xfe, b'a']).unwrap();
        assert!(matches!(read(d.path()), Err(ProjectError::Plan(m)) if m.contains("UTF-8")));
    }

    #[test]
    fn read_returns_plan_with_mtime() {
        let d = dir_with("- [ ] a\n");
        let p = read(d.path()).unwrap().unwrap();
        assert_eq!(p.tasks.len(), 1);
        assert!(p.saved_at.unwrap().ends_with('Z'));
    }

    #[test]
    fn save_refuses_no_tasks_and_keeps_old_file() {
        let d = dir_with("- [ ] old\n");
        let e = save(d.path(), "# nothing\n").unwrap_err();
        assert_eq!(e.to_string(), "This plan has no checklist lines (- [ ]).");
        assert_eq!(file(&d), "- [ ] old\n");
    }

    #[test]
    fn save_creates_the_folder_and_returns_the_plan() {
        let d = tempfile::tempdir().unwrap();
        let p = save(d.path(), "- [ ] a\r\n").unwrap();
        assert!(p.saved_at.is_some());
        assert_eq!(file(&d), "- [ ] a\r\n");
        assert!(!d.path().join(".ai-app-studio/.plan.md.tmp").exists());
    }

    #[cfg(unix)]
    #[test]
    fn failed_write_keeps_the_old_file() {
        use std::os::unix::fs::PermissionsExt;
        let d = dir_with("- [ ] old\n");
        let folder = d.path().join(".ai-app-studio");
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap();
        let res = save(d.path(), "- [ ] new\n");
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
        if res.is_ok() {
            return; // running as root: permissions don't apply
        }
        assert_eq!(file(&d), "- [ ] old\n");
    }

    #[test]
    fn set_task_flips_only_the_first_task_in_the_other_state() {
        let d = dir_with("# S\n- [x] same\n- [ ] same\n- [ ] same\n- [ ] other\n");
        let p = set_task(d.path(), "same", true).unwrap();
        assert_eq!(p.tasks.iter().filter(|k| k.done).count(), 2);
        assert_eq!(
            file(&d),
            "# S\n- [x] same\n- [x] same\n- [ ] same\n- [ ] other\n"
        );
        set_task(d.path(), "same", false).unwrap();
        assert_eq!(
            file(&d),
            "# S\n- [ ] same\n- [x] same\n- [ ] same\n- [ ] other\n"
        );
    }

    #[test]
    fn set_task_keeps_every_other_byte_including_crlf_and_skips_fences() {
        let src = "intro\r\n* [ ] a\r\n  d\r\n\r\n```\r\n- [ ] a\r\n```\r\n+ [X] b";
        let d = dir_with(src);
        set_task(d.path(), "a", true).unwrap();
        assert_eq!(file(&d), src.replacen("[ ] a", "[x] a", 1));
        set_task(d.path(), "b", false).unwrap();
        assert!(file(&d).ends_with("+ [ ] b"));
    }

    #[test]
    fn set_task_without_a_match_errs_and_changes_nothing() {
        let d = dir_with("- [x] a\n");
        let e = set_task(d.path(), "a", true).unwrap_err();
        assert_eq!(e.to_string(), "This task is no longer in the plan.");
        assert!(set_task(d.path(), "zzz", true).is_err());
        assert_eq!(file(&d), "- [x] a\n");
    }
}
