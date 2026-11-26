//! The search results list under the toolbar's search field (every breakpoint, the phone's
//! field next to Day/Month, and the web build): matches across all dates, grouped by day,
//! upcoming first. Up/Down move the selection, Enter or a click jumps to the day and opens
//! the event, Escape closes. The calendar grid itself is not filtered by the search.

use std::hash::{Hash, Hasher};

use chrono::{Datelike, Duration, NaiveDate};
use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use super::{first_of_month, today, views, week_start, CalendarCraftApp};
use crate::i18n::{self, t};
use crate::model::ViewMode;
use crate::search::{self, SearchHit, SearchResults};
use crate::theme::{font, font_bold, space, text};

const ROW_H: f32 = 44.0;
const HEADER_H: f32 = 28.0;
const SECTION_H: f32 = 26.0;

/// What the toolbar's search field did this frame, for the results list drawn after it.
#[derive(Default)]
pub struct SearchUi {
    /// Selected row in the results.
    pub sel: usize,
    /// The field's rectangle this frame (`None` when the field is not shown).
    pub field: Option<Rect>,
    pub focused: bool,
    /// Hidden by a click elsewhere; shown again by typing or focusing the field.
    pub hidden: bool,
    last_query: String,
    cache: Option<(u64, SearchResults)>,
    /// Bring the selected row into view (after a key press).
    scroll: bool,
    /// Results drawn last frame; a change draws a few more frames so the list fits them.
    drawn: Option<u64>,
    settle: u8,
}

impl CalendarCraftApp {
    /// Fingerprint of what a search reads, so results are recomputed only when it changes.
    fn search_key(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.search_query.hash(&mut h);
        today().hash(&mut h);
        self.store.settings.timezone.hash(&mut h);
        self.store.settings.active_calendar_set.hash(&mut h);
        for s in self.store.user_sets() {
            (&s.id, &s.calendar_ids).hash(&mut h);
        }
        for c in &self.store.calendars {
            (&c.id, &c.name, c.visible).hash(&mut h);
        }
        for a in &self.store.accounts {
            (&a.id, a.enabled).hash(&mut h);
        }
        for e in &self.store.events {
            (&e.id, &e.calendar_id, &e.title, e.date, e.end_date, e.start_time, e.end_time, e.all_day).hash(&mut h);
            (&e.rrule, &e.location, &e.notes, &e.attendees, &e.exdates).hash(&mut h);
        }
        h.finish()
    }

    fn search_results(&mut self) -> SearchResults {
        let key = self.search_key();
        if let Some((k, r)) = &self.search_ui.cache {
            if *k == key {
                return r.clone();
            }
        }
        let r = search::search(&self.store, &self.search_query, today(), search::DEFAULT_LIMIT);
        self.search_ui.cache = Some((key, r.clone()));
        r
    }

    /// Note the field drawn this frame (call right after the search field).
    pub(super) fn note_search_field(&mut self, field: &egui::Response) {
        self.search_ui.field = Some(field.rect);
        self.search_ui.focused = field.has_focus();
        if field.clicked() {
            self.search_ui.hidden = false;
        }
    }

    fn search_list_open(&self) -> bool {
        !self.search_query.trim().is_empty() && !self.search_ui.hidden
    }

