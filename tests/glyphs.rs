//! Every character the interface draws as text must be in the bundled Inter font (or in egui's
//! own fallback fonts for the few symbols noted below); a missing glyph shows as an empty
//! square, as the Settings close button did with "✕". Icons are drawn as vector paths instead
//! (src/ui/icons.rs).

use ab_glyph::{Font, FontRef};

/// Symbols Inter lacks that egui's bundled fallback fonts do draw.
const FALLBACK_OK: &[char] = &['⌘'];

#[test]
fn every_ui_character_has_a_glyph() {
    let inter = FontRef::try_from_slice(include_bytes!("../assets/fonts/Inter-Regular.ttf")).unwrap();
    let bold = FontRef::try_from_slice(include_bytes!("../assets/fonts/Inter-SemiBold.ttf")).unwrap();
    let sources = [
        ("src/ui/mod.rs", include_str!("../src/ui/mod.rs")),
        ("src/ui/side.rs", include_str!("../src/ui/side.rs")),
        ("src/ui/views.rs", include_str!("../src/ui/views.rs")),
        ("src/ui/popups.rs", include_str!("../src/ui/popups.rs")),
        ("src/ui/repeat.rs", include_str!("../src/ui/repeat.rs")),
        ("src/rrule.rs", include_str!("../src/rrule.rs")),
        ("src/ui/icons.rs", include_str!("../src/ui/icons.rs")),
        ("src/i18n.rs", include_str!("../src/i18n.rs")),
    ];
    let mut missing = Vec::new();
    for (file, text) in sources {
        for (n, line) in text.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            // Only what is inside string literals reaches the screen.
            let mut in_str = false;
            let mut prev = ' ';
            for c in code.chars() {
                if c == '"' && prev != '\\' {
                    in_str = !in_str;
                } else if in_str && !c.is_ascii() && !FALLBACK_OK.contains(&c) && (inter.glyph_id(c).0 == 0 || bold.glyph_id(c).0 == 0) {
                    missing.push(format!("{file}:{}: {c:?} (U+{:04X})", n + 1, c as u32));
                }
                prev = c;
            }
        }
    }
    assert!(missing.is_empty(), "characters without a glyph:\n{}", missing.join("\n"));
}
