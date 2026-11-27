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
    /// Saturday and Sunday in Month and Week: a faint cool tint, apart from out-of-month grey.
    pub weekend: Color32,
    /// Weekday names of Saturday and Sunday in the Month and Week headers.
    pub weekend_text: Color32,
    /// Days outside the shown month (Month view): clearly dimmer than the month's days.
    pub outside: Color32,
    /// Day numbers on those days; event bars there are veiled toward `outside`.
    pub outside_text: Color32,
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
            weekend: over(hex(0x8EA2E0), 20, hex(0x262626)),
            weekend_text: hex(0x9DAAD6),
            outside: hex(0x191919),
            outside_text: over(Color32::WHITE, 84, hex(0x191919)),
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
            weekend: over(hex(0x5B74C4), 16, white),
            weekend_text: hex(0x5A6A9C),
            outside: over(Color32::BLACK, 18, white),
            outside_text: over(Color32::BLACK, 110, white),
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

/// SnowUI input metrics: 12px side and 8px top/bottom padding, 36px high, 8px corners.
pub const INPUT_PADDING: Margin = Margin { left: 12, right: 12, top: 8, bottom: 8 };
pub const INPUT_HEIGHT: f32 = 36.0;

fn input_stroke(p: &Palette, focused: bool, hovered: bool) -> Stroke {
    if focused {
        Stroke::new(1.5_f32, p.primary)
    } else if hovered {
        Stroke::new(1.0_f32, p.text_3)
    } else {
        Stroke::new(1.0_f32, p.line)
    }
}

/// A text field in the SnowUI input style (any single- or multi-line `TextEdit`). The frame is
/// drawn here, under the text, so every field gets the same padding, border and focus ring.
pub fn text_field(ui: &mut egui::Ui, p: &Palette, edit: egui::TextEdit<'_>) -> egui::Response {
    text_field_padded(ui, p, edit, INPUT_PADDING)
}

/// `text_field` with other padding (room for an icon inside the field).
pub fn text_field_padded(ui: &mut egui::Ui, p: &Palette, edit: egui::TextEdit<'_>, padding: Margin) -> egui::Response {
    let bg = ui.painter().add(egui::Shape::Noop);
    let mut r = ui.add(edit.frame(false).margin(padding).min_size(egui::vec2(0.0, INPUT_HEIGHT)));
    // egui reports the text's rect, inside the padding; the field is the outer one.
    r.rect += padding;
    let fill = if r.enabled() { p.surface } else { p.hover };
    let shape = egui::epaint::RectShape::new(r.rect, CornerRadius::same(8), fill, input_stroke(p, r.has_focus(), r.hovered()), egui::StrokeKind::Inside);
    ui.painter().set(bg, shape);
    r
}

/// Room a dialog's scroll bar takes beside the content (bar plus its margin).
pub const SCROLL_GUTTER: f32 = 12.0;

/// Scroll bars for dialogs: they keep their own gutter (never covering the content) and stay
/// visible, in the text colour at low opacity, whenever the content is taller than the dialog,
/// not only on hover. They widen a little under the pointer.
pub fn visible_scroll_bars(ui: &mut egui::Ui) {
    ui.spacing_mut().scroll = egui::style::ScrollStyle {
        floating: true,
        bar_width: 8.0,
        floating_width: 6.0,
        floating_allocated_width: SCROLL_GUTTER,
        bar_inner_margin: SCROLL_GUTTER - 8.0,
        bar_outer_margin: 0.0,
        handle_min_length: 24.0,
        foreground_color: true,
        dormant_background_opacity: 0.06,
        active_background_opacity: 0.1,
        interact_background_opacity: 0.14,
        dormant_handle_opacity: 0.32,
        active_handle_opacity: 0.45,
        interact_handle_opacity: 0.6,
    };
}

/// A dialog body that scrolls vertically when taller than `max_h`, its bar shown while it does.
pub fn dialog_scroll(max_h: f32) -> egui::ScrollArea {
    egui::ScrollArea::vertical().max_height(max_h).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
}

/// A drop-down in the same style as `text_field`; the list itself keeps the menu style.
pub fn select<R>(ui: &mut egui::Ui, p: &Palette, combo: egui::ComboBox, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<Option<R>> {
    let menu_style = ui.style().clone();
    ui.scope(|ui| {
        let s = ui.style_mut();
        s.spacing.button_padding = egui::vec2(INPUT_PADDING.left as f32, INPUT_PADDING.top as f32);
        s.spacing.interact_size.y = INPUT_HEIGHT;
        let w = &mut s.visuals.widgets;
        for (state, hovered, open) in [(&mut w.inactive, false, false), (&mut w.hovered, true, false), (&mut w.active, true, false), (&mut w.open, false, true)] {
            state.bg_fill = p.surface;
            state.weak_bg_fill = p.surface;
            state.bg_stroke = input_stroke(p, open, hovered);
            state.expansion = 0.0;
        }
        let combo = combo.icon(|ui, rect, visuals, _open, _side| {
            // A chevron, like the other icons, instead of egui's filled triangle.
            let c = rect.center();
            let w = rect.width().min(12.0) / 2.0;
            let pts = vec![c + egui::vec2(-w, -w / 2.0), c + egui::vec2(0.0, w / 2.0), c + egui::vec2(w, -w / 2.0)];
            ui.painter().add(egui::epaint::PathShape::line(pts, Stroke::new(1.5_f32, visuals.fg_stroke.color)));
        });
        combo.show_ui(ui, |ui| {
            ui.set_style(menu_style);
            add_contents(ui)
        })
    })
    .inner
}