    /// Up, Down and Enter while the field has focus and results show. Runs before the field
    /// is drawn, so a single-line field does not see them (Enter would end editing).
    pub(super) fn search_keys(&mut self, ctx: &egui::Context) {
        if self.search_query != self.search_ui.last_query {
            self.search_ui.last_query = self.search_query.clone();
            self.search_ui.sel = 0;
            self.search_ui.hidden = false;
        }
        if !self.search_ui.focused || !self.search_list_open() {
            return;
        }
        let (down, up, enter) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            )
        });
        if !(down || up || enter) {
            return;
        }
        let results = self.search_results();
        let n = results.hits.len();
        if n == 0 {
            return;
        }
        if down {
            self.search_ui.sel = (self.search_ui.sel + 1).min(n - 1);
            self.search_ui.scroll = true;
        }
        if up {
            self.search_ui.sel = self.search_ui.sel.saturating_sub(1);
            self.search_ui.scroll = true;
        }
        if enter {
            let hit = results.hits[self.search_ui.sel.min(n - 1)].clone();
            self.open_search_hit(&hit);
        }
    }

    /// Go to the occurrence's day and open the event; the search closes.
    pub(super) fn open_search_hit(&mut self, hit: &SearchHit) {
        let day = hit.occurrence.date;
        if matches!(self.view_mode, ViewMode::Year | ViewMode::Quarter) {
            self.set_view(ViewMode::Month);
        }
        self.select_date(day);
        if !self.month_pos.is_nan() {
            self.month_synced = day;
            self.month_target = Some(views::row_of(week_start(first_of_month(day))) as f64);
        }
        self.agenda_scroll_to = Some(day);
        self.selected_event = Some(hit.event_id.clone());
        self.open_inspector(&hit.event_id, None);
        self.search_query.clear();
        self.search_open = false;
        self.search_ui.hidden = false;
        self.search_ui.focused = false;
    }

    /// The results list, under the field drawn this frame. Call after the layout.
    pub(super) fn search_popup(&mut self, ctx: &egui::Context) {
        let field = self.search_ui.field.take();
        let focused = std::mem::take(&mut self.search_ui.focused);
        self.search_ui.focused = focused;
        let Some(field) = field else { return };
        if !self.search_list_open() {
            return;
        }
        let p = *self.palette();
        let results = self.search_results();
        let drawn = self.search_ui.cache.as_ref().map(|(k, _)| *k);
        if drawn != self.search_ui.drawn {
            self.search_ui.drawn = drawn;
            self.search_ui.settle = 3;
        }
        // The area and its scroll list take their size from the frame before.
        if self.search_ui.settle > 0 {
            self.search_ui.settle -= 1;
            ctx.request_repaint();
        }
        let screen = ctx.screen_rect();
        let mobile = self.is_mobile(ctx);
        let width = if mobile { screen.width() - 16.0 } else { 400.0_f32.min(screen.width() - 16.0) };
        let left = if mobile { screen.left() + 8.0 } else { (field.right() - width).max(screen.left() + 8.0) };
        let top = field.bottom() + space::S4;
        let max_h = (screen.bottom() - top - 12.0).clamp(120.0, 460.0);
        let mut pick: Option<SearchHit> = None;
        let area = egui::Area::new("search_results".into()).order(egui::Order::Foreground).fixed_pos(Pos2::new(left, top)).show(ctx, |ui| {
            egui::Frame::new().fill(p.surface).stroke(Stroke::new(1.0_f32, p.line)).corner_radius(CornerRadius::same(12)).inner_margin(egui::Margin::same(6)).shadow(ctx.style().visuals.popup_shadow).show(ui, |ui| {
                ui.set_width(width - 14.0);
                if results.hits.is_empty() {
                    ui.add_space(space::S8);
                    ui.label(RichText::new(t("No matches")).font(font(text::T14)).color(p.text_2));
                    ui.add_space(space::S8);
                    return;
                }
                // Ask for the list's own height (the area otherwise keeps the size it had for the
                // previous query until it is drawn again).
                let mut est = results.hits.len() as f32 * ROW_H;
                let mut last: Option<(bool, NaiveDate)> = None;
                for h in &results.hits {
                    if last.is_none_or(|(up, _)| up != h.upcoming) && !h.upcoming {
                        est += SECTION_H;
                    }
                    if last.is_none_or(|(up, d)| up != h.upcoming || d != h.occurrence.date) {
                        est += HEADER_H;
                    }
                    last = Some((h.upcoming, h.occurrence.date));
                }
                if results.more > 0 {
                    est += 32.0;
                }
                egui::ScrollArea::vertical().id_salt(("search_results_scroll", drawn)).max_height(max_h).min_scrolled_height(est.min(max_h)).auto_shrink([false, true]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let now = today();
                    let mut last_day: Option<NaiveDate> = None;
                    let mut in_past = false;
                    for (i, hit) in results.hits.iter().enumerate() {
                        if !hit.upcoming && !in_past {
                            in_past = true;
                            last_day = None;
                            section(ui, &p, t("Past"));
                        }
                        let day = hit.occurrence.date;
                        if last_day != Some(day) {
                            last_day = Some(day);
                            day_header(ui, &p, day, now);
                        }
                        let selected = i == self.search_ui.sel;
                        let resp = self.search_row(ui, &p, hit, selected);
                        if selected && self.search_ui.scroll {
                            resp.scroll_to_me(None);
                        }
                        if resp.hovered() && ui.input(|inp| inp.pointer.delta() != Vec2::ZERO) {
                            self.search_ui.sel = i;
                        }
                        if resp.clicked() {
                            pick = Some(hit.clone());
                        }
                    }
                    if results.more > 0 {
                        ui.add_space(space::S8);
                        ui.label(RichText::new(format!("+{} {}", results.more, t("more – type more to narrow the search"))).font(font(text::T12)).color(p.text_2));
                        ui.add_space(space::S4);
                    }
                });
            });
        });
        self.search_ui.scroll = false;
        if let Some(hit) = pick {
            self.open_search_hit(&hit);
            return;
        }
        // A click elsewhere hides the list (the query stays; focusing the field shows it again).
        let outside = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|pt| !area.response.rect.contains(pt) && !field.contains(pt)));
        if outside {
            self.search_ui.hidden = true;
        }
    }

    fn search_row(&self, ui: &mut egui::Ui, p: &crate::theme::Palette, hit: &SearchHit, selected: bool) -> egui::Response {
        let ev = &hit.occurrence;
        let cal = self.store.calendar_by_id(&ev.calendar_id);
        let color = self.cal_color(&ev.calendar_id);
        let w = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, ROW_H), Sense::click());
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        let painter = ui.painter_at(rect);
        if selected {
            painter.rect_filled(rect, CornerRadius::same(8), p.primary_faint);
        }
        let time_w = 54.0;
        let time = if ev.all_day {
            t("all-day").to_string()
        } else {
            ev.start_time.map(|s| s.format("%H:%M").to_string()).unwrap_or_default()
        };
        let left = rect.left() + 10.0;
        painter.circle_filled(Pos2::new(left + 4.0, rect.center().y), 4.0, color);
        let time_galley = painter.layout(time, font(text::T12), p.text_2, time_w - 8.0);
        painter.galley(Pos2::new(left + 16.0, rect.center().y - time_galley.size().y / 2.0), time_galley, p.text_2);
        let text_left = left + 16.0 + time_w;
        let text_w = (rect.right() - text_left - 8.0).max(20.0);
        let title = elide(&painter, ev.title.clone(), font_bold(text::T14), p.text, text_w);
        painter.galley(Pos2::new(text_left, rect.top() + 5.0), title, p.text);
        let mut sub = cal.map(|c| c.name.clone()).unwrap_or_default();
        if let Some(loc) = ev.location.as_deref().filter(|l| !l.trim().is_empty()) {
            sub = if sub.is_empty() { loc.to_string() } else { format!("{sub} · {loc}") };
        }
        let sub = elide(&painter, sub, font(text::T12), p.text_2, text_w);
        painter.galley(Pos2::new(text_left, rect.top() + 24.0), sub, p.text_2);
        resp
    }
}

