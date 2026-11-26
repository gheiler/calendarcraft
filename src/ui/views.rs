//! The calendar views. Month and Quarter draw one grid with shared single-pixel borders;
//! all-day and multi-day events run as bars across the days of a week; Year lines all twelve
//! months up on the same grid; Day and Week are time grids.

use chrono::{Datelike, Duration, NaiveDate, NaiveTime, Timelike};
use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use super::{add_months, first_of_month, today, week_start, CalendarCraftApp};

/// Week rows the Month view shows at once.
const MONTH_ROWS: f32 = 6.0;

/// Monday 5 January 1970, the week counted as row 0 of the Month strip.
fn epoch_monday() -> NaiveDate {
    NaiveDate::from_ymd_opt(1970, 1, 5).unwrap_or_default()
}

/// The row of the week holding `d`.
pub(super) fn row_of(d: NaiveDate) -> i64 {
    (week_start(d) - epoch_monday()).num_days().div_euclid(7)
}

/// The Monday of row `row`.
pub(super) fn week_of_row(row: i64) -> NaiveDate {
    epoch_monday() + Duration::days(row * 7)
}
use crate::i18n::{self, t};
use crate::model::{CalendarEvent, ViewMode};
use crate::theme::{self, font, font_bold, over, text, Palette};

/// A multi-day or all-day event placed in one week row.
struct Span {
    ev: CalendarEvent,
    first: usize,
    last: usize,
    lane: usize,
}

/// Lay out the bars of a week: events that are all-day or span days, each in the first lane
/// free over its days.
fn week_spans(app: &CalendarCraftApp, start: NaiveDate, days: usize) -> (Vec<Span>, Vec<usize>) {
    let mut found: Vec<(CalendarEvent, usize, usize)> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for i in 0..days {
        let d = start + Duration::days(i as i64);
        for ev in app.events_on(d) {
            let spans = ev.all_day || ev.end_date.is_some_and(|e| e > ev.date);
            if !spans || seen.contains(&format!("{}#{}", ev.id, occurrence_start(&ev, d))) {
                continue;
            }
            let occ = occurrence_start(&ev, d);
            seen.push(format!("{}#{occ}", ev.id));
            let len = ev.end_date.map(|e| (e - ev.date).num_days()).unwrap_or(0).max(0);
            let occ_end = occ + Duration::days(len);
            let first = (occ - start).num_days().max(0) as usize;
            let last = ((occ_end - start).num_days().min(days as i64 - 1)) as usize;
            found.push((ev, first, last));
        }
    }
    found.sort_by_key(|(ev, first, last)| (*first, usize::MAX - (last - first), ev.title.clone()));
    let mut lanes: Vec<Vec<bool>> = Vec::new();
    let mut out = Vec::new();
    for (ev, first, last) in found {
        let lane = (0..).find(|l| lanes.get(*l).is_none_or(|row: &Vec<bool>| (first..=last).all(|c| !row[c]))).unwrap_or(0);
        if lanes.len() <= lane {
            lanes.resize(lane + 1, vec![false; days]);
        }
        lanes[lane][first..=last].fill(true);
        out.push(Span { ev, first, last, lane });
    }
    let per_day: Vec<usize> = (0..days).map(|c| lanes.iter().rposition(|row| row[c]).map(|l| l + 1).unwrap_or(0)).collect();
    (out, per_day)
}

/// The first day of the occurrence of `ev` that covers `day`.
fn occurrence_start(ev: &CalendarEvent, day: NaiveDate) -> NaiveDate {
    let span = ev.end_date.filter(|e| *e > ev.date).map(|e| (e - ev.date).num_days()).unwrap_or(0);
    (0..=span).map(|k| day - Duration::days(k)).find(|d| ev.starts_on(*d)).unwrap_or(ev.date)
}

fn timed(app: &CalendarCraftApp, d: NaiveDate) -> Vec<CalendarEvent> {
    app.events_on(d).into_iter().filter(|e| !e.all_day && e.start_time.is_some() && e.end_date.is_none_or(|end| end <= e.date)).collect()
}

/// A bar for an all-day or multi-day event.
fn draw_bar(painter: &egui::Painter, p: &Palette, rect: Rect, color: Color32, title: &str, size: f32, selected: bool) {
    let fill = if selected { color } else { theme::event_fill(color, p) };
    painter.rect_filled(rect, CornerRadius::same(4), fill);
    if !selected {
        painter.rect_filled(Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height())), CornerRadius { nw: 4, sw: 4, ne: 0, se: 0 }, color);
    }
    let txt = if selected { contrast_on(color) } else { theme::event_text(color, p) };
    let clip = painter.with_clip_rect(rect.shrink2(Vec2::new(2.0, 0.0)).intersect(painter.clip_rect()));
    clip.text(Pos2::new(rect.left() + 8.0, rect.center().y), Align2::LEFT_CENTER, title, font_bold(size), txt);
}

/// A timed event in a month cell: dot, time, title.
fn draw_line(painter: &egui::Painter, p: &Palette, rect: Rect, color: Color32, ev: &CalendarEvent, size: f32, selected: bool, show_time: bool) {
    if selected {
        painter.rect_filled(rect, CornerRadius::same(4), theme::event_fill(color, p));
    }
    let clip = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    clip.circle_filled(Pos2::new(rect.left() + 6.0, rect.center().y), 3.5, color);
    let mut x = rect.left() + 14.0;
    if show_time {
        if let Some(s) = ev.start_time {
            let r = clip.text(Pos2::new(x, rect.center().y), Align2::LEFT_CENTER, s.format("%H:%M").to_string(), font(size), p.text_2);
            x = r.right() + 5.0;
        }
    }
    clip.text(Pos2::new(x, rect.center().y), Align2::LEFT_CENTER, &ev.title, font(size), p.text);
}

