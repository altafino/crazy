//! Verifying and storing a built AppImage (PRD M4 step 4; A11, M4-D6). Everything here is
//! blocking IO; the service runs it in `spawn_blocking`.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const CHUNK: usize = 1024 * 1024;

// ------------------------------------------------------------------ sha-256

fn hex(digest: &[u8]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 of a file, streamed in 1 MiB chunks.
pub(crate) fn sha256_file(path: &Path) -> io::Result<String> {
    let mut f = File::open(path)?;
    let (mut hash, mut buf) = (Sha256::new(), vec![0u8; CHUNK]);
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            return Ok(hex(&hash.finalize()));
        }
        hash.update(&buf[..n]);
    }
}

// ------------------------------------------------------------------- verify

/// A11: nonempty, ELF magic, AppImage type-2 magic (`AI\x02` at offset 8). `Err` is the
/// reason for `verifyFailed`.
pub(crate) fn verify_appimage(path: &Path) -> Result<(), String> {
    let mut f = File::open(path).map_err(|e| format!("Cannot read the AppImage: {e}"))?;
    let mut head = [0u8; 11];
    let mut got = 0;
    while got < head.len() {
        match f.read(&mut head[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) => return Err(format!("Cannot read the AppImage: {e}")),
        }
    }
    if got == 0 {
        return Err("The AppImage is empty".into());
    }
    if got < head.len() || &head[..4] != b"\x7fELF" {
        return Err("The file is not an ELF executable".into());
    }
    if &head[8..11] != b"AI\x02" {
        return Err("The file is not a type-2 AppImage".into());
    }
    Ok(())
}

// -------------------------------------------------------------------- store

/// What `store_release` recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stored {
    pub sha256: String,
    pub size_bytes: u64,
}

fn tmp_of(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    target.with_file_name(name)
}

