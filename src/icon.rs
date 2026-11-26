//! The CalendarCraft logo (`assets/icon.png`), used as the native window and dock icon.

use egui::IconData;

/// The same 512×512 PNG the web build uses as its favicon.
pub const PNG: &[u8] = include_bytes!("../assets/icon.png");

/// Decode [`PNG`] into RGBA [`IconData`] for `ViewportBuilder::with_icon`.
pub fn load() -> Result<IconData, String> {
    let image = image::load_from_memory(PNG).map_err(|e| e.to_string())?;
    let image = image.into_rgba8();
    Ok(IconData {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    })
}

/// Tell macOS to use the logo as the Dock icon.
///
/// A `cargo run` binary is not an `.app` bundle, so the Dock would otherwise show a generic
/// executable icon. eframe will also try this from `ViewportBuilder::with_icon`, but it gives
/// up if `NSApp` is still null; calling [`NSApplication::sharedApplication`][shared] first
/// makes sure the application exists.
///
/// [shared]: objc2_app_kit::NSApplication::sharedApplication
#[cfg(target_os = "macos")]
pub fn set_dock_icon() {
    use objc2::ClassType;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::{MainThreadMarker, NSData};

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(PNG);
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
}
