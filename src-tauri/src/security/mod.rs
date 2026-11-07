//! Input validation for names, slugs, identifiers and paths.

use std::path::{Component, Path, PathBuf};

/// A user-facing validation message.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{0}")]
pub(crate) struct Invalid(pub String);

fn invalid<T>(msg: impl Into<String>) -> Result<T, Invalid> {
    Err(Invalid(msg.into()))
}

/// Returns the trimmed name.
pub(crate) fn validate_name(name: &str) -> Result<String, Invalid> {
    let name = name.trim();
    if name.is_empty() {
        return invalid("enter a name");
    }
    if name.chars().count() > 80 {
        return invalid("name must be 80 characters or fewer");
    }
    // Names end up in README.md and index.html, so keep them single-line.
    if name.chars().any(char::is_control) {
        return invalid("name must not contain control characters");
    }
    Ok(name.to_owned())
}

pub(crate) fn slugify(name: &str) -> Result<String, Invalid> {
    let mut slug = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        return invalid("choose a name with letters or digits");
    }
    Ok(slug.to_owned())
}

/// A user-supplied slug must already be in canonical slug form.
pub(crate) fn validate_slug(slug: &str) -> Result<(), Invalid> {
    match slugify(slug) {
        Ok(s) if s == slug => Ok(()),
        _ => invalid("slug may contain only lowercase letters, digits and single hyphens"),
    }
}

pub(crate) fn validate_identifier(id: &str) -> Result<(), Invalid> {
    let segments: Vec<&str> = id.split('.').collect();
    let ok = segments.len() >= 2
        && segments.iter().all(|s| {
            let mut chars = s.chars();
            chars.next().is_some_and(|c| c.is_ascii_alphabetic())
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '-')
        });
    if ok {
        Ok(())
    } else {
        invalid(
            "identifier must be reverse-DNS like com.example.app: at least two parts, \
             each starting with a letter and using letters, digits or hyphens",
        )
    }
}

/// `com.aiappstudio.<slug without hyphens>`; an identifier segment must start with a
/// letter, so digit-leading slugs get an `app` prefix.
pub(crate) fn default_identifier(slug: &str) -> String {
    let compact: String = slug.chars().filter(|c| *c != '-').collect();
    let lead = if compact.starts_with(|c: char| c.is_ascii_digit()) {
        "app"
    } else {
        ""
    };
    format!("com.aiappstudio.{lead}{compact}")
}

pub(crate) fn cargo_name(slug: &str) -> String {
    let mut name: String = slug
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if matches!(c, 'a'..='z' | '0'..='9' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    if name.starts_with(|c: char| c.is_ascii_digit()) {
        name.insert_str(0, "app-");
    }
    name
}

/// Expands a leading `~/` using `home` (the HOME env var). Other `~` forms are refused.
pub(crate) fn expand_tilde(raw: &str, home: Option<&Path>) -> Result<PathBuf, Invalid> {
    if raw == "~" || raw.starts_with("~/") {
        let Some(home) = home else {
            return invalid("cannot expand ~: HOME is not set");
        };
        return Ok(home.join(raw.trim_start_matches('~').trim_start_matches('/')));
    }
    if raw.starts_with('~') {
        return invalid("~user paths are not supported; use an absolute path");
    }
    Ok(PathBuf::from(raw))
}

/// Parent folder of a new project: absolute, no `..`, exists, writable.
pub(crate) fn validate_parent_dir(parent: &Path) -> Result<(), Invalid> {
    if !parent.is_absolute() {
        return invalid("choose an absolute folder path");
    }
    if parent.components().any(|c| c == Component::ParentDir) {
        return invalid("folder path must not contain '..'");
    }
    if !parent.is_dir() {
        return invalid("folder does not exist");
    }
    // Permission bits miss ACLs and read-only mounts, so probe with a real write.
    let probe = parent.join(format!(
        ".ai-app-studio-write-test-{}",
        uuid::Uuid::new_v4()
    ));
    match std::fs::File::create_new(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            Ok(())
        }
        Err(_) => invalid("folder is not writable"),
    }
}

