//! The CalendarCraft logo (`assets/icon.png`), used as the native window and dock icon.
//!
//! On the desktop the day number (and a short month in the header band) is drawn for today, and
//! refreshed at midnight or when the window becomes active. The bundled PNG — already dated
//! "NOV 24" — stays the web favicon and the fallback if rendering fails.

use std::io::Cursor;

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use chrono::{Datelike, NaiveDate};
use egui::IconData;
use image::{Rgba, RgbaImage};

/// The same 512×512 PNG the web build uses as its favicon.
pub const PNG: &[u8] = include_bytes!("../assets/icon.png");

const SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.ttf");

/// Where `assets/icon.png` (512×512) prints "NOV" and "24": boxes a little larger than the
/// glyphs, filled with the band's red and the page's white before the new date is drawn.
const MONTH_BOX: (u32, u32, u32, u32) = (150, 128, 362, 200);
const DAY_BOX: (u32, u32, u32, u32) = (136, 240, 376, 400);
const RED: [u8; 4] = [0xFF, 0x47, 0x47, 0xFF];
const PAGE: [u8; 4] = [0xFF, 0xFF, 0xFF, 0xFF];
const INK: [u8; 4] = [0x1C, 0x1C, 0x1C, 0xFF];
/// Ink centres and cap heights of the original glyphs.
const MONTH_AT: (f32, f32, f32) = (256.0, 159.0, 46.0);
const DAY_AT: (f32, f32, f32) = (256.0, 321.0, 124.0);

const MONTHS: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];

/// Decode [`PNG`] into RGBA [`IconData`] for `ViewportBuilder::with_icon`.
pub fn load() -> Result<IconData, String> {
    let image = image::load_from_memory(PNG).map_err(|e| e.to_string())?;
    Ok(to_icon(image.into_rgba8()))
}

/// The logo with `day`'s month and date painted on it.
pub fn load_for_date(day: NaiveDate) -> Result<IconData, String> {
    render_image(day).map(to_icon)
}

/// Today's dated icon, or the bundled PNG if rendering fails.
pub fn load_for_today() -> Result<IconData, String> {
    load_for_date(chrono::Local::now().date_naive()).or_else(|_| load())
}

/// PNG bytes of the dated icon (for the macOS Dock).
pub fn png_for_date(day: NaiveDate) -> Result<Vec<u8>, String> {
    let image = render_image(day)?;
    let mut buf = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(buf)
}

fn to_icon(image: RgbaImage) -> IconData {
    IconData {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    }
}

fn render_image(day: NaiveDate) -> Result<RgbaImage, String> {
    let mut image = image::load_from_memory(PNG)
        .map_err(|e| e.to_string())?
        .into_rgba8();
    let font = FontRef::try_from_slice(SEMIBOLD).map_err(|e| e.to_string())?;
    fill(&mut image, MONTH_BOX, RED);
    fill(&mut image, DAY_BOX, PAGE);
    let month = MONTHS[(day.month0() as usize).min(11)];
    draw_centered(&mut image, &font, month, MONTH_AT, PAGE);
    draw_centered(&mut image, &font, &day.day().to_string(), DAY_AT, INK);
    Ok(image)
}

fn fill(image: &mut RgbaImage, (x0, y0, x1, y1): (u32, u32, u32, u32), color: [u8; 4]) {
    for y in y0..=y1.min(image.height() - 1) {
        for x in x0..=x1.min(image.width() - 1) {
            image.put_pixel(x, y, Rgba(color));
        }
    }
}

/// Draw `text` so its ink is centred on (`cx`, `cy`) with capitals `cap` pixels tall.
fn draw_centered(
    image: &mut RgbaImage,
    font: &FontRef<'_>,
    text: &str,
    (cx, cy, cap): (f32, f32, f32),
    color: [u8; 4],
) {
    // Inter's capitals and figures are 0.727 em tall.
    let scale = PxScale::from(cap / 0.727);
    let scaled = font.as_scaled(scale);
    let mut glyphs = Vec::new();
    let mut x = 0.0;
    let mut prev = None;
    for c in text.chars() {
        let id = scaled.glyph_id(c);
        if let Some(p) = prev {
            x += scaled.kern(p, id);
        }
        if let Some(g) =
            font.outline_glyph(id.with_scale_and_position(scale, ab_glyph::point(x, 0.0)))
        {
            glyphs.push(g);
        }
        x += scaled.h_advance(id);
        prev = Some(id);
    }
    let Some(first) = glyphs.first() else { return };
    let mut ink = first.px_bounds();
    for g in &glyphs {
        let b = g.px_bounds();
        ink.min.x = ink.min.x.min(b.min.x);
        ink.min.y = ink.min.y.min(b.min.y);
        ink.max.x = ink.max.x.max(b.max.x);
        ink.max.y = ink.max.y.max(b.max.y);
    }
    let dx = (cx - (ink.min.x + ink.max.x) / 2.0).round() as i32;
    let dy = (cy - (ink.min.y + ink.max.y) / 2.0).round() as i32;
    for g in &glyphs {
        let b = g.px_bounds();
        g.draw(|px, py, v| {
            let ix = b.min.x as i32 + px as i32 + dx;
            let iy = b.min.y as i32 + py as i32 + dy;
            if v <= 0.0
                || ix < 0
                || iy < 0
                || ix >= image.width() as i32
                || iy >= image.height() as i32
            {
                return;
            }
            let dst = image.get_pixel(ix as u32, iy as u32).0;
            let a = v.min(1.0);
            let mix = |c: u8, d: u8| (c as f32 * a + d as f32 * (1.0 - a)).round() as u8;
            image.put_pixel(
                ix as u32,
                iy as u32,
                Rgba([
                    mix(color[0], dst[0]),
                    mix(color[1], dst[1]),
                    mix(color[2], dst[2]),
                    dst[3],
                ]),
            );
        });
    }
}

