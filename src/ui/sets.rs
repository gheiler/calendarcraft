//! Calendar sets: the pills over the sidebar's calendar list (All plus the sets you made) and
//! Settings › Sets, where sets are made, renamed, changed and deleted. All is computed (every
//! visible calendar) and not stored; the pills only show once a set exists.

use eframe::egui::{self, Align, Color32, CornerRadius, Layout, RichText, Vec2};

use super::icons::{self, Icon};
use super::side::small_icon;
use super::CalendarCraftApp;
use crate::i18n::t;
use crate::model::CalendarSet;
use crate::theme::{self, font, font_bold, space, text, Palette};

/// The set being made (`id` is `None`) or changed in Settings › Sets.
#[derive(Debug, Clone, Default)]
pub struct SetForm {
    pub id: Option<String>,
    pub name: String,
    pub calendar_ids: Vec<String>,
    pub error: Option<&'static str>,
}

/// Why a set cannot be saved, if it cannot.
pub fn validate_set(name: &str, calendar_ids: &[String]) -> Option<&'static str> {
    if name.trim().is_empty() {
        Some("Give the set a name.")
    } else if calendar_ids.is_empty() {
        Some("Pick at least one calendar.")
    } else {
        None
    }
}

impl CalendarCraftApp {
    /// All and the user's sets as pills; nothing when there are no sets.
    pub(super) fn set_pills(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let sets: Vec<CalendarSet> = self.store.user_sets().cloned().collect();
        if sets.is_empty() {
            return;
        }
        let active = self.store.active_user_set().map(|s| s.id.clone());
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
            let pill = |ui: &mut egui::Ui, label: &str, on: bool| {
                let text = RichText::new(label).font(font(text::T12)).color(if on { p.on_primary_strong } else { p.text });
                ui.add(egui::Button::new(text).fill(if on { p.primary_strong } else { p.hover }).corner_radius(CornerRadius::same(255)).min_size(Vec2::new(0.0, 24.0)))
            };
            if pill(ui, t("All"), active.is_none()).clicked() && active.is_some() {
                self.store.settings.active_calendar_set = None;
                self.mark_changed();
            }
            for set in &sets {
                let on = active.as_deref() == Some(set.id.as_str());
                if pill(ui, &set.name, on).clicked() && !on {
                    self.store.settings.active_calendar_set = Some(set.id.clone());
                    self.mark_changed();
                }
            }
            if small_icon(ui, p, Icon::Gear).on_hover_text(t("Edit sets")).clicked() {
                self.settings_open = true;
                self.settings_tab = 2;
            }
        });
        if let Some(set) = self.store.active_user_set() {
            if self.store.set_calendars(set).next().is_none() {
                let hint = ui.add(egui::Label::new(RichText::new(t("This set has no calendars left, so nothing is shown. Edit it in Settings › Sets.")).font(font(text::T12)).color(p.text_2)).wrap().sense(egui::Sense::click()));
                if hint.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    self.settings_open = true;
                    self.settings_tab = 2;
                }
            }
        }
    }

    /// Settings › Sets.
    pub(super) fn settings_sets(&mut self, ui: &mut egui::Ui, p: &Palette) {
        ui.add(egui::Label::new(RichText::new(t("A set shows only the calendars you pick, such as Work or Family. Its pill appears in the sidebar next to All.")).font(font(text::T12)).color(p.text_2)).wrap());
        ui.add_space(space::S8);
        let sets: Vec<CalendarSet> = self.store.user_sets().cloned().collect();
        for set in &sets {
            egui::Frame::new().fill(p.hover).corner_radius(CornerRadius::same(12)).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let editing = self.set_form.as_ref().is_some_and(|f| f.id.as_deref() == Some(set.id.as_str()));
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if !editing && self.set_delete.as_deref() != Some(set.id.as_str()) {
                            if ui.add(egui::Button::new(RichText::new(t("Delete")).color(p.danger))).clicked() {
                                self.set_delete = Some(set.id.clone());
                            }
                            if ui.button(t("Edit")).clicked() {
                                self.set_form = Some(SetForm { id: Some(set.id.clone()), name: set.name.clone(), calendar_ids: set.calendar_ids.clone(), error: None });
                                self.set_delete = None;
                            }
                        }
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            ui.add(egui::Label::new(RichText::new(&set.name).font(font_bold(text::T14))).truncate());
                        });
                    });
                });
                let names: Vec<(Color32, String)> = self.store.set_calendars(set).map(|c| (theme::color_of(&c.color_hex), c.name.clone())).collect();
                if names.is_empty() {
                    ui.label(RichText::new(t("No calendars")).font(font(text::T12)).color(p.danger));
                } else {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
                        for (color, name) in names {
                            // Dot and name as one item, so a wrap never parts them.
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
                                ui.painter().circle_filled(r.center(), 4.0, color);
                                ui.add(egui::Label::new(RichText::new(name).font(font(text::T12)).color(p.text_2)).wrap_mode(egui::TextWrapMode::Extend));
                            });
                            ui.add_space(space::S4);
                        }
                    });
                }
                if self.set_delete.as_deref() == Some(set.id.as_str()) {
                    ui.add_space(space::S8);
                    ui.add(egui::Label::new(RichText::new(t("Delete this set? Its calendars and events stay.")).font(font(text::T14))).wrap());
                    ui.horizontal(|ui| {
                        if ui.add(egui::Button::new(RichText::new(t("Delete")).color(Color32::WHITE)).fill(p.danger)).clicked() {
                            self.delete_set(&set.id);
                        }
                        if ui.button(t("Cancel")).clicked() {
                            self.set_delete = None;
                        }
                    });
                }
                if editing {
                    self.set_form_ui(ui, p);
                }
            });
            ui.add_space(space::S8);
        }
        if self.set_form.as_ref().is_some_and(|f| f.id.is_none()) {
            egui::Frame::new().fill(p.hover).corner_radius(CornerRadius::same(12)).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(t("New set")).font(font_bold(text::T14)));
                self.set_form_ui(ui, p);
            });
        } else if theme::primary_button(ui, p, &format!("+  {}", t("New set"))).clicked() {
            self.set_form = Some(SetForm::default());
            self.set_delete = None;
        }
    }

    /// Name and calendar ticks, then Save / Cancel.
    fn set_form_ui(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let accounts = self.store.accounts.clone();
        let calendars = self.store.calendars.clone();
        let Some(f) = self.set_form.as_mut() else { return };
        ui.add_space(space::S8);
        let field_w = (ui.available_width() - 90.0 - space::S12).clamp(120.0, 300.0);
        egui::Grid::new("setform").num_columns(2).spacing([space::S12, space::S8]).show(ui, |ui| {
            ui.label(RichText::new(t("Name")).font(font(text::T12)).color(p.text_2));
            theme::text_field(ui, p, egui::TextEdit::singleline(&mut f.name).hint_text(t("Work")).desired_width(field_w));
            ui.end_row();
        });
        ui.add_space(space::S4);
        ui.label(RichText::new(t("Calendars")).font(font(text::T12)).color(p.text_2));
        let mut groups: Vec<(String, Vec<&crate::model::CalendarSource>)> = accounts
            .iter()
            .map(|a| (a.name.clone(), calendars.iter().filter(|c| c.account_id == a.id).collect::<Vec<_>>()))
            .filter(|(_, cals)| !cals.is_empty())
            .collect();
        let orphans: Vec<_> = calendars.iter().filter(|c| !accounts.iter().any(|a| a.id == c.account_id)).collect();
        if !orphans.is_empty() {
            groups.push((String::new(), orphans));
        }
        for (account, cals) in groups {
            if !account.is_empty() {
                ui.label(RichText::new(account).font(font_bold(text::T12)).color(p.text_2));
            }
            for c in cals {
                // The same coloured tick as the sidebar's calendar list.
                let on = f.calendar_ids.contains(&c.id);
                let color = theme::color_of(&c.color_hex);
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), egui::Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(rect, CornerRadius::same(6), p.surface);
                }
                let box_r = egui::Rect::from_center_size(egui::Pos2::new(rect.left() + 12.0, rect.center().y), Vec2::splat(14.0));
                if on {
                    ui.painter().rect_filled(box_r, CornerRadius::same(4), color);
                    icons::paint(ui.painter(), box_r.shrink(2.0), Icon::Check, Color32::WHITE);
                } else {
                    ui.painter().rect_stroke(box_r, CornerRadius::same(4), egui::Stroke::new(1.5_f32, color), egui::StrokeKind::Inside);
                }
                ui.painter().text(egui::Pos2::new(rect.left() + 28.0, rect.center().y), egui::Align2::LEFT_CENTER, &c.name, font(text::T14), p.text);
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    if on {
                        f.calendar_ids.retain(|id| *id != c.id);
                    } else {
                        f.calendar_ids.push(c.id.clone());
                    }
                    f.error = None;
                }
            }
        }
        if calendars.is_empty() {
            ui.label(RichText::new(t("Add a calendar account first.")).font(font(text::T12)).color(p.text_2));
        }
        if let Some(err) = f.error {
            ui.label(RichText::new(t(err)).font(font(text::T12)).color(p.danger));
        }
        ui.add_space(space::S4);
        let mut save = false;
        let mut cancel = false;
        ui.horizontal(|ui| {
            save = theme::primary_button(ui, p, t("Save")).clicked();
            cancel = ui.button(t("Cancel")).clicked();
        });
        if cancel {
            self.set_form = None;
        } else if save {
            self.save_set_form();
        }
    }

    fn save_set_form(&mut self) {
        let Some(f) = self.set_form.as_mut() else { return };
        // Keep only calendars that still exist, in the order of the calendar list.
        let ids: Vec<String> = self.store.calendars.iter().filter(|c| f.calendar_ids.contains(&c.id)).map(|c| c.id.clone()).collect();
        if let Some(err) = validate_set(&f.name, &ids) {
            f.error = Some(err);
            return;
        }
        let name = f.name.trim().to_string();
        match f.id.clone() {
            Some(id) => {
                if let Some(set) = self.store.calendar_sets.iter_mut().find(|s| s.id == id) {
                    set.name = name;
                    set.calendar_ids = ids;
                }
            }
            None => {
                let id = self.store.next_id("set");
                self.store.calendar_sets.push(CalendarSet { id, name, calendar_ids: ids });
            }
        }
        self.set_form = None;
        self.mark_changed();
    }

    fn delete_set(&mut self, id: &str) {
        self.store.calendar_sets.retain(|s| s.id != id);
        if self.store.settings.active_calendar_set.as_deref() == Some(id) {
            self.store.settings.active_calendar_set = None;
        }
        self.set_delete = None;
        self.mark_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::validate_set;

    #[test]
    fn a_set_needs_a_name_and_a_calendar() {
        assert!(validate_set("  ", &["c".into()]).is_some());
        assert!(validate_set("Work", &[]).is_some());
        assert!(validate_set("Work", &["c".into()]).is_none());
    }
}
