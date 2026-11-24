//! SnowUI tokens (the SnowUI Design System Figma file, the same values artcraftCloud uses) and
//! how they become an egui style: light and dark colours with Purple as the primary, the SnowUI
//! type scale on the system font (SF Pro on macOS, Inter elsewhere and on the web), and roomy
//! buttons.

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke, TextStyle};
use serde::{Deserialize, Serialize};

/// `system`, `light` or `dark`, as artcraftCloud's Settings store it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "light" | "white" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::System,
        }
    }
}

/// One palette (SnowUI or SnowUI-Dark).
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    /// Page behind everything (SnowUI Background 2 / dark page).
    pub page: Color32,
    /// Cards, popups, the calendar grid (Background 1).
    pub surface: Color32,
    /// The sidebar (Surface 3 over the page).
    pub sidebar: Color32,
    pub text: Color32,
    /// Black 40 / White 40.
    pub text_2: Color32,
    /// Black 20: weekend and out-of-month numbers.
    pub text_3: Color32,
    /// Black 10: lines between days.
    pub line: Color32,
    /// Black 4: hover and the shaded weekend.
    pub hover: Color32,
    pub primary: Color32,
    /// The primary for text and filled buttons (5.9:1 on white).
    pub primary_strong: Color32,
    pub on_primary_strong: Color32,
    pub primary_soft: Color32,
    pub primary_faint: Color32,
    pub danger: Color32,
    pub shadow: Color32,
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// A colour over another at `alpha` (0–255), flattened (egui blends premultiplied colours, which
/// makes thin lines look darker than the token).
pub fn over(top: Color32, alpha: u8, under: Color32) -> Color32 {
    let a = alpha as f32 / 255.0;
    let mix = |t: u8, u: u8| (t as f32 * a + u as f32 * (1.0 - a)).round() as u8;
    Color32::from_rgb(mix(top.r(), under.r()), mix(top.g(), under.g()), mix(top.b(), under.b()))
}

pub const PURPLE: Color32 = hex(0xB899EB);
pub const PURPLE_STRONG: Color32 = hex(0x6E48B8);

/// SnowUI's secondary colours, in the order new calendars take them.
pub const CALENDAR_COLORS: &[(&str, &str)] = &[
    ("Purple", "#B899EB"),
    ("Indigo", "#ADADFB"),
    ("Blue", "#33D1FF"),
    ("Cyan", "#A0BCE8"),
    ("Mint", "#6BE6D3"),
    ("Green", "#71DD8C"),
    ("Yellow", "#FFCC00"),
    ("Orange", "#FFB55B"),
    ("Red", "#FF4747"),
];

pub fn palette(dark: bool) -> Palette {
    if dark {
        let page = hex(0x1C1C1C);
        Palette {
            dark,
            page,
            surface: hex(0x262626),
            sidebar: over(Color32::WHITE, 10, page),
            text: Color32::WHITE,
            text_2: over(Color32::WHITE, 102, page),
            text_3: over(Color32::WHITE, 51, page),
            line: over(Color32::WHITE, 38, page),
            hover: over(Color32::WHITE, 26, page),
            primary: PURPLE,
            primary_strong: PURPLE,
            on_primary_strong: Color32::BLACK,
            primary_soft: over(PURPLE, 51, page),
            primary_faint: over(PURPLE, 31, page),
            danger: hex(0xFF6B6B),
            shadow: Color32::from_black_alpha(110),
        }
    } else {
        let page = hex(0xF9F9FA);
        let white = Color32::WHITE;
        Palette {
            dark,
            page,
            surface: white,
            sidebar: page,
            text: Color32::BLACK,
            text_2: over(Color32::BLACK, 102, white),
            text_3: over(Color32::BLACK, 51, white),
            line: over(Color32::BLACK, 26, white),
            hover: over(Color32::BLACK, 10, white),
            primary: PURPLE,
            primary_strong: PURPLE_STRONG,
            on_primary_strong: white,
            primary_soft: over(PURPLE, 51, white),
            primary_faint: over(PURPLE, 31, white),
            danger: hex(0xD92626),
            shadow: Color32::from_black_alpha(26),
        }
    }
}

/// SnowUI spacing scale.
pub mod space {
    pub const S4: f32 = 4.0;
    pub const S8: f32 = 8.0;
    pub const S12: f32 = 12.0;
    pub const S16: f32 = 16.0;
    pub const S20: f32 = 20.0;
    pub const S28: f32 = 28.0;
}

/// SnowUI text sizes (Regular 400 and Semibold 600).
pub mod text {
    pub const T12: f32 = 12.0;
    pub const T14: f32 = 14.0;
    pub const T16: f32 = 16.0;
    pub const T18: f32 = 18.0;
    pub const T24: f32 = 24.0;
}

pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

pub fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn font_bold(size: f32) -> FontId {
    FontId::new(size, semibold())
}