fn elide(painter: &egui::Painter, text: String, font_id: egui::FontId, color: Color32, width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text, font_id, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    painter.layout_job(job)
}

fn section(ui: &mut egui::Ui, p: &crate::theme::Palette, label: &str) {
    ui.add_space(space::S8);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
    ui.painter().text(Pos2::new(rect.left() + 10.0, rect.center().y), Align2::LEFT_CENTER, label.to_uppercase(), font_bold(11.0), p.text_3);
}

fn day_header(ui: &mut egui::Ui, p: &crate::theme::Palette, day: NaiveDate, now: NaiveDate) {
    ui.add_space(space::S4);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::hover());
    let painter = ui.painter_at(rect);
    let mut label = format!("{}, {} {} {}", i18n::weekday_short(day.weekday()), day.day(), i18n::month_short(day.month()), day.year());
    if day == now {
        label = format!("{} · {label}", t("Today"));
    } else if day == now + Duration::days(1) {
        label = format!("{} · {label}", t("Tomorrow"));
    }
    let color = if day == now { p.primary_strong } else { p.text_2 };
    painter.text(Pos2::new(rect.left() + 10.0, rect.center().y), Align2::LEFT_CENTER, label, font_bold(text::T12), color);
    painter.line_segment([Pos2::new(rect.left() + 10.0, rect.bottom() - 0.5), Pos2::new(rect.right() - 10.0, rect.bottom() - 0.5)], Stroke::new(1.0_f32, p.line));
}
