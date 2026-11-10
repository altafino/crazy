//! Screenshots of a created app's window: three moments of a preview, and the release's test
//! launch. Shown on the project cards and in Installed Apps.
//!
//! A Wayland window can't be read by another program, so the app runs through XWayland while it
//! is shot, and the window is found by a marker in its environment that only the run being shot
//! carries. Shots that look like the one before are skipped.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
#[cfg(test)]
use std::time::UNIX_EPOCH;
use std::time::{Duration, Instant, SystemTime};

use image::RgbaImage;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::broadcast;

const MARK: &str = "AI_APP_STUDIO_SHOTS";
/// Most shots a set keeps (`1.png` .. `3.png`).
pub(crate) const MAX: usize = 3;
/// A preview: the window as it first shows, then the screens the user has clicked to.
pub(crate) const PREVIEW_MOMENTS: [Duration; 3] = [
    Duration::from_secs(3),
    Duration::from_secs(20),
    Duration::from_secs(60),
];
/// A release's test launch: its start screen, as it settles.
pub(crate) const RELEASE_MOMENTS: [Duration; 3] = [
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(9),
];
/// Share of pixels that must change for a shot to count as a new screen: a blinking caret or a
/// ticking clock is not one, a changed line of text is (about 1% of an 800x600 window).
const NEW_FROM: f64 = 0.002;

/// Project ids whose shots changed; `commands` forwards them to the UI.
static CHANGED: LazyLock<broadcast::Sender<String>> = LazyLock::new(|| broadcast::channel(64).0);

pub(crate) fn subscribe() -> broadcast::Receiver<String> {
    CHANGED.subscribe()
}

/// The preview shots of a project: `<data>/screenshots/<project-id>`.
pub(crate) fn preview_dir(data_dir: &Path, project_id: &str) -> PathBuf {
    data_dir.join("screenshots").join(project_id)
}

/// A release's shots, kept with its files (and removed with them).
pub(crate) fn release_dir(release_dir: &Path) -> PathBuf {
    release_dir.join("shots")
}

/// Makes `env` run the app so its window can be shot, marked with `token`. False (and `env`
/// unchanged) without an X display.
pub(crate) fn prepare_env(env: &mut Vec<(String, String)>, token: &str) -> bool {
    if !env.iter().any(|(k, v)| k == "DISPLAY" && !v.is_empty()) {
        return false;
    }
    env.retain(|(k, _)| {
        !matches!(
            k.as_str(),
            "GDK_BACKEND" | "WEBKIT_DISABLE_DMABUF_RENDERER" | MARK
        )
    });
    // ponytail: the app runs through XWayland while shot, which can look softer under
    // fractional scaling; a portal screencast would avoid it but asks the user every time.
    env.push(("GDK_BACKEND".into(), "x11".into()));
    // WebKit's DMABUF frames never reach an X window grab; the page would come out blank.
    env.push(("WEBKIT_DISABLE_DMABUF_RENDERER".into(), "1".into()));
    env.push((MARK.into(), token.into()));
    true
}

/// Waits up to `wait` for the marked window, then shoots it at each of `moments` after it
/// showed, into `dir`. The first saved shot replaces the set that was there. Ends early when
/// the window goes away; dropping the future stops it.
pub(crate) async fn take(
    env: Vec<(String, String)>,
    token: String,
    dir: PathBuf,
    project_id: String,
    wait: Option<Duration>,
    moments: &[Duration],
) {
    let display = env
        .iter()
        .find(|(k, _)| k == "DISPLAY")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    let started = Instant::now();
    let shown = loop {
        if grab_blocking(&display, &token).await.is_some() {
            break Instant::now();
        }
        if wait.is_some_and(|w| started.elapsed() >= w) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    let mut saved = 0;
    let mut last: Option<RgbaImage> = None;
    for at in moments {
        tokio::time::sleep_until((shown + *at).into()).await;
        let Some(shot) = grab_blocking(&display, &token).await else {
            return;
        };
        if last.as_ref().is_some_and(|l| same(l, &shot)) {
            continue;
        }
        let (d, n, img) = (dir.clone(), saved + 1, shot.clone());
        let ok = tokio::task::spawn_blocking(move || save(&d, n, &img))
            .await
            .is_ok_and(|r| r.is_ok());
        if !ok {
            return;
        }
        saved += 1;
        last = Some(shot);
        let _ = CHANGED.send(project_id.clone());
    }
}

/// Writes shot `n` (1-based); shot 1 first clears the folder of the set before.
fn save(dir: &Path, n: usize, img: &RgbaImage) -> image::ImageResult<()> {
    if n == 1 {
        let _ = std::fs::remove_dir_all(dir);
    }
    std::fs::create_dir_all(dir)?;
    let part = dir.join(format!("{n}.png.part"));
    img.save_with_format(&part, image::ImageFormat::Png)?;
    std::fs::rename(&part, dir.join(format!("{n}.png")))?;
    Ok(())
}

fn same(a: &RgbaImage, b: &RgbaImage) -> bool {
    if a.dimensions() != b.dimensions() {
        return false;
    }
    let changed = a
        .pixels()
        .zip(b.pixels())
        .filter(|(x, y)| x.0.iter().zip(y.0).any(|(c, d)| c.abs_diff(d) > 16))
        .count();
    (changed as f64) < NEW_FROM * f64::from(a.width() * a.height())
}

/// The shots in `dir`, in order.
pub(crate) fn list(dir: &Path) -> Vec<Shot> {
    use base64::Engine;
    (1..=MAX)
        .filter_map(|n| {
            let path = dir.join(format!("{n}.png"));
            let png = std::fs::read(&path).ok()?;
            Some(Shot {
                path: path.display().to_string(),
                // A data URL, as for the app icons: the asset protocol stays limited to
                // attachments.
                src: format!(
                    "data:image/png;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(png)
                ),
            })
        })
        .collect()
}

/// When the set in `dir` was started (its first shot), for picking the newer of two sets.
pub(crate) fn started(dir: &Path) -> Option<SystemTime> {
    dir.join("1.png").metadata().ok()?.modified().ok()
}

/// A project's shots: what its card shows and what Installed Apps shows.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectShots {
    /// The newest set: the last preview's or the newest release's.
    pub latest: Vec<Shot>,
    /// The installed (active) release's.
    pub installed: Vec<Shot>,
}

