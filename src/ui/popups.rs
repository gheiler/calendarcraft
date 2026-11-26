//! Event details in a popover beside the event, the new-event dialog (double-click a day or a
//! time, ⌘N or ＋), Settings with accounts, and the toast.

use chrono::{Duration, NaiveDate, NaiveTime};
use eframe::egui::{self, Align, Align2, Color32, CornerRadius, Layout, Pos2, Rect, RichText, Stroke, Vec2};

use super::icons::Icon;
use super::side::{icon_button, segmented, view_select};
use super::{parse_hhmm, today, AccountForm, CalendarCraftApp, Inspector, NewEvent, MISSING_PASSWORD};
use crate::filesync::{self, ConflictChoice};
use crate::host;
use crate::i18n::{self, t};
use crate::mcp::handle_mcp_request;
use crate::model::{AccountKind, CalendarEvent, CalendarSource, ViewMode};
use crate::nlp::parse_quick_entry;
use crate::theme::{self, font, font_bold, space, text, Palette};

/// How far the close button reaches into the popover's padding, so its 16px cross ends on the
/// same edge as the buttons below (the cross sits 8px inside its 32px button).
const CLOSE_INSET: f32 = 8.0;

const ALERTS: &[(Option<u32>, &str)] = &[(None, "None"), (Some(0), "At time of event"), (Some(5), "5 minutes before"), (Some(10), "10 minutes before"), (Some(15), "15 minutes before"), (Some(30), "30 minutes before"), (Some(60), "1 hour before"), (Some(1440), "1 day before")];
const REPEATS: &[(&str, &str)] = &[("", "Never"), ("FREQ=DAILY", "Every day"), ("FREQ=WEEKLY", "Every week"), ("FREQ=MONTHLY", "Every month"), ("FREQ=YEARLY", "Every year")];

/// Margin left on each side of a modal so it never touches the screen edge.
const DIALOG_MARGIN: f32 = 12.0;
/// Padding of the larger dialogs (Settings, new event, file conflict, migration).
const DIALOG_PAD: f32 = 24.0;

/// Outer width for a dialog: `preferred` on a wide screen, the screen minus margins
/// on a phone, and never wider than the screen.
fn dialog_outer_width(screen_w: f32, preferred: f32) -> f32 {
    (screen_w - 2.0 * DIALOG_MARGIN).min(preferred).max(160.0)
}

/// Content width inside a dialog frame with `pad` on each side (so 2 * pad in total).
fn dialog_content_width(screen_w: f32, preferred: f32, pad: f32) -> f32 {
    (dialog_outer_width(screen_w, preferred) - 2.0 * pad).max(120.0)
}

/// Room left for a dialog body after chrome (title, tabs, frame padding).
fn dialog_max_body_height(screen_h: f32, chrome: f32) -> f32 {
    (screen_h - 2.0 * DIALOG_MARGIN - chrome).max(120.0)
}

fn set_dialog_width(ui: &mut egui::Ui, width: f32) {
    ui.set_width(width);
    ui.set_max_width(width);
}

/// Text fields for the editable times and dates.
struct Texts<'a> {
    date: &'a mut String,
    start: &'a mut String,
    end: &'a mut String,
}

fn field_label(ui: &mut egui::Ui, p: &Palette, label: &str) {
    ui.label(RichText::new(label).font(font(text::T12)).color(p.text_2));
}

