//! The frame around the calendar: the toolbar, the sidebar (quick entry, full-width mini
//! calendar with event dots, calendars, and the day-by-day list that scrolls with the
//! selection), and the phone layout.

use chrono::{Datelike, Duration, NaiveDate};
use eframe::egui::{self, Align, Align2, Color32, CornerRadius, Layout, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use super::{first_of_month, today, week_start, CalendarCraftApp};
use crate::i18n::{self, t};
use crate::model::{AccountKind, CalendarEvent, ViewMode};
use crate::nlp::parse_quick_entry;
use crate::theme::{self, font, font_bold, space, text};

const SIDEBAR_W: f32 = 288.0;

impl CalendarCraftApp {
    pub(super) fn desktop_layout(&mut self, ctx: &egui::Context) {
        let p = *self.palette();
        egui::TopBottomPanel::top("toolbar")
            .exact_height(60.0)
            .frame(egui::Frame::new().fill(p.surface).inner_margin(egui::Margin::symmetric(16, 0)).stroke(Stroke::new(1.0_f32, p.line)))
            .show(ctx, |ui| self.toolbar(ui));
        if self.prefs.sidebar {
            egui::SidePanel::left("sidebar")
                .exact_width(SIDEBAR_W)
                .resizable(false)
                .frame(egui::Frame::new().fill(p.sidebar).inner_margin(egui::Margin::same(16)).stroke(Stroke::new(1.0_f32, p.line)))
                .show(ctx, |ui| self.sidebar(ui));
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.surface)).show(ctx, |ui| self.calendar_area(ui));
    }

    pub(super) fn calendar_area(&mut self, ui: &mut egui::Ui) {
        match self.view_mode {
            ViewMode::Day => self.time_grid(ui, 1),
            ViewMode::Week => self.time_grid(ui, 7),
            ViewMode::Month => self.month_view(ui),
            ViewMode::Quarter => self.quarter_view(ui),
            ViewMode::Year => self.year_view(ui),
        }
    }

    fn title_text(&self) -> (String, String) {
        let d = self.selected_date;
        match self.view_mode {
            ViewMode::Day => (format!("{} {}", d.day(), i18n::month_name(d.month())), d.year().to_string()),
            ViewMode::Week => {
                let s = week_start(d);
                let e = s + Duration::days(6);
                if s.month() == e.month() {
                    (i18n::month_name(s.month()).to_string(), s.year().to_string())
                } else {
                    (format!("{} – {}", i18n::month_short(s.month()), i18n::month_short(e.month())), e.year().to_string())
                }
            }
            ViewMode::Month => (i18n::month_name(d.month()).to_string(), d.year().to_string()),
            ViewMode::Quarter => {
                let first = first_of_month(d);
                let last = super::add_months(first, 2);
                (format!("{} – {}", i18n::month_short(first.month()), i18n::month_short(last.month())), last.year().to_string())
            }
            ViewMode::Year => (d.year().to_string(), String::new()),
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let p = *self.palette();
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = space::S8;
            if icon_button(ui, &p, "☰", t("Show or hide the sidebar")).clicked() {
                self.prefs.sidebar = !self.prefs.sidebar;
                self.mark_changed();
            }
            ui.add_space(space::S8);
            let (big, small) = self.title_text();
            ui.label(RichText::new(big).font(font_bold(text::T24)).color(p.text));
            if !small.is_empty() {
                ui.label(RichText::new(small).font(font(text::T24)).color(p.text_2));
            }
            if self.busy > 0 {
                ui.add(egui::Spinner::new().size(14.0).color(p.primary));
            }
            if let Some(doc) = &self.file_doc {
                let label = if doc.dirty { format!("{} •", doc.name) } else { doc.name.clone() };
                pill(ui, &p, &label);
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = space::S8;
                if icon_button(ui, &p, "⚙", t("Settings")).clicked() {
                    self.settings_open = true;
                }
                if theme::primary_button(ui, &p, &format!("+  {}", t("New"))).on_hover_text(t("New event (⌘N)")).clicked() {
                    self.open_new_event(self.selected_date, None);
                }
                ui.add(egui::TextEdit::singleline(&mut self.search_query).hint_text(format!("🔍  {}", t("Search"))).desired_width(170.0).margin(egui::Margin::symmetric(10, 7)));
                ui.add_space(space::S8);
                segmented(ui, &p, &mut self.view_mode, &[ViewMode::Day, ViewMode::Week, ViewMode::Month, ViewMode::Quarter, ViewMode::Year]).then(|| {
                    self.prefs.view = self.view_mode;
                    self.timeline_scrolled = false;
                    self.mark_changed();
                });
                ui.add_space(space::S8);
                if icon_button(ui, &p, "›", t("Next")).clicked() {
                    self.step(1);
                }
                if ui.add(egui::Button::new(t("Today")).min_size(Vec2::new(0.0, 32.0))).clicked() {
                    self.select_date(today());
                }
                if icon_button(ui, &p, "‹", t("Previous")).clicked() {
                    self.step(-1);
                }
            });
        });
    }

    pub(super) fn sidebar(&mut self, ui: &mut egui::Ui) {
        let p = *self.palette();
        ui.spacing_mut().item_spacing.y = space::S12;
        self.quick_entry(ui);
        self.mini_calendar(ui, true);
        ui.add_space(space::S4);
        self.calendars_section(ui);
        sep(ui, &p);
        self.agenda(ui, ui.available_height());
    }

    fn quick_entry(&mut self, ui: &mut egui::Ui) {
        let p = *self.palette();
        let resp = ui.add(
            egui::TextEdit::singleline(&mut self.quick_input)
                .hint_text(t("Lunch with Maya tomorrow at 12:30"))
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(12, 9)),
        );
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if !self.quick_input.trim().is_empty() {
            let cal = self.default_calendar().unwrap_or_default();
            if let Some(parsed) = parse_quick_entry(&self.quick_input, self.selected_date, &self.store.calendars, &cal) {
                let when = match (parsed.all_day, parsed.start_time) {
                    (false, Some(s)) => format!("{} {}, {}", i18n::weekday_short(parsed.date.weekday()), parsed.date.format("%-d.%-m."), s.format("%H:%M")),
                    _ => format!("{} {}", i18n::weekday_short(parsed.date.weekday()), parsed.date.format("%-d.%-m.")),
                };
                ui.label(RichText::new(format!("{} · {when}", parsed.title)).font(font(text::T12)).color(p.text_2));
                if enter {
                    let day = parsed.date;
                    let id = self.store.next_id("ev");
                    let ev = parsed.into_event(id, &cal);
                    let title = ev.title.clone();
                    self.commit_event(ev);
                    self.select_date(day);
                    self.quick_input.clear();
                    self.flash(format!("{} \"{title}\"", t("Added")), false);
                }
            }
        }
    }

    /// The month at full sidebar width; a day shows one dot for one event, two for more.
    pub(super) fn mini_calendar(&mut self, ui: &mut egui::Ui, with_header: bool) {
        let p = *self.palette();
        let month = self.mini_month;
        if with_header {
            ui.horizontal(|ui| {
                ui.label(RichText::new(i18n::month_name(month.month())).font(font_bold(text::T16)));
                ui.label(RichText::new(month.year().to_string()).font(font(text::T16)).color(p.text_2));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if small_icon(ui, &p, "›").clicked() {
                        self.mini_month = super::add_months(month, 1);
                    }
                    if small_icon(ui, &p, "‹").clicked() {
                        self.mini_month = super::add_months(month, -1);
                    }
                });
            });
        }
        let width = ui.available_width();
        let cell_w = width / 7.0;
        let cell_h = (cell_w * 0.92).clamp(30.0, 44.0);
        let head_h = 20.0;
        let first = week_start(month);
        let rows = 6;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, head_h + cell_h * rows as f32), Sense::hover());
        let painter = ui.painter_at(rect);
        for c in 0..7 {
            let wd = chrono::Weekday::try_from(c as u8).unwrap_or(chrono::Weekday::Mon);
            let x = rect.left() + cell_w * (c as f32 + 0.5);
            painter.text(Pos2::new(x, rect.top() + head_h * 0.5), Align2::CENTER_CENTER, i18n::weekday_letter(wd), font(text::T12), p.text_2);
        }
        let now = today();
        for r in 0..rows {
            for c in 0..7 {
                let day = first + Duration::days((r * 7 + c) as i64);
                let cell = Rect::from_min_size(Pos2::new(rect.left() + c as f32 * cell_w, rect.top() + head_h + r as f32 * cell_h), Vec2::new(cell_w, cell_h));
                let resp = ui.interact(cell, ui.id().with(("mini", day)), Sense::click());
                let center = Pos2::new(cell.center().x, cell.top() + cell_h * 0.42);
                let radius = (cell_h * 0.36).min(15.0);
                let in_month = day.month() == month.month();
                let selected = day == self.selected_date;
                let mut num_color = if in_month { p.text } else { p.text_3 };
                if day == now {
                    painter.circle_filled(center, radius, p.primary_strong);
                    num_color = p.on_primary_strong;
                } else if selected {
                    painter.circle_filled(center, radius, p.primary_soft);
                } else if resp.hovered() {
                    painter.circle_filled(center, radius, p.hover);
                }
                let f = if day == now || selected { font_bold(text::T14) } else { font(text::T14) };
                painter.text(center, Align2::CENTER_CENTER, day.day().to_string(), f, num_color);
                let events = self.store.events_on_date(day, "");
                let dots: Vec<Color32> = events.iter().take(2).map(|e| self.cal_color(&e.calendar_id)).collect();
                let y = cell.top() + cell_h * 0.86;
                match dots.len() {
                    1 => {
                        painter.circle_filled(Pos2::new(cell.center().x, y), 2.4, dots[0]);
                    }
                    2 => {
                        painter.circle_filled(Pos2::new(cell.center().x - 3.5, y), 2.4, dots[0]);
                        painter.circle_filled(Pos2::new(cell.center().x + 3.5, y), 2.4, dots[1]);
                    }
                    _ => {}
                }
                if resp.clicked() {
                    self.select_date(day);
                    if self.view_mode == ViewMode::Year {
                        self.set_view(ViewMode::Month);
                    }
                }
                if resp.double_clicked() {
                    self.open_new_event(day, None);
                }
            }
        }
    }

    fn calendars_section(&mut self, ui: &mut egui::Ui) {
        let p = *self.palette();
        let header = ui.horizontal(|ui| {
            let arrow = if self.calendars_open { "⏷" } else { "⏵" };
            let r = ui.add(egui::Label::new(RichText::new(format!("{arrow}  {}", t("Calendars"))).font(font_bold(text::T14)).color(p.text_2)).sense(Sense::click()));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if small_icon(ui, &p, "+").on_hover_text(t("Add a calendar account")).clicked() {
                    self.settings_open = true;
                    self.settings_tab = 1;
                }
                if small_icon(ui, &p, "⟳").on_hover_text(t("Refresh")).clicked() {
                    self.refresh_all();
                }
            });
            r
        });
        if header.inner.clicked() {
            self.calendars_open = !self.calendars_open;
        }
        if !self.calendars_open {
            return;
        }
        // Calendar sets as chips.
        if self.store.calendar_sets.len() > 1 {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                for set in self.store.calendar_sets.clone() {
                    let on = self.store.settings.active_calendar_set.as_deref() == Some(set.id.as_str());
                    let text = RichText::new(&set.name).font(font(text::T12)).color(if on { p.on_primary_strong } else { p.text });
                    let b = egui::Button::new(text).fill(if on { p.primary_strong } else { p.hover }).corner_radius(CornerRadius::same(255)).min_size(Vec2::new(0.0, 24.0));
                    if ui.add(b).clicked() {
                        self.store.settings.active_calendar_set = Some(set.id.clone());
                        self.mark_changed();
                    }
                }
            });
        }
        let accounts = self.store.accounts.clone();
        let max_h = (ui.available_height() * 0.45).clamp(120.0, 240.0);
        egui::ScrollArea::vertical().id_salt("calendars").max_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for acc in &accounts {
                let cals: Vec<_> = self.store.calendars.iter().filter(|c| c.account_id == acc.id).cloned().collect();
                if cals.is_empty() && acc.kind == AccountKind::Local {
                    continue;
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&acc.name).font(font(text::T12)).color(p.text_2));
                    if let Some(err) = self.status.get(&acc.id) {
                        ui.label(RichText::new("⚠").color(p.danger)).on_hover_text(err);
                    }
                });
                for cal in cals {
                    let color = theme::color_of(&cal.color_hex);
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(rect, CornerRadius::same(6), p.hover);
                    }
                    let box_r = Rect::from_center_size(Pos2::new(rect.left() + 12.0, rect.center().y), Vec2::splat(14.0));
                    if cal.visible {
                        ui.painter().rect_filled(box_r, CornerRadius::same(4), color);
                        ui.painter().text(box_r.center(), Align2::CENTER_CENTER, "✓", font_bold(10.0), Color32::WHITE);
                    } else {
                        ui.painter().rect_stroke(box_r, CornerRadius::same(4), Stroke::new(1.5_f32, color), egui::StrokeKind::Inside);
                    }
                    let mut label = cal.name.clone();
                    if cal.read_only {
                        label.push_str("  🔒");
                    }
                    ui.painter().text(Pos2::new(rect.left() + 28.0, rect.center().y), Align2::LEFT_CENTER, label, font(text::T14), p.text);
                    if resp.clicked() {
                        if let Some(c) = self.store.calendars.iter_mut().find(|c| c.id == cal.id) {
                            c.visible = !c.visible;
                        }
                        self.mark_changed();
                    }
                }
            }
        });
    }

    /// Days with their events, starting a month before the selection. Scrolling it moves the
    /// selection (and with it the calendar); selecting elsewhere scrolls it.
    pub(super) fn agenda(&mut self, ui: &mut egui::Ui, height: f32) {
        let p = *self.palette();
        if self.selected_date < self.agenda_center - Duration::days(25) || self.selected_date > self.agenda_center + Duration::days(80) {
            self.agenda_center = self.selected_date;
            self.agenda_scroll_to = Some(self.selected_date);
        }
        let start = self.agenda_center - Duration::days(30);
        let end = self.agenda_center + Duration::days(90);
        let now = today();
        let jump = self.agenda_scroll_to.take();
        let mut headers: Vec<(NaiveDate, f32)> = Vec::new();
        let mut clicked: Option<(String, Rect)> = None;
        let mut picked_day: Option<NaiveDate> = None;
        let out = egui::ScrollArea::vertical().id_salt("agenda").max_height(height).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let mut d = start;
            while d <= end {
                let events = self.events_on(d);
                if events.is_empty() && d != now && d != self.selected_date && jump != Some(d) {
                    d += Duration::days(1);
                    continue;
                }
                let is_today = d == now;
                let head = ui.horizontal(|ui| {
                    let mut label = format!("{}, {} {}", i18n::weekday_name(d.weekday()), d.day(), i18n::month_short(d.month()));
                    if is_today {
                        label = format!("{} · {label}", t("Today"));
                    }
                    let color = if is_today { p.primary_strong } else if d == self.selected_date { p.text } else { p.text_2 };
                    let r = ui.add(egui::Label::new(RichText::new(label).font(font_bold(text::T12)).color(color)).sense(Sense::click()));
                    if r.clicked() {
                        picked_day = Some(d);
                    }
                    if r.double_clicked() {
                        self.open_new_event(d, None);
                    }
                });
                let top = head.response.rect.top();
                headers.push((d, top));
                if jump == Some(d) {
                    ui.scroll_to_rect(head.response.rect, Some(Align::TOP));
                }
                if events.is_empty() {
                    ui.label(RichText::new(t("No events")).font(font(text::T12)).color(p.text_3));
                }
                for ev in &events {
                    if let Some(rect) = agenda_row(ui, &p, ev, self.cal_color(&ev.calendar_id), d, self.inspector.as_ref().is_some_and(|i| i.event_id == ev.id)) {
                        clicked = Some((ev.id.clone(), rect));
                    }
                }
                ui.add_space(space::S8);
                d += Duration::days(1);
            }
        });
        // The list moved under the user's hand: select the day at its top.
        let offset = out.state.offset.y;
        let user_scrolled = (offset - self.agenda_offset).abs() > 0.5 && jump.is_none() && ui.rect_contains_pointer(out.inner_rect);
        self.agenda_offset = offset;
        if user_scrolled {
            let top = out.inner_rect.top() + 4.0;
            if let Some((day, _)) = headers.iter().rev().find(|(_, y)| *y <= top).or(headers.first()) {
                if *day != self.selected_date {
                    self.selected_date = *day;
                    self.mini_month = first_of_month(*day);
                }
            }
        }
        if let Some(day) = picked_day {
            self.selected_date = day;
            self.mini_month = first_of_month(day);
        }
        if let Some((id, rect)) = clicked {
            self.open_inspector(&id, Some(rect));
        }
    }

    pub(super) fn mobile_layout(&mut self, ctx: &egui::Context) {
        let p = *self.palette();
        egui::TopBottomPanel::top("m_top")
            .frame(egui::Frame::new().fill(p.surface).inner_margin(egui::Margin::symmetric(12, 10)).stroke(Stroke::new(1.0_f32, p.line)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let d = self.selected_date;
                    ui.label(RichText::new(i18n::month_name(d.month())).font(font_bold(text::T18)));
                    ui.label(RichText::new(d.year().to_string()).font(font(text::T18)).color(p.text_2));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if theme::primary_button(ui, &p, "+").clicked() {
                            self.open_new_event(self.selected_date, None);
                        }
                        if icon_button(ui, &p, "⚙", t("Settings")).clicked() {
                            self.settings_open = true;
                        }
                        if ui.button(t("Today")).clicked() {
                            self.select_date(today());
                        }
                    });
                });
                ui.add_space(space::S4);
                let mut mode = if self.view_mode == ViewMode::Day { ViewMode::Day } else { ViewMode::Month };
                if segmented(ui, &p, &mut mode, &[ViewMode::Day, ViewMode::Month]) {
                    self.set_view(mode);
                }
            });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.surface).inner_margin(egui::Margin::symmetric(12, 8))).show(ctx, |ui| {
            if self.view_mode == ViewMode::Day {
                // A week strip, then the day.
                self.week_strip(ui);
                sep(ui, &p);
                self.time_grid(ui, 1);
            } else {
                self.mini_month = first_of_month(self.selected_date);
                // Swipe or scroll sideways to change month.
                let (dx, dy) = ui.input(|i| (i.smooth_scroll_delta.x, i.smooth_scroll_delta.y));
                let _ = dy;
                self.scroll_accum += dx;
                if self.scroll_accum.abs() > 80.0 {
                    let dir = if self.scroll_accum < 0.0 { 1 } else { -1 };
                    self.select_date(super::add_months(self.selected_date, dir));
                    self.scroll_accum = 0.0;
                }
                self.mini_calendar(ui, false);
                sep(ui, &p);
                let h = ui.available_height();
                self.agenda(ui, h);
            }
        });
    }

    fn week_strip(&mut self, ui: &mut egui::Ui) {
        let p = *self.palette();
        let s = week_start(self.selected_date);
        let w = ui.available_width() / 7.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 54.0), Sense::hover());
        for i in 0..7 {
            let d = s + Duration::days(i);
            let cell = Rect::from_min_size(Pos2::new(rect.left() + w * i as f32, rect.top()), Vec2::new(w, 54.0));
            let r = ui.interact(cell, ui.id().with(("strip", d)), Sense::click());
            let painter = ui.painter();
            painter.text(Pos2::new(cell.center().x, cell.top() + 12.0), Align2::CENTER_CENTER, i18n::weekday_letter(d.weekday()), font(text::T12), p.text_2);
            let c = Pos2::new(cell.center().x, cell.top() + 36.0);
            let mut col = p.text;
            if d == today() {
                painter.circle_filled(c, 15.0, p.primary_strong);
                col = p.on_primary_strong;
            } else if d == self.selected_date {
                painter.circle_filled(c, 15.0, p.primary_soft);
            }
            painter.text(c, Align2::CENTER_CENTER, d.day().to_string(), font_bold(text::T16), col);
            if r.clicked() {
                self.select_date(d);
            }
        }
        let dx = ui.input(|i| i.smooth_scroll_delta.x);
        self.scroll_accum += dx;
        if self.scroll_accum.abs() > 80.0 {
            let dir = if self.scroll_accum < 0.0 { 7 } else { -7 };
            self.select_date(self.selected_date + Duration::days(dir));
            self.scroll_accum = 0.0;
        }
    }
}