/// The project folder itself: absent or an empty directory, and clear of Studio's data
/// dir and of every existing library workspace (inside or containing).
pub(crate) fn validate_target(
    target: &Path,
    studio_data_dir: &Path,
    workspaces: &[PathBuf],
) -> Result<(), Invalid> {
    match std::fs::read_dir(target) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                return invalid("that folder already exists and is not empty");
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return invalid("that path exists and is not a usable folder"),
    }
    let target = resolve(target);
    if target.starts_with(resolve(studio_data_dir)) {
        return invalid("that folder is inside Studio's own data folder");
    }
    if workspaces
        .iter()
        .map(|w| resolve(w))
        .any(|w| target.starts_with(&w) || w.starts_with(&target))
    {
        return invalid("that folder overlaps another project in your library");
    }
    Ok(())
}

/// A project folder the user asked to move to the Trash: an existing folder that is not
/// `/`, the home folder or one of its ancestors, Studio's data dir or anything around it,
/// and that holds no other library workspace.
pub(crate) fn validate_trash(
    target: &Path,
    studio_data_dir: &Path,
    home: Option<&Path>,
    other_workspaces: &[PathBuf],
) -> Result<(), Invalid> {
    if !target.is_absolute() || !target.is_dir() {
        return invalid("the project folder is not there");
    }
    let target = resolve(target);
    if target.parent().is_none() {
        return invalid("that is the root folder");
    }
    if home.is_some_and(|h| resolve(h).starts_with(&target)) {
        return invalid("that is your home folder or contains it");
    }
    let data = resolve(studio_data_dir);
    if target.starts_with(&data) || data.starts_with(&target) {
        return invalid("that folder overlaps Studio's own data folder");
    }
    if other_workspaces
        .iter()
        .any(|w| resolve(w).starts_with(&target))
    {
        return invalid("that folder contains another project in your library");
    }
    Ok(())
}

