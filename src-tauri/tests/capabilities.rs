//! PRD 6.2 / 7: the frontend must never get shell execution. Fails if any capability
//! grants a `shell:` permission or the shell plugin is added as a dependency.

use std::fs;
use std::path::Path;

#[test]
fn no_shell_permission_in_capabilities() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities");
    let mut checked = 0;
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "json") {
            let text = fs::read_to_string(&path).unwrap();
            assert!(
                !text.contains("shell:"),
                "{} grants a shell permission",
                path.display()
            );
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "no capability files found in {}",
        dir.display()
    );
}

#[test]
fn shell_plugin_is_not_a_dependency() {
    let manifest =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    assert!(
        !manifest.contains("tauri-plugin-shell"),
        "tauri-plugin-shell must not be added"
    );
}