/// The fields of an event, shared by the popover and the dialog. Returns whether anything changed.
fn event_fields(ui: &mut egui::Ui, p: &Palette, ev: &mut CalendarEvent, texts: Texts, calendars: &[CalendarSource], editable: bool) -> bool {
    let mut changed = false;
    // Widths below are the text inside a field; the SnowUI padding adds 24px to each.
    let pad = (theme::INPUT_PADDING.left + theme::INPUT_PADDING.right) as f32;
    let label_w = 70.0;
    let col_w = (ui.available_width() - label_w - space::S12).max(100.0);
    let field_w = (ui.available_width() - label_w - space::S12 - 16.0).clamp(100.0, 244.0) - pad;
    ui.add_enabled_ui(editable, |ui| {
        egui::Grid::new(ui.id().with("fields")).num_columns(2).spacing([space::S12, space::S8]).min_col_width(label_w).show(ui, |ui| {
            field_label(ui, p, t("Calendar"));
            let current = calendars.iter().find(|c| c.id == ev.calendar_id).map(|c| c.name.clone()).unwrap_or_else(|| t("Choose…").into());
            theme::select(ui, p, egui::ComboBox::from_id_salt(ui.id().with("cal")).selected_text(current).width(field_w), |ui| {
                for c in calendars {
                    let label = RichText::new(format!("●  {}", c.name)).color(theme::color_of(&c.color_hex));
                    if ui.selectable_label(c.id == ev.calendar_id, label).clicked() {
                        ev.calendar_id = c.id.clone();
                        changed = true;
                    }
                }
            });
            ui.end_row();

            field_label(ui, p, t("All-day"));
            changed |= ui.checkbox(&mut ev.all_day, "").changed();
            ui.end_row();

            field_label(ui, p, t("Date"));
            // 110 and 56 match the original fields, so "12:00" stays readable. The
            // painted field is desired_width plus padding; when that row is wider
            // than the column, the times go under the date instead of being squeezed.
            let date_w = 110.0;
            let time_w = 56.0;
            let row_needed = date_w + pad + space::S8 + (time_w + pad) * 2.0 + 3.0 * space::S8 + 8.0;
            let stack = !ev.all_day && col_w < row_needed;
            if stack {
                changed |= theme::text_field(ui, p, egui::TextEdit::singleline(texts.date).desired_width(date_w).hint_text("2026-11-24")).changed();
                ui.end_row();
                field_label(ui, p, "");
                ui.horizontal(|ui| {
                    changed |= theme::text_field(ui, p, egui::TextEdit::singleline(texts.start).desired_width(time_w).hint_text("09:00")).changed();
                    ui.label("–");
                    changed |= theme::text_field(ui, p, egui::TextEdit::singleline(texts.end).desired_width(time_w).hint_text("10:00")).changed();
                });
            } else {
                ui.horizontal(|ui| {
                    changed |= theme::text_field(ui, p, egui::TextEdit::singleline(texts.date).desired_width(date_w).hint_text("2026-11-24")).changed();
                    if !ev.all_day {
                        changed |= theme::text_field(ui, p, egui::TextEdit::singleline(texts.start).desired_width(time_w).hint_text("09:00")).changed();
                        ui.label("–");
                        changed |= theme::text_field(ui, p, egui::TextEdit::singleline(texts.end).desired_width(time_w).hint_text("10:00")).changed();
                    }
                });
            }
            ui.end_row();

            if ev.all_day {
                field_label(ui, p, t("Until"));
                let mut end = ev.end_date.map(|d| d.to_string()).unwrap_or_default();
                if theme::text_field(ui, p, egui::TextEdit::singleline(&mut end).desired_width(110.0_f32.min(field_w)).hint_text(t("same day"))).changed() {
                    ev.end_date = NaiveDate::parse_from_str(end.trim(), "%Y-%m-%d").ok();
                    changed = true;
                }
                ui.end_row();
            }

            field_label(ui, p, t("Location"));
            let mut loc = ev.location.clone().unwrap_or_default();
            if theme::text_field(ui, p, egui::TextEdit::singleline(&mut loc).desired_width(field_w).hint_text(t("Add location"))).changed() {
                ev.location = Some(loc).filter(|l| !l.trim().is_empty());
                changed = true;
            }
            ui.end_row();

            field_label(ui, p, t("Link"));
            let mut url = ev.conference_url.clone().unwrap_or_default();
            if theme::text_field(ui, p, egui::TextEdit::singleline(&mut url).desired_width(field_w).hint_text("https://")).changed() {
                ev.conference_url = Some(url).filter(|u| !u.trim().is_empty());
                changed = true;
            }
            ui.end_row();

            field_label(ui, p, t("Alert"));
            let label = ALERTS.iter().find(|(m, _)| *m == ev.alert_minutes_before).map(|(_, l)| t(l).to_string()).unwrap_or_else(|| format!("{} min", ev.alert_minutes_before.unwrap_or(0)));
            theme::select(ui, p, egui::ComboBox::from_id_salt(ui.id().with("alert")).selected_text(label).width(field_w), |ui| {
                for (m, l) in ALERTS {
                    if ui.selectable_label(ev.alert_minutes_before == *m, t(l)).clicked() {
                        ev.alert_minutes_before = *m;
                        changed = true;
                    }
                }
            });
            ui.end_row();

            field_label(ui, p, t("Repeat"));
            let rule = ev.rrule.clone().unwrap_or_default();
            let known = REPEATS.iter().find(|(r, _)| *r == rule);
            let label = known.map(|(_, l)| t(l).to_string()).unwrap_or_else(|| t("Custom").to_string());
            theme::select(ui, p, egui::ComboBox::from_id_salt(ui.id().with("repeat")).selected_text(label).width(field_w), |ui| {
                for (r, l) in REPEATS {
                    if ui.selectable_label(rule == *r, t(l)).clicked() {
                        ev.rrule = Some(r.to_string()).filter(|r| !r.is_empty());
                        changed = true;
                    }
                }
            });
            ui.end_row();
        });
        field_label(ui, p, t("Notes"));
        changed |= theme::text_field(ui, p, egui::TextEdit::multiline(&mut ev.notes).desired_rows(3).desired_width(f32::INFINITY).hint_text(t("Add notes"))).changed();
    });
    changed
}

/// Apply the date and time texts; an error names the field.
fn apply_texts(ev: &mut CalendarEvent, date: &str, start: &str, end: &str) -> Result<(), String> {
    ev.date = NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d").map_err(|_| t("The date should look like 2026-11-24").to_string())?;
    if ev.all_day {
        ev.start_time = None;
        ev.end_time = None;
    } else {
        let s = parse_hhmm(start).ok_or_else(|| t("The start time should look like 09:30").to_string())?;
        let e = if end.trim().is_empty() { s + Duration::hours(1) } else { parse_hhmm(end).ok_or_else(|| t("The end time should look like 10:30").to_string())? };
        ev.start_time = Some(s);
        ev.end_time = Some(e);
        if e <= s && ev.end_date.is_none() {
            return Err(t("The event ends before it starts").to_string());
        }
    }
    Ok(())
}

impl CalendarCraftApp {
    pub fn open_inspector(&mut self, id: &str, anchor: Option<Rect>) {
        let Some(ev) = self.store.events.iter().find(|e| e.id == id).cloned() else { return };
        self.new_event = None;
        self.inspector = Some(Inspector {
            event_id: id.to_string(),
            anchor,
            date_text: ev.date.to_string(),
            start_text: ev.start_time.map(|t| t.format("%H:%M").to_string()).unwrap_or_default(),
            end_text: ev.end_time.map(|t| t.format("%H:%M").to_string()).unwrap_or_default(),
            draft: ev,
            confirm_delete: false,
        });
    }

    pub fn open_new_event(&mut self, day: NaiveDate, at: Option<NaiveTime>) {
        self.inspector = None;
        let cal = self.default_calendar().unwrap_or_default();
        let start = at.unwrap_or_else(|| NaiveTime::from_hms_opt(9, 0, 0).unwrap());
        let draft = CalendarEvent {
            id: self.store.next_id("ev"),
            calendar_id: cal,
            title: String::new(),
            date: day,
            start_time: Some(start),
            end_time: Some(start + Duration::hours(1)),
            alert_minutes_before: Some(10),
            zone: crate::tz::EventZone::from_settings(&self.store.settings.timezone),
            ..Default::default()
        };
        self.new_event = Some(NewEvent {
            sentence: String::new(),
            day,
            date_text: day.to_string(),
            start_text: start.format("%H:%M").to_string(),
            end_text: (start + Duration::hours(1)).format("%H:%M").to_string(),
            draft,
            focus_done: false,
        });
    }

