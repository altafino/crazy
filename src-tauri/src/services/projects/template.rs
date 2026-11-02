//! Copying the bundled template and parameterizing it. JSON and TOML are edited as
//! parsed documents, never with text replacement.

use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use super::ProjectError;

const SKIP: &[&str] = &["node_modules", "target", "dist", ".git"];

pub(super) struct Params<'a> {
    pub name: &'a str,
    pub slug: &'a str,
    pub identifier: &'a str,
    pub cargo_name: &'a str,
}

pub(super) fn copy_template(from: &Path, to: &Path) -> Result<(), ProjectError> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        if SKIP.iter().any(|s| entry.file_name() == *s) {
            continue;
        }
        let dest = to.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_template(&entry.path(), &dest)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

pub(super) fn substitute(dir: &Path, p: &Params<'_>) -> Result<(), ProjectError> {
    edit_json(&dir.join("package.json"), |v| {
        *v.get_mut("name").ok_or("missing name")? = json!(p.slug);
        Ok(())
    })?;
    edit_json(&dir.join("src-tauri/tauri.conf.json"), |v| {
        v["productName"] = json!(p.name);
        v["identifier"] = json!(p.identifier);
        v["mainBinaryName"] = json!(p.cargo_name);
        let window = v
            .pointer_mut("/app/windows/0")
            .ok_or("missing app.windows[0]")?;
        window["title"] = json!(p.name);
        Ok(())
    })?;

    let cargo = dir.join("src-tauri/Cargo.toml");
    let mut doc: toml_edit::DocumentMut = fs::read_to_string(&cargo)?
        .parse()
        .map_err(|e: toml_edit::TomlError| ProjectError::Template(format!("Cargo.toml: {e}")))?;
    doc["package"]["name"] = toml_edit::value(p.cargo_name);
    fs::write(&cargo, doc.to_string())?;

    let html = dir.join("index.html");
    let text = fs::read_to_string(&html)?;
    let (start, end) = (text.find("<title>"), text.find("</title>"));
    let (Some(start), Some(end)) = (start, end) else {
        return Err(ProjectError::Template("index.html has no <title>".into()));
    };
    fs::write(
        &html,
        format!(
            "{}<title>{}{}",
            &text[..start],
            escape_html(p.name),
            &text[end..]
        ),
    )?;

    let readme = dir.join("README.md");
    let text = fs::read_to_string(&readme)?;
    // Replace the first heading line, or prepend one.
    let rest = match text.split_once('\n') {
        Some((first, rest)) if first.starts_with("# ") => rest,
        _ => text.as_str(),
    };
    fs::write(&readme, format!("# {}\n{rest}", p.name))?;
    Ok(())
}

/// Reads `path`, applies `f` to the parsed value, writes it back pretty-printed.
pub(super) fn edit_json(
    path: &Path,
    f: impl FnOnce(&mut Value) -> Result<(), &'static str>,
) -> Result<(), ProjectError> {
    let file = path.file_name().unwrap_or_default().to_string_lossy();
    let mut value: Value = serde_json::from_str(&fs::read_to_string(path)?)
        .map_err(|e| ProjectError::Template(format!("{file}: {e}")))?;
    f(&mut value).map_err(|e| ProjectError::Template(format!("{file}: {e}")))?;
    let mut out =
        serde_json::to_string_pretty(&value).map_err(|e| ProjectError::Template(e.to_string()))?;
    out.push('\n');
    fs::write(path, out)?;
    Ok(())
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