/// The system typeface: SF Pro on macOS when present, Inter (bundled, OFL) otherwise. egui's
/// own fonts stay behind them for symbols and other scripts.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    #[allow(unused_mut)]
    let mut regular: Vec<u8> = include_bytes!("../assets/fonts/Inter-Regular.ttf").to_vec();
    let bold: Vec<u8> = include_bytes!("../assets/fonts/Inter-SemiBold.ttf").to_vec();
    #[cfg(target_os = "macos")]
    {
        // SF Pro is a variable font; egui draws its default (Regular) instance for both, and
        // keeps Inter SemiBold for headings so weights still differ.
        if let Ok(sf) = std::fs::read("/System/Library/Fonts/SFNS.ttf") {
            regular = sf;
        }
    }
    fonts.font_data.insert("system".into(), FontData::from_owned(regular).into());
    fonts.font_data.insert("system-semibold".into(), FontData::from_owned(bold).into());
    let fallbacks: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut prop = vec!["system".to_string()];
    prop.extend(fallbacks.iter().cloned());
    fonts.families.insert(FontFamily::Proportional, prop);
    let mut sb = vec!["system-semibold".to_string(), "system".to_string()];
    sb.extend(fallbacks);
    fonts.families.insert(semibold(), sb);
    ctx.set_fonts(fonts);
}

/// Apply the palette and SnowUI metrics to egui.
pub fn apply(ctx: &egui::Context, p: &Palette, animations: bool) {
    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Small, font(text::T12)),
        (TextStyle::Body, font(text::T14)),
        (TextStyle::Button, font(text::T14)),
        (TextStyle::Monospace, FontId::monospace(13.0)),
        (TextStyle::Heading, font_bold(text::T24)),
    ]
    .into();
    let s = &mut style.spacing;
    s.button_padding = egui::vec2(space::S16, 7.0);
    s.item_spacing = egui::vec2(space::S8, space::S8);
    s.interact_size = egui::vec2(40.0, 32.0);
    s.window_margin = Margin::same(space::S20 as i8);
    s.menu_margin = Margin::same(space::S8 as i8);
    s.combo_width = 160.0;
    s.text_edit_width = 240.0;
    s.icon_width = 16.0;
    s.icon_spacing = 6.0;
    style.animation_time = if animations { 0.12 } else { 0.0 };

    let mut v = if p.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.dark_mode = p.dark;
    v.override_text_color = Some(p.text);
    v.panel_fill = p.page;
    v.window_fill = p.surface;
    v.window_stroke = Stroke::new(1.0_f32, p.line);
    v.window_corner_radius = CornerRadius::same(16);
    v.menu_corner_radius = CornerRadius::same(12);
    v.window_shadow = egui::epaint::Shadow { offset: [0, 8], blur: 28, spread: 0, color: p.shadow };
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 4], blur: 16, spread: 0, color: p.shadow };
    v.extreme_bg_color = p.surface;
    v.faint_bg_color = p.hover;
    v.code_bg_color = p.hover;
    v.hyperlink_color = p.primary_strong;
    v.selection.bg_fill = p.primary_soft;
    v.selection.stroke = Stroke::new(1.0_f32, p.primary_strong);
    v.text_cursor.stroke = Stroke::new(2.0_f32, p.primary_strong);
    let radius = CornerRadius::same(8);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.surface;
    w.noninteractive.weak_bg_fill = p.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.line);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.noninteractive.corner_radius = radius;
    w.inactive.bg_fill = p.hover;
    w.inactive.weak_bg_fill = p.hover;
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.inactive.corner_radius = radius;
    w.hovered.bg_fill = p.primary_faint;
    w.hovered.weak_bg_fill = p.primary_faint;
    w.hovered.bg_stroke = Stroke::new(1.0_f32, p.primary_soft);
    w.hovered.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.hovered.corner_radius = radius;
    w.hovered.expansion = 0.0;
    w.active.bg_fill = p.primary_soft;
    w.active.weak_bg_fill = p.primary_soft;
    w.active.bg_stroke = Stroke::new(1.0_f32, p.primary);
    w.active.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.active.corner_radius = radius;
    w.active.expansion = 0.0;
    w.open = w.active;
    style.visuals = v;
    ctx.set_style(style);
}

/// A filled primary button (white text on #6E48B8; black on #B899EB in dark).
pub fn primary_button(ui: &mut egui::Ui, p: &Palette, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(label).color(p.on_primary_strong).font(font_bold(text::T14)))
            .fill(p.primary_strong)
            .corner_radius(CornerRadius::same(8))
            .min_size(egui::vec2(0.0, 32.0)),
    )
}

/// A quiet button: no fill until hovered.
pub fn ghost_button(ui: &mut egui::Ui, label: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add(egui::Button::new(label).frame(true).fill(Color32::TRANSPARENT).min_size(egui::vec2(0.0, 32.0)))
}

/// Parse `#RRGGBB`.
pub fn color_of(hex_text: &str) -> Color32 {
    crate::model::parse_hex_color(hex_text).map(|[r, g, b]| Color32::from_rgb(r, g, b)).unwrap_or(PURPLE)
}

/// A readable text colour for event text drawn in the calendar's colour on a tinted background:
/// the colour itself darkened in light mode, lightened in dark mode.
pub fn event_text(color: Color32, p: &Palette) -> Color32 {
    if p.dark {
        over(Color32::WHITE, 150, color)
    } else {
        over(Color32::BLACK, 150, color)
    }
}

/// The tinted background of an event chip.
pub fn event_fill(color: Color32, p: &Palette) -> Color32 {
    over(color, if p.dark { 70 } else { 56 }, p.surface)
}