    pub(super) fn popups(&mut self, ctx: &egui::Context) {
        let p = *self.palette();
        self.inspector_popover(ctx, &p);
        self.new_event_dialog(ctx, &p);
        self.settings_dialog(ctx, &p);
        self.delete_confirm_dialog(ctx, &p);
        self.file_conflict_dialog(ctx, &p);
        self.migration_dialog(ctx, &p);
        if let Some((msg, error)) = self.banner.clone() {
            egui::Area::new("toast".into()).order(egui::Order::Tooltip).anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -24.0)).show(ctx, |ui| {
                egui::Frame::new().fill(if error { p.danger } else { p.text }).corner_radius(CornerRadius::same(12)).inner_margin(egui::Margin::symmetric(16, 10)).shadow(ctx.style().visuals.popup_shadow).show(ui, |ui| {
                    ui.set_max_width((ctx.screen_rect().width() - 48.0).min(420.0));
                    ui.add(egui::Label::new(RichText::new(msg).color(if error { Color32::WHITE } else { p.surface })).wrap());
                });
            });
        }
    }

    fn inspector_popover(&mut self, ctx: &egui::Context, p: &Palette) {
        let Some(mut ins) = self.inspector.take() else { return };
        let screen = ctx.screen_rect();
        let mobile = self.is_mobile(ctx);
        // A full-width bottom sheet on a phone; 460px on the desktop so the date
        // and both times fit on one row at their full size.
        let width = if mobile { screen.width() } else { 460.0_f32.min(screen.width() - 16.0).max(200.0) };
        let max_h = if mobile { (screen.height() * 0.92).min(screen.height() - 8.0) } else { (screen.height() - 16.0).min(560.0) };
        let sheet_h = self.popup_sizes.get("inspector").map(|s| s.y.min(max_h)).unwrap_or_else(|| if mobile { (screen.height() * 0.6).min(max_h) } else { max_h });
        let editable = self.calendar_writable(&ins.draft.calendar_id) && !ins.draft.locked;
        let calendars: Vec<CalendarSource> = if editable { self.writable_calendars() } else { self.store.calendars.clone() };
        let pos = match (mobile, ins.anchor) {
            (true, _) => Pos2::new(0.0, screen.bottom() - sheet_h),
            (false, Some(a)) => {
                let right = a.right() + 10.0;
                let x = if right + width < screen.right() - 8.0 { right } else { (a.left() - width - 10.0).max(8.0) };
                Pos2::new(x, (a.top() - 40.0).clamp(8.0, (screen.bottom() - sheet_h).max(8.0)))
            }
            (false, None) => Pos2::new(screen.center().x - width / 2.0, screen.top() + 90.0),
        };
        let color = self.cal_color(&ins.draft.calendar_id);
        let mut close = false;
        let mut save = false;
        let mut delete = false;
        let mut error: Option<String> = None;
        let mut body_size: Option<Vec2> = None;
        let area = egui::Area::new("inspector".into()).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            let corner = if mobile { CornerRadius { nw: 16, ne: 16, sw: 0, se: 0 } } else { CornerRadius::same(16) };
            egui::Frame::new().fill(p.surface).stroke(Stroke::new(1.0_f32, p.line)).corner_radius(corner).shadow(ctx.style().visuals.window_shadow).inner_margin(egui::Margin::same(20)).show(ui, |ui| {
                let inner = (width - 40.0).max(120.0);
                set_dialog_width(ui, inner);
                ui.horizontal(|ui| {
                    // The close button is placed by hand on the right so its 16px cross ends on the
                    // same edge as Save below (the frame's 20px padding); the title gets the rest.
                    let row = ui.available_rect_before_wrap();
                    let close_rect = Rect::from_min_size(Pos2::new(row.right() - 32.0 + CLOSE_INSET, row.top()), Vec2::splat(32.0));
                    let close_resp = ui.interact(close_rect, ui.id().with("inspector-close"), egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(t("Close"));
                    if close_resp.is_pointer_button_down_on() {
                        ui.painter().rect_filled(close_rect, CornerRadius::same(8), p.primary_soft);
                    } else if close_resp.hovered() {
                        ui.painter().rect_filled(close_rect, CornerRadius::same(8), p.hover);
                    }
                    super::icons::paint(ui.painter(), Rect::from_center_size(close_rect.center(), Vec2::splat(16.0)), Icon::Close, p.text);
                    if close_resp.clicked() {
                        close = true;
                    }
                    let (r, _) = ui.allocate_exact_size(Vec2::new(12.0, 32.0), egui::Sense::hover());
                    ui.painter().circle_filled(r.center(), 6.0, color);
                    let title_w = (close_rect.left() - ui.cursor().left() - 8.0).max(40.0);
                    ui.add_enabled(editable, egui::TextEdit::singleline(&mut ins.draft.title).font(font_bold(text::T18)).frame(false).desired_width(title_w).hint_text(t("Title")));
                });
                // Keep the title and close button visible; scroll the fields when the sheet is tall.
                let body_h = (max_h - 72.0).max(80.0);
                let content_h = self.popup_sizes.get("inspector_body").map(|s| s.y).unwrap_or(body_h);
                let scroll = egui::ScrollArea::vertical().max_height(body_h).min_scrolled_height(content_h.min(body_h)).show(ui, |ui| {
                    set_dialog_width(ui, inner);
                    ui.label(RichText::new(describe_when(&ins.draft)).font(font(text::T14)).color(p.text_2));
                    if !editable {
                        let why = if ins.draft.locked { t("Part of a repeating series with changes; edit it in the app that made it.") } else { t("This calendar is read only.") };
                        ui.horizontal_wrapped(|ui| {
                            super::icons::inline(ui, Icon::Lock, 12.0, p.text_2);
                            ui.add(egui::Label::new(RichText::new(why).font(font(text::T12)).color(p.text_2)).wrap());
                        });
                    }
                    ui.add_space(space::S8);
                    let Inspector { draft, date_text, start_text, end_text, .. } = &mut ins;
                    event_fields(ui, p, draft, Texts { date: date_text, start: start_text, end: end_text }, &calendars, editable);
                    ui.add_space(space::S12);
                    ui.horizontal_wrapped(|ui| {
                        if editable {
                            if ins.confirm_delete {
                                if ui.add(egui::Button::new(RichText::new(t("Delete event")).color(Color32::WHITE)).fill(p.danger)).clicked() {
                                    delete = true;
                                }
                                if ui.button(t("Keep")).clicked() {
                                    ins.confirm_delete = false;
                                }
                            } else if ui.add(egui::Button::new(RichText::new(t("Delete")).color(p.danger)).fill(Color32::TRANSPARENT)).clicked() {
                                ins.confirm_delete = true;
                            }
                        }
                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                            if editable {
                                if theme::primary_button(ui, p, t("Save")).clicked() {
                                    save = true;
                                }
                                if ui.button(t("Cancel")).clicked() {
                                    close = true;
                                }
                            } else if ui.button(t("Close")).clicked() {
                                close = true;
                            }
                        });
                    });
                });
                body_size = Some(scroll.content_size);
            });
        });
        // A click elsewhere closes it (not the click that opened it).
        self.settle(ctx, "inspector", area.response.rect.size());
        if let Some(size) = body_size {
            self.settle(ctx, "inspector_body", size);
        }
        let outside = ctx.input(|i| i.pointer.any_pressed()) && ctx.input(|i| i.pointer.interact_pos()).is_some_and(|pt| !area.response.rect.contains(pt)) && ins.anchor.is_none_or(|a| !a.contains(ctx.input(|i| i.pointer.interact_pos()).unwrap_or_default()));
        if save {
            let mut ev = ins.draft.clone();
            match apply_texts(&mut ev, &ins.date_text, &ins.start_text, &ins.end_text) {
                Ok(()) => {
                    let title = ev.title.clone();
                    self.commit_event(ev);
                    self.flash(format!("{} \"{title}\"", t("Saved")), false);
                    return;
                }
                Err(e) => error = Some(e),
            }
        }
        if delete {
            self.delete_event(&ins.event_id);
            return;
        }
        if let Some(e) = error {
            self.flash(e, true);
        }
        if close || (outside && !self.is_mobile(ctx) && self.delete_confirm.is_none()) {
            return;
        }
        if self.inspector.is_none() {
            self.inspector = Some(ins);
        }
    }

    /// "Delete event?" for the selected event (Delete or Backspace).
    fn delete_confirm_dialog(&mut self, ctx: &egui::Context, p: &Palette) {
        let Some(id) = self.delete_confirm.clone() else { return };
        let Some(ev) = self.store.events.iter().find(|e| e.id == id).cloned() else {
            self.delete_confirm = None;
            return;
        };
        let content_w = dialog_content_width(ctx.screen_rect().width(), 360.0, 20.0);
        let mut cancel = false;
        let mut delete = false;
        let modal = egui::Modal::new("delete-confirm".into()).frame(egui::Frame::new().fill(p.surface).stroke(Stroke::new(1.0_f32, p.line)).corner_radius(CornerRadius::same(16)).inner_margin(egui::Margin::same(20)).shadow(ctx.style().visuals.window_shadow)).show(ctx, |ui| {
            set_dialog_width(ui, content_w);
            ui.label(RichText::new(t("Delete event?")).font(font_bold(text::T16)).color(p.text));
            ui.add_space(space::S4);
            let title = if ev.title.trim().is_empty() { t("Untitled").to_string() } else { super::side::one_line(&ev.title) };
            ui.add(egui::Label::new(RichText::new(format!("“{title}” · {}", describe_when(&ev))).font(font(text::T14)).color(p.text_2)).wrap());
            ui.label(RichText::new(t("This can’t be undone.")).font(font(text::T12)).color(p.text_2));
            ui.add_space(space::S12);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(egui::Button::new(RichText::new(t("Delete")).color(Color32::WHITE)).fill(p.danger).min_size(Vec2::new(0.0, 30.0))).clicked() {
                    delete = true;
                }
                if ui.add(egui::Button::new(t("Cancel")).min_size(Vec2::new(0.0, 30.0))).clicked() {
                    cancel = true;
                }
            });
        });
        if delete {
            self.delete_confirm = None;
            self.delete_event(&id);
        } else if cancel || modal.should_close() {
            self.delete_confirm = None;
        }
    }

    /// An account's error: the short message, and its details (HTTP status, host and path,
    /// what the server said) behind an ⓘ toggle.
    fn account_error(&mut self, ui: &mut egui::Ui, p: &Palette, account_id: &str, what: &str) {
        let (summary, details) = crate::sync::split_error(what);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.add(egui::Label::new(RichText::new(summary).font(font(text::T12)).color(p.danger)).wrap());
            if summary == t(MISSING_PASSWORD) && self.account_edit.as_ref().is_none_or(|e| e.id != account_id) {
                let b = egui::Button::new(RichText::new(t("Enter password")).font(font(text::T12)).color(p.primary_strong)).frame(false);
                if ui.add(b).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    self.start_account_edit(account_id);
                }
            }
            if details.is_some() {
                let open = self.error_details.contains(account_id);
                let label = if open { t("Hide details") } else { t("Details") };
                let b = egui::Button::new(RichText::new(label).font(font(text::T12)).color(p.text_2)).frame(false);
                let info = super::icons::inline(ui, Icon::Info, 14.0, p.text_2).interact(egui::Sense::click());
                let text = ui.add(b);
                let text = text.on_hover_cursor(egui::CursorIcon::PointingHand);
                if info.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() || text.clicked() {
                    if open {
                        self.error_details.remove(account_id);
                    } else {
                        self.error_details.insert(account_id.to_string());
                    }
                }
            }
        });
        if let Some(details) = details.filter(|_| self.error_details.contains(account_id)) {
            egui::Frame::new().fill(p.surface).stroke(Stroke::new(1.0_f32, p.line)).corner_radius(CornerRadius::same(8)).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(egui::Label::new(RichText::new(details).font(egui::FontId::monospace(11.0)).color(p.text_2)).wrap().selectable(true));
            });
        }
    }

    fn new_event_dialog(&mut self, ctx: &egui::Context, p: &Palette) {
        let Some(mut ne) = self.new_event.take() else { return };
        let calendars = self.writable_calendars();
        let mut add = false;
        let mut cancel = false;
        let screen = ctx.screen_rect();
        let content_w = dialog_content_width(screen.width(), 500.0, DIALOG_PAD);
        let max_h = dialog_max_body_height(screen.height(), 2.0 * DIALOG_PAD + 36.0);
        let modal = egui::Modal::new("new_event".into()).frame(egui::Frame::new().fill(p.surface).corner_radius(CornerRadius::same(16)).inner_margin(egui::Margin::same(24)).shadow(ctx.style().visuals.window_shadow)).show(ctx, |ui| {
            set_dialog_width(ui, content_w);
            ui.label(RichText::new(t("New event")).font(font_bold(text::T18)));
            let content_h = self.popup_sizes.get("new_event_body").map(|s| s.y).unwrap_or(max_h);
            let scroll = egui::ScrollArea::vertical().max_height(max_h).min_scrolled_height(content_h.min(max_h)).show(ui, |ui| {
            set_dialog_width(ui, content_w);
            ui.add_space(space::S4);
            let r = theme::text_field(ui, p, egui::TextEdit::singleline(&mut ne.sentence).hint_text(t("Type it: Dinner with Ada friday 7pm at Osteria")).desired_width(f32::INFINITY));
            if !ne.focus_done {
                r.request_focus();
                ne.focus_done = true;
            }
            if r.changed() {
                // "tomorrow", "friday"… count from today; without one the event stays on its day.
                if let Some(parsed) = parse_quick_entry(&ne.sentence, today(), &self.store.calendars, &ne.draft.calendar_id) {
                    ne.draft.title = parsed.title.clone();
                    ne.draft.all_day = parsed.all_day;
                    ne.draft.location = parsed.location.clone();
                    if let Some(c) = parsed.calendar_id.clone().filter(|c| calendars.iter().any(|x| &x.id == c)) {
                        ne.draft.calendar_id = c;
                    }
                    ne.date_text = if parsed.date_given { parsed.date } else { ne.day }.to_string();
                    if let Some(s) = parsed.start_time {
                        ne.start_text = s.format("%H:%M").to_string();
                        ne.end_text = parsed.end_time.unwrap_or(s + Duration::hours(1)).format("%H:%M").to_string();
                    }
                    if parsed.alert_minutes_before.is_some() {
                        ne.draft.alert_minutes_before = parsed.alert_minutes_before;
                    }
                }
            }
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !ne.draft.title.trim().is_empty() {
                add = true;
            }
            ui.add_space(space::S8);
            theme::text_field(ui, p, egui::TextEdit::singleline(&mut ne.draft.title).font(font_bold(text::T16)).hint_text(t("Title")).desired_width(f32::INFINITY));
            ui.add_space(space::S4);
            if calendars.is_empty() {
                ui.label(RichText::new(t("There is no calendar to add to. Add an account or a calendar in Settings.")).color(p.danger));
            }
            let NewEvent { draft, date_text, start_text, end_text, .. } = &mut ne;
            event_fields(ui, p, draft, Texts { date: date_text, start: start_text, end: end_text }, &calendars, true);
            ui.add_space(space::S12);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::primary_button(ui, p, t("Add")).clicked() {
                    add = true;
                }
                if ui.button(t("Cancel")).clicked() {
                    cancel = true;
                }
            });
            });
            scroll.content_size
        });
        self.settle(ctx, "new_event", modal.response.rect.size());
        self.settle(ctx, "new_event_body", modal.inner);
        if modal.should_close() {
            cancel = true;
        }
        if add {
            let mut ev = ne.draft.clone();
            if ev.title.trim().is_empty() {
                ev.title = t("New event").into();
            }
            if ev.calendar_id.is_empty() || !calendars.iter().any(|c| c.id == ev.calendar_id) {
                self.flash(t("Choose a calendar first"), true);
                self.new_event = Some(ne);
                return;
            }
            match apply_texts(&mut ev, &ne.date_text, &ne.start_text, &ne.end_text) {
                Ok(()) => {
                    let (day, title) = (ev.date, ev.title.clone());
                    self.commit_event(ev);
                    self.select_date(day);
                    self.flash(format!("{} \"{title}\"", t("Added")), false);
                    return;
                }
                Err(e) => self.flash(e, true),
            }
        }
        if !cancel {
            self.new_event = Some(ne);
        }
    }

    fn settings_dialog(&mut self, ctx: &egui::Context, p: &Palette) {
        if !self.settings_open {
            return;
        }
        let screen = ctx.screen_rect();
        let content_w = dialog_content_width(screen.width(), 620.0, DIALOG_PAD);
        let max_h = dialog_max_body_height(screen.height(), 2.0 * DIALOG_PAD + 88.0);
        let modal = egui::Modal::new("settings".into()).frame(egui::Frame::new().fill(p.surface).corner_radius(CornerRadius::same(16)).inner_margin(egui::Margin::same(24)).shadow(ctx.style().visuals.window_shadow)).show(ctx, |ui| {
            set_dialog_width(ui, content_w);
            ui.horizontal(|ui| {
                ui.label(RichText::new(t("Settings")).font(font_bold(text::T18)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icon_button(ui, p, Icon::Close, t("Close")).clicked() {
                        self.settings_open = false;
                    }
                });
            });
            ui.horizontal(|ui| {
                for (i, label) in [t("General"), t("Accounts"), t("Sets"), "MCP"].iter().enumerate() {
                    let on = self.settings_tab == i as u8;
                    let b = egui::Button::new(RichText::new(*label).font(if on { font_bold(text::T14) } else { font(text::T14) }).color(if on { p.primary_strong } else { p.text_2 })).fill(if on { p.primary_faint } else { Color32::TRANSPARENT });
                    if ui.add(b).clicked() {
                        self.settings_tab = i as u8;
                    }
                }
            });
            ui.add_space(space::S8);
            // Cap the body so the modal, with its title and tabs, stays on the screen, and ask
            // for that height up front so the list is not squeezed (it still shrinks to fit
            // shorter tabs).
            egui::ScrollArea::vertical().max_height(max_h).min_scrolled_height(max_h).show(ui, |ui| {
                set_dialog_width(ui, content_w);
                match self.settings_tab {
                    0 => self.settings_general(ui, p),
                    1 => self.settings_accounts(ui, p),
                    2 => self.settings_sets(ui, p),
                    _ => self.settings_mcp(ui, p),
                }
            });
        });
        self.settle(ctx, "settings", modal.response.rect.size());
        if modal.should_close() {
            self.settings_open = false;
        }
    }

    fn settings_general(&mut self, ui: &mut egui::Ui, p: &Palette) {
        egui::Grid::new("general").num_columns(2).spacing([space::S16, space::S12]).show(ui, |ui| {
            field_label(ui, p, t("Appearance"));
            ui.horizontal(|ui| {
                for (value, label) in [("system", t("System")), ("light", t("Light")), ("dark", t("Dark"))] {
                    if ui.selectable_label(self.prefs.theme == value, label).clicked() {
                        self.prefs.theme = value.into();
                        self.mark_changed();
                    }
                }
            });
            ui.end_row();
            field_label(ui, p, t("Language"));
            let current = if self.prefs.language == "auto" { t("Automatic").to_string() } else { i18n::language_name(&self.prefs.language).to_string() };
            theme::select(ui, p, egui::ComboBox::from_id_salt("lang").selected_text(current).width((ui.available_width() - 8.0).clamp(120.0, 200.0)), |ui| {
                if ui.selectable_label(self.prefs.language == "auto", t("Automatic")).clicked() {
                    self.prefs.language = "auto".into();
                    self.mark_changed();
                }
                for (code, name) in i18n::LANGUAGES {
                    if ui.selectable_label(self.prefs.language == *code, *name).clicked() {
                        self.prefs.language = (*code).into();
                        self.mark_changed();
                    }
                }
            });
            ui.end_row();
            field_label(ui, p, t("Animations"));
            ui.horizontal(|ui| {
                for (value, label) in [("system", t("System")), ("on", t("On")), ("off", t("Off"))] {
                    if ui.selectable_label(self.prefs.animations == value, label).clicked() {
                        self.prefs.animations = value.into();
                        self.mark_changed();
                    }
                }
            });
            ui.end_row();
            field_label(ui, p, t("Default view"));
            let mut v = self.prefs.view;
            let views = [ViewMode::Day, ViewMode::Week, ViewMode::Month, ViewMode::Quarter, ViewMode::Year];
            let changed = if ui.available_width() < 420.0 {
                view_select(ui, p, &mut v, &views)
            } else {
                segmented(ui, p, &mut v, &views)
            };
            if changed {
                self.set_view(v);
            }
            ui.end_row();
            field_label(ui, p, t("Day starts at"));
            theme::select(ui, p, egui::ComboBox::from_id_salt("daystart").selected_text(format!("{:02}:00", self.prefs.day_start)), |ui| {
                for h in 0..=12u8 {
                    if ui.selectable_label(self.prefs.day_start == h, format!("{h:02}:00")).clicked() {
                        self.prefs.day_start = h;
                        self.timeline_scrolled = false;
                        self.mark_changed();
                    }
                }
            });
            ui.end_row();
            field_label(ui, p, t("New events go to"));
            let cals = self.writable_calendars();
            let current = self.default_calendar().and_then(|id| cals.iter().find(|c| c.id == id).map(|c| c.name.clone())).unwrap_or_else(|| t("Choose…").into());
            theme::select(ui, p, egui::ComboBox::from_id_salt("defcal").selected_text(current).width((ui.available_width() - 8.0).clamp(120.0, 200.0)), |ui| {
                for c in &cals {
                    if ui.selectable_label(self.store.settings.default_calendar_id == c.id, &c.name).clicked() {
                        self.store.settings.default_calendar_id = c.id.clone();
                        self.mark_changed();
                    }
                }
            });
            ui.end_row();
            if host::hosted() {
                field_label(ui, p, t("Check files every"));
                let mut mins = (filesync::sync_interval_secs(self.store.settings.file_sync_interval_secs) / 60).max(1);
                ui.horizontal(|ui| {
                    if ui.add(egui::DragValue::new(&mut mins).range(1..=30)).changed() {
                        self.store.settings.file_sync_interval_secs = mins * 60;
                        self.mark_changed();
                    }
                    ui.label(t("minutes"));
                });
                ui.end_row();
            }
        });
    }

    fn settings_accounts(&mut self, ui: &mut egui::Ui, p: &Palette) {
        for acc in self.store.accounts.clone() {
            egui::Frame::new().fill(p.hover).corner_radius(CornerRadius::same(12)).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                // Name and buttons share a row on wide cards; on a phone the buttons get their
                // own row so the name is not squeezed behind them.
                let narrow = ui.available_width() < 380.0;
                let buttons = |ui: &mut egui::Ui, this: &mut Self| {
                    if !acc.host_managed && ui.add(egui::Button::new(RichText::new(t("Remove")).color(p.danger))).clicked() {
                        this.remove_account(&acc.id);
                    }
                    if matches!(acc.kind, AccountKind::Caldav | AccountKind::Ics | AccountKind::File) && ui.button(t("Refresh")).clicked() {
                        this.refresh_account(&acc.id);
                    }
                    let editing = this.account_edit.as_ref().is_some_and(|e| e.id == acc.id);
                    if !acc.host_managed && !editing && matches!(acc.kind, AccountKind::Caldav | AccountKind::Ics) && ui.button(t("Edit")).clicked() {
                        this.start_account_edit(&acc.id);
                    }
                };
                if narrow {
                    ui.add(egui::Label::new(RichText::new(&acc.name).font(font_bold(text::T14))).truncate());
                } else {
                    ui.horizontal(|ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            buttons(ui, self);
                            // What is left of the row, so a long name is cut instead of the buttons.
                            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                ui.add(egui::Label::new(RichText::new(&acc.name).font(font_bold(text::T14))).truncate());
                            });
                        });
                    });
                }
                let kind = match acc.kind {
                    AccountKind::Caldav if acc.host_managed => t("CalDAV · provided by artcraftCloud").to_string(),
                    AccountKind::Caldav => "CalDAV".to_string(),
                    AccountKind::Ics => t("Subscription (read only)").to_string(),
                    AccountKind::File if acc.id == filesync::ACC_FILES => t("In my files").to_string(),
                    AccountKind::File => t("Opened file").to_string(),
                    AccountKind::Local => t("On this device").to_string(),
                };
                let url = acc.server_url.clone().unwrap_or_default();
                let detail = [kind, acc.email.clone(), url.clone()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
                if !detail.is_empty() {
                    // Own row, elided to the card width, so a long URL cannot run under the buttons.
                    let tip = if url.is_empty() { detail.clone() } else { url };
                    ui.add(egui::Label::new(RichText::new(detail).font(font(text::T12)).color(p.text_2)).truncate())
                        .on_hover_text(tip);
                }
                if narrow {
                    ui.horizontal(|ui| ui.with_layout(Layout::right_to_left(Align::Center), |ui| buttons(ui, self)));
                }
                // Below the buttons, across the card, so a long message is not covered.
                if let Some(err) = self.status.get(&acc.id).cloned() {
                    self.account_error(ui, p, &acc.id, &err);
                }
                if self.account_edit.as_ref().is_some_and(|e| e.id == acc.id) {
                    self.account_edit_form(ui, p, &acc);
                }
            });
        }
        ui.add_space(space::S12);
        ui.label(RichText::new(t("Add an account")).font(font_bold(text::T16)));
        let f = &mut self.account_form;
        ui.horizontal(|ui| {
            if ui.selectable_label(!f.kind_ics, t("CalDAV account")).clicked() {
                f.kind_ics = false;
            }
            if ui.selectable_label(f.kind_ics, t("Subscription (.ics)")).clicked() {
                f.kind_ics = true;
            }
        });
        let field_w = (ui.available_width() - 90.0 - space::S12).clamp(120.0, 300.0);
        egui::Grid::new("addacc").num_columns(2).spacing([space::S12, space::S8]).show(ui, |ui| {
            field_label(ui, p, t("Name"));
            theme::text_field(ui, p, egui::TextEdit::singleline(&mut f.name).hint_text(t("Work")).desired_width(field_w));
            ui.end_row();
            field_label(ui, p, t("Address"));
            theme::text_field(ui, p, egui::TextEdit::singleline(&mut f.url).hint_text(if f.kind_ics { "https://…/calendar.ics" } else { "https://caldav.icloud.com" }).desired_width(field_w));
            ui.end_row();
            if !f.kind_ics {
                field_label(ui, p, t("User name"));
                theme::text_field(ui, p, egui::TextEdit::singleline(&mut f.username).hint_text("ada@example.org").desired_width(field_w));
                ui.end_row();
                field_label(ui, p, t("Password"));
                theme::text_field(ui, p, egui::TextEdit::singleline(&mut f.password).password(true).hint_text(t("App password")).desired_width(field_w));
                ui.end_row();
            }
        });
        if let Some(err) = &f.error {
            ui.label(RichText::new(err).color(p.danger));
        }
        let connect = theme::primary_button(ui, p, t("Connect")).clicked();
        ui.add(egui::Label::new(RichText::new(t("iCloud: https://caldav.icloud.com with an app-specific password. Fastmail: https://caldav.fastmail.com. Nextcloud: https://<server>/remote.php/dav. Google needs a sign-in CalendarCraft cannot do yet: add its secret iCal address as a subscription instead.")).font(font(text::T12)).color(p.text_2)).wrap());
        if cfg!(target_arch = "wasm32") && self.host.remote_proxy.is_none() {
            ui.add(egui::Label::new(RichText::new(t("In a plain browser page most servers refuse to answer (CORS). Inside artcraftCloud the requests go through its proxy.")).font(font(text::T12)).color(p.text_2)).wrap());
        }
        if connect {
            self.add_account_from_form();
        }
        ui.add_space(space::S12);
        let local_heading = if host::hosted() { t("Calendar in my files") } else { t("Calendar on this device") };
        ui.label(RichText::new(local_heading).font(font_bold(text::T16)));
        ui.horizontal(|ui| {
            let id = ui.id().with("localname");
            let mut name: String = ui.data_mut(|d| d.get_temp(id).unwrap_or_default());
            let name_w = (ui.available_width() - 100.0).clamp(100.0, 220.0);
            theme::text_field(ui, p, egui::TextEdit::singleline(&mut name).hint_text(t("Personal")).desired_width(name_w));
            if ui.button(t("Create")).clicked() && !name.trim().is_empty() {
                self.add_local_calendar(name.trim());
                name.clear();
            }
            ui.data_mut(|d| d.insert_temp(id, name));
        });
        let _ = AccountForm::default;
    }

    /// The edit form inside an account's card: name, address and, for CalDAV, the user name
    /// and a password that keeps the stored one when left empty.
    fn account_edit_form(&mut self, ui: &mut egui::Ui, p: &Palette, acc: &crate::model::AccountConfig) {
        let has_password = self.secrets.contains_key(&acc.id) || host::hosted();
        let caldav = acc.kind == AccountKind::Caldav;
        let mut save = false;
        let mut cancel = false;
        let Some(f) = self.account_edit.as_mut() else { return };
        ui.add_space(space::S8);
        let field_w = (ui.available_width() - 90.0 - space::S12 - 24.0).clamp(120.0, 300.0);
        egui::Grid::new(("editacc", &acc.id)).num_columns(2).spacing([space::S12, space::S8]).show(ui, |ui| {
            field_label(ui, p, t("Name"));
            theme::text_field(ui, p, egui::TextEdit::singleline(&mut f.name).hint_text(t("Work")).desired_width(field_w));
            ui.end_row();
            field_label(ui, p, t("Address"));
            theme::text_field(ui, p, egui::TextEdit::singleline(&mut f.url).hint_text("https://caldav.icloud.com").desired_width(field_w));
            ui.end_row();
            if caldav {
                field_label(ui, p, t("User name"));
                theme::text_field(ui, p, egui::TextEdit::singleline(&mut f.username).hint_text("ada@example.org").desired_width(field_w));
                ui.end_row();
                field_label(ui, p, t("Password"));
                let hint = if has_password { t("Unchanged") } else { t("Enter the password") };
                theme::text_field(ui, p, egui::TextEdit::singleline(&mut f.password).password(true).hint_text(hint).desired_width(field_w));
                ui.end_row();
            }
        });
        if caldav {
            let note = if has_password { t("Leave the password empty to keep the saved one.") } else { t("No password is saved for this account.") };
            ui.add(egui::Label::new(RichText::new(note).font(font(text::T12)).color(p.text_2)).wrap());
        }
        if let Some(err) = &f.error {
            ui.add(egui::Label::new(RichText::new(err).font(font(text::T12)).color(p.danger)).wrap());
        }
        ui.add_space(space::S4);
        ui.horizontal(|ui| {
            if theme::primary_button(ui, p, t("Save")).clicked() {
                save = true;
            }
            if ui.button(t("Cancel")).clicked() {
                cancel = true;
            }
        });
        if save {
            self.save_account_edit();
        } else if cancel {
            self.account_edit = None;
        }
    }

    fn settings_mcp(&mut self, ui: &mut egui::Ui, p: &Palette) {
        ui.label(t("Connect an MCP client over stdio:"));
        ui.code("calendarcraft-mcp --config calendarcraft.config.json");
        ui.add_space(space::S8);
        ui.label(RichText::new(t("Try a request against this calendar:")).color(p.text_2));
        theme::text_field(ui, p, egui::TextEdit::multiline(&mut self.mcp_request).code_editor().desired_rows(7).desired_width(f32::INFINITY));
        if theme::primary_button(ui, p, t("Run")).clicked() {
            self.mcp_response = match serde_json::from_str::<serde_json::Value>(&self.mcp_request) {
                Ok(v) => handle_mcp_request(&mut self.store, &v).map(|r| serde_json::to_string_pretty(&r).unwrap_or_default()).unwrap_or_else(|| "(notification)".into()),
                Err(e) => format!("JSON: {e}"),
            };
            self.mark_changed();
        }
        if !self.mcp_response.is_empty() {
            theme::text_field(ui, p, egui::TextEdit::multiline(&mut self.mcp_response).code_editor().desired_rows(7).desired_width(f32::INFINITY));
        }
    }

    fn file_conflict_dialog(&mut self, ctx: &egui::Context, p: &Palette) {
        if self.file_conflict.is_none() {
            return;
        }
        let name = self.file_conflict.as_ref().map(|c| c.name.clone()).unwrap_or_default();
        let content_w = dialog_content_width(ctx.screen_rect().width(), 420.0, DIALOG_PAD);
        let max_h = dialog_max_body_height(ctx.screen_rect().height(), 2.0 * DIALOG_PAD);
        let modal = egui::Modal::new("file_conflict".into()).frame(egui::Frame::new().fill(p.surface).corner_radius(CornerRadius::same(16)).inner_margin(egui::Margin::same(24)).shadow(ctx.style().visuals.window_shadow)).show(ctx, |ui| {
            set_dialog_width(ui, content_w);
            egui::ScrollArea::vertical().max_height(max_h).show(ui, |ui| {
                set_dialog_width(ui, content_w);
                ui.add(egui::Label::new(RichText::new(t("Someone else changed this calendar. Choose how to continue.")).font(font_bold(text::T16))).wrap());
                ui.add_space(space::S8);
                ui.add(egui::Label::new(RichText::new(format!("{name}\n{}", t("This file changed on another device or tab."))).font(font(text::T14)).color(p.text_2)).wrap());
                ui.add_space(space::S16);
                ui.horizontal_wrapped(|ui| {
                    if ui.button(t("Reload theirs")).clicked() {
                        self.resolve_file_conflict(ConflictChoice::ReloadTheirs);
                    }
                    if theme::primary_button(ui, p, t("Keep mine")).clicked() {
                        self.resolve_file_conflict(ConflictChoice::KeepMine);
                    }
                    if ui.button(t("Save a copy")).clicked() {
                        self.resolve_file_conflict(ConflictChoice::SaveACopy);
                    }
                });
            });
        });
        let _ = modal;
    }

    fn migration_dialog(&mut self, ctx: &egui::Context, p: &Palette) {
        let Some(m) = self.migration.as_ref() else { return };
        if m.pending > 0 || (m.saved.is_empty() && m.failed.is_empty()) {
            return;
        }
        let saved = m.saved.clone();
        let failed = m.failed.clone();
        let content_w = dialog_content_width(ctx.screen_rect().width(), 420.0, DIALOG_PAD);
        let max_h = dialog_max_body_height(ctx.screen_rect().height(), 2.0 * DIALOG_PAD);
        let modal = egui::Modal::new("file_migration".into()).frame(egui::Frame::new().fill(p.surface).corner_radius(CornerRadius::same(16)).inner_margin(egui::Margin::same(24)).shadow(ctx.style().visuals.window_shadow)).show(ctx, |ui| {
            set_dialog_width(ui, content_w);
            egui::ScrollArea::vertical().max_height(max_h).show(ui, |ui| {
                set_dialog_width(ui, content_w);
                ui.add(egui::Label::new(RichText::new(t("Your calendars were saved as files in Calendars.")).font(font_bold(text::T16))).wrap());
                ui.add_space(space::S8);
                if !saved.is_empty() {
                    ui.add(egui::Label::new(RichText::new(saved.join(", ")).font(font(text::T14)).color(p.text_2)).wrap());
                }
                if !failed.is_empty() {
                    ui.add_space(space::S8);
                    ui.add(egui::Label::new(RichText::new(format!("{} {}", t("Could not save the calendar as a file."), failed.join(", "))).font(font(text::T14)).color(p.danger)).wrap());
                }
                ui.add_space(space::S16);
                if theme::primary_button(ui, p, t("OK")).clicked() {
                    self.migration = None;
                }
            });
        });
        if modal.should_close() {
            self.migration = None;
        }
    }
}