/// Saturday and Sunday (the week starts on Monday throughout the app).
pub(super) fn is_weekend(d: chrono::Weekday) -> bool {
    matches!(d, chrono::Weekday::Sat | chrono::Weekday::Sun)
}

/// A Month cell's background: outside the shown month dimmer than anything in it, weekends tinted.
pub(super) fn day_fill(p: &Palette, d: NaiveDate, in_month: bool) -> Color32 {
    if !in_month {
        p.outside
    } else if is_weekend(d.weekday()) {
        p.weekend
    } else {
        p.surface
    }
}

fn contrast_on(c: Color32) -> Color32 {
    let l = 0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32;
    if l > 150.0 { Color32::BLACK } else { Color32::WHITE }
}

impl CalendarCraftApp {
    /// Scroll over the grid moves a week (Month) or the period (Quarter, Year).
    fn wheel(&mut self, ui: &egui::Ui, rect: Rect, step_days: Option<i64>) {
        if !ui.rect_contains_pointer(rect) || self.inspector.is_some() || self.new_event.is_some() {
            self.scroll_accum = 0.0;
            return;
        }
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        self.scroll_accum += dy;
        if self.scroll_accum.abs() >= 70.0 {
            let dir = if self.scroll_accum < 0.0 { 1 } else { -1 };
            match step_days {
                Some(n) => self.select_date(self.selected_date + Duration::days(n * dir)),
                None => self.step(dir),
            }
            self.scroll_accum = 0.0;
        }
    }