fn agenda_row(ui: &mut egui::Ui, p: &theme::Palette, ev: &CalendarEvent, color: Color32, day: NaiveDate, selected: bool) -> Option<Rect> {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 40.0), Sense::click());
    let painter = ui.painter_at(rect);
    if selected {
        painter.rect_filled(rect, CornerRadius::same(8), p.primary_faint);
    } else if resp.hovered() {
        painter.rect_filled(rect, CornerRadius::same(8), p.hover);
    }
    let bar = Rect::from_min_size(Pos2::new(rect.left() + 6.0, rect.top() + 7.0), Vec2::new(3.0, rect.height() - 14.0));
    painter.rect_filled(bar, CornerRadius::same(2), color);
    let title_pos = Pos2::new(rect.left() + 18.0, rect.top() + 13.0);
    painter.text(title_pos, Align2::LEFT_CENTER, &ev.title, font(text::T14), p.text);
    let when = if ev.all_day || ev.start_time.is_none() {
        t("all-day").to_string()
    } else if ev.date != day && !ev.starts_on(day) {
        t("continues").to_string()
    } else {
        match (ev.start_time, ev.end_time) {
            (Some(s), Some(e)) => format!("{} – {}", s.format("%H:%M"), e.format("%H:%M")),
            (Some(s), None) => s.format("%H:%M").to_string(),
            _ => String::new(),
        }
    };
    let sub = match &ev.location {
        Some(l) if !l.is_empty() => format!("{when} · {l}"),
        _ => when,
    };
    painter.text(Pos2::new(rect.left() + 18.0, rect.top() + 29.0), Align2::LEFT_CENTER, sub, font(text::T12), p.text_2);
    resp.clicked().then_some(rect)
}

