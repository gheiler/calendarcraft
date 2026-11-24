use crate::mcp::handle_mcp_request;
use crate::model::{AccountConfig, CalendarEvent, CalendarSource, CalendarStore, TaskItem, ViewMode};
use crate::nlp::parse_quick_entry;
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveTime, Timelike};
use eframe::egui::{self, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use serde_json::json;
use std::sync::Arc;

// SnowUI Purple Design System Palette
const SNOW_PURPLE_50: Color32 = Color32::from_rgb(250, 245, 255);
const SNOW_PURPLE_100: Color32 = Color32::from_rgb(243, 232, 255);
const SNOW_PURPLE_300: Color32 = Color32::from_rgb(216, 180, 254);
const SNOW_PURPLE_500: Color32 = Color32::from_rgb(168, 85, 247);
const SNOW_PURPLE_600: Color32 = Color32::from_rgb(147, 51, 234); // Primary SnowUI Brand

const SNOW_SLATE_900: Color32 = Color32::from_rgb(15, 23, 42);   // App Dark Surface
const SNOW_SLATE_800: Color32 = Color32::from_rgb(30, 41, 59);   // Container / Cards
const SNOW_SLATE_700: Color32 = Color32::from_rgb(51, 65, 85);   // Border Dark
const SNOW_SLATE_200: Color32 = Color32::from_rgb(226, 232, 240); // Border Light
const SNOW_SLATE_50: Color32 = Color32::from_rgb(248, 250, 252);  // App Light Surface

pub struct CalendarCraftApp {
    pub store: CalendarStore,
    pub config_path: Option<String>,
    pub selected_date: NaiveDate,
    pub view_mode: ViewMode,
    pub quick_input: String,
    pub search_query: String,
    pub selected_event_id: Option<String>,
    pub show_accounts_modal: bool,
    pub show_mcp_modal: bool,
    pub status_banner: Option<String>,

    // Account configuration form
    pub new_acc_name: String,
    pub new_acc_provider: String,
    pub new_acc_email: String,
    pub new_acc_server: String,
    pub new_cal_name: String,
    pub new_cal_color: String,
    pub config_json_editor: String,

    // MCP testing playground
    pub mcp_request_input: String,
    pub mcp_response_output: String,

    // Screenshot automation
    pub screenshot_path: Option<String>,
    pub frame_count: u32,
    pub screenshot_requested: bool,
}

impl CalendarCraftApp {
    pub fn new(store: CalendarStore, config_path: Option<String>, screenshot_path: Option<String>) -> Self {
        let today = Local::now().date_naive();
        let view_mode = store.settings.default_view;
        let selected_event_id = store.events.first().map(|e| e.id.clone());
        let sample_mcp = serde_json::to_string_pretty(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "quick_add_natural",
                "arguments": {
                    "sentence": "Product Review tomorrow at 2pm-3pm @ Boardroom /Engineering"
                }
            }
        }))
        .unwrap_or_default();

        let cfg_preview = store.to_json_pretty().unwrap_or_default();

        Self {
            store,
            config_path,
            selected_date: today,
            view_mode,
            quick_input: String::new(),
            search_query: String::new(),
            selected_event_id,
            show_accounts_modal: false,
            show_mcp_modal: false,
            status_banner: None,
            new_acc_name: String::new(),
            new_acc_provider: "CalDAV".to_string(),
            new_acc_email: String::new(),
            new_acc_server: "https://caldav.example.com".to_string(),
            new_cal_name: "Focus Schedule".to_string(),
            new_cal_color: "#9333EA".to_string(),
            config_json_editor: cfg_preview,
            mcp_request_input: sample_mcp,
            mcp_response_output: String::new(),
            screenshot_path,
            frame_count: 0,
            screenshot_requested: false,
        }
    }

    fn persist_if_configured(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(ref path) = self.config_path {
            let _ = self.store.save_to_path(path);
        }
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(win) = web_sys::window() {
                if let Ok(Some(storage)) = win.local_storage() {
                    if let Ok(json) = self.store.to_json_pretty() {
                        let _ = storage.set_item("calendarcraft_config", &json);
                    }
                }
            }
        }
    }

    fn apply_theme(&self, ctx: &egui::Context) {
        let mut visuals = if self.store.settings.dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };

        if self.store.settings.dark_mode {
            visuals.panel_fill = SNOW_SLATE_900;
            visuals.window_fill = Color32::from_rgb(18, 24, 38);
            visuals.extreme_bg_color = Color32::from_rgb(10, 15, 28);
            visuals.widgets.noninteractive.bg_fill = SNOW_SLATE_800;
            visuals.widgets.inactive.bg_fill = Color32::from_rgb(37, 48, 69);
            visuals.widgets.hovered.bg_fill = Color32::from_rgb(55, 68, 94);
            visuals.widgets.active.bg_fill = SNOW_PURPLE_600;
            visuals.selection.bg_fill = SNOW_PURPLE_600;
        } else {
            visuals.panel_fill = SNOW_SLATE_50;
            visuals.window_fill = Color32::WHITE;
            visuals.extreme_bg_color = Color32::WHITE;
            visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(241, 245, 249);
            visuals.widgets.inactive.bg_fill = Color32::from_rgb(243, 244, 246);
            visuals.widgets.hovered.bg_fill = SNOW_PURPLE_100;
            visuals.widgets.active.bg_fill = SNOW_PURPLE_600;
            visuals.selection.bg_fill = SNOW_PURPLE_600;
        }
        ctx.set_visuals(visuals);
    }

    fn cal_color(&self, cal_id: &str) -> Color32 {
        if let Some(c) = self.store.calendar_by_id(cal_id) {
            let [r, g, b] = c.rgb();
            Color32::from_rgb(r, g, b)
        } else {
            SNOW_PURPLE_600
        }
    }

    fn submit_quick_entry(&mut self) {
        let default_cal = self.store.settings.default_calendar_id.clone();
        if let Some(parsed) = parse_quick_entry(
            &self.quick_input,
            self.selected_date,
            &self.store.calendars,
            &default_cal,
        ) {
            self.selected_date = parsed.date;
            if parsed.is_task {
                let t = TaskItem {
                    id: self.store.next_id("task"),
                    calendar_id: parsed.calendar_id.unwrap_or(default_cal),
                    title: parsed.title.clone(),
                    due_date: Some(parsed.date),
                    due_time: parsed.start_time,
                    completed: false,
                    priority: 2,
                };
                self.store.tasks.push(t);
                self.status_banner = Some(format!("Added task '{}' for {}", parsed.title, parsed.date));
            } else {
                let id = self.store.next_id("ev");
                let title = parsed.title.clone();
                let date = parsed.date;
                let ev = parsed.into_event(id.clone(), &default_cal);
                self.store.events.push(ev);
                self.selected_event_id = Some(id);
                self.status_banner = Some(format!("Scheduled '{}' on {}", title, date));
            }
            self.quick_input.clear();
            self.persist_if_configured();
        }
    }

    fn step_date(&mut self, dir: i64) {
        self.selected_date = match self.view_mode {
            ViewMode::Day => self.selected_date + Duration::days(dir),
            ViewMode::Week => self.selected_date + Duration::days(dir * 7),
            ViewMode::Month => {
                let mut y = self.selected_date.year();
                let mut m = self.selected_date.month() as i64 + dir;
                while m > 12 {
                    m -= 12;
                    y += 1;
                }
                while m < 1 {
                    m += 12;
                    y -= 1;
                }
                NaiveDate::from_ymd_opt(y, m as u32, self.selected_date.day().min(28)).unwrap_or(self.selected_date)
            }
            ViewMode::Quarter => {
                let mut y = self.selected_date.year();
                let mut m = self.selected_date.month() as i64 + dir * 3;
                while m > 12 {
                    m -= 12;
                    y += 1;
                }
                while m < 1 {
                    m += 12;
                    y -= 1;
                }
                NaiveDate::from_ymd_opt(y, m as u32, 1).unwrap_or(self.selected_date)
            }
            ViewMode::Year => {
                NaiveDate::from_ymd_opt(self.selected_date.year() + dir as i32, self.selected_date.month(), 1)
                    .unwrap_or(self.selected_date)
            }
        };
    }

    fn render_top_bar(&mut self, ui: &mut egui::Ui) {
        let dark = self.store.settings.dark_mode;
        let subtle_text = if dark {
            Color32::from_rgb(160, 174, 192)
        } else {
            Color32::from_rgb(100, 116, 139)
        };

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);

            // SnowUI Purple Brand Badge
            let (badge_rect, _) = ui.allocate_exact_size(Vec2::new(32.0, 32.0), Sense::hover());
            ui.painter().rect_filled(badge_rect, CornerRadius::same(8), SNOW_PURPLE_600);
            let today_num = Local::now().day().to_string();
            ui.painter().text(
                badge_rect.center(),
                egui::Align2::CENTER_CENTER,
                today_num,
                FontId::proportional(15.0),
                Color32::WHITE,
            );

            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new("CalendarCraft").strong().size(15.5));
                let subtitle = if let Some(ref p) = self.config_path {
                    format!("Preloaded: {p}")
                } else {
                    format!("SnowUI Purple • {} accounts", self.store.accounts.len())
                };
                ui.label(RichText::new(subtitle).size(10.5).color(subtle_text));
            });

            ui.add_space(14.0);

            // Navigation < Today >
            if ui.button("◀").clicked() {
                self.step_date(-1);
            }
            if ui.button("Today").clicked() {
                self.selected_date = Local::now().date_naive();
            }
            if ui.button("▶").clicked() {
                self.step_date(1);
            }

            ui.add_space(8.0);
            let title_date = match self.view_mode {
                ViewMode::Day => self.selected_date.format("%A, %B %d, %Y").to_string(),
                ViewMode::Week => {
                    let start = self.selected_date - Duration::days(self.selected_date.weekday().num_days_from_monday() as i64);
                    let end = start + Duration::days(6);
                    format!("{} – {}", start.format("%b %d"), end.format("%b %d, %Y"))
                }
                ViewMode::Month => self.selected_date.format("%B %Y").to_string(),
                ViewMode::Quarter => {
                    let q = (self.selected_date.month() - 1) / 3 + 1;
                    format!("Q{} {}", q, self.selected_date.year())
                }
                ViewMode::Year => self.selected_date.format("%Y").to_string(),
            };
            ui.label(RichText::new(title_date).strong().size(17.0));

            ui.add_space(14.0);

            // Segmented view switcher (Day, Week, Month, Quarter, Year)
            for mode in [ViewMode::Day, ViewMode::Week, ViewMode::Month, ViewMode::Quarter, ViewMode::Year] {
                let selected = self.view_mode == mode;
                let btn = egui::Button::new(
                    RichText::new(mode.label())
                        .size(13.0)
                        .color(if selected { Color32::WHITE } else { ui.visuals().text_color() }),
                )
                .fill(if selected {
                    SNOW_PURPLE_600
                } else {
                    ui.visuals().widgets.inactive.bg_fill
                })
                .corner_radius(CornerRadius::same(6));
                if ui.add(btn).clicked() {
                    self.view_mode = mode;
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                let theme_lbl = if self.store.settings.dark_mode { "☀ Light" } else { "🌙 Dark" };
                if ui.button(theme_lbl).clicked() {
                    self.store.settings.dark_mode = !self.store.settings.dark_mode;
                }
                if ui.button("🔌 MCP").clicked() {
                    self.show_mcp_modal = true;
                }
                if ui.button("⚙ Settings").clicked() {
                    self.config_json_editor = self.store.to_json_pretty().unwrap_or_default();
                    self.show_accounts_modal = true;
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.search_query)
                        .hint_text("🔍 Filter events...")
                        .desired_width(140.0),
                );
            });
        });

        ui.add_space(6.0);
        // Natural-language quick entry bar
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(RichText::new("＋").size(17.0).color(SNOW_PURPLE_600).strong());
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.quick_input)
                    .hint_text("Quick Add: e.g. 'Lunch with Maya tomorrow 12:30pm @ Blue Bottle /Personal' or 'todo Quarterly audit friday'")
                    .desired_width(ui.available_width() - 110.0),
            );
            let enter_pressed = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let add_btn = egui::Button::new(RichText::new("Add Event").strong().color(Color32::WHITE))
                .fill(SNOW_PURPLE_600)
                .corner_radius(CornerRadius::same(6));
            if ui.add(add_btn).clicked() || enter_pressed {
                self.submit_quick_entry();
            }
        });

        // Live natural-language parse chip
        if !self.quick_input.trim().is_empty() {
            let def_cal = self.store.settings.default_calendar_id.clone();
            if let Some(preview) = parse_quick_entry(&self.quick_input, self.selected_date, &self.store.calendars, &def_cal) {
                ui.horizontal(|ui| {
                    ui.add_space(34.0);
                    let kind = if preview.is_task { "Task" } else { "Event" };
                    let when = if preview.all_day {
                        format!("{} (All day)", preview.date)
                    } else if let (Some(s), Some(e)) = (preview.start_time, preview.end_time) {
                        format!("{} {}–{}", preview.date, s.format("%H:%M"), e.format("%H:%M"))
                    } else {
                        preview.date.to_string()
                    };
                    let cal_name = preview
                        .calendar_id
                        .as_deref()
                        .and_then(|id| self.store.calendar_by_id(id))
                        .map(|c| c.name.as_str())
                        .unwrap_or("Calendar");
                    ui.label(
                        RichText::new(format!(
                            "↳ Preview [{kind}]: '{}' • {when} • /{cal_name}{}",
                            preview.title,
                            preview.location.as_ref().map(|l| format!(" • 📍 {l}")).unwrap_or_default()
                        ))
                        .size(11.5)
                        .color(SNOW_PURPLE_300),
                    );
                });
            }
        }

        if let Some(ref msg) = self.status_banner.clone() {
            ui.horizontal(|ui| {
                ui.add_space(34.0);
                ui.label(RichText::new(format!("✓ {msg}")).size(11.5).color(Color32::from_rgb(52, 211, 153)));
                if ui.small_button("×").clicked() {
                    self.status_banner = None;
                }
            });
        }
        ui.add_space(4.0);
    }

    fn render_left_sidebar(&mut self, ui: &mut egui::Ui) {
        let dark = self.store.settings.dark_mode;
        let muted = if dark {
            Color32::from_rgb(148, 163, 184)
        } else {
            Color32::from_rgb(100, 116, 139)
        };

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(6.0);
            self.render_mini_month(ui, self.selected_date.year(), self.selected_date.month(), true);

            ui.add_space(10.0);
            ui.separator();

            // Calendar Sets switcher
            ui.add_space(4.0);
            ui.label(RichText::new("CALENDAR SETS").size(10.5).strong().color(muted));
            let active_set = self
                .store
                .settings
                .active_calendar_set
                .clone()
                .unwrap_or_else(|| "set_all".to_string());
            let sets = self.store.calendar_sets.clone();
            ui.horizontal_wrapped(|ui| {
                for set in sets {
                    let is_active = set.id == active_set;
                    if ui.selectable_label(is_active, &set.name).clicked() {
                        self.store.settings.active_calendar_set = Some(set.id);
                        self.persist_if_configured();
                    }
                }
            });

            ui.add_space(8.0);
            ui.separator();

            // Accounts & Calendars
            ui.add_space(4.0);
            ui.label(RichText::new("ACCOUNTS & CALENDARS").size(10.5).strong().color(muted));
            let accounts = self.store.accounts.clone();
            for acc in accounts {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let mut enabled = acc.enabled;
                    if ui.checkbox(&mut enabled, "").changed() {
                        if let Some(a) = self.store.accounts.iter_mut().find(|x| x.id == acc.id) {
                            a.enabled = enabled;
                        }
                        self.persist_if_configured();
                    }
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(RichText::new(&acc.name).strong().size(12.5));
                        ui.label(RichText::new(format!("{} • {}", acc.provider, acc.email)).size(10.0).color(muted));
                    });
                });

                let cals: Vec<CalendarSource> = self
                    .store
                    .calendars
                    .iter()
                    .filter(|c| c.account_id == acc.id)
                    .cloned()
                    .collect();

                for cal in cals {
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        let [r, g, b] = cal.rgb();
                        let swatch_color = Color32::from_rgb(r, g, b);
                        let (dot_rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 10.0), Sense::hover());
                        ui.painter().circle_filled(dot_rect.center(), 4.5, swatch_color);

                        let mut vis = cal.visible;
                        if ui.checkbox(&mut vis, &cal.name).changed() {
                            if let Some(c) = self.store.calendars.iter_mut().find(|x| x.id == cal.id) {
                                c.visible = vis;
                            }
                            self.persist_if_configured();
                        }
                    });
                }
            }

            ui.add_space(10.0);
            ui.separator();

            // Upcoming Agenda stream
            ui.add_space(4.0);
            ui.label(RichText::new("UPCOMING AGENDA").size(10.5).strong().color(muted));

            for day_offset in 0..5 {
                let d = self.selected_date + Duration::days(day_offset);
                let day_events: Vec<CalendarEvent> = self
                    .store
                    .events_on_date(d, &self.search_query)
                    .into_iter()
                    .cloned()
                    .collect();
                let day_tasks: Vec<TaskItem> = self
                    .store
                    .tasks
                    .iter()
                    .filter(|t| t.due_date == Some(d) && self.store.is_calendar_visible(&t.calendar_id))
                    .cloned()
                    .collect();

                if day_events.is_empty() && day_tasks.is_empty() && day_offset > 0 {
                    continue;
                }

                ui.add_space(5.0);
                let hdr = if d == Local::now().date_naive() {
                    format!("TODAY • {}", d.format("%a, %b %d"))
                } else {
                    d.format("%A, %b %d").to_string()
                };
                ui.label(RichText::new(hdr).size(11.0).strong().color(SNOW_PURPLE_600));

                for ev in day_events {
                    let col = self.cal_color(&ev.calendar_id);
                    let time_str = if ev.all_day {
                        "All-day".to_string()
                    } else if let Some(st) = ev.start_time {
                        st.format("%H:%M").to_string()
                    } else {
                        "--:--".to_string()
                    };

                    let is_sel = self.selected_event_id.as_deref() == Some(ev.id.as_str());
                    let frame_bg = if is_sel {
                        Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 48)
                    } else {
                        Color32::TRANSPARENT
                    };

                    egui::Frame::new()
                        .fill(frame_bg)
                        .corner_radius(CornerRadius::same(5))
                        .inner_margin(egui::Margin::symmetric(6, 3))
                        .show(ui, |ui| {
                            let resp = ui.horizontal(|ui| {
                                let (bar, _) = ui.allocate_exact_size(Vec2::new(3.5, 24.0), Sense::hover());
                                ui.painter().rect_filled(bar, CornerRadius::same(2), col);
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 1.0;
                                    ui.label(RichText::new(&ev.title).size(12.0).strong());
                                    let sub = match &ev.location {
                                        Some(loc) => format!("{time_str} • {loc}"),
                                        None => time_str,
                                    };
                                    ui.label(RichText::new(sub).size(10.5).color(muted));
                                });
                            });
                            if resp.response.interact(Sense::click()).clicked() {
                                self.selected_event_id = Some(ev.id.clone());
                                self.selected_date = ev.date;
                            }
                        });
                }

                for task in day_tasks {
                    ui.horizontal(|ui| {
                        let mut done = task.completed;
                        if ui.checkbox(&mut done, "").changed() {
                            if let Some(t) = self.store.tasks.iter_mut().find(|x| x.id == task.id) {
                                t.completed = done;
                            }
                            self.persist_if_configured();
                        }
                        let txt = if done {
                            RichText::new(&task.title).strikethrough().size(11.5).color(muted)
                        } else {
                            RichText::new(&task.title).size(11.5)
                        };
                        ui.label(txt);
                    });
                }
            }
            ui.add_space(12.0);
        });
    }

    fn render_mini_month(&mut self, ui: &mut egui::Ui, year: i32, month: u32, interactive_nav: bool) {
        let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap_or(self.selected_date);
        let today = Local::now().date_naive();

        ui.horizontal(|ui| {
            ui.label(RichText::new(first.format("%B %Y").to_string()).strong().size(13.0));
            if interactive_nav {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("›").clicked() {
                        let next_m = if month == 12 { 1 } else { month + 1 };
                        let next_y = if month == 12 { year + 1 } else { year };
                        self.selected_date = NaiveDate::from_ymd_opt(next_y, next_m, 1).unwrap_or(self.selected_date);
                    }
                    if ui.small_button("‹").clicked() {
                        let prev_m = if month == 1 { 12 } else { month - 1 };
                        let prev_y = if month == 1 { year - 1 } else { year };
                        self.selected_date = NaiveDate::from_ymd_opt(prev_y, prev_m, 1).unwrap_or(self.selected_date);
                    }
                });
            }
        });

        ui.add_space(3.0);
        egui::Grid::new(format!("mini_month_{year}_{month}"))
            .spacing([4.0, 4.0])
            .min_col_width(26.0)
            .show(ui, |ui| {
                for wd in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"] {
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new(wd).size(10.5).color(Color32::GRAY));
                    });
                }
                ui.end_row();

                let offset = first.weekday().num_days_from_monday() as i64;
                let grid_start = first - Duration::days(offset);

                for week in 0..6 {
                    for day_idx in 0..7 {
                        let cell_date = grid_start + Duration::days(week * 7 + day_idx);
                        let in_month = cell_date.month() == month;
                        let is_today = cell_date == today;
                        let is_selected = cell_date == self.selected_date;
                        let has_events = !self.store.events_on_date(cell_date, "").is_empty();

                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(26.0, 24.0), Sense::click());
                        if is_today {
                            ui.painter()
                                .circle_filled(rect.center(), 10.5, SNOW_PURPLE_600);
                        } else if is_selected {
                            ui.painter()
                                .circle_filled(rect.center(), 10.5, SNOW_PURPLE_500);
                        } else if resp.hovered() {
                            ui.painter().circle_filled(
                                rect.center(),
                                10.5,
                                Color32::from_rgba_unmultiplied(147, 51, 234, 40),
                            );
                        }

                        let text_col = if is_today || is_selected {
                            Color32::WHITE
                        } else if in_month {
                            ui.visuals().text_color()
                        } else {
                            Color32::from_gray(105)
                        };

                        ui.painter().text(
                            rect.center() - Vec2::new(0.0, 1.0),
                            egui::Align2::CENTER_CENTER,
                            cell_date.day().to_string(),
                            FontId::proportional(11.0),
                            text_col,
                        );

                        if has_events && !is_today && !is_selected {
                            ui.painter().circle_filled(
                                Pos2::new(rect.center().x, rect.bottom() - 3.0),
                                1.8,
                                SNOW_PURPLE_500,
                            );
                        }

                        if resp.clicked() {
                            self.selected_date = cell_date;
                        }
                    }
                    ui.end_row();
                }
            });
    }

    fn render_week_or_day_view(&mut self, ui: &mut egui::Ui, days_count: usize) {
        let start_date = if days_count == 1 {
            self.selected_date
        } else {
            let off = self.selected_date.weekday().num_days_from_monday() as i64;
            self.selected_date - Duration::days(off)
        };

        let dark = self.store.settings.dark_mode;
        let grid_line = if dark { SNOW_SLATE_700 } else { SNOW_SLATE_200 };
        let gutter_w = 56.0;
        let avail_w = ui.available_width().max(320.0);
        let col_w = ((avail_w - gutter_w) / days_count as f32).max(90.0);
        let today = Local::now().date_naive();

        // Day column headers
        ui.horizontal(|ui| {
            ui.allocate_exact_size(Vec2::new(gutter_w, 54.0), Sense::hover());
            for d_idx in 0..days_count {
                let d = start_date + Duration::days(d_idx as i64);
                let is_today = d == today;
                let (hdr_rect, resp) = ui.allocate_exact_size(Vec2::new(col_w, 54.0), Sense::click());
                if resp.clicked() {
                    self.selected_date = d;
                }

                let wd_txt = d.format("%a").to_string().to_uppercase();
                ui.painter().text(
                    Pos2::new(hdr_rect.center().x, hdr_rect.top() + 12.0),
                    egui::Align2::CENTER_CENTER,
                    wd_txt,
                    FontId::proportional(10.5),
                    if is_today { SNOW_PURPLE_600 } else { Color32::GRAY },
                );

                let pill_center = Pos2::new(hdr_rect.center().x, hdr_rect.top() + 34.0);
                if is_today {
                    ui.painter()
                        .circle_filled(pill_center, 13.0, SNOW_PURPLE_600);
                } else if d == self.selected_date {
                    ui.painter()
                        .circle_filled(pill_center, 13.0, SNOW_PURPLE_500);
                }
                ui.painter().text(
                    pill_center,
                    egui::Align2::CENTER_CENTER,
                    d.day().to_string(),
                    FontId::proportional(14.0),
                    if is_today || d == self.selected_date {
                        Color32::WHITE
                    } else {
                        ui.visuals().text_color()
                    },
                );
            }
        });

        // All-day banner row
        ui.horizontal(|ui| {
            let (lbl_r, _) = ui.allocate_exact_size(Vec2::new(gutter_w, 26.0), Sense::hover());
            ui.painter().text(
                lbl_r.center(),
                egui::Align2::CENTER_CENTER,
                "all-day",
                FontId::proportional(10.0),
                Color32::GRAY,
            );
            for d_idx in 0..days_count {
                let d = start_date + Duration::days(d_idx as i64);
                let (cell_r, _) = ui.allocate_exact_size(Vec2::new(col_w, 26.0), Sense::hover());
                let all_day_evs: Vec<CalendarEvent> = self
                    .store
                    .events_on_date(d, &self.search_query)
                    .into_iter()
                    .filter(|e| e.all_day)
                    .cloned()
                    .collect();
                for (k, ev) in all_day_evs.iter().take(2).enumerate() {
                    let col = self.cal_color(&ev.calendar_id);
                    let pill = Rect::from_min_max(
                        Pos2::new(cell_r.left() + 3.0, cell_r.top() + 2.0 + k as f32 * 12.0),
                        Pos2::new(cell_r.right() - 3.0, cell_r.bottom() - 2.0),
                    );
                    ui.painter().rect_filled(pill, CornerRadius::same(4), col);
                    ui.painter().text(
                        Pos2::new(pill.left() + 6.0, pill.center().y),
                        egui::Align2::LEFT_CENTER,
                        &ev.title,
                        FontId::proportional(10.5),
                        Color32::WHITE,
                    );
                }
            }
        });

        ui.separator();

        // Hourly timeline with work-hours shading
        let start_hour: u32 = 7;
        let end_hour: u32 = 21;
        let hour_h: f32 = 52.0;
        let total_h = (end_hour - start_hour) as f32 * hour_h;

        egui::ScrollArea::vertical()
            .id_salt("timeline_scroll")
            .show(ui, |ui| {
                let (canvas_rect, _) =
                    ui.allocate_exact_size(Vec2::new(gutter_w + col_w * days_count as f32, total_h), Sense::hover());
                let painter = ui.painter_at(canvas_rect);

                // Subtle work-hours shading
                let work_top = canvas_rect.top()
                    + (self.store.settings.work_day_start as f32 - start_hour as f32).max(0.0) * hour_h;
                let work_bot = canvas_rect.top()
                    + (self.store.settings.work_day_end as f32 - start_hour as f32)
                        .min((end_hour - start_hour) as f32)
                        * hour_h;
                let work_rect = Rect::from_min_max(
                    Pos2::new(canvas_rect.left() + gutter_w, work_top),
                    Pos2::new(canvas_rect.right(), work_bot),
                );
                painter.rect_filled(
                    work_rect,
                    CornerRadius::ZERO,
                    if dark {
                        Color32::from_rgba_unmultiplied(147, 51, 234, 8)
                    } else {
                        Color32::from_rgba_unmultiplied(147, 51, 234, 12)
                    },
                );

                // Horizontal hour lines
                for h in start_hour..=end_hour {
                    let y = canvas_rect.top() + (h - start_hour) as f32 * hour_h;
                    painter.line_segment(
                        [Pos2::new(canvas_rect.left() + gutter_w, y), Pos2::new(canvas_rect.right(), y)],
                        Stroke::new(1.0_f32, grid_line),
                    );
                    if h < end_hour {
                        painter.text(
                            Pos2::new(canvas_rect.left() + gutter_w - 8.0, y + 7.0),
                            egui::Align2::RIGHT_CENTER,
                            format!("{h:02}:00"),
                            FontId::proportional(10.5),
                            Color32::GRAY,
                        );
                    }
                }

                // Vertical day dividers & timed cards
                let mut clicked_event: Option<String> = None;
                for d_idx in 0..days_count {
                    let col_left = canvas_rect.left() + gutter_w + d_idx as f32 * col_w;
                    painter.line_segment(
                        [Pos2::new(col_left, canvas_rect.top()), Pos2::new(col_left, canvas_rect.bottom())],
                        Stroke::new(1.0_f32, grid_line),
                    );

                    let d = start_date + Duration::days(d_idx as i64);
                    let timed_events: Vec<CalendarEvent> = self
                        .store
                        .events_on_date(d, &self.search_query)
                        .into_iter()
                        .filter(|e| !e.all_day && e.start_time.is_some())
                        .cloned()
                        .collect();

                    for ev in timed_events {
                        let st = ev.start_time.unwrap_or_else(|| NaiveTime::from_hms_opt(9, 0, 0).unwrap());
                        let et = ev
                            .end_time
                            .unwrap_or_else(|| NaiveTime::from_hms_opt((st.hour() + 1).min(23), st.minute(), 0).unwrap());

                        let st_f = st.hour() as f32 + st.minute() as f32 / 60.0;
                        let et_f = (et.hour() as f32 + et.minute() as f32 / 60.0).max(st_f + 0.45);

                        if et_f < start_hour as f32 || st_f > end_hour as f32 {
                            continue;
                        }

                        let top_y = canvas_rect.top() + (st_f - start_hour as f32).max(0.0) * hour_h + 1.5;
                        let bot_y = canvas_rect.top()
                            + (et_f - start_hour as f32).min((end_hour - start_hour) as f32) * hour_h
                            - 1.5;

                        let card_rect = Rect::from_min_max(
                            Pos2::new(col_left + 3.0, top_y),
                            Pos2::new(col_left + col_w - 4.0, bot_y.max(top_y + 24.0)),
                        );

                        let col = self.cal_color(&ev.calendar_id);
                        let is_sel = self.selected_event_id.as_deref() == Some(ev.id.as_str());
                        let bg = Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), if is_sel { 230 } else { 190 });
                        painter.rect_filled(card_rect, CornerRadius::same(6), bg);
                        if is_sel {
                            painter.rect_stroke(
                                card_rect,
                                CornerRadius::same(6),
                                Stroke::new(2.0_f32, Color32::WHITE),
                                egui::StrokeKind::Inside,
                            );
                        }

                        let stripe = Rect::from_min_max(
                            card_rect.left_top(),
                            Pos2::new(card_rect.left() + 4.0, card_rect.bottom()),
                        );
                        painter.rect_filled(stripe, CornerRadius::same(2), col);

                        let clip = painter.with_clip_rect(card_rect.shrink(4.0));
                        clip.text(
                            Pos2::new(card_rect.left() + 8.0, card_rect.top() + 9.0),
                            egui::Align2::LEFT_CENTER,
                            &ev.title,
                            FontId::proportional(11.5),
                            Color32::WHITE,
                        );
                        if card_rect.height() >= 34.0 {
                            let sub = match &ev.location {
                                Some(loc) => format!("{}–{} • {loc}", st.format("%H:%M"), et.format("%H:%M")),
                                None => format!("{}–{}", st.format("%H:%M"), et.format("%H:%M")),
                            };
                            clip.text(
                                Pos2::new(card_rect.left() + 8.0, card_rect.top() + 24.0),
                                egui::Align2::LEFT_CENTER,
                                sub,
                                FontId::proportional(10.0),
                                Color32::from_rgba_unmultiplied(255, 255, 255, 220),
                            );
                        }

                        let ev_resp = ui.interact(card_rect, ui.id().with(&ev.id), Sense::click());
                        if ev_resp.clicked() {
                            clicked_event = Some(ev.id.clone());
                        }
                    }

                    // Current time line for today
                    if d == today {
                        let now = Local::now().time();
                        let now_f = now.hour() as f32 + now.minute() as f32 / 60.0;
                        if now_f >= start_hour as f32 && now_f <= end_hour as f32 {
                            let ny = canvas_rect.top() + (now_f - start_hour as f32) * hour_h;
                            painter.circle_filled(Pos2::new(col_left, ny), 4.0, SNOW_PURPLE_600);
                            painter.line_segment(
                                [Pos2::new(col_left, ny), Pos2::new(col_left + col_w, ny)],
                                Stroke::new(1.8_f32, SNOW_PURPLE_600),
                            );
                        }
                    }
                }

                if let Some(id) = clicked_event {
                    self.selected_event_id = Some(id);
                }
            });
    }

    /// Redesigned SnowUI Month View: clean, full-square calendar cards that render as whole distinct squares
    fn render_month_view(&mut self, ui: &mut egui::Ui) {
        let year = self.selected_date.year();
        let month = self.selected_date.month();
        let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap_or(self.selected_date);
        let offset = first.weekday().num_days_from_monday() as i64;
        let grid_start = first - Duration::days(offset);
        let today = Local::now().date_naive();
        let dark = self.store.settings.dark_mode;

        let avail_w = ui.available_width().max(400.0);
        let avail_h = ui.available_height().max(400.0);
        let header_h = 28.0;
        let gap = 4.0;
        let col_w = ((avail_w - gap * 6.0) / 7.0).floor().max(60.0);
        let row_h = ((avail_h - header_h - gap * 5.0) / 6.0).floor().max(75.0);

        // Weekday Column Headers
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for wd in ["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"] {
                let (r, _) = ui.allocate_exact_size(Vec2::new(col_w, header_h), Sense::hover());
                ui.painter().text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    wd,
                    FontId::proportional(11.0),
                    if dark { Color32::from_rgb(148, 163, 184) } else { Color32::from_rgb(100, 116, 139) },
                );
            }
        });

        let card_bg_cur = if dark { Color32::from_rgb(24, 32, 47) } else { Color32::WHITE };
        let card_bg_other = if dark { Color32::from_rgb(17, 24, 39) } else { Color32::from_rgb(241, 245, 249) };
        let card_border = if dark { SNOW_SLATE_700 } else { SNOW_SLATE_200 };

        for week in 0..6 {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for day_i in 0..7 {
                    let d = grid_start + Duration::days(week * 7 + day_i);
                    let in_m = d.month() == month;
                    let is_today = d == today;
                    let is_sel = d == self.selected_date;

                    let (cell_rect, resp) = ui.allocate_exact_size(Vec2::new(col_w, row_h), Sense::click());
                    if resp.clicked() {
                        self.selected_date = d;
                    }

                    // Draw whole square card with crisp SnowUI rounded borders and background
                    let bg = if is_sel {
                        if dark { Color32::from_rgb(38, 48, 72) } else { SNOW_PURPLE_50 }
                    } else if in_m {
                        card_bg_cur
                    } else {
                        card_bg_other
                    };

                    ui.painter().rect_filled(cell_rect, CornerRadius::same(8), bg);
                    let border_stroke = if is_sel {
                        Stroke::new(1.8_f32, SNOW_PURPLE_500)
                    } else {
                        Stroke::new(1.0_f32, card_border)
                    };
                    ui.painter().rect_stroke(cell_rect, CornerRadius::same(8), border_stroke, egui::StrokeKind::Inside);

                    // Day Number Pill / Header inside the square
                    let badge_pos = Pos2::new(cell_rect.right() - 14.0, cell_rect.top() + 14.0);
                    if is_today {
                        ui.painter().circle_filled(badge_pos, 10.0, SNOW_PURPLE_600);
                    }
                    ui.painter().text(
                        badge_pos,
                        egui::Align2::CENTER_CENTER,
                        d.day().to_string(),
                        FontId::proportional(11.5),
                        if is_today {
                            Color32::WHITE
                        } else if in_m {
                            ui.visuals().text_color()
                        } else {
                            Color32::GRAY
                        },
                    );

                    // Render up to 3 event badges inside the square
                    let evs: Vec<CalendarEvent> = self
                        .store
                        .events_on_date(d, &self.search_query)
                        .into_iter()
                        .cloned()
                        .collect();

                    for (idx, ev) in evs.iter().take(3).enumerate() {
                        let pill_y = cell_rect.top() + 26.0 + idx as f32 * 16.0;
                        if pill_y + 14.0 > cell_rect.bottom() - 2.0 {
                            break;
                        }
                        let pill = Rect::from_min_max(
                            Pos2::new(cell_rect.left() + 4.0, pill_y),
                            Pos2::new(cell_rect.right() - 4.0, pill_y + 14.0),
                        );
                        let col = self.cal_color(&ev.calendar_id);
                        ui.painter().rect_filled(pill, CornerRadius::same(4), col);
                        ui.painter().with_clip_rect(pill.shrink(2.0)).text(
                            Pos2::new(pill.left() + 4.0, pill.center().y),
                            egui::Align2::LEFT_CENTER,
                            &ev.title,
                            FontId::proportional(9.5),
                            Color32::WHITE,
                        );
                    }

                    if evs.len() > 3 {
                        ui.painter().text(
                            Pos2::new(cell_rect.left() + 6.0, cell_rect.bottom() - 8.0),
                            egui::Align2::LEFT_CENTER,
                            format!("+{} more", evs.len() - 3),
                            FontId::proportional(8.5),
                            Color32::GRAY,
                        );
                    }
                }
            });
            ui.add_space(gap);
        }
    }

    /// SnowUI Quarterly View (Q1, Q2, Q3, Q4 with 3 full interactive months side by side)
    fn render_quarter_view(&mut self, ui: &mut egui::Ui) {
        let year = self.selected_date.year();
        let cur_q = (self.selected_date.month() - 1) / 3 + 1;
        let start_month = (cur_q - 1) * 3 + 1;

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.heading(RichText::new(format!("Q{} {} ({} – {})",
                cur_q,
                year,
                NaiveDate::from_ymd_opt(year, start_month, 1).unwrap().format("%B"),
                NaiveDate::from_ymd_opt(year, start_month + 2, 1).unwrap().format("%B"))
            ).size(18.0).strong());

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                for q in (1..=4).rev() {
                    let is_active = q == cur_q;
                    let btn = egui::Button::new(RichText::new(format!("Q{q}")).size(12.0).color(
                        if is_active { Color32::WHITE } else { ui.visuals().text_color() }
                    ))
                    .fill(if is_active { SNOW_PURPLE_600 } else { ui.visuals().widgets.inactive.bg_fill })
                    .corner_radius(CornerRadius::same(6));
                    if ui.add(btn).clicked() {
                        let target_m = (q - 1) * 3 + 1;
                        self.selected_date = NaiveDate::from_ymd_opt(year, target_m, 1).unwrap_or(self.selected_date);
                    }
                }
            });
        });

        ui.add_space(10.0);

        // 3 Month Cards side-by-side or wrapped
        let avail_w = ui.available_width().max(600.0);
        let card_w = ((avail_w - 32.0) / 3.0).max(220.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for m_offset in 0..3 {
                    let m = start_month + m_offset;
                    egui::Frame::group(ui.style())
                        .fill(if self.store.settings.dark_mode { SNOW_SLATE_800 } else { Color32::WHITE })
                        .stroke(Stroke::new(1.0_f32, if self.store.settings.dark_mode { SNOW_SLATE_700 } else { SNOW_SLATE_200 }))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.set_width(card_w);
                            self.render_mini_month(ui, year, m, false);

                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(4.0);
                            let m_first = NaiveDate::from_ymd_opt(year, m, 1).unwrap();
                            let m_last = if m == 12 {
                                NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap() - Duration::days(1)
                            } else {
                                NaiveDate::from_ymd_opt(year, m + 1, 1).unwrap() - Duration::days(1)
                            };
                            let m_evs: Vec<&CalendarEvent> = self.store.events.iter()
                                .filter(|e| e.date >= m_first && e.date <= m_last)
                                .collect();
                            ui.label(RichText::new(format!("{} Scheduled Events", m_evs.len())).size(11.0).color(SNOW_PURPLE_500).strong());
                            for ev in m_evs.into_iter().take(4) {
                                ui.label(RichText::new(format!("• {} ({})", ev.title, ev.date.format("%b %d"))).size(11.0));
                            }
                        });
                }
            });
        });
    }

    /// Fixed SnowUI Year View: 4 columns x 3 rows grid, cleanly budgeted without overflow
    fn render_year_view(&mut self, ui: &mut egui::Ui) {
        let year = self.selected_date.year();
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.heading(RichText::new(format!("{year} Calendar")).size(18.0).strong());
            });
            ui.add_space(10.0);

            let avail_w = ui.available_width().max(640.0);
            let col_w = ((avail_w - 40.0) / 4.0).max(180.0);

            for row in 0..3 {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    for col in 0..4 {
                        let m = row * 4 + col + 1;
                        egui::Frame::group(ui.style())
                            .fill(if self.store.settings.dark_mode { SNOW_SLATE_800 } else { Color32::WHITE })
                            .stroke(Stroke::new(1.0_f32, if self.store.settings.dark_mode { SNOW_SLATE_700 } else { SNOW_SLATE_200 }))
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(egui::Margin::same(8))
                            .show(ui, |ui| {
                                ui.set_width(col_w);
                                self.render_mini_month(ui, year, m, false);
                            });
                    }
                });
                ui.add_space(8.0);
            }
        });
    }

    fn render_inspector_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.label(RichText::new("EVENT INSPECTOR").size(10.5).strong().color(Color32::GRAY));
        ui.add_space(4.0);

        let selected_ev = self
            .selected_event_id
            .as_ref()
            .and_then(|id| self.store.events.iter().find(|e| &e.id == id))
            .cloned();

        if let Some(mut ev) = selected_ev {
            let mut changed = false;
            let mut delete_clicked = false;

            let col = self.cal_color(&ev.calendar_id);
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(Vec2::new(12.0, 12.0), Sense::hover());
                ui.painter().circle_filled(dot.center(), 5.0, col);
                if ui.text_edit_singleline(&mut ev.title).changed() {
                    changed = true;
                }
            });

            ui.add_space(6.0);
            ui.label(RichText::new(format!("📅 {}", ev.date.format("%A, %b %d, %Y"))).size(12.0));

            if ui.checkbox(&mut ev.all_day, "All-day event").changed() {
                changed = true;
            }

            if !ev.all_day {
                let st = ev
                    .start_time
                    .map(|t| t.format("%H:%M").to_string())
                    .unwrap_or_else(|| "09:00".to_string());
                let et = ev
                    .end_time
                    .map(|t| t.format("%H:%M").to_string())
                    .unwrap_or_else(|| "10:00".to_string());
                ui.label(RichText::new(format!("⏰ {st} – {et}")).size(12.0));
            }

            ui.add_space(6.0);
            ui.label(RichText::new("Calendar").size(11.0).color(Color32::GRAY));
            egui::ComboBox::from_id_salt("inspector_cal_combo")
                .selected_text(
                    self.store
                        .calendar_by_id(&ev.calendar_id)
                        .map(|c| c.name.as_str())
                        .unwrap_or("Select"),
                )
                .show_ui(ui, |ui| {
                    for c in &self.store.calendars {
                        if ui
                            .selectable_value(&mut ev.calendar_id, c.id.clone(), &c.name)
                            .changed()
                        {
                            changed = true;
                        }
                    }
                });

            ui.add_space(6.0);
            ui.label(RichText::new("Location").size(11.0).color(Color32::GRAY));
            let mut loc_str = ev.location.clone().unwrap_or_default();
            if ui.text_edit_singleline(&mut loc_str).changed() {
                ev.location = if loc_str.trim().is_empty() { None } else { Some(loc_str) };
                changed = true;
            }

            if let Some(ref url) = ev.conference_url {
                ui.add_space(4.0);
                ui.hyperlink_to("🎥 Join Video Call", url);
            }

            if !ev.attendees.is_empty() {
                ui.add_space(6.0);
                ui.label(RichText::new("Invitees").size(11.0).color(Color32::GRAY));
                for att in &ev.attendees {
                    ui.label(RichText::new(format!("• {att}")).size(11.5));
                }
            }

            ui.add_space(6.0);
            ui.label(RichText::new("Notes").size(11.0).color(Color32::GRAY));
            if ui
                .add(egui::TextEdit::multiline(&mut ev.notes).desired_rows(4))
                .changed()
            {
                changed = true;
            }

            ui.add_space(10.0);
            if ui
                .button(RichText::new("🗑 Delete Event").color(Color32::from_rgb(248, 113, 113)))
                .clicked()
            {
                delete_clicked = true;
            }

            if delete_clicked {
                self.store.events.retain(|x| x.id != ev.id);
                self.selected_event_id = self.store.events.first().map(|e| e.id.clone());
                self.persist_if_configured();
            } else if changed {
                if let Some(slot) = self.store.events.iter_mut().find(|x| x.id == ev.id) {
                    *slot = ev;
                }
                self.persist_if_configured();
            }
        } else {
            ui.label("Select any event card to inspect or edit its details.");
        }
    }

    fn render_modals(&mut self, ctx: &egui::Context) {
        if self.show_accounts_modal {
            let mut open = self.show_accounts_modal;
            egui::Window::new("⚙ Preloaded Configuration & Accounts")
                .open(&mut open)
                .default_width(620.0)
                .show(ctx, |ui| {
                    ui.label("CalendarCraft boots with preloaded accounts, calendar sets, and events.");
                    ui.separator();

                    ui.heading("Add Account");
                    egui::Grid::new("add_acc_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Account Name:");
                        ui.text_edit_singleline(&mut self.new_acc_name);
                        ui.end_row();

                        ui.label("Provider:");
                        ui.text_edit_singleline(&mut self.new_acc_provider);
                        ui.end_row();

                        ui.label("Email / Principal:");
                        ui.text_edit_singleline(&mut self.new_acc_email);
                        ui.end_row();

                        ui.label("Server URL:");
                        ui.text_edit_singleline(&mut self.new_acc_server);
                        ui.end_row();

                        ui.label("Initial Calendar & Hex:");
                        ui.horizontal(|ui| {
                            ui.text_edit_singleline(&mut self.new_cal_name);
                            ui.text_edit_singleline(&mut self.new_cal_color);
                        });
                        ui.end_row();
                    });

                    if ui.button("＋ Add Preconfigured Account").clicked() && !self.new_acc_name.trim().is_empty() {
                        let acc_id = self.store.next_id("acc");
                        let cal_id = self.store.next_id("cal");
                        self.store.accounts.push(AccountConfig {
                            id: acc_id.clone(),
                            name: self.new_acc_name.trim().to_string(),
                            provider: self.new_acc_provider.trim().to_string(),
                            email: self.new_acc_email.trim().to_string(),
                            server_url: Some(self.new_acc_server.trim().to_string()),
                            enabled: true,
                        });
                        self.store.calendars.push(CalendarSource {
                            id: cal_id.clone(),
                            account_id: acc_id,
                            name: self.new_cal_name.trim().to_string(),
                            color_hex: self.new_cal_color.trim().to_string(),
                            visible: true,
                            read_only: false,
                        });
                        for s in &mut self.store.calendar_sets {
                            if s.id == "set_all" {
                                s.calendar_ids.push(cal_id.clone());
                            }
                        }
                        self.config_json_editor = self.store.to_json_pretty().unwrap_or_default();
                        self.persist_if_configured();
                    }

                    ui.separator();
                    ui.heading("Live JSON Configuration Preload");
                    ui.add(
                        egui::TextEdit::multiline(&mut self.config_json_editor)
                            .code_editor()
                            .desired_rows(10)
                            .desired_width(f32::INFINITY),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Apply JSON Config Now").clicked() {
                            match CalendarStore::from_json_str(&self.config_json_editor) {
                                Ok(new_store) => {
                                    self.store = new_store;
                                    self.persist_if_configured();
                                    self.status_banner = Some("Reloaded configuration from JSON.".to_string());
                                }
                                Err(e) => {
                                    self.status_banner = Some(e);
                                }
                            }
                        }
                        if ui.button("Refresh From Current State").clicked() {
                            self.config_json_editor = self.store.to_json_pretty().unwrap_or_default();
                        }
                    });
                });
            self.show_accounts_modal = open;
        }

        if self.show_mcp_modal {
            let mut open = self.show_mcp_modal;
            egui::Window::new("🔌 Model Context Protocol (MCP) Server")
                .open(&mut open)
                .default_width(640.0)
                .show(ctx, |ui| {
                    ui.label("Connect any MCP client via stdio:");
                    ui.code("calendarcraft-mcp --config calendarcraft.config.json");
                    ui.separator();
                    ui.label(RichText::new("Test MCP JSON-RPC Request Live Against Active Store:").strong());
                    ui.add(
                        egui::TextEdit::multiline(&mut self.mcp_request_input)
                            .code_editor()
                            .desired_rows(8)
                            .desired_width(f32::INFINITY),
                    );
                    if ui.button("▶ Execute MCP Request").clicked() {
                        match serde_json::from_str::<serde_json::Value>(&self.mcp_request_input) {
                            Ok(val) => {
                                if let Some(resp) = handle_mcp_request(&mut self.store, &val) {
                                    self.mcp_response_output =
                                        serde_json::to_string_pretty(&resp).unwrap_or_default();
                                    self.persist_if_configured();
                                } else {
                                    self.mcp_response_output = "(Notification acknowledged)".to_string();
                                }
                            }
                            Err(e) => {
                                self.mcp_response_output = format!("JSON parse error: {e}");
                            }
                        }
                    }
                    if !self.mcp_response_output.is_empty() {
                        ui.label(RichText::new("MCP Response:").strong());
                        ui.add(
                            egui::TextEdit::multiline(&mut self.mcp_response_output)
                                .code_editor()
                                .desired_rows(8)
                                .desired_width(f32::INFINITY),
                        );
                    }
                });
            self.show_mcp_modal = open;
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn handle_screenshot_lifecycle(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot_path.clone() else {
            return;
        };

        ctx.request_repaint();
        self.frame_count += 1;

        let mut captured: Option<Arc<egui::ColorImage>> = None;
        ctx.input(|i| {
            for event in &i.raw.events {
                if let egui::Event::Screenshot { image, .. } = event {
                    captured = Some(Arc::clone(image));
                }
            }
        });

        if let Some(img) = captured {
            if let Some(parent) = std::path::Path::new(&path).parent() {
                if !parent.as_os_str().is_empty() {
                    let _ = std::fs::create_dir_all(parent);
                }
            }
            let mut rgba = Vec::with_capacity(img.pixels.len() * 4);
            for px in &img.pixels {
                rgba.extend_from_slice(&px.to_array());
            }
            if let Err(err) = image::save_buffer(
                &path,
                &rgba,
                img.width() as u32,
                img.height() as u32,
                image::ColorType::Rgba8,
            ) {
                eprintln!("Screenshot save failed: {err}");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            std::process::exit(0);
        }

        if self.frame_count >= 120 && !self.screenshot_requested {
            self.screenshot_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
    }
}

impl eframe::App for CalendarCraftApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_theme(ctx);

        #[cfg(not(target_arch = "wasm32"))]
        self.handle_screenshot_lifecycle(ctx);

        egui::TopBottomPanel::top("top_toolbar").show(ctx, |ui| {
            self.render_top_bar(ui);
        });

        egui::SidePanel::left("left_sidebar")
            .default_width(275.0)
            .min_width(240.0)
            .resizable(true)
            .show(ctx, |ui| {
                self.render_left_sidebar(ui);
            });

        egui::SidePanel::right("right_inspector")
            .default_width(245.0)
            .min_width(210.0)
            .resizable(true)
            .show(ctx, |ui| {
                self.render_inspector_panel(ui);
            });

        egui::CentralPanel::default().show(ctx, |ui| match self.view_mode {
            ViewMode::Day => self.render_week_or_day_view(ui, 1),
            ViewMode::Week => self.render_week_or_day_view(ui, 7),
            ViewMode::Month => self.render_month_view(ui),
            ViewMode::Quarter => self.render_quarter_view(ui),
            ViewMode::Year => self.render_year_view(ui),
        });

        self.render_modals(ctx);
    }
}