    /// Month: weeks on a continuous strip that scrolls by the pixel (wheel, trackpad or drag), as
    /// in Fantastical. Six rows fill the view; the month owning the middle row is the one named
    /// in the title, shown in the mini calendar and drawn in full colour. When scrolling stops,
    /// the strip settles on the nearest week edge.
    pub(super) fn month_view(&mut self, ui: &mut egui::Ui) {
        let p = *self.palette();
        let area = ui.available_rect_before_wrap();
        let grid_resp = ui.allocate_rect(area, Sense::drag());
        let head = 34.0;
        let grid = Rect::from_min_max(Pos2::new(area.left(), area.top() + head), area.max);
        let cell_w = area.width() / 7.0;
        let cell_h = grid.height() / MONTH_ROWS;
        self.month_scroll(ui, &grid_resp, grid, cell_h);
        let month = self.month_focus;
        let top_row = self.month_pos.floor();
        let first = week_of_row(top_row as i64);
        let y0 = grid.top() - (self.month_pos - top_row) as f32 * cell_h;
        let rows = MONTH_ROWS as usize + 1;
        let painter = ui.painter_at(area);
        let grid_painter = ui.painter_at(grid);
        let now = today();

        for c in 0..7 {
            let wd = chrono::Weekday::try_from(c as u8).unwrap_or(chrono::Weekday::Mon);
            let x = area.left() + cell_w * (c as f32 + 1.0) - 10.0;
            painter.text(Pos2::new(x, area.top() + head * 0.55), Align2::RIGHT_CENTER, i18n::weekday_short(wd).to_uppercase(), font_bold(text::T12), if is_weekend(wd) { p.weekend_text } else { p.text_2 });
        }
        // Backgrounds: other months clearly dimmer, weekends tinted, the selection highlighted.
        let mut cells: Vec<(NaiveDate, Rect)> = Vec::new();
        for r in 0..rows {
            for c in 0..7 {
                let d = first + Duration::days((r * 7 + c) as i64);
                let cell = Rect::from_min_size(Pos2::new(grid.left() + c as f32 * cell_w, y0 + r as f32 * cell_h), Vec2::new(cell_w, cell_h));
                let fill = if d == self.selected_date {
                    p.primary_faint
                } else {
                    day_fill(&p, d, d.month() == month.month())
                };
                grid_painter.rect_filled(cell, CornerRadius::ZERO, fill);
                cells.push((d, cell));
            }
        }
        // Shared borders: one line between neighbours.
        let stroke = Stroke::new(1.0_f32, p.line);
        painter.line_segment([Pos2::new(grid.left(), grid.top() + 0.5), Pos2::new(grid.right(), grid.top() + 0.5)], stroke);
        for r in 0..=rows {
            let y = (y0 + r as f32 * cell_h).round() + 0.5;
            grid_painter.line_segment([Pos2::new(grid.left(), y), Pos2::new(grid.right(), y)], stroke);
        }
        for c in 1..7 {
            let x = (grid.left() + c as f32 * cell_w).round() + 0.5;
            grid_painter.line_segment([Pos2::new(x, grid.top()), Pos2::new(x, grid.bottom())], stroke);
        }
        // Clicks on days (registered before the events so events win); only the visible part.
        for (d, cell) in &cells {
            let visible = cell.intersect(grid);
            if visible.height() < 4.0 {
                continue;
            }
            let r = ui.interact(visible, ui.id().with(("mday", *d)), Sense::click());
            if r.clicked() {
                self.select_date(*d);
                self.selected_event = None;
            }
            if r.double_clicked() {
                self.open_new_event(*d, None);
            }
        }
        // Events.
        let line_h = 21.0;
        let top_pad = 30.0;
        let max_lines = (((cell_h - top_pad - 4.0) / line_h).floor() as usize).max(1);
        let selected_id = self.highlighted_event();
        let mut clicked: Option<(String, Rect, bool)> = None;
        // Some(double-clicked) when clicked.
        let hit = |ui: &egui::Ui, rect: Rect, id: egui::Id| -> Option<bool> {
            let visible = rect.intersect(grid);
            if visible.height() <= 2.0 {
                return None;
            }
            let r = ui.interact(visible, id, Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            (r.clicked() || r.double_clicked()).then(|| r.double_clicked())
        };
        for r in 0..rows {
            let ws = first + Duration::days((r * 7) as i64);
            let (spans, lanes_per_day) = week_spans(self, ws, 7);
            let row_top = y0 + r as f32 * cell_h + top_pad;
            let mut hidden = [0usize; 7];
            for s in &spans {
                if s.lane + 1 > max_lines || (s.lane + 1 == max_lines && (s.first..=s.last).any(|c| lanes_per_day[c] + timed(self, ws + Duration::days(c as i64)).len() > max_lines)) {
                    for h in hidden.iter_mut().take(s.last + 1).skip(s.first) {
                        *h += 1;
                    }
                    continue;
                }
                let rect = Rect::from_min_max(
                    Pos2::new(grid.left() + s.first as f32 * cell_w + 4.0, row_top + s.lane as f32 * line_h + 1.0),
                    Pos2::new(grid.left() + (s.last + 1) as f32 * cell_w - 4.0, row_top + s.lane as f32 * line_h + line_h - 2.0),
                );
                let color = self.cal_color(&s.ev.calendar_id);
                draw_bar(&grid_painter, &p, rect, color, &s.ev.title, text::T12, selected_id.as_deref() == Some(s.ev.id.as_str()));
                if let Some(double) = hit(ui, rect, ui.id().with(("mspan", &s.ev.id, ws))) {
                    clicked = Some((s.ev.id.clone(), rect, double));
                }
            }
            for c in 0..7 {
                let d = ws + Duration::days(c as i64);
                let items = timed(self, d);
                let used = lanes_per_day[c].min(max_lines);
                let room = max_lines.saturating_sub(used);
                let overflow = items.len() + hidden[c] > room;
                let show = if overflow { room.saturating_sub(1) } else { items.len() };
                for (k, ev) in items.iter().take(show).enumerate() {
                    let y = row_top + (used + k) as f32 * line_h;
                    let rect = Rect::from_min_max(Pos2::new(grid.left() + c as f32 * cell_w + 4.0, y + 1.0), Pos2::new(grid.left() + (c + 1) as f32 * cell_w - 4.0, y + line_h - 2.0));
                    let color = self.cal_color(&ev.calendar_id);
                    draw_line(&grid_painter, &p, rect, color, ev, text::T12, selected_id.as_deref() == Some(ev.id.as_str()), cell_w > 120.0);
                    if let Some(double) = hit(ui, rect, ui.id().with(("mline", &ev.id, d))) {
                        clicked = Some((ev.id.clone(), rect, double));
                    }
                }
                let more = items.len() - show + hidden[c];
                if more > 0 {
                    let y = row_top + (max_lines - 1) as f32 * line_h;
                    let rect = Rect::from_min_size(Pos2::new(grid.left() + c as f32 * cell_w + 4.0, y), Vec2::new(cell_w - 8.0, line_h));
                    grid_painter.text(Pos2::new(rect.left() + 6.0, rect.center().y), Align2::LEFT_CENTER, format!("+{more} {}", t("more")), font_bold(text::T12), p.text_2);
                    if hit(ui, rect, ui.id().with(("more", d))).is_some() {
                        self.select_date(d);
                        self.set_view(ViewMode::Day);
                    }
                }
            }
        }
        // Veil what sits on days outside the month, so they read as dimmer than the month.
        let veil = Color32::from_rgba_unmultiplied(p.outside.r(), p.outside.g(), p.outside.b(), if p.dark { 150 } else { 140 });
        for (d, cell) in &cells {
            if d.month() != month.month() && *d != self.selected_date {
                grid_painter.rect_filled(cell.shrink(1.0), CornerRadius::ZERO, veil);
            }
        }
        // Day numbers.
        for (d, cell) in &cells {
            let pos = Pos2::new(cell.right() - 10.0, cell.top() + 15.0);
            let in_month = d.month() == month.month();
            let label = if d.day() == 1 { format!("{} {}", d.day(), i18n::month_short(d.month())) } else { d.day().to_string() };
            if *d == now {
                let galley = grid_painter.layout_no_wrap(label.clone(), font_bold(text::T14), p.on_primary_strong);
                let w = galley.size().x.max(14.0) + 12.0;
                let pill = Rect::from_center_size(Pos2::new(pos.x - w / 2.0 + 4.0, pos.y), Vec2::new(w, 24.0));
                grid_painter.rect_filled(pill, CornerRadius::same(12), p.primary_strong);
                grid_painter.text(pill.center(), Align2::CENTER_CENTER, label, font_bold(text::T14), p.on_primary_strong);
            } else {
                let col = if in_month { p.text } else { p.outside_text };
                grid_painter.text(pos, Align2::RIGHT_CENTER, label, if d.day() == 1 { font_bold(text::T14) } else { font(text::T14) }, col);
            }
        }
        if let Some((id, rect, double)) = clicked {
            self.click_event(&id, rect, double);
        }
    }

    /// Move the month strip: wheel and trackpad by the pixel, a drag by the pointer, a new
    /// selection or a scrolled list by an eased glide; settle on a week edge when input stops.
    /// Only a change of the focused month or top week is passed on, so the other panes are not
    /// touched while the strip moves within a week.
    fn month_scroll(&mut self, ui: &egui::Ui, resp: &egui::Response, grid: Rect, cell_h: f32) {
        let now = ui.input(|i| i.time);
        let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.1) as f64;
        let row_h = cell_h.max(1.0) as f64;
        if self.month_pos.is_nan() {
            self.month_pos = row_of(week_start(first_of_month(self.selected_date))) as f64;
            self.month_synced = self.selected_date;
            self.month_focus = first_of_month(self.selected_date);
        }
        // A selection made elsewhere (Today, arrows, the mini calendar, the list): another month
        // is framed from its first week; a day of this month is brought into view.
        if self.selected_date != self.month_synced {
            self.month_synced = self.selected_date;
            let row = row_of(self.selected_date) as f64;
            if first_of_month(self.selected_date) != self.month_focus {
                self.month_target = Some(row_of(week_start(first_of_month(self.selected_date))) as f64);
            } else if row < self.month_pos || row + 1.0 > self.month_pos + MONTH_ROWS as f64 {
                self.month_target = Some(if row < self.month_pos { row } else { row + 1.0 - MONTH_ROWS as f64 });
            }
        }
        // The list on the left was scrolled by hand: keep its first day in view.
        if let Some(day) = self.agenda_moved.take() {
            let row = row_of(day) as f64;
            if row < self.month_pos.floor() || row + 1.0 > self.month_pos + MONTH_ROWS as f64 {
                self.month_target = Some(row);
            }
        }
        let busy = self.inspector.is_some() || self.new_event.is_some() || self.settings_open;
        let mut moved = 0.0_f64;
        if !busy && ui.rect_contains_pointer(grid) {
            let dy = ui.input(|i| i.smooth_scroll_delta.y) as f64;
            if dy != 0.0 {
                moved -= dy / row_h;
            }
        }
        if resp.dragged() {
            moved -= resp.drag_delta().y as f64 / row_h;
        }
        if moved != 0.0 {
            self.month_pos += moved;
            self.month_target = None;
            self.month_input_at = now;
            self.month_settle = true;
        } else if self.month_settle && !resp.dragged() && now - self.month_input_at > 0.16 {
            self.month_settle = false;
            self.month_target = Some(self.month_pos.round());
        }
        if let Some(target) = self.month_target {
            let k = if self.reduced_motion() { 1.0 } else { 1.0 - (-dt * 14.0).exp() };
            self.month_pos += (target - self.month_pos) * k;
            if (target - self.month_pos).abs() * row_h < 0.5 {
                self.month_pos = target;
                self.month_target = None;
            }
        }
        if self.month_target.is_some() || self.month_settle {
            ui.ctx().request_repaint();
        }
        // The month owning the middle of the view; the top week for the list.
        let middle = week_of_row((self.month_pos + MONTH_ROWS as f64 / 2.0).floor() as i64) + Duration::days(3);
        let focus = first_of_month(middle);
        if focus != self.month_focus {
            self.month_focus = focus;
            self.mini_month = focus;
        }
        let top = week_of_row(self.month_pos.round() as i64);
        if moved != 0.0 && top != self.month_top_week {
            self.agenda_scroll_to = Some(top);
        }
        self.month_top_week = top;
    }

