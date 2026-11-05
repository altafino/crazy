//! The project's app icon: every file the template ships in `src-tauri/icons/`, generated from one
//! picked image (PNG, JPEG, WebP or SVG). Preview and the next release use it; the files show up
//! in Changes like any other edit.

use std::fs;
use std::io::{self, Cursor};
use std::path::Path;

use base64::Engine;
use image::imageops::{self, FilterType};
use image::{DynamicImage, ImageFormat, RgbaImage};

use super::ProjectError;
use crate::services::builds::store::find_icon;

pub(crate) const ICONS_DIR: &str = "src-tauri/icons";
/// Smaller pictures look blurry at the 512 px size the launcher may show (SVG scales: no limit).
pub(crate) const MIN_SIDE: u32 = 256;
/// SVGs are drawn at this size before every smaller size is made from it.
const SVG_SIDE: f32 = 1024.0;
pub(crate) const NOT_AN_IMAGE: &str = "Pick a PNG, JPEG, WebP or SVG image.";
/// The PNG files of the template's icon set and their sizes.
const PNGS: &[(&str, u32)] = &[
    ("32x32.png", 32),
    ("128x128.png", 128),
    ("128x128@2x.png", 256),
    ("icon.png", 512),
    ("Square30x30Logo.png", 30),
    ("Square44x44Logo.png", 44),
    ("Square71x71Logo.png", 71),
    ("Square89x89Logo.png", 89),
    ("Square107x107Logo.png", 107),
    ("Square142x142Logo.png", 142),
    ("Square150x150Logo.png", 150),
    ("Square284x284Logo.png", 284),
    ("Square310x310Logo.png", 310),
    ("StoreLogo.png", 50),
];
const ICO_SIZES: &[u32] = &[16, 24, 32, 48, 64, 256];
const ICNS_SIZES: &[u32] = &[16, 32, 64, 128, 256, 512];

fn icon_error(msg: impl Into<String>) -> ProjectError {
    ProjectError::Icon(msg.into())
}

/// Writes the whole icon set from `source`. A picture that isn't square is centered on a
/// transparent square. Everything is encoded before the first write, so a bad picture leaves the
/// old icons untouched.
pub(crate) fn set_icon(project_dir: &Path, source: &Path) -> Result<(), ProjectError> {
    let square = to_square(load(source)?);
    let sized = |s: u32| imageops::resize(&square, s, s, FilterType::Lanczos3);

    let mut files = Vec::with_capacity(PNGS.len() + 2);
    for &(name, s) in PNGS {
        files.push((name, png(&sized(s))?));
    }
    files.push(("icon.ico", ico(&sized)?));
    files.push(("icon.icns", icns(&sized)?));

    let dir = project_dir.join(ICONS_DIR);
    fs::create_dir_all(&dir)?;
    for (name, data) in files {
        let tmp = dir.join(format!(".{name}.tmp"));
        fs::write(&tmp, data)?;
        fs::rename(&tmp, dir.join(name))?;
    }
    Ok(())
}

/// The picture as it would become the icon, 128 px, as a `data:` URL (the wizard's preview). Fails
/// with the same message `set_icon` would.
pub(crate) fn preview(source: &Path) -> Result<String, ProjectError> {
    let small = imageops::resize(&to_square(load(source)?), 128, 128, FilterType::Lanczos3);
    Ok(data_url(&png(&small)?))
}

/// The icon as a `data:` URL for the UI (the asset protocol stays limited to attachments).
pub(crate) fn icon_data_url(project_dir: &Path) -> Option<String> {
    let small = project_dir.join(ICONS_DIR).join("128x128.png");
    let path = if small.is_file() {
        small
    } else {
        find_icon(project_dir)?
    };
    Some(data_url(&fs::read(path).ok()?))
}

