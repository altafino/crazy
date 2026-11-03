//! Verifying and storing a built AppImage (PRD M4 step 4; A11, M4-D6). Everything here is
//! blocking IO; the service runs it in `spawn_blocking`.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const CHUNK: usize = 1024 * 1024;

// ------------------------------------------------------------------ sha-256

// ponytail: own SHA-256 because `sha2` is only a transitive proc-macro dependency (not a
// direct one) and the brief says to report before adding a crate. Swap for `sha2` if added.
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// Streaming SHA-256.
pub(crate) struct Sha256 {
    h: [u32; 8],
    block: [u8; 64],
    filled: usize,
    len: u64,
}

impl Sha256 {
    pub(crate) fn new() -> Self {
        Self {
            h: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            block: [0; 64],
            filled: 0,
            len: 0,
        }
    }

    fn compress(h: &mut [u32; 8], block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (i, c) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*c);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = *h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }

    pub(crate) fn update(&mut self, mut data: &[u8]) {
        self.len += data.len() as u64;
        while !data.is_empty() {
            let n = (64 - self.filled).min(data.len());
            self.block[self.filled..self.filled + n].copy_from_slice(&data[..n]);
            self.filled += n;
            data = &data[n..];
            if self.filled == 64 {
                Self::compress(&mut self.h, &self.block);
                self.filled = 0;
            }
        }
    }

    /// Lowercase hex digest.
    pub(crate) fn finish(mut self) -> String {
        let bits = self.len * 8;
        self.update(&[0x80]);
        while self.filled != 56 {
            self.update(&[0]);
        }
        self.update(&bits.to_be_bytes());
        self.h.iter().map(|x| format!("{x:08x}")).collect()
    }
}

/// SHA-256 of a file, streamed in 1 MiB chunks.
pub(crate) fn sha256_file(path: &Path) -> io::Result<String> {
    let mut f = File::open(path)?;
    let (mut hash, mut buf) = (Sha256::new(), vec![0u8; CHUNK]);
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            return Ok(hash.finish());
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
fn copy_atomic(src: &Path, dest: &Path, mode: u32) -> io::Result<Stored> {
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
            sha256: hash.finish(),
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
    fn sha256_matches_known_vectors() {
        let hex = |data: &[u8]| {
            let mut h = Sha256::new();
            h.update(data);
            h.finish()
        };
        assert_eq!(
            hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        // Streaming in odd pieces equals one shot (a million 'a').
        let mut h = Sha256::new();
        for _ in 0..1000 {
            h.update(&[b'a'; 1000]);
        }
        assert_eq!(
            h.finish(),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
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
