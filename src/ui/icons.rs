//! Icons drawn as vector paths (the canvas counterpart of inline SVG), so none depends on a glyph
//! being in a font: egui's fonts have no ✕, and the others came from mismatched fallback fonts.
//! Each is drawn in a 16×16 box on a 1.5px stroke, after SF Symbols' and Fantastical's look.

use eframe::egui::{self, epaint::PathShape, Color32, CornerRadius, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Vec2};

use crate::theme::Palette;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Close,
    Gear,
    Search,
    /// A window with its left pane: show or hide the sidebar.
    Sidebar,
    ChevronLeft,
    ChevronRight,
    ChevronDown,
    Refresh,
    Warning,
    Lock,
    Check,
    Plus,
}

/// Draw `icon` centred in `rect` (its 16×16 design box scaled to fit).
pub fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let s = rect.width().min(rect.height()) / 16.0;
    let o = rect.center() - Vec2::splat(8.0 * s);
    let pt = |x: f32, y: f32| Pos2::new(o.x + x * s, o.y + y * s);
    let stroke = Stroke::new(1.5 * s.max(0.75), color);
    let line = |pts: &[(f32, f32)]| {
        painter.add(PathShape::line(pts.iter().map(|(x, y)| pt(*x, *y)).collect(), stroke));
    };
    match icon {
        Icon::Close => {
            line(&[(3.5, 3.5), (12.5, 12.5)]);
            line(&[(12.5, 3.5), (3.5, 12.5)]);
        }
        Icon::Plus => {
            line(&[(8.0, 2.5), (8.0, 13.5)]);
            line(&[(2.5, 8.0), (13.5, 8.0)]);
        }
        Icon::ChevronLeft => line(&[(10.0, 3.0), (5.0, 8.0), (10.0, 13.0)]),
        Icon::ChevronRight => line(&[(6.0, 3.0), (11.0, 8.0), (6.0, 13.0)]),
        Icon::ChevronDown => line(&[(3.5, 6.0), (8.0, 10.5), (12.5, 6.0)]),
        Icon::Check => line(&[(3.0, 8.5), (6.5, 12.0), (13.0, 4.5)]),
        Icon::Search => {
            painter.circle_stroke(pt(7.0, 7.0), 4.5 * s, stroke);
            line(&[(10.3, 10.3), (13.5, 13.5)]);
        }
        Icon::Sidebar => {
            let r = Rect::from_min_max(pt(1.5, 2.5), pt(14.5, 13.5));
            painter.rect_stroke(r, CornerRadius::same((2.5 * s) as u8), stroke, StrokeKind::Middle);
            line(&[(6.0, 2.5), (6.0, 13.5)]);
            line(&[(3.2, 5.0), (4.6, 5.0)]);
            line(&[(3.2, 7.0), (4.6, 7.0)]);
            line(&[(3.2, 9.0), (4.6, 9.0)]);
        }
        Icon::Gear => {
            let c = pt(8.0, 8.0);
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::TAU / 8.0;
                let (sin, cos) = a.sin_cos();
                painter.line_segment([c + Vec2::new(cos, sin) * 4.6 * s, c + Vec2::new(cos, sin) * 6.8 * s], Stroke::new(2.2 * s.max(0.75), color));
            }
            painter.circle_stroke(c, 4.6 * s, stroke);
            painter.circle_stroke(c, 1.9 * s, stroke);
        }
        Icon::Refresh => {
            let c = pt(8.0, 8.0);
            let r = 5.0 * s;
            let pts: Vec<Pos2> = (0..=20).map(|k| {
                let a = (-60.0 + k as f32 * 13.5_f32).to_radians();
                c + Vec2::new(a.cos(), a.sin()) * r
            }).collect();
            let end = pts[0];
            painter.add(PathShape::line(pts, stroke));
            // The arrowhead at the open end, pointing up.
            painter.add(PathShape::line(vec![end + Vec2::new(-3.2, 0.4) * s, end, end + Vec2::new(0.3, -3.2) * s], stroke));
        }
        Icon::Warning => {
            painter.add(PathShape::closed_line(vec![pt(8.0, 2.0), pt(14.5, 13.5), pt(1.5, 13.5)], stroke));
            line(&[(8.0, 6.0), (8.0, 9.5)]);
            painter.circle_filled(pt(8.0, 11.6), 0.9 * s, color);
        }
        Icon::Lock => {
            painter.rect_stroke(Rect::from_min_max(pt(3.5, 7.5), pt(12.5, 14.0)), CornerRadius::same((1.5 * s) as u8), stroke, StrokeKind::Middle);
            let pts: Vec<Pos2> = (0..=12).map(|k| {
                let a = std::f32::consts::PI + k as f32 * std::f32::consts::PI / 12.0;
                pt(8.0, 5.5) + Vec2::new(a.cos(), a.sin()) * 2.8 * s
            }).collect();
            let mut path = vec![pt(5.2, 7.5)];
            path.extend(pts);
            path.push(pt(10.8, 7.5));
            painter.add(PathShape::line(path, stroke));
        }
    }
}

/// A square, borderless icon button (32×32, 16px icon) with a hover fill and a tooltip.
pub fn button(ui: &mut egui::Ui, p: &Palette, icon: Icon, tip: &str) -> Response {
    sized_button(ui, p, icon, tip, 32.0, 16.0, p.text)
}

/// The smaller sidebar variant (24×24, 14px icon, secondary colour).
pub fn small_button(ui: &mut egui::Ui, p: &Palette, icon: Icon, tip: &str) -> Response {
    sized_button(ui, p, icon, tip, 24.0, 14.0, p.text_2)
}

fn sized_button(ui: &mut egui::Ui, p: &Palette, icon: Icon, tip: &str, size: f32, glyph: f32, color: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if ui.is_rect_visible(rect) {
        if resp.is_pointer_button_down_on() {
            ui.painter().rect_filled(rect, CornerRadius::same(8), p.primary_soft);
        } else if resp.hovered() {
            ui.painter().rect_filled(rect, CornerRadius::same(8), p.hover);
        }
        paint(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(glyph)), icon, color);
    }
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if tip.is_empty() { resp } else { resp.on_hover_text(tip) }
}

/// An icon on its own, the size of a line of text.
pub fn inline(ui: &mut egui::Ui, icon: Icon, size: f32, color: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint(ui.painter(), rect, icon, color);
    resp
}