/// Any PNG (e.g. an installed app's launcher icon) scaled to `side` px, as a `data:` URL.
pub(crate) fn thumbnail_data_url(path: &Path, side: u32) -> Option<String> {
    let img = image::open(path).ok()?;
    let small = imageops::resize(&to_square(img), side, side, FilterType::Lanczos3);
    png(&small).ok().map(|b| data_url(&b))
}

fn data_url(png: &[u8]) -> String {
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    )
}

fn is_svg(source: &Path, bytes: &[u8]) -> bool {
    source
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
        || (image::guess_format(bytes).is_err()
            && String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).contains("<svg"))
}

/// Decodes the picture; SVGs are drawn at 1024 px, other pictures must be at least 256 px.
fn load(source: &Path) -> Result<DynamicImage, ProjectError> {
    let bytes = fs::read(source)?;
    if is_svg(source, &bytes) {
        return render_svg(&bytes);
    }
    let img = image::load_from_memory(&bytes).map_err(|_| icon_error(NOT_AN_IMAGE))?;
    let (w, h) = (img.width(), img.height());
    if w.min(h) < MIN_SIDE {
        return Err(icon_error(format!(
            "This image is {w} × {h} pixels. Pick one at least {MIN_SIDE} × {MIN_SIDE}."
        )));
    }
    Ok(img)
}

fn render_svg(bytes: &[u8]) -> Result<DynamicImage, ProjectError> {
    use resvg::{tiny_skia, usvg};
    let mut opt = usvg::Options::default();
    opt.fontdb_mut().load_system_fonts(); // text in a logo keeps its letters
    let tree = usvg::Tree::from_data(bytes, &opt)
        .map_err(|e| icon_error(format!("This SVG can't be read: {e}")))?;
    let size = tree.size();
    let scale = SVG_SIDE / size.width().max(size.height());
    let (w, h) = (
        (size.width() * scale).round().max(1.0) as u32,
        (size.height() * scale).round().max(1.0) as u32,
    );
    let mut pixmap =
        tiny_skia::Pixmap::new(w, h).ok_or_else(|| icon_error("This SVG has no size to draw."))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let png = pixmap
        .encode_png()
        .map_err(|e| icon_error(format!("Couldn't draw the SVG: {e}")))?;
    image::load_from_memory(&png).map_err(|_| icon_error(NOT_AN_IMAGE))
}

fn to_square(img: DynamicImage) -> RgbaImage {
    let rgba = img.into_rgba8();
    let (w, h) = rgba.dimensions();
    if w == h {
        return rgba;
    }
    let side = w.max(h);
    let mut canvas = RgbaImage::new(side, side); // transparent
    imageops::overlay(
        &mut canvas,
        &rgba,
        i64::from((side - w) / 2),
        i64::from((side - h) / 2),
    );
    canvas
}

fn png(img: &RgbaImage) -> Result<Vec<u8>, ProjectError> {
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png)
        .map_err(|e| icon_error(format!("Couldn't encode the icon: {e}")))?;
    Ok(out.into_inner())
}

fn ico(sized: &dyn Fn(u32) -> RgbaImage) -> io::Result<Vec<u8>> {
    let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
    for &s in ICO_SIZES {
        let img = ico::IconImage::from_rgba_data(s, s, sized(s).into_raw());
        dir.add_entry(ico::IconDirEntry::encode(&img)?);
    }
    let mut out = Vec::new();
    dir.write(&mut out)?;
    Ok(out)
}

