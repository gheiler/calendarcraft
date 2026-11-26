//! The frame around the calendar: the toolbar, the sidebar (quick entry, full-width mini
//! calendar with event dots, calendars, and the day-by-day list that scrolls with the
//! selection), and the phone layout.

use chrono::{Datelike, Duration, NaiveDate};
use eframe::egui::{self, Align, Align2, Color32, CornerRadius, Layout, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use super::icons::{self, Icon};
use super::{first_of_month, today, week_start, CalendarCraftApp};
use crate::i18n::{self, t};
use crate::model::{AccountKind, CalendarEvent, ViewMode};
use crate::nlp::parse_quick_entry;
use crate::theme::{self, font, font_bold, space, text};

const SIDEBAR_W: f32 = 288.0;
const SIDEBAR_MARGIN: egui::Margin = egui::Margin { left: 16, right: 16, top: 12, bottom: 16 };
const TOOLBAR_H: f32 = 60.0;
/// Narrower windows show the sidebar over the calendar instead of beside it.
const DOCK_MIN_W: f32 = 1000.0;
/// Narrower windows get the view drop-down instead of the pills. These widths are egui
/// points, which match CSS pixels on the web and logical points on the desktop.
const COMPACT_W: f32 = 1400.0;
/// At this width and below, the search field folds into an icon that expands on click.
const SEARCH_ICON_W: f32 = 1110.0;
/// The tight layout: a smaller title, and New becomes "+".
const TIGHT_W: f32 = 860.0;
/// Leftover space the reserved title and the controls must leave, so egui does not shrink
/// a widget until it touches its neighbour.
const FIT_GAP: f32 = 16.0;
/// The left-edge strip that peeks the sidebar, and how long the pointer rests there.
const EDGE_W: f32 = 6.0;
const PEEK_DELAY_MS: u64 = 250;

impl CalendarCraftApp {
    /// Wide windows dock the sidebar (full height, the toolbar beside it, its toggle in the
    /// sidebar's own top row). When it is hidden, or the window is narrower than `DOCK_MIN_W`,
    /// the same toggle sits in the toolbar's leading slot, at the same spot, and the sidebar
    /// comes back docked (wide) or over the calendar (narrow). Resting the pointer on the left
    /// edge peeks it, and ⌘\ / Ctrl+\ toggles it.
    pub(super) fn desktop_layout(&mut self, ctx: &egui::Context) {
        let p = *self.palette();
        let docked = self.sidebar_docked(ctx);
        if docked {
            self.sidebar_overlay = false;
            egui::SidePanel::left("sidebar")
                .exact_width(SIDEBAR_W)
                .resizable(false)
                .frame(egui::Frame::new().fill(p.sidebar).inner_margin(SIDEBAR_MARGIN).stroke(Stroke::new(1.0_f32, p.line)))
                .show(ctx, |ui| self.sidebar(ui));
        }
        egui::TopBottomPanel::top("toolbar")
            .exact_height(TOOLBAR_H)
            .frame(egui::Frame::new().fill(p.surface).inner_margin(egui::Margin::symmetric(16, 0)).stroke(Stroke::new(1.0_f32, p.line)))
            .show(ctx, |ui| self.toolbar(ui, docked));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.surface)).show(ctx, |ui| self.calendar_area(ui));
        if !docked {
            self.edge_peek(ctx);
            if self.sidebar_overlay {
                self.sidebar_over(ctx, &p);
            }
        }
    }

    pub fn sidebar_docked(&self, ctx: &egui::Context) -> bool {
        self.prefs.sidebar && ctx.screen_rect().width() >= DOCK_MIN_W
    }

    /// The toggle: hide a docked sidebar; otherwise dock it again (wide) or open it over the
    /// calendar (narrow); an open overlay closes.
    pub fn toggle_sidebar(&mut self, ctx: &egui::Context) {
        let wide = ctx.screen_rect().width() >= DOCK_MIN_W;
        if self.sidebar_docked(ctx) {
            self.prefs.sidebar = false;
        } else if self.sidebar_overlay && !(wide && self.sidebar_peek) {
            self.sidebar_overlay = false;
        } else if wide {
            self.prefs.sidebar = true;
            self.sidebar_overlay = false;
        } else {
            self.sidebar_overlay = true;
        }
        self.sidebar_peek = false;
        self.mark_changed();
    }

    /// Rest the pointer on the window's left edge (not over the toolbar) to peek the sidebar.
    fn edge_peek(&mut self, ctx: &egui::Context) {
        if self.sidebar_overlay || self.inspector.is_some() || self.new_event.is_some() || self.settings_open {
            self.edge_since = None;
            return;
        }
        let now = ctx.input(|i| i.time);
        let at_edge = ctx.input(|i| i.pointer.hover_pos()).is_some_and(|pos| pos.x <= EDGE_W && pos.y > TOOLBAR_H) && !ctx.input(|i| i.any_touches() || i.pointer.any_down());
        match (at_edge, self.edge_since) {
            (false, _) => self.edge_since = None,
            (true, None) => {
                self.edge_since = Some(now);
                ctx.request_repaint_after(std::time::Duration::from_millis(PEEK_DELAY_MS));
            }
            (true, Some(since)) if now - since >= PEEK_DELAY_MS as f64 / 1000.0 => {
                self.sidebar_overlay = true;
                self.sidebar_peek = true;
                self.edge_since = None;
            }
            (true, Some(_)) => ctx.request_repaint_after(std::time::Duration::from_millis(40)),
        }
    }

    /// The sidebar over the calendar, with a shadow. A peek closes when the pointer leaves it;
    /// an opened one on a click outside or Esc.
    fn sidebar_over(&mut self, ctx: &egui::Context, p: &theme::Palette) {
        let screen = ctx.screen_rect();
        let rect = Rect::from_min_size(screen.min, Vec2::new(SIDEBAR_W, screen.height()));
        egui::Area::new("sidebar_overlay".into()).order(egui::Order::Middle).fixed_pos(rect.min).show(ctx, |ui| {
            egui::Frame::new().fill(p.sidebar).inner_margin(SIDEBAR_MARGIN).stroke(Stroke::new(1.0_f32, p.line)).shadow(ctx.style().visuals.window_shadow).show(ui, |ui| {
                ui.set_width(SIDEBAR_W - (SIDEBAR_MARGIN.left + SIDEBAR_MARGIN.right) as f32);
                ui.set_height(screen.height() - (SIDEBAR_MARGIN.top + SIDEBAR_MARGIN.bottom) as f32);
                self.sidebar(ui);
            });
        });
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let clicked_outside = ctx.input(|i| i.pointer.any_pressed()) && pointer.is_some_and(|pos| !rect.contains(pos)) && self.inspector.is_none() && self.new_event.is_none() && !self.settings_open;
        let left = self.sidebar_peek && pointer.is_some_and(|pos| pos.x > rect.right() + 24.0) && !ctx.input(|i| i.pointer.any_down());
        if clicked_outside || left || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.sidebar_overlay = false;
            self.sidebar_peek = false;
        }
    }

    pub(super) fn calendar_area(&mut self, ui: &mut egui::Ui) {
        if self.last_view != Some(self.view_mode) {
            // Entering Month frames the selection's month afresh.
            self.month_pos = f64::NAN;
            self.month_target = None;
            self.last_view = Some(self.view_mode);
        }
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
            ViewMode::Month => {
                let m = if self.month_pos.is_nan() { first_of_month(d) } else { self.month_focus };
                (i18n::month_name(m.month()).to_string(), m.year().to_string())
            }
            ViewMode::Quarter => {
                let first = first_of_month(d);
                let last = super::add_months(first, 2);
                (format!("{} – {}", i18n::month_short(first.month()), i18n::month_short(last.month())), last.year().to_string())
            }
            ViewMode::Year => (d.year().to_string(), String::new()),
        }
    }

    /// The toolbar. At 1400 points and below the view pills become a drop-down; at 1110 and
    /// below the search field folds into an icon that expands on click, next to the view
    /// selector; at 860 and below the title shrinks again and New becomes "+". Widths are the
    /// window size in egui points (CSS pixels on the web, logical points on the desktop). The
    /// title reserves the longest month name so a short month cannot let the previous arrow
    /// sit where "December" would reach, and the layout tightens a step further if the
    /// reserved title and the controls would otherwise overlap or shrink into each other.
    fn toolbar(&mut self, ui: &mut egui::Ui, docked: bool) {
        let p = *self.palette();
        let width = ui.available_width();
        let screen_w = ui.ctx().screen_rect().width();
        let leading = if docked { 0.0 } else { 32.0 + space::S8 + space::S4 };
        let file_name = self.file_doc.as_ref().map(|d| if d.dirty { format!("{} •", d.name) } else { d.name.clone() });
        // Fit checks use the collapsed search chrome, so opening the field does not flip the
        // rest of the toolbar into a tighter mode.
        let mode_width = |compact: bool, search_icon: bool, tight: bool| {
            let size = if tight { text::T18 } else if compact { 20.0 } else { text::T24 };
            let file = if tight { 0.0 } else { file_name.as_ref().map(|n| pill_width(ui, n) + space::S8).unwrap_or(0.0) };
            let spinner = if self.busy > 0 { 14.0 + space::S8 } else { 0.0 };
            leading + reserved_title_width(ui, size, self.view_mode) + spinner + file + controls_width(ui, compact, search_icon, tight, false) + FIT_GAP
        };
        let mut compact = screen_w <= COMPACT_W;
        let mut search_icon = screen_w <= SEARCH_ICON_W;
        let mut tight = screen_w <= TIGHT_W;
        if !compact && mode_width(false, false, false) > width {
            compact = true;
        }
        if !search_icon && mode_width(true, false, false) > width {
            compact = true;
            search_icon = true;
        }
        if !tight && mode_width(true, true, false) > width {
            compact = true;
            search_icon = true;
            tight = true;
        }
        let search_expanded = self.search_open || !self.search_query.is_empty();
        // Prefer an inline field next to the view selector; if that would crowd the
        // title, open a popover instead, still anchored on the search icon.
        let mut search_popup = false;
        if search_icon && search_expanded {
            if !tight && mode_width(compact, false, true) <= width {
                tight = true;
            }
            if mode_width(compact, false, tight) > width {
                search_popup = true;
            }
        }
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = space::S8;
            if !docked {
                let tip = if ui.ctx().screen_rect().width() >= DOCK_MIN_W { t("Show the sidebar") } else { t("Sidebar") };
                if icon_button(ui, &p, Icon::Sidebar, &format!("{tip}  (⌘\\ · Ctrl+\\)")).clicked() {
                    self.toggle_sidebar(ui.ctx());
                }
                ui.add_space(space::S4);
            }
            let (big, small) = self.title_text();
            let size = if tight { text::T18 } else if compact { 20.0 } else { text::T24 };
            // Keep the previous arrow off the title by reserving the widest month (plus year
            // or day number) at this size, so short names do not free space a long name needs.
            let title_w = reserved_title_width(ui, size, self.view_mode);
            ui.allocate_ui_with_layout(Vec2::new(title_w, ui.available_height()), Layout::left_to_right(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = space::S8;
                ui.label(RichText::new(&big).font(font_bold(size)).color(p.text));
                if !small.is_empty() {
                    ui.label(RichText::new(&small).font(font(size)).color(p.text_2));
                }
            });
            if self.busy > 0 {
                ui.add(egui::Spinner::new().size(14.0).color(p.primary));
            }
            if let Some(doc) = &self.file_doc {
                if !tight {
                    let label = if doc.dirty { format!("{} •", doc.name) } else { doc.name.clone() };
                    pill(ui, &p, &label);
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = space::S8;
                if icon_button(ui, &p, Icon::Gear, t("Settings")).clicked() {
                    self.settings_open = true;
                }
                let new_label = if tight { "+".to_string() } else { format!("+  {}", t("New")) };
                if theme::primary_button(ui, &p, &new_label).on_hover_text(t("New event (⌘N)")).clicked() {
                    self.open_new_event(self.selected_date, None);
                }
                // Search sits immediately to the right of the view selector (it is added
                // before the selector in this right-to-left group) at every breakpoint.
                let inner = if tight { 120.0 } else { 150.0 };
                let show_inline = !search_icon || (search_expanded && !search_popup);
                if show_inline {
                    let search = search_text_field(ui, &p, &mut self.search_query, inner);
                    apply_search_focus_and_dismiss(ui, &search, &mut self.search_open, &mut self.search_query);
                } else {
                    let icon = icon_button(ui, &p, Icon::Search, t("Search"));
                    if icon.clicked() {
                        self.search_open = true;
                    }
                    if search_popup {
                        let outer = search_outer_width(inner);
                        let pos = Pos2::new(icon.rect.right() - outer, icon.rect.bottom() + space::S4);
                        let popup = egui::Area::new("toolbar_search".into()).order(egui::Order::Foreground).fixed_pos(pos).show(ui.ctx(), |ui| {
                            egui::Frame::new().fill(p.surface).corner_radius(CornerRadius::same(8)).inner_margin(egui::Margin::same(4)).shadow(ui.ctx().style().visuals.popup_shadow).show(ui, |ui| {
                                search_text_field(ui, &p, &mut self.search_query, inner)
                            }).inner
                        });
                        apply_search_focus_and_dismiss(ui, &popup.inner, &mut self.search_open, &mut self.search_query);
                        let clicked_outside = ui.ctx().input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|pt| !popup.response.rect.contains(pt) && !icon.rect.contains(pt)));
                        if clicked_outside && self.search_query.is_empty() {
                            self.search_open = false;
                        }
                    }
                }
                let views = [ViewMode::Day, ViewMode::Week, ViewMode::Month, ViewMode::Quarter, ViewMode::Year];
                let changed = if compact { view_select(ui, &p, &mut self.view_mode, &views) } else { segmented(ui, &p, &mut self.view_mode, &views) };
                if changed {
                    self.prefs.view = self.view_mode;
                    self.timeline_scrolled = false;
                    self.mark_changed();
                }
                ui.add_space(space::S4);
                if icon_button(ui, &p, Icon::ChevronRight, t("Next")).clicked() {
                    self.step(1);
                }
                if ui.add(egui::Button::new(t("Today")).min_size(Vec2::new(0.0, 32.0))).clicked() {
                    self.go_today();
                }
                if icon_button(ui, &p, Icon::ChevronLeft, t("Previous")).clicked() {
                    self.step(-1);
                }
            });
        });
    }

    pub(super) fn sidebar(&mut self, ui: &mut egui::Ui) {
        let p = *self.palette();
        ui.spacing_mut().item_spacing.y = space::S12;
        // The top row lines up with the toolbar: the toggle where the toolbar shows it when the
        // sidebar is hidden, then the quick entry; a rule continues the toolbar's bottom edge.
        let top = ui.max_rect().top() - SIDEBAR_MARGIN.top as f32;
        let row = ui.horizontal(|ui| {
            ui.set_height(TOOLBAR_H - 2.0 * SIDEBAR_MARGIN.top as f32);
            ui.spacing_mut().item_spacing.x = space::S8;
            if icon_button(ui, &p, Icon::Sidebar, &format!("{}  (⌘\\ · Ctrl+\\)", t("Hide the sidebar"))).clicked() {
                self.toggle_sidebar(ui.ctx());
            }
            self.quick_entry_field(ui);
        });
        let rule_y = top + TOOLBAR_H - 0.5;
        let full = ui.max_rect().expand2(Vec2::new(SIDEBAR_MARGIN.left as f32, 0.0));
        ui.painter().with_clip_rect(full.expand(1.0)).line_segment([Pos2::new(full.left(), rule_y), Pos2::new(full.right(), rule_y)], Stroke::new(1.0_f32, p.line));
        let _ = row;
        ui.add_space(space::S8);
        self.quick_entry_preview(ui);
        self.mini_calendar(ui, true);
        ui.add_space(space::S4);
        self.calendars_section(ui);
        sep(ui, &p);
        self.agenda(ui, ui.available_height());
    }

    fn quick_entry_field(&mut self, ui: &mut egui::Ui) {
        let p = *self.palette();
        let resp = theme::text_field(ui, &p, egui::TextEdit::singleline(&mut self.quick_input).id_salt("quick").hint_text(t("Lunch with Maya tomorrow at 12:30")).desired_width(f32::INFINITY));
        self.quick_enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    }

    /// What the quick entry understood, under the field; Enter adds it.
    fn quick_entry_preview(&mut self, ui: &mut egui::Ui) {
        let p = *self.palette();
        let enter = std::mem::take(&mut self.quick_enter);
        if !self.quick_input.trim().is_empty() {
            let cal = self.default_calendar().unwrap_or_default();
            if let Some(parsed) = parse_quick_entry(&self.quick_input, super::today(), &self.store.calendars, &cal).map(|mut q| {
                if !q.date_given {
                    q.date = self.selected_date;
                }
                q
            }) {
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
                    if small_icon(ui, &p, Icon::ChevronRight).clicked() {
                        self.mini_month = super::add_months(month, 1);
                    }
                    if small_icon(ui, &p, Icon::ChevronLeft).clicked() {
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
                } else if resp.hovered() && !ui.input(|i| i.any_touches()) {
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
            let arrow = if self.calendars_open { Icon::ChevronDown } else { Icon::ChevronRight };
            let a = icons::inline(ui, arrow, 12.0, p.text_2).interact(Sense::click());
            let r = ui.add(egui::Label::new(RichText::new(t("Calendars")).font(font_bold(text::T14)).color(p.text_2)).sense(Sense::click())).union(a);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if small_icon(ui, &p, Icon::Plus).on_hover_text(t("Add a calendar account")).clicked() {
                    self.settings_open = true;
                    self.settings_tab = 1;
                }
                if small_icon(ui, &p, Icon::Refresh).on_hover_text(t("Refresh")).clicked() {
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
                        icons::inline(ui, Icon::Warning, 14.0, p.danger).on_hover_text(err);
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
                        icons::paint(ui.painter(), box_r.shrink(2.0), Icon::Check, Color32::WHITE);
                    } else {
                        ui.painter().rect_stroke(box_r, CornerRadius::same(4), Stroke::new(1.5_f32, color), egui::StrokeKind::Inside);
                    }
                    let name = ui.painter().text(Pos2::new(rect.left() + 28.0, rect.center().y), Align2::LEFT_CENTER, &cal.name, font(text::T14), p.text);
                    if cal.read_only {
                        icons::paint(ui.painter(), Rect::from_center_size(Pos2::new(name.right() + 12.0, rect.center().y), Vec2::splat(12.0)), Icon::Lock, p.text_3);
                    }
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

    /// Days with their events around the selection. Scrolling it by hand never changes the
    /// selection: it only reports the day now at its top (the Month strip follows, the mini
    /// calendar changes month when that day's month changes). Its rows change only when the
    /// selection does, so nothing is inserted or removed under a moving finger.
    pub(super) fn agenda(&mut self, ui: &mut egui::Ui, height: f32) {
        let p = *self.palette();
        let jump = self.agenda_scroll_to.take();
        let wanted = jump.unwrap_or(self.selected_date);
        if wanted < self.agenda_center - Duration::days(50) || wanted > self.agenda_center + Duration::days(110) {
            self.agenda_center = wanted;
        }
        let start = self.agenda_center - Duration::days(60);
        let end = self.agenda_center + Duration::days(120);
        let now = today();
        let touching = ui.input(|i| i.any_touches());
        let mut jumped = false;
        let mut headers: Vec<(NaiveDate, f32)> = Vec::new();
        let mut clicked: Option<(String, Rect)> = None;
        let mut picked_day: Option<NaiveDate> = None;
        let out = egui::ScrollArea::vertical().id_salt("agenda").max_height(height).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let mut d = start;
            while d <= end {
                let events = self.events_on(d);
                if events.is_empty() && d != now && d != self.selected_date {
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
                headers.push((d, head.response.rect.top()));
                if let Some(target) = jump {
                    if !jumped && d >= target {
                        ui.scroll_to_rect(head.response.rect, Some(Align::TOP));
                        jumped = true;
                    }
                }
                if events.is_empty() {
                    ui.label(RichText::new(t("No events")).font(font(text::T12)).color(p.text_3));
                }
                for ev in &events {
                    if let Some(rect) = agenda_row(ui, &p, ev, self.cal_color(&ev.calendar_id), d, self.inspector.as_ref().is_some_and(|i| i.event_id == ev.id), !touching) {
                        clicked = Some((ev.id.clone(), rect));
                    }
                }
                ui.add_space(space::S8);
                d += Duration::days(1);
            }
        });
        // Moved by hand (wheel, trackpad or finger): pass on the day at the top, once per day.
        let offset = out.state.offset.y;
        let by_hand = (offset - self.agenda_offset).abs() > 0.5 && jump.is_none() && (touching || ui.rect_contains_pointer(out.inner_rect));
        self.agenda_offset = offset;
        if by_hand {
            let top = out.inner_rect.top() + 4.0;
            if let Some((day, _)) = headers.iter().rev().find(|(_, y)| *y <= top).or(headers.first()) {
                if self.agenda_top != Some(*day) {
                    self.agenda_top = Some(*day);
                    self.agenda_moved = Some(*day);
                    let month = first_of_month(*day);
                    if month != self.mini_month {
                        self.mini_month = month;
                    }
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
                    let d = if self.view_mode == ViewMode::Day { self.selected_date } else { self.mini_month };
                    ui.label(RichText::new(i18n::month_name(d.month())).font(font_bold(text::T18)));
                    ui.label(RichText::new(d.year().to_string()).font(font(text::T18)).color(p.text_2));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if theme::primary_button(ui, &p, "+").clicked() {
                            self.open_new_event(self.selected_date, None);
                        }
                        if icon_button(ui, &p, Icon::Gear, t("Settings")).clicked() {
                            self.settings_open = true;
                        }
                        if ui.button(t("Today")).clicked() {
                            self.go_today();
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

/// One event in the list. No hover tint while a finger is down (it would chase the rows
/// sliding under the finger).
fn agenda_row(ui: &mut egui::Ui, p: &theme::Palette, ev: &CalendarEvent, color: Color32, day: NaiveDate, selected: bool, hover: bool) -> Option<Rect> {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 40.0), Sense::click());
    let painter = ui.painter_at(rect);
    if selected {
        painter.rect_filled(rect, CornerRadius::same(8), p.primary_faint);
    } else if hover && resp.hovered() {
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


fn font_width(ui: &egui::Ui, text: &str, font: egui::FontId) -> f32 {
    ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font, Color32::WHITE).size().x)
}

/// Space for the title at `size`, using the longest month name in the current language.
fn reserved_title_width(ui: &egui::Ui, size: f32, view: ViewMode) -> f32 {
    let gap = space::S8;
    let month = (1u32..=12).map(|m| font_width(ui, i18n::month_name(m), font_bold(size))).fold(0.0_f32, f32::max);
    let year = font_width(ui, "0000", font(size));
    match view {
        ViewMode::Year => font_width(ui, "0000", font_bold(size)),
        ViewMode::Day => font_width(ui, "31 ", font_bold(size)) + month + gap + year,
        ViewMode::Month | ViewMode::Week | ViewMode::Quarter => month + gap + year,
    }
}

fn pill_width(ui: &egui::Ui, label: &str) -> f32 {
    font_width(ui, label, font(text::T12)) + 20.0
}


fn search_text_field(ui: &mut egui::Ui, p: &theme::Palette, query: &mut String, inner: f32) -> egui::Response {
    let pad = egui::Margin { left: 34, ..theme::INPUT_PADDING };
    let outer = search_outer_width(inner);
    let search = ui.allocate_ui_with_layout(Vec2::new(outer, theme::INPUT_HEIGHT), Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_size(Vec2::new(outer, theme::INPUT_HEIGHT));
        theme::text_field_padded(ui, p, egui::TextEdit::singleline(query).id_salt("search").hint_text(t("Search")).desired_width(inner), pad)
    }).inner;
    icons::paint(ui.painter(), Rect::from_center_size(Pos2::new(search.rect.left() + 18.0, search.rect.center().y), Vec2::splat(14.0)), Icon::Search, p.text_3);
    search
}

fn apply_search_focus_and_dismiss(ui: &egui::Ui, search: &egui::Response, search_open: &mut bool, query: &mut String) {
    if *search_open && !search.has_focus() {
        search.request_focus();
    }
    if search.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        query.clear();
        *search_open = false;
    } else if search.lost_focus() && query.is_empty() {
        *search_open = false;
    }
}

/// Painted width of the search field: the inner text width plus the left icon padding and
/// the right input padding (`text_field_padded` draws the frame outside the text).
fn search_outer_width(inner: f32) -> f32 {
    inner + 34.0 + theme::INPUT_PADDING.right as f32
}

/// Minimum width of the toolbar's right-hand cluster (settings, new, search, views, nav).
fn controls_width(ui: &egui::Ui, compact: bool, search_icon: bool, tight: bool, search_field: bool) -> f32 {
    let gap = space::S8;
    let pad = space::S16 * 2.0;
    let icon = 32.0;
    let text_btn = |label: &str, bold: bool| font_width(ui, label, if bold { font_bold(text::T14) } else { font(text::T14) }) + pad;
    let new_w = if tight { text_btn("+", true) } else { text_btn(&format!("+  {}", t("New")), true) };
    let search_w = if search_icon && !search_field {
        icon
    } else if tight {
        search_outer_width(120.0)
    } else {
        search_outer_width(150.0)
    };
    let views_w = if compact {
        112.0
    } else {
        let views = [ViewMode::Day, ViewMode::Week, ViewMode::Month, ViewMode::Quarter, ViewMode::Year];
        let labels: f32 = views.iter().map(|m| font_width(ui, t(m.label()), font_bold(text::T14)) + pad).sum();
        labels + 2.0 * 4.0 + 6.0
    };
    icon + gap + new_w + gap + search_w + gap + views_w + gap + space::S4 + icon + gap + text_btn(t("Today"), false) + gap + icon
}

pub fn sep(ui: &mut egui::Ui, p: &theme::Palette) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::ZERO, p.line);
}

pub fn icon_button(ui: &mut egui::Ui, p: &theme::Palette, icon: Icon, tip: &str) -> egui::Response {
    icons::button(ui, p, icon, tip)
}

pub fn small_icon(ui: &mut egui::Ui, p: &theme::Palette, icon: Icon) -> egui::Response {
    icons::small_button(ui, p, icon, "")
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
        // Read left to right also inside the toolbar's right-to-left group.
        let rtl = ui.layout().main_dir() == egui::Direction::RightToLeft;
        ui.horizontal(|ui| {
            let ordered: Vec<&ViewMode> = if rtl { options.iter().rev().collect() } else { options.iter().collect() };
            for mode in ordered {
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

/// The views as a drop-down, for narrow toolbars. Returns true when the choice changed.
pub fn view_select(ui: &mut egui::Ui, p: &theme::Palette, value: &mut ViewMode, options: &[ViewMode]) -> bool {
    let mut changed = false;
    let current = t(value.label());
    theme::select(ui, p, egui::ComboBox::from_id_salt("view_select").selected_text(RichText::new(current).font(font_bold(text::T14))).width(112.0), |ui| {
        for mode in options {
            if ui.selectable_label(*value == *mode, t(mode.label())).clicked() && *value != *mode {
                *value = *mode;
                changed = true;
            }
        }
    });
    changed
}