/// Tell macOS to use the (dated) logo as the Dock icon.
///
/// A `cargo run` binary is not an `.app` bundle, so the Dock would otherwise show a generic
/// executable icon. eframe will also try this from `ViewportBuilder::with_icon`, but it gives
/// up if `NSApp` is still null; calling [`NSApplication::sharedApplication`][shared] first
/// makes sure the application exists.
///
/// [shared]: objc2_app_kit::NSApplication::sharedApplication
#[cfg(target_os = "macos")]
pub fn set_dock_icon() {
    let png = png_for_date(chrono::Local::now().date_naive()).unwrap_or_else(|_| PNG.to_vec());
    set_dock_icon_png(&png);
}

/// Put this PNG on the Dock (used when the date changes or the window becomes active).
#[cfg(target_os = "macos")]
pub fn set_dock_icon_for_date(day: NaiveDate) {
    let png = png_for_date(day).unwrap_or_else(|_| PNG.to_vec());
    set_dock_icon_png(&png);
}

#[cfg(target_os = "macos")]
fn set_dock_icon_png(png: &[u8]) {
    use objc2::ClassType;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::{MainThreadMarker, NSData};

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(png);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: `image` is a valid `NSImage` created from our PNG; `app` is the shared application.
    unsafe { app.setApplicationIconImage(Some(&image)) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn decodes_the_bundled_png() {
        assert!(PNG.starts_with(b"\x89PNG\r\n\x1a\n"));
        let icon = load().expect("assets/icon.png should decode");
        assert_eq!(icon.width, 512);
        assert_eq!(icon.height, 512);
        assert_eq!(icon.rgba.len(), 512 * 512 * 4);
        // Rounded-rect logo: some pixels are opaque, some are transparent.
        let pixels = icon.rgba.as_chunks::<4>().0;
        let opaque = pixels.iter().filter(|p| p[3] == 255).count();
        let clear = pixels.iter().filter(|p| p[3] == 0).count();
        assert!(opaque > 10_000, "expected a filled glyph, opaque={opaque}");
        assert!(
            clear > 1_000,
            "expected a transparent corner, clear={clear}"
        );
    }

    #[test]
    fn dated_icons_differ_for_different_days() {
        let jan1 = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let nov26 = NaiveDate::from_ymd_opt(2026, 11, 26).unwrap();
        let a = load_for_date(jan1).expect("jan 1");
        let b = load_for_date(nov26).expect("nov 26");
        assert_eq!(a.width, 512);
        assert_eq!(a.rgba.len(), b.rgba.len());
        assert_ne!(
            a.rgba, b.rgba,
            "day 1 and day 26 should paint different pixels"
        );
        // The number region (lower half of the white page) must actually change.
        let region = |icon: &IconData| {
            let mut out = Vec::new();
            for y in 250..390 {
                let start = (y * 512 + 160) * 4;
                let end = (y * 512 + 360) * 4;
                out.extend_from_slice(&icon.rgba[start..end]);
            }
            out
        };
        assert_ne!(region(&a), region(&b));
    }

    /// `CALENDARCRAFT_ICON_PREVIEW=<dir> cargo test writes_dated_preview` saves sample icons.
    #[test]
    fn writes_dated_preview() {
        let Some(dir) = std::env::var_os("CALENDARCRAFT_ICON_PREVIEW") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("preview dir");
        for (name, (y, m, d)) in [
            ("polish-icon-dated.png", (2026, 11, 26)),
            ("polish-icon-dated-jan1.png", (2026, 1, 1)),
        ] {
            let png = png_for_date(NaiveDate::from_ymd_opt(y, m, d).unwrap()).expect("png");
            std::fs::write(dir.join(name), png).expect("write preview");
        }
    }
}