fn describe_when(ev: &CalendarEvent) -> String {
    use chrono::Datelike;
    let day = format!("{}, {} {} {}", i18n::weekday_name(ev.date.weekday()), ev.date.day(), i18n::month_name(ev.date.month()), ev.date.year());
    let mut out = match (ev.all_day, ev.start_time, ev.end_time) {
        (false, Some(s), Some(e)) => format!("{day} · {} – {}", s.format("%H:%M"), e.format("%H:%M")),
        (false, Some(s), None) => format!("{day} · {}", s.format("%H:%M")),
        _ => format!("{day} · {}", t("all-day")),
    };
    if let Some(end) = ev.end_date.filter(|e| *e > ev.date) {
        out.push_str(&format!(" → {} {}", end.day(), i18n::month_short(end.month())));
    }
    if ev.rrule.is_some() {
        out.push_str(&format!(" · {}", t("repeats")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{dialog_content_width, dialog_max_body_height, dialog_outer_width, DIALOG_MARGIN, DIALOG_PAD};

    #[test]
    fn dialogs_fit_inside_phone_and_desktop_widths() {
        assert_eq!(dialog_outer_width(390.0, 440.0), 390.0 - 2.0 * DIALOG_MARGIN);
        assert_eq!(dialog_outer_width(430.0, 620.0), 430.0 - 2.0 * DIALOG_MARGIN);
        assert_eq!(dialog_outer_width(1440.0, 440.0), 440.0);
        assert_eq!(dialog_outer_width(1440.0, 620.0), 620.0);
        assert!(dialog_outer_width(390.0, 420.0) <= 390.0);
        assert!(dialog_content_width(390.0, 440.0, DIALOG_PAD) + 2.0 * DIALOG_PAD + 2.0 * DIALOG_MARGIN <= 390.0);
        assert!(dialog_content_width(390.0, 620.0, DIALOG_PAD) + 2.0 * DIALOG_PAD + 2.0 * DIALOG_MARGIN <= 390.0);
        assert!(dialog_max_body_height(844.0, 88.0 + 2.0 * DIALOG_PAD) + 88.0 + 2.0 * DIALOG_PAD + 2.0 * DIALOG_MARGIN <= 844.0);
    }
}