/// Canonicalizes the deepest existing ancestor so symlinks cannot hide an overlap.
fn resolve(path: &Path) -> PathBuf {
    let mut tail = Vec::new();
    let mut cur = path;
    loop {
        if let Ok(base) = cur.canonicalize() {
            return tail.iter().rev().fold(base, |acc, c| acc.join(c));
        }
        match (cur.parent(), cur.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name);
                cur = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(validate_name("  Hi  ").unwrap(), "Hi");
        assert!(validate_name("   ").is_err());
        assert!(validate_name(&"x".repeat(81)).is_err());
        assert!(
            validate_name(&"é".repeat(80)).is_ok(),
            "limit counts chars, not bytes"
        );
        assert!(validate_name("a\nb").is_err());
    }

    #[test]
    fn slugs() {
        assert_eq!(slugify("  My  Cool_App!! ").unwrap(), "my-cool-app");
        assert_eq!(slugify("3D Viewer").unwrap(), "3d-viewer");
        assert_eq!(slugify("Ünï Café").unwrap(), "n-caf");
        let err = slugify("日本語 🎉").unwrap_err();
        assert_eq!(err.0, "choose a name with letters or digits");
        assert!(validate_slug("my-app").is_ok());
        for bad in ["My-App", "-a", "a--b", "a-", ""] {
            assert!(validate_slug(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn identifiers() {
        for ok in ["com.example.app", "a.b", "com.my-co.App2"] {
            assert!(validate_identifier(ok).is_ok(), "{ok}");
        }
        for bad in [
            "app", "com..x", "com.1x", "com.x_y", "com.-x", ".com.x", "com.x.",
        ] {
            assert!(validate_identifier(bad).is_err(), "{bad}");
        }
        assert_eq!(
            default_identifier("my-cool-app"),
            "com.aiappstudio.mycoolapp"
        );
        // Default must itself be valid, even for digit-leading slugs.
        assert_eq!(
            default_identifier("3d-viewer"),
            "com.aiappstudio.app3dviewer"
        );
        assert!(validate_identifier(&default_identifier("42")).is_ok());
    }

    #[test]
    fn cargo_names() {
        assert_eq!(cargo_name("my-app"), "my-app");
        assert_eq!(cargo_name("3d-viewer"), "app-3d-viewer");
        assert_eq!(cargo_name("A.B"), "a-b");
    }

    #[test]
    fn tilde() {
        let home = Path::new("/home/u");
        assert_eq!(
            expand_tilde("~/x/y", Some(home)).unwrap(),
            Path::new("/home/u/x/y")
        );
        assert_eq!(expand_tilde("~", Some(home)).unwrap(), Path::new("/home/u"));
        assert!(expand_tilde("~/x", None).is_err());
        assert!(expand_tilde("~bob/x", Some(home)).is_err());
        assert_eq!(expand_tilde("/abs", None).unwrap(), Path::new("/abs"));
    }

    #[test]
    fn parent_dir_rules() {
        let dir = tempfile::tempdir().unwrap();
        assert!(validate_parent_dir(dir.path()).is_ok());
        assert!(validate_parent_dir(Path::new("relative/dir")).is_err());
        assert!(validate_parent_dir(&dir.path().join("..").join("x")).is_err());
        assert!(validate_parent_dir(&dir.path().join("missing")).is_err());
        assert!(
            std::fs::read_dir(dir.path()).unwrap().next().is_none(),
            "write probe must clean up"
        );
    }

    #[cfg(unix)]
    #[test]
    fn unwritable_parent_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        let writable_anyway = std::fs::File::create(dir.path().join("t")).is_ok(); // running as root
        let r = validate_parent_dir(dir.path());
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(r.is_ok(), writable_anyway);
    }

    #[test]
    fn trash_rules() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let home = root.join("home");
        let data = home.join(".local/share/ai-app-studio");
        let proj = home.join("Projects/app");
        let other = home.join("Projects/app/sub/other");
        for d in [&data, &other] {
            std::fs::create_dir_all(d).unwrap();
        }
        let ok = home.join("Projects/solo");
        std::fs::create_dir_all(&ok).unwrap();
        let check = |p: &Path, ws: &[PathBuf]| validate_trash(p, &data, Some(&home), ws);

        assert!(check(&ok, &[]).is_ok());
        assert!(
            check(&home.join("Projects/missing"), &[]).is_err(),
            "not there"
        );
        assert!(check(Path::new("Projects/solo"), &[]).is_err(), "relative");
        assert!(check(Path::new("/"), &[]).is_err(), "root");
        assert!(check(&home, &[]).is_err(), "home");
        assert!(check(&root, &[]).is_err(), "contains home");
        assert!(check(&data, &[]).is_err(), "data dir");
        assert!(
            check(&home.join(".local"), &[]).is_err(),
            "contains data dir"
        );
        assert!(
            check(&proj, std::slice::from_ref(&other)).is_err(),
            "holds another project"
        );
        assert!(check(&proj, std::slice::from_ref(&ok)).is_ok());
    }

    #[test]
    fn target_rules() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let data = root.join("data");
        let lib_ws = root.join("projects").join("existing");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&lib_ws).unwrap();
        std::fs::write(lib_ws.join("f"), "x").unwrap();
        let ws = [lib_ws.clone()];

        assert!(validate_target(&root.join("projects").join("new"), &data, &ws).is_ok());
        let empty = root.join("empty");
        std::fs::create_dir(&empty).unwrap();
        assert!(validate_target(&empty, &data, &ws).is_ok());

        let full = root.join("full");
        std::fs::create_dir(&full).unwrap();
        std::fs::write(full.join("f"), "x").unwrap();
        assert!(validate_target(&full, &data, &ws).is_err());
        let file = root.join("afile");
        std::fs::write(&file, "x").unwrap();
        assert!(validate_target(&file, &data, &ws).is_err());

        assert!(validate_target(&data.join("proj"), &data, &ws).is_err());
        assert!(validate_target(&lib_ws.join("nested"), &data, &ws).is_err());
        assert!(
            validate_target(&root.join("projects"), &data, &ws).is_err(),
            "contains a workspace"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_cannot_hide_overlap() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let data = root.join("data");
        std::fs::create_dir(&data).unwrap();
        std::os::unix::fs::symlink(&data, root.join("link")).unwrap();
        assert!(validate_target(&root.join("link").join("proj"), &data, &[]).is_err());
    }
}