    /// Three months side by side in one grid (Fantastical's Quarter): every month gets six week
    /// rows so the rows line up across the months.
    pub(super) fn quarter_view(&mut self, ui: &mut egui::Ui) {
        let p = *self.palette();
        let area = ui.available_rect_before_wrap();
        ui.allocate_rect(area, Sense::hover());
        let painter = ui.painter_at(area);
        let start = first_of_month(self.selected_date);
        let block_w = area.width() / 3.0;
        let title_h = 40.0;
        let head_h = 24.0;
        let rows = 6;
        let cell_w = block_w / 7.0;
        let cell_h = (area.height() - title_h - head_h) / rows as f32;
        let now = today();
        let stroke = Stroke::new(1.0_f32, p.line);
        let selected_id = self.highlighted_event();
        let mut clicked: Option<(String, Rect, bool)> = None;
        let line_h = 18.0;
        let max_lines = ((cell_h - 26.0) / line_h).floor() as usize;
        for m in 0..3 {
            let month = add_months(start, m);
            let left = area.left() + m as f32 * block_w;
            let title_rect = Rect::from_min_size(Pos2::new(left, area.top()), Vec2::new(block_w, title_h));
            let current = month.month() == now.month() && month.year() == now.year();
            let tr = painter.text(Pos2::new(left + 14.0, title_rect.center().y), Align2::LEFT_CENTER, i18n::month_name(month.month()), font_bold(text::T18), if current { p.primary_strong } else { p.text });
            painter.text(Pos2::new(tr.right() + 6.0, title_rect.center().y), Align2::LEFT_CENTER, month.year().to_string(), font(text::T18), p.text_2);
            for c in 0..7 {
                let wd = chrono::Weekday::try_from(c as u8).unwrap_or(chrono::Weekday::Mon);
                painter.text(Pos2::new(left + cell_w * (c as f32 + 0.5), area.top() + title_h + head_h * 0.5), Align2::CENTER_CENTER, i18n::weekday_short(wd), font_bold(11.0), if is_weekend(wd) { p.weekend_text } else { p.text_2 });
            }
            let grid_top = area.top() + title_h + head_h;
            let first = week_start(month);
            for r in 0..rows {
                let ws = first + Duration::days((r * 7) as i64);
                for c in 0..7 {
                    let d = ws + Duration::days(c as i64);
                    let cell = Rect::from_min_size(Pos2::new(left + c as f32 * cell_w, grid_top + r as f32 * cell_h), Vec2::new(cell_w, cell_h));
                    let in_month = d.month() == month.month();
                    if !in_month {
                        painter.rect_filled(cell, CornerRadius::ZERO, p.outside);
                        continue;
                    }
                    if d == self.selected_date {
                        painter.rect_filled(cell, CornerRadius::ZERO, p.primary_faint);
                    } else if is_weekend(d.weekday()) {
                        painter.rect_filled(cell, CornerRadius::ZERO, p.weekend);
                    }
                    let resp = ui.interact(cell, ui.id().with(("qday", d, m)), Sense::click());
                    if resp.clicked() {
                        self.select_date(d);
                        self.selected_event = None;
                    }
                    if resp.double_clicked() {
                        self.open_new_event(d, None);
                    }
                    let num = Pos2::new(cell.left() + 8.0, cell.top() + 12.0);
                    if d == now {
                        painter.circle_filled(Pos2::new(num.x + 5.0, num.y), 10.0, p.primary_strong);
                        painter.text(Pos2::new(num.x + 5.0, num.y), Align2::CENTER_CENTER, d.day().to_string(), font_bold(text::T12), p.on_primary_strong);
                    } else {
                        painter.text(num, Align2::LEFT_CENTER, d.day().to_string(), font(text::T12), p.text);
                    }
                    let events = self.events_on(d);
                    if events.is_empty() {
                        continue;
                    }
                    if max_lines == 0 || cell_w < 46.0 {
                        // Too small for text: up to three dots.
                        for (k, ev) in events.iter().take(3).enumerate() {
                            painter.circle_filled(Pos2::new(cell.right() - 8.0 - k as f32 * 7.0, cell.top() + 12.0), 2.6, self.cal_color(&ev.calendar_id));
                        }
                        continue;
                    }
                    let overflow = events.len() > max_lines;
                    let show = if overflow { max_lines.saturating_sub(1) } else { events.len() };
                    for (k, ev) in events.iter().take(show).enumerate() {
                        let y = cell.top() + 24.0 + k as f32 * line_h;
                        let rect = Rect::from_min_max(Pos2::new(cell.left() + 3.0, y), Pos2::new(cell.right() - 3.0, y + line_h - 2.0));
                        let color = self.cal_color(&ev.calendar_id);
                        let sel = selected_id.as_deref() == Some(ev.id.as_str());
                        if ev.all_day || ev.end_date.is_some() {
                            draw_bar(&painter, &p, rect, color, &ev.title, 11.0, sel);
                        } else {
                            draw_line(&painter, &p, rect, color, ev, 11.0, sel, false);
                        }
                        let r = ui.interact(rect, ui.id().with(("qev", &ev.id, d)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                        if r.clicked() || r.double_clicked() {
                            clicked = Some((ev.id.clone(), rect, r.double_clicked()));
                        }
                    }
                    if overflow {
                        let y = cell.top() + 24.0 + show as f32 * line_h + line_h / 2.0;
                        painter.text(Pos2::new(cell.left() + 8.0, y), Align2::LEFT_CENTER, format!("+{}", events.len() - show), font_bold(11.0), p.text_2);
                    }
                }
            }
            // Shared borders inside the month; a stronger line between months.
            for r in 0..=rows {
                let y = (grid_top + r as f32 * cell_h).round() + 0.5;
                painter.line_segment([Pos2::new(left, y), Pos2::new(left + block_w, y)], stroke);
            }
            for c in 1..7 {
                let x = (left + c as f32 * cell_w).round() + 0.5;
                painter.line_segment([Pos2::new(x, grid_top), Pos2::new(x, grid_top + rows as f32 * cell_h)], stroke);
            }
            if m > 0 {
                let x = left.round() + 0.5;
                painter.line_segment([Pos2::new(x, area.top()), Pos2::new(x, area.bottom())], Stroke::new(1.5_f32, over(p.text, 40, p.surface)));
            }
        }
        if let Some((id, rect, double)) = clicked {
            self.click_event(&id, rect, double);
        }
        self.wheel(ui, area, None);
    }

    /// Twelve months on one grid; days with events are tinted by how busy they are.
    pub(super) fn year_view(&mut self, ui: &mut egui::Ui) {
        let p = *self.palette();
        let area = ui.available_rect_before_wrap().shrink(16.0);
        ui.allocate_rect(area, Sense::hover());
        let painter = ui.painter_at(area.expand(16.0));
        let cols = if area.width() / area.height() > 1.25 { 4 } else { 3 };
        let rows = 12 / cols;
        let gap = 16.0;
        let block_w = (area.width() - gap * (cols as f32 - 1.0)) / cols as f32;
        let block_h = (area.height() - gap * (rows as f32 - 1.0)) / rows as f32;
        let year = self.selected_date.year();
        let now = today();
        for i in 0..12 {
            let (bc, br) = (i % cols, i / cols);
            let block = Rect::from_min_size(Pos2::new(area.left() + bc as f32 * (block_w + gap), area.top() + br as f32 * (block_h + gap)), Vec2::new(block_w, block_h));
            let month = NaiveDate::from_ymd_opt(year, i as u32 + 1, 1).unwrap_or(now);
            let current = month.month() == now.month() && year == now.year();
            painter.text(Pos2::new(block.left() + 6.0, block.top() + 12.0), Align2::LEFT_CENTER, i18n::month_name(month.month()), font_bold(text::T16), if current { p.primary_strong } else { p.text });
            let head = 28.0;
            let wd_h = 18.0;
            let cell_w = block_w / 7.0;
            let cell_h = ((block_h - head - wd_h) / 6.0).max(10.0);
            for c in 0..7 {
                let wd = chrono::Weekday::try_from(c as u8).unwrap_or(chrono::Weekday::Mon);
                painter.text(Pos2::new(block.left() + cell_w * (c as f32 + 0.5), block.top() + head + wd_h * 0.4), Align2::CENTER_CENTER, i18n::weekday_letter(wd), font(11.0), p.text_3);
            }
            let first = week_start(month);
            for k in 0..42 {
                let d = first + Duration::days(k);
                if d.month() != month.month() {
                    continue;
                }
                let (r, c) = (k / 7, k % 7);
                let cell = Rect::from_min_size(Pos2::new(block.left() + c as f32 * cell_w, block.top() + head + wd_h + r as f32 * cell_h), Vec2::new(cell_w, cell_h));
                let center = cell.center();
                let radius = (cell_h.min(cell_w) * 0.44).min(14.0);
                let count = self.store.events_on_date(d, "").len();
                let mut col = if c >= 5 { p.text_2 } else { p.text };
                if d == now {
                    painter.circle_filled(center, radius, p.primary_strong);
                    col = p.on_primary_strong;
                } else if d == self.selected_date {
                    painter.circle_stroke(center, radius, Stroke::new(1.5_f32, p.primary_strong));
                }
                if d != now && count > 0 {
                    let heat = match count {
                        1 => p.primary_faint,
                        2 => p.primary_soft,
                        _ => over(p.primary, 140, p.surface),
                    };
                    painter.circle_filled(center, radius, heat);
                }
                painter.text(center, Align2::CENTER_CENTER, d.day().to_string(), if d == now { font_bold(12.0) } else { font(12.0) }, col);
                let resp = ui.interact(cell, ui.id().with(("yday", d)), Sense::click());
                if resp.clicked() {
                    self.select_date(d);
                }
                if resp.double_clicked() {
                    self.select_date(d);
                    self.set_view(ViewMode::Day);
                }
                if count > 0 {
                    resp.on_hover_text(format!("{count} {}", t(if count == 1 { "event" } else { "events" })));
                }
            }
        }
        self.wheel(ui, area, None);
    }

    /// Day (1 column) or Week (7) on a time grid.
    pub(super) fn time_grid(&mut self, ui: &mut egui::Ui, days: usize) {
        let p = *self.palette();
        let start = if days == 1 { self.selected_date } else { week_start(self.selected_date) };
        let gutter = 58.0;
        let width = ui.available_width();
        let col_w = (width - gutter) / days as f32;
        let now = today();
        let selected_id = self.highlighted_event();
        let mut clicked: Option<(String, Rect, bool)> = None;

        // Day headers.
        if days > 1 || !self.is_mobile(ui.ctx()) {
            let (head, _) = ui.allocate_exact_size(Vec2::new(width, 52.0), Sense::hover());
            let painter = ui.painter_at(head);
            for i in 0..days {
                let d = start + Duration::days(i as i64);
                let x = head.left() + gutter + col_w * i as f32;
                let cell = Rect::from_min_size(Pos2::new(x, head.top()), Vec2::new(col_w, head.height()));
                let r = ui.interact(cell, ui.id().with(("whead", d)), Sense::click());
                if r.clicked() {
                    self.select_date(d);
                }
                let label_pos = Pos2::new(x + 12.0, head.center().y);
                let tint = days > 1 && is_weekend(d.weekday());
                if tint {
                    painter.rect_filled(cell, CornerRadius::ZERO, p.weekend);
                }
                let wd = painter.text(label_pos, Align2::LEFT_CENTER, i18n::weekday_short(d.weekday()), font(text::T14), if tint { p.weekend_text } else { p.text_2 });
                let num_pos = Pos2::new(wd.right() + 18.0, head.center().y);
                if d == now {
                    painter.circle_filled(num_pos, 14.0, p.primary_strong);
                    painter.text(num_pos, Align2::CENTER_CENTER, d.day().to_string(), font_bold(text::T16), p.on_primary_strong);
                } else {
                    painter.text(num_pos, Align2::CENTER_CENTER, d.day().to_string(), font_bold(text::T16), if d == self.selected_date { p.primary_strong } else { p.text });
                }
            }
        }
        // All-day lane.
        let (spans, per_day) = week_spans(self, start, days);
        let lanes = per_day.iter().copied().max().unwrap_or(0);
        let lane_h = 22.0;
        let all_h = (lanes.max(1) as f32) * lane_h + 8.0;
        let (all_rect, _) = ui.allocate_exact_size(Vec2::new(width, all_h), Sense::hover());
        {
            let painter = ui.painter_at(all_rect);
            painter.text(Pos2::new(all_rect.left() + gutter - 8.0, all_rect.top() + 14.0), Align2::RIGHT_CENTER, t("all-day"), font(11.0), p.text_2);
            painter.line_segment([Pos2::new(all_rect.left(), all_rect.bottom() - 0.5), Pos2::new(all_rect.right(), all_rect.bottom() - 0.5)], Stroke::new(1.0_f32, p.line));
            // Day cells first so the event bars on top of them get the clicks (as in Month).
            for i in 0..days {
                let d = start + Duration::days(i as i64);
                let cell = Rect::from_min_size(Pos2::new(all_rect.left() + gutter + col_w * i as f32, all_rect.top()), Vec2::new(col_w, all_h));
                if days > 1 && is_weekend(d.weekday()) {
                    painter.rect_filled(cell.with_max_y(all_rect.bottom() - 1.0), CornerRadius::ZERO, p.weekend);
                }
                let r = ui.interact(cell, ui.id().with(("aday", d)), Sense::click());
                if r.double_clicked() {
                    self.open_new_event(d, None);
                } else if r.clicked() {
                    self.selected_event = None;
                }
            }
            for s in &spans {
                let rect = Rect::from_min_max(
                    Pos2::new(all_rect.left() + gutter + s.first as f32 * col_w + 3.0, all_rect.top() + 4.0 + s.lane as f32 * lane_h),
                    Pos2::new(all_rect.left() + gutter + (s.last + 1) as f32 * col_w - 3.0, all_rect.top() + 4.0 + s.lane as f32 * lane_h + lane_h - 3.0),
                );
                let color = self.cal_color(&s.ev.calendar_id);
                draw_bar(&painter, &p, rect, color, &s.ev.title, text::T12, selected_id.as_deref() == Some(s.ev.id.as_str()));
                let r = ui.interact(rect, ui.id().with(("aspan", &s.ev.id)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                if r.clicked() || r.double_clicked() {
                    clicked = Some((s.ev.id.clone(), rect, r.double_clicked()));
                }
            }
        }
        // Hours.
        let hour_h = 52.0;
        let scroll_to = (!self.timeline_scrolled).then_some(self.prefs.day_start as f32 * hour_h);
        self.timeline_scrolled = true;
        let mut sa = egui::ScrollArea::vertical().id_salt(("hours", days)).auto_shrink([false, false]);
        if let Some(y) = scroll_to {
            sa = sa.vertical_scroll_offset(y);
        }
        let mut new_at: Option<(NaiveDate, NaiveTime)> = None;
        sa.show(ui, |ui| {
            let (canvas, _) = ui.allocate_exact_size(Vec2::new(width, hour_h * 24.0), Sense::hover());
            let painter = ui.painter_at(canvas);
            let work = Rect::from_min_max(
                Pos2::new(canvas.left() + gutter, canvas.top() + self.store.settings.work_day_start as f32 * hour_h),
                Pos2::new(canvas.right(), canvas.top() + self.store.settings.work_day_end as f32 * hour_h),
            );
            painter.rect_filled(Rect::from_min_max(Pos2::new(canvas.left() + gutter, canvas.top()), canvas.max), CornerRadius::ZERO, over(p.text, 4, p.surface));
            painter.rect_filled(work, CornerRadius::ZERO, p.surface);
            // Weekend columns: the tint over working hours, a little deeper outside them.
            if days > 1 {
                for i in 0..days {
                    let d = start + Duration::days(i as i64);
                    if !is_weekend(d.weekday()) {
                        continue;
                    }
                    let x = canvas.left() + gutter + col_w * i as f32;
                    let col = Rect::from_min_size(Pos2::new(x, canvas.top()), Vec2::new(col_w, canvas.height()));
                    painter.rect_filled(col, CornerRadius::ZERO, over(p.text, 4, p.weekend));
                    painter.rect_filled(col.intersect(work), CornerRadius::ZERO, p.weekend);
                }
            }
            for h in 0..24 {
                let y = canvas.top() + h as f32 * hour_h;
                painter.line_segment([Pos2::new(canvas.left() + gutter, y + 0.5), Pos2::new(canvas.right(), y + 0.5)], Stroke::new(1.0_f32, p.line));
                if h > 0 {
                    painter.text(Pos2::new(canvas.left() + gutter - 8.0, y), Align2::RIGHT_CENTER, format!("{h:02}:00"), font(11.0), p.text_2);
                }
            }
            for i in 0..days {
                let x = canvas.left() + gutter + col_w * i as f32;
                painter.line_segment([Pos2::new(x + 0.5, canvas.top()), Pos2::new(x + 0.5, canvas.bottom())], Stroke::new(1.0_f32, p.line));
                let d = start + Duration::days(i as i64);
                let col = Rect::from_min_size(Pos2::new(x, canvas.top()), Vec2::new(col_w, canvas.height()));
                let r = ui.interact(col, ui.id().with(("tcol", d)), Sense::click());
                if r.double_clicked() {
                    if let Some(pos) = r.interact_pointer_pos() {
                        let minutes = (((pos.y - canvas.top()) / hour_h) * 60.0 / 30.0).floor() as u32 * 30;
                        new_at = Some((d, NaiveTime::from_hms_opt((minutes / 60).min(23), minutes % 60, 0).unwrap_or(NaiveTime::MIN)));
                    }
                } else if r.clicked() {
                    self.select_date(d);
                    self.selected_event = None;
                }
                // Overlapping events share the column.
                let mut items = timed(self, d);
                items.sort_by_key(|e| e.start_time);
                let mut placed: Vec<(CalendarEvent, f32, f32, usize, usize)> = Vec::new();
                let mut cluster: Vec<usize> = Vec::new();
                let mut cluster_end = 0.0f32;
                let mut columns_end: Vec<f32> = Vec::new();
                let finish = |placed: &mut Vec<(CalendarEvent, f32, f32, usize, usize)>, cluster: &mut Vec<usize>, n: usize| {
                    for idx in cluster.drain(..) {
                        placed[idx].4 = n;
                    }
                };
                for ev in items {
                    let s = ev.start_time.map(|t| t.hour() as f32 + t.minute() as f32 / 60.0).unwrap_or(9.0);
                    let e = ev.end_time.map(|t| t.hour() as f32 + t.minute() as f32 / 60.0).filter(|e| *e > s).unwrap_or(s + 1.0).max(s + 0.5);
                    if s >= cluster_end && !cluster.is_empty() {
                        let n = columns_end.len();
                        finish(&mut placed, &mut cluster, n);
                        columns_end.clear();
                    }
                    let col_idx = columns_end.iter().position(|end| *end <= s).unwrap_or(columns_end.len());
                    if col_idx == columns_end.len() {
                        columns_end.push(e);
                    } else {
                        columns_end[col_idx] = e;
                    }
                    cluster_end = cluster_end.max(e);
                    placed.push((ev, s, e, col_idx, 1));
                    cluster.push(placed.len() - 1);
                }
                let n = columns_end.len();
                finish(&mut placed, &mut cluster, n);
                for (ev, s, e, ci, n) in placed {
                    let w = (col_w - 6.0) / n as f32;
                    let rect = Rect::from_min_max(Pos2::new(x + 3.0 + w * ci as f32, canvas.top() + s * hour_h + 1.0), Pos2::new(x + 3.0 + w * (ci + 1) as f32 - 2.0, canvas.top() + e * hour_h - 1.0));
                    let color = self.cal_color(&ev.calendar_id);
                    let sel = selected_id.as_deref() == Some(ev.id.as_str());
                    painter.rect_filled(rect, CornerRadius::same(6), if sel { color } else { theme::event_fill(color, &p) });
                    painter.rect_filled(Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height())), CornerRadius { nw: 6, sw: 6, ne: 0, se: 0 }, color);
                    let txt = if sel { contrast_on(color) } else { theme::event_text(color, &p) };
                    let clip = painter.with_clip_rect(rect.shrink(2.0));
                    clip.text(Pos2::new(rect.left() + 9.0, rect.top() + 11.0), Align2::LEFT_CENTER, &ev.title, font_bold(text::T12), txt);
                    if rect.height() > 34.0 {
                        let mut sub = match (ev.start_time, ev.end_time) {
                            (Some(a), Some(b)) => format!("{} – {}", a.format("%H:%M"), b.format("%H:%M")),
                            (Some(a), None) => a.format("%H:%M").to_string(),
                            _ => String::new(),
                        };
                        if let Some(l) = ev.location.as_ref().filter(|l| !l.trim().is_empty()) {
                            sub = format!("{sub} · {}", super::side::one_line(l));
                        }
                        clip.text(Pos2::new(rect.left() + 9.0, rect.top() + 27.0), Align2::LEFT_CENTER, sub, font(11.0), txt);
                    }
                    let r = ui.interact(rect, ui.id().with(("tev", &ev.id, d)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                    if r.clicked() || r.double_clicked() {
                        clicked = Some((ev.id.clone(), rect, r.double_clicked()));
                    }
                }
                if d == now {
                    let t = chrono::Local::now().time();
                    let y = canvas.top() + (t.hour() as f32 + t.minute() as f32 / 60.0) * hour_h;
                    let red = Color32::from_rgb(0xFF, 0x47, 0x47);
                    painter.circle_filled(Pos2::new(x + 1.0, y), 4.5, red);
                    painter.line_segment([Pos2::new(x, y), Pos2::new(x + col_w, y)], Stroke::new(1.5_f32, red));
                }
            }
        });
        if let Some((d, t)) = new_at {
            self.open_new_event(d, Some(t));
        }
        if let Some((id, rect, double)) = clicked {
            self.click_event(&id, rect, double);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_rows_count_weeks_from_a_monday() {
        let d = NaiveDate::from_ymd_opt(2026, 11, 24).unwrap();
        let row = row_of(d);
        assert_eq!(week_of_row(row), week_start(d));
        assert_eq!(week_of_row(row + 1), week_start(d) + Duration::days(7));
        // Every day of a week shares its row; the next Monday starts the next one.
        for k in 0..7 {
            assert_eq!(row_of(week_start(d) + Duration::days(k)), row);
        }
        assert_eq!(row_of(week_start(d) + Duration::days(7)), row + 1);
        // Before 1970 too.
        let old = NaiveDate::from_ymd_opt(1969, 12, 31).unwrap();
        assert_eq!(week_of_row(row_of(old)), week_start(old));
    }
}