/// `installed_root` is the project's `AppLayout::root()`, `active` its active version.
pub(crate) fn for_project(
    data_dir: &Path,
    project_id: &str,
    installed_root: &Path,
    active: Option<&str>,
) -> ProjectShots {
    let releases = installed_root.join("releases");
    let mut sets: Vec<PathBuf> = std::fs::read_dir(&releases)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| release_dir(&e.path()))
        .collect();
    sets.push(preview_dir(data_dir, project_id));
    let newest = sets.iter().filter_map(|d| Some((started(d)?, d))).max();
    ProjectShots {
        latest: newest.map(|(_, d)| list(d)).unwrap_or_default(),
        installed: active
            .map(|v| list(&release_dir(&releases.join(v))))
            .unwrap_or_default(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Shot {
    pub path: String,
    /// The picture, as a data URL.
    pub src: String,
}

async fn grab_blocking(display: &str, token: &str) -> Option<RgbaImage> {
    let (d, t) = (display.to_owned(), token.to_owned());
    tokio::task::spawn_blocking(move || x11::grab(&d, &t))
        .await
        .ok()
        .flatten()
}

/// Whether `pid` was started with `MARK=token` (children inherit it).
fn marked(pid: u32, token: &str) -> bool {
    let want = format!("{MARK}={token}");
    std::fs::read(format!("/proc/{pid}/environ"))
        .is_ok_and(|env| env.split(|b| *b == 0).any(|kv| kv == want.as_bytes()))
}

#[cfg(target_os = "linux")]
mod x11 {
    use image::RgbaImage;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        AtomEnum, ConnectionExt, ImageFormat, ImageOrder, MapState, Window,
    };

    /// The largest visible window of a marked process, as RGBA. None when there is none (yet)
    /// or the display can't be read.
    pub(super) fn grab(display: &str, token: &str) -> Option<RgbaImage> {
        let (conn, screen) = x11rb::connect(Some(display)).ok()?;
        let root = conn.setup().roots.get(screen)?.root;
        let pid_atom = conn
            .intern_atom(true, b"_NET_WM_PID")
            .ok()?
            .reply()
            .ok()?
            .atom;
        let mut best: Option<(Window, u16, u16)> = None;
        // Clients sit at most a frame or two below the root.
        let mut level = vec![root];
        for _ in 0..3 {
            let mut next = Vec::new();
            for w in level {
                let Ok(tree) = conn.query_tree(w).ok()?.reply() else {
                    continue;
                };
                next.extend(tree.children);
            }
            for &w in &next {
                let Some(pid) = conn
                    .get_property(false, w, pid_atom, AtomEnum::CARDINAL, 0, 1)
                    .ok()
                    .and_then(|c| c.reply().ok())
                    .and_then(|r| r.value32().and_then(|mut v| v.next()))
                else {
                    continue;
                };
                if !super::marked(pid, token) {
                    continue;
                }
                let viewable = conn
                    .get_window_attributes(w)
                    .ok()
                    .and_then(|c| c.reply().ok())
                    .is_some_and(|a| a.map_state == MapState::VIEWABLE);
                let Some(g) = conn.get_geometry(w).ok().and_then(|c| c.reply().ok()) else {
                    continue;
                };
                let area = u32::from(g.width) * u32::from(g.height);
                if viewable
                    && g.width >= 64
                    && g.height >= 64
                    && best.is_none_or(|(_, bw, bh)| area > u32::from(bw) * u32::from(bh))
                {
                    best = Some((w, g.width, g.height));
                }
            }
            level = next;
        }
        let (w, width, height) = best?;
        let img = conn
            .get_image(ImageFormat::Z_PIXMAP, w, 0, 0, width, height, !0)
            .ok()?
            .reply()
            .ok()?;
        let setup = conn.setup();
        let bpp = setup
            .pixmap_formats
            .iter()
            .find(|f| f.depth == img.depth)?
            .bits_per_pixel;
        // Every 24- and 32-bit visual GTK uses; anything else is skipped.
        if bpp != 32 || setup.image_byte_order != ImageOrder::LSB_FIRST {
            return None;
        }
        let px = usize::from(width) * usize::from(height);
        let data = img.data.get(..px * 4)?;
        let mut rgba = Vec::with_capacity(px * 4);
        for p in data.as_chunks::<4>().0 {
            let a = if img.depth == 32 { p[3] } else { 255 };
            rgba.extend_from_slice(&[p[2], p[1], p[0], a]);
        }
        RgbaImage::from_raw(u32::from(width), u32::from(height), rgba)
    }
}

#[cfg(not(target_os = "linux"))]
mod x11 {
    pub(super) fn grab(_: &str, _: &str) -> Option<image::RgbaImage> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_is_only_changed_with_an_x_display() {
        let mut env = vec![("WAYLAND_DISPLAY".to_owned(), "wayland-0".to_owned())];
        assert!(!prepare_env(&mut env, "t"));
        assert_eq!(env.len(), 1);
        env.push(("DISPLAY".into(), ":0".into()));
        env.push(("GDK_BACKEND".into(), "wayland".into()));
        assert!(prepare_env(&mut env, "t"));
        let get = |k: &str| {
            env.iter()
                .filter(|(n, _)| n == k)
                .map(|(_, v)| v.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(get("GDK_BACKEND"), ["x11"]);
        assert_eq!(get(MARK), ["t"]);
    }

    #[test]
    fn a_blinking_caret_is_the_same_screen_a_new_page_is_not() {
        let base = RgbaImage::from_pixel(200, 100, image::Rgba([240, 240, 240, 255]));
        let mut caret = base.clone();
        for y in 40..60 {
            caret.put_pixel(50, y, image::Rgba([0, 0, 0, 255]));
        }
        assert!(same(&base, &caret));
        let mut page = base.clone();
        for (x, y, p) in page.enumerate_pixels_mut() {
            if y < 50 && x < 150 {
                *p = image::Rgba([30, 30, 60, 255]);
            }
        }
        assert!(!same(&base, &page));
        // One changed line of text in an 800x600 window.
        let base = RgbaImage::from_pixel(800, 600, image::Rgba([24, 24, 27, 255]));
        let mut line = base.clone();
        for x in 390..660 {
            for y in 370..385 {
                if (x + y) % 2 == 0 {
                    line.put_pixel(x, y, image::Rgba([160, 160, 170, 255]));
                }
            }
        }
        assert!(!same(&base, &line));
        assert!(!same(&base, &RgbaImage::new(100, 100)));
    }

    #[test]
    fn a_new_set_replaces_the_old_one_and_lists_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("shots");
        let img = RgbaImage::new(4, 4);
        for n in 1..=3 {
            save(&dir, n, &img).unwrap();
        }
        assert_eq!(list(&dir).len(), 3);
        save(&dir, 1, &img).unwrap();
        let shots = list(&dir);
        assert_eq!(shots.len(), 1);
        assert!(shots[0].path.ends_with("1.png"));
    }

    #[test]
    fn cards_show_the_newest_set_installed_apps_the_active_release() {
        let tmp = tempfile::tempdir().unwrap();
        let (data, root) = (tmp.path().join("data"), tmp.path().join("installed"));
        let img = RgbaImage::new(4, 4);
        let at = |d: &Path, secs: u64| {
            let f = std::fs::File::options()
                .write(true)
                .open(d.join("1.png"))
                .unwrap();
            f.set_modified(UNIX_EPOCH + Duration::from_secs(secs))
                .unwrap();
        };
        let (old, new) = (
            release_dir(&root.join("releases/0.1.0")),
            release_dir(&root.join("releases/0.2.0")),
        );
        let preview = preview_dir(&data, "p");
        for (d, secs) in [(&old, 100), (&new, 300), (&preview, 200)] {
            save(d, 1, &img).unwrap();
            at(d, secs);
        }
        save(&old, 2, &img).unwrap();
        let s = for_project(&data, "p", &root, Some("0.1.0"));
        assert!(s.latest[0].path.contains("0.2.0"));
        assert_eq!(s.installed.len(), 2);
        at(&preview, 400);
        assert!(
            for_project(&data, "p", &root, None).latest[0]
                .path
                .contains("screenshots")
        );
        assert!(
            for_project(&data, "q", &tmp.path().join("none"), None)
                .latest
                .is_empty()
        );
    }

    #[test]
    fn only_processes_started_with_the_token_are_marked() {
        assert!(!marked(std::process::id(), "not-this-one"));
        let child = std::process::Command::new("sleep")
            .arg("5")
            .env(MARK, "tok")
            .spawn()
            .unwrap();
        let pid = child.id();
        // The environment shows once the child has exec'd.
        let start = Instant::now();
        while !marked(pid, "tok") && start.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(marked(pid, "tok"));
        assert!(!marked(pid, "other"));
        let mut child = child;
        let _ = child.kill();
        let _ = child.wait();
    }
}