/// Streams `src` to `dest` through a temp sibling, then renames: `dest` is either the complete
/// file or untouched. Returns the SHA-256 of the bytes written. The temp file is removed on any
/// error.
pub(crate) fn copy_atomic(src: &Path, dest: &Path, mode: u32) -> io::Result<Stored> {
    let tmp = tmp_of(dest);
    let result = (|| {
        let mut input = File::open(src)?;
        let mut out = File::create(&tmp)?;
        let (mut hash, mut buf, mut size) = (Sha256::new(), vec![0u8; CHUNK], 0u64);
        loop {
            let n = input.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
            out.write_all(&buf[..n])?;
            size += n as u64;
        }
        out.sync_all()?;
        drop(out);
        fs::set_permissions(&tmp, fs::Permissions::from_mode(mode))?;
        fs::rename(&tmp, dest)?;
        Ok(Stored {
            sha256: hex(&hash.finalize()),
            size_bytes: size,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// The icon Studio shows and GNOME launches with (M4-D12): the project's 256 px icon, else
/// `icon.png`, else none.
pub(crate) fn find_icon(project_dir: &Path) -> Option<PathBuf> {
    let icons = project_dir.join("src-tauri/icons");
    ["128x128@2x.png", "icon.png"]
        .iter()
        .map(|n| icons.join(n))
        .find(|p| p.is_file())
}

/// Copies the AppImage (executable) and the icon (if any) into the release directory. On any
/// error nothing partial is left behind in the directory except what already existed there.
pub(crate) fn store_release(
    artifact: &Path,
    icon: Option<&Path>,
    appimage_dest: &Path,
    icon_dest: &Path,
) -> io::Result<Stored> {
    if let Some(dir) = appimage_dest.parent() {
        fs::create_dir_all(dir)?;
    }
    let stored = copy_atomic(artifact, appimage_dest, 0o755)?;
    if let Some(icon) = icon
        && let Err(e) = copy_atomic(icon, icon_dest, 0o644)
    {
        let _ = fs::remove_file(appimage_dest);
        return Err(e);
    }
    Ok(stored)
}

/// Startup: removes leftover `*.tmp` files under `<data_dir>/installed/*/releases/`, in the
/// releases directory itself and one level below (the version directories).
pub(crate) fn remove_leftover_temp_files(data_dir: &Path) {
    fn sweep(dir: &Path, depth: u8) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let path = e.path();
            let Ok(t) = e.file_type() else { continue };
            if t.is_file() && path.extension().is_some_and(|x| x == "tmp") {
                let _ = fs::remove_file(&path);
            } else if t.is_dir() && depth > 0 {
                sweep(&path, depth - 1);
            }
        }
    }
    let Ok(apps) = fs::read_dir(data_dir.join("installed")) else {
        return;
    };
    for app in apps.flatten() {
        sweep(&app.path().join("releases"), 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn appimage_bytes(extra: usize) -> Vec<u8> {
        let mut b = b"\x7fELF\x02\x01\x01\x00AI\x02".to_vec();
        b.resize(11 + extra, 7);
        b
    }

    #[test]
    fn verify_checks_size_elf_and_appimage_magic() {
        let d = tmp();
        let p = d.path().join("a.AppImage");
        assert!(verify_appimage(&p).is_err()); // missing
        fs::write(&p, b"").unwrap();
        assert_eq!(verify_appimage(&p).unwrap_err(), "The AppImage is empty");
        fs::write(&p, b"#!/bin/sh\necho hi\n").unwrap();
        assert!(verify_appimage(&p).unwrap_err().contains("ELF"));
        fs::write(&p, b"\x7fELF\x02\x01\x01\x00AI\x01rest").unwrap(); // type 1
        assert!(verify_appimage(&p).unwrap_err().contains("type-2"));
        fs::write(&p, b"\x7fELF").unwrap(); // truncated
        assert!(verify_appimage(&p).is_err());
        fs::write(&p, appimage_bytes(100)).unwrap();
        assert!(verify_appimage(&p).is_ok());
    }

    #[test]
    fn store_copies_hashes_and_makes_the_file_executable() {
        let d = tmp();
        let src = d.path().join("in.AppImage");
        let bytes = appimage_bytes(3 * CHUNK + 17); // several chunks
        fs::write(&src, &bytes).unwrap();
        let icon = d.path().join("i.png");
        fs::write(&icon, b"png").unwrap();
        let dir = d.path().join("releases/1.0.0");
        let (app, ico) = (dir.join("app.AppImage"), dir.join("icon.png"));
        let st = store_release(&src, Some(&icon), &app, &ico).unwrap();
        assert_eq!(st.size_bytes, bytes.len() as u64);
        assert_eq!(st.sha256, sha256_file(&src).unwrap());
        assert_eq!(fs::read(&app).unwrap(), bytes);
        assert_eq!(fs::read(&ico).unwrap(), b"png");
        assert_eq!(
            fs::metadata(&app).unwrap().permissions().mode() & 0o111,
            0o111
        );
        assert!(!tmp_of(&app).exists() && !tmp_of(&ico).exists());
    }

    #[test]
    fn a_failed_copy_leaves_no_partial_file() {
        let d = tmp();
        let dir = d.path().join("releases/1.0.0");
        // Source missing: nothing at all appears.
        let r = store_release(
            &d.path().join("missing"),
            None,
            &dir.join("app.AppImage"),
            &dir.join("icon.png"),
        );
        assert!(r.is_err());
        let left: Vec<_> = fs::read_dir(&dir).unwrap().flatten().collect();
        assert!(left.is_empty(), "{left:?}");

        // A broken icon rolls the AppImage back too.
        let src = d.path().join("in.AppImage");
        fs::write(&src, appimage_bytes(10)).unwrap();
        let r = store_release(
            &src,
            Some(&d.path().join("no-icon.png")),
            &dir.join("app.AppImage"),
            &dir.join("icon.png"),
        );
        assert!(r.is_err());
        assert!(fs::read_dir(&dir).unwrap().next().is_none());

        // Rename over an existing complete file is atomic: a failed attempt keeps the old one.
        let app = dir.join("app.AppImage");
        fs::write(&app, b"old").unwrap();
        assert!(copy_atomic(&d.path().join("missing"), &app, 0o755).is_err());
        assert_eq!(fs::read(&app).unwrap(), b"old");
        assert!(!tmp_of(&app).exists());
    }

    #[test]
    fn icon_prefers_the_256px_file_then_icon_png_then_none() {
        let d = tmp();
        assert!(find_icon(d.path()).is_none());
        let icons = d.path().join("src-tauri/icons");
        fs::create_dir_all(&icons).unwrap();
        fs::write(icons.join("32x32.png"), "x").unwrap();
        assert!(find_icon(d.path()).is_none());
        fs::write(icons.join("icon.png"), "x").unwrap();
        assert_eq!(find_icon(d.path()).unwrap(), icons.join("icon.png"));
        fs::write(icons.join("128x128@2x.png"), "x").unwrap();
        assert_eq!(find_icon(d.path()).unwrap(), icons.join("128x128@2x.png"));
    }

    #[test]
    fn startup_cleanup_removes_only_temp_files_under_releases() {
        let d = tmp();
        let app = d.path().join("installed/p1");
        fs::create_dir_all(app.join("releases/1.0.0")).unwrap();
        for f in [
            "releases/1.0.0/app.AppImage",
            "releases/1.0.0/app.AppImage.tmp",
            "releases/1.0.0/icon.png.tmp",
            "releases/stray.tmp",
            "icon.png",
            "icon.png.tmp",
        ] {
            fs::write(app.join(f), "x").unwrap();
        }
        remove_leftover_temp_files(d.path());
        assert!(app.join("releases/1.0.0/app.AppImage").exists());
        assert!(!app.join("releases/1.0.0/app.AppImage.tmp").exists());
        assert!(!app.join("releases/1.0.0/icon.png.tmp").exists());
        assert!(!app.join("releases/stray.tmp").exists());
        // Outside `releases/` is not ours to sweep.
        assert!(app.join("icon.png").exists() && app.join("icon.png.tmp").exists());
        remove_leftover_temp_files(&d.path().join("nowhere")); // no panic
    }
}