fn icns(sized: &dyn Fn(u32) -> RgbaImage) -> io::Result<Vec<u8>> {
    let mut family = icns::IconFamily::new();
    for &s in ICNS_SIZES {
        let img = icns::Image::from_data(icns::PixelFormat::RGBA, s, s, sized(s).into_raw())?;
        family.add_icon(&img)?;
    }
    let mut out = Vec::new();
    family.write(&mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(dir: &Path, w: u32, h: u32) -> std::path::PathBuf {
        let p = dir.join("pic.png");
        RgbaImage::from_pixel(w, h, image::Rgba([200, 30, 30, 255]))
            .save(&p)
            .unwrap();
        p
    }

    #[test]
    fn writes_every_size_and_centers_a_wide_picture() {
        let d = tempfile::tempdir().unwrap();
        let src = picture(d.path(), 600, 300);
        set_icon(d.path(), &src).unwrap();
        let icons = d.path().join(ICONS_DIR);
        for &(name, s) in PNGS {
            let img = image::open(icons.join(name)).unwrap();
            assert_eq!((img.width(), img.height()), (s, s), "{name}");
        }
        // 600 x 300 on a square: the top quarter is transparent, the middle is the picture.
        let big = image::open(icons.join("icon.png")).unwrap().into_rgba8();
        assert_eq!(big.get_pixel(256, 10)[3], 0);
        assert_eq!(big.get_pixel(256, 256)[3], 255);
        let ico = ico::IconDir::read(fs::File::open(icons.join("icon.ico")).unwrap()).unwrap();
        assert_eq!(ico.entries().len(), ICO_SIZES.len());
        let icns =
            icns::IconFamily::read(fs::File::open(icons.join("icon.icns")).unwrap()).unwrap();
        assert!(icns.available_icons().len() >= ICNS_SIZES.len());
        assert!(
            icon_data_url(d.path())
                .unwrap()
                .starts_with("data:image/png;base64,iVBOR")
        );
    }

    #[test]
    fn refuses_small_or_non_images_and_keeps_the_old_icon() {
        let d = tempfile::tempdir().unwrap();
        let icons = d.path().join(ICONS_DIR);
        fs::create_dir_all(&icons).unwrap();
        fs::write(icons.join("128x128.png"), b"old").unwrap();

        let small = picture(d.path(), 255, 400);
        let e = set_icon(d.path(), &small).unwrap_err().to_string();
        assert_eq!(
            e,
            "This image is 255 × 400 pixels. Pick one at least 256 × 256."
        );
        let text = d.path().join("notes.txt");
        fs::write(&text, "hello").unwrap();
        let e = set_icon(d.path(), &text).unwrap_err().to_string();
        assert_eq!(e, NOT_AN_IMAGE);
        let bad_svg = d.path().join("bad.svg");
        fs::write(&bad_svg, "<svg nope").unwrap();
        let e = set_icon(d.path(), &bad_svg).unwrap_err().to_string();
        assert!(e.starts_with("This SVG can't be read"), "{e}");

        assert_eq!(fs::read(icons.join("128x128.png")).unwrap(), b"old");
        assert!(!icons.join("icon.png").exists());
    }

    #[test]
    fn an_svg_of_any_size_becomes_the_full_icon_set() {
        let d = tempfile::tempdir().unwrap();
        // 40 x 20 user units: tiny, but vector, so it is drawn at 1024 px and centered.
        let src = d.path().join("logo.svg");
        fs::write(
            &src,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="#3584e4"/></svg>"##,
        )
        .unwrap();
        set_icon(d.path(), &src).unwrap();
        let big = image::open(d.path().join(ICONS_DIR).join("icon.png"))
            .unwrap()
            .into_rgba8();
        assert_eq!(big.dimensions(), (512, 512));
        assert_eq!(big.get_pixel(256, 20)[3], 0, "top padding is transparent");
        assert_eq!(
            *big.get_pixel(256, 256),
            image::Rgba([0x35, 0x84, 0xe4, 255])
        );
        assert!(preview(&src).unwrap().starts_with("data:image/png;base64,"));
    }

    #[test]
    fn thumbnails_any_png() {
        let d = tempfile::tempdir().unwrap();
        let src = picture(d.path(), 300, 300);
        assert!(thumbnail_data_url(&src, 80).is_some());
        assert!(thumbnail_data_url(&d.path().join("missing.png"), 80).is_none());
    }

    #[test]
    fn no_icon_means_no_data_url() {
        let d = tempfile::tempdir().unwrap();
        assert!(icon_data_url(d.path()).is_none());
    }
}