pub fn sep(ui: &mut egui::Ui, p: &theme::Palette) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::ZERO, p.line);
}

pub fn icon_button(ui: &mut egui::Ui, p: &theme::Palette, icon: &str, tip: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(icon).font(font(text::T16)).color(p.text)).fill(Color32::TRANSPARENT).min_size(Vec2::new(32.0, 32.0))).on_hover_text(tip)
}

pub fn small_icon(ui: &mut egui::Ui, p: &theme::Palette, icon: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(icon).font(font(text::T14)).color(p.text_2)).fill(Color32::TRANSPARENT).min_size(Vec2::new(24.0, 24.0)))
}

pub fn pill(ui: &mut egui::Ui, p: &theme::Palette, label: &str) {
    egui::Frame::new().fill(p.primary_faint).corner_radius(CornerRadius::same(255)).inner_margin(egui::Margin::symmetric(10, 3)).show(ui, |ui| {
        ui.label(RichText::new(label).font(font(text::T12)).color(p.primary_strong));
    });
}

/// SnowUI segmented control. Returns true when the choice changed.
pub fn segmented(ui: &mut egui::Ui, p: &theme::Palette, value: &mut ViewMode, options: &[ViewMode]) -> bool {
    let mut changed = false;
    egui::Frame::new().fill(p.hover).corner_radius(CornerRadius::same(10)).inner_margin(egui::Margin::same(3)).show(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.horizontal(|ui| {
            for mode in options {
                let on = *value == *mode;
                let label = RichText::new(t(mode.label())).font(if on { font_bold(text::T14) } else { font(text::T14) }).color(if on { p.text } else { p.text_2 });
                let b = egui::Button::new(label).fill(if on { p.surface } else { Color32::TRANSPARENT }).corner_radius(CornerRadius::same(8)).min_size(Vec2::new(0.0, 28.0)).stroke(if on { Stroke::new(1.0_f32, p.line) } else { Stroke::NONE });
                if ui.add(b).clicked() && !on {
                    *value = *mode;
                    changed = true;
                }
            }
        });
    });
    changed
}
