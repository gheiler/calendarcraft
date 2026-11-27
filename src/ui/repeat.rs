//! The event form's Repeat rows: the presets, and "Custom…" with how often, which weekdays or
//! which day of the month, and when it ends (Apple Calendar / Fantastical style). A rule with
//! parts this form can't edit is shown with its summary and a note, and never rewritten unless
//! another repeat is picked.

use chrono::{Datelike, NaiveDate};
use craft_ical::values::When;
use eframe::egui::{self, CornerRadius, RichText, Vec2};

use super::icons::{self, Icon};
use crate::i18n::{self, t};
use crate::model::CalendarEvent;
use crate::rrule::{self, Custom, End, Freq, MonthlyOn, RRule, PRESETS, WEEKDAYS};
use crate::theme::{self, font, space, text, Palette};
use crate::tz::{self, EventZone};

/// Most repeats and occurrences the form accepts.
const MAX_NUMBER: u32 = 999;

/// What the Repeat rows keep between frames: whether Custom is open, and the typed numbers and
/// end date (kept as typed while they don't read yet).
#[derive(Debug, Clone, Default)]
pub struct RepeatForm {
    pub open: bool,
    pub interval: String,
    pub count: String,
    pub until: String,
    /// The rule these texts were last filled from.
    synced: Option<Option<String>>,
    /// A weekday chip was clicked off while it was the only one.
    need_day: bool,
}

/// The day an `UNTIL` value ends on in the event's zone.
pub fn until_day(zone: &EventZone) -> impl Fn(When) -> NaiveDate + '_ {
    move |w| match w {
        When::Utc(t) if !zone.is_floating() => tz::wall_in_event_zone(zone, t.and_utc().timestamp()).date(),
        other => other.wall().date(),
    }
}

/// "Every 2 weeks on Mon, Wed, Fri until 31 Dec 2026" for an event's rule; `None` without one.
pub fn summary_of(ev: &CalendarEvent) -> Option<String> {
    let text = ev.rrule.as_deref().map(str::trim).filter(|r| !r.is_empty())?;
    let (core, _) = crate::recur::split_exdates(text);
    Some(match RRule::parse(core) {
        Some(r) => rrule::summary(&r, ev.date, &until_day(&ev.zone)),
        None => t("repeats").to_string(),
    })
}

fn parse_number(s: &str) -> Option<u32> {
    s.trim().parse::<u32>().ok().filter(|n| (1..=MAX_NUMBER).contains(n))
}

impl RepeatForm {
    /// A form for an event whose rule is `rule`.
    pub fn for_event(ev: &CalendarEvent) -> RepeatForm {
        let mut f = RepeatForm::default();
        f.sync(ev, true);
        f
    }

    fn sync(&mut self, ev: &CalendarEvent, force: bool) {
        let text = ev.rrule.clone().filter(|r| !r.trim().is_empty());
        if !force && self.synced.as_ref() == Some(&text) {
            return;
        }
        let custom = custom_of(ev);
        let preset = match &text {
            None => Some(0),
            Some(_) => custom.as_ref().and_then(|c| c.preset(ev.date)),
        };
        self.open = preset.is_none();
        let c = custom.unwrap_or_else(|| Custom::default_for(ev.date));
        self.interval = c.interval.to_string();
        self.count = match c.end {
            End::After(n) => n.to_string(),
            _ => "10".into(),
        };
        self.until = match c.end {
            End::On(d) => d.to_string(),
            _ => String::new(),
        };
        self.synced = Some(text);
        self.need_day = false;
    }

    fn mark_synced(&mut self, ev: &CalendarEvent) {
        self.synced = Some(ev.rrule.clone().filter(|r| !r.trim().is_empty()));
    }

    /// What is wrong with the typed numbers or end date, for the Save button.
    pub fn error(&self, ev: &CalendarEvent) -> Option<&'static str> {
        if !self.open {
            return None;
        }
        let c = custom_of(ev)?;
        if parse_number(&self.interval).is_none() {
            return Some(t("The repeat interval should be a number from 1 to 999"));
        }
        match c.end {
            End::After(_) if parse_number(&self.count).is_none() => Some(t("The repeat count should be a number from 1 to 999")),
            End::On(_) if NaiveDate::parse_from_str(self.until.trim(), "%Y-%m-%d").is_err() => Some(t("The repeat end date should look like 2026-12-31")),
            End::On(d) if d < ev.date => Some(t("The repeat ends before the event starts")),
            _ => None,
        }
    }
}

fn custom_of(ev: &CalendarEvent) -> Option<Custom> {
    let text = ev.rrule.as_deref()?;
    let (core, extra) = crate::recur::split_exdates(text);
    if !extra.is_empty() {
        return None;
    }
    Custom::from_rule(&RRule::parse(core)?, ev.date, &until_day(&ev.zone))
}

fn unit_label(freq: Freq, n: u32) -> &'static str {
    let one = n == 1;
    match freq {
        Freq::Daily => if one { t("day") } else { t("days") },
        Freq::Weekly => if one { t("week") } else { t("weeks") },
        Freq::Monthly => if one { t("month") } else { t("months") },
        _ => if one { t("year") } else { t("years") },
    }
}

fn label(ui: &mut egui::Ui, p: &Palette, s: &str) {
    ui.label(RichText::new(s).font(font(text::T12)).color(p.text_2));
}

/// A weekday toggle in the calendar-set pill style.
fn chip(ui: &mut egui::Ui, p: &Palette, s: &str, on: bool, width: f32) -> egui::Response {
    let text = RichText::new(s).font(font(text::T12)).color(if on { p.on_primary_strong } else { p.text });
    ui.add_sized([width, 28.0], egui::Button::new(text).fill(if on { p.primary_strong } else { p.hover }).corner_radius(CornerRadius::same(255)))
}

/// Width for a short drop-down that still leaves room beside it in a `col_w` column.
fn combo_w(col_w: f32, want: f32) -> f32 {
    want.min(col_w * 0.45).max(64.0)
}

/// The Repeat row and, for Custom, the rows under it, inside the form's two-column grid.
/// Returns whether the event changed.
pub fn repeat_rows(ui: &mut egui::Ui, p: &Palette, ev: &mut CalendarEvent, form: &mut RepeatForm, field_w: f32, col_w: f32, summary_row: bool) -> bool {
    let mut changed = false;
    form.sync(ev, false);
    let start = ev.date;
    let text = ev.rrule.clone().filter(|r| !r.trim().is_empty());
    let custom = custom_of(ev);
    let preset = match &text {
        None => Some(0),
        Some(_) => custom.as_ref().and_then(|c| c.preset(start)),
    };
    let show_custom = form.open || preset.is_none();
    let pad = (theme::INPUT_PADDING.left + theme::INPUT_PADDING.right) as f32;

    label(ui, p, t("Repeat"));
    let selected = match preset.filter(|_| !show_custom) {
        Some(i) => t(PRESETS[i].1).to_string(),
        None => t("Custom…").to_string(),
    };
    theme::select(ui, p, egui::ComboBox::from_id_salt(ui.id().with("repeat")).selected_text(selected).width(field_w).height(400.0), |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        for (i, (r, l)) in PRESETS.iter().enumerate() {
            if ui.selectable_label(!show_custom && preset == Some(i), t(l)).clicked() {
                ev.rrule = Some(r.to_string()).filter(|r| !r.is_empty());
                form.sync(ev, true);
                form.open = false;
                changed = true;
            }
        }
        ui.separator();
        if ui.selectable_label(show_custom, t("Custom…")).clicked() && !show_custom {
            if text.is_none() {
                ev.rrule = Some(Custom::default_for(start).to_rule(start).to_text());
                changed = true;
            }
            form.sync(ev, true);
            form.open = true;
        }
    });
    ui.end_row();
    if !show_custom {
        return changed;
    }

    let Some(mut c) = custom else {
        // Kept as it is: the summary, the rule itself, and why it can't be edited here.
        label(ui, p, "");
        ui.vertical(|ui| {
            ui.set_max_width(col_w);
            {
                if let Some(s) = summary_of(ev) {
                    ui.add(egui::Label::new(RichText::new(s).font(font(text::T14)).color(p.text)).wrap());
                }
                ui.horizontal_wrapped(|ui| {
                    icons::inline(ui, Icon::Lock, 12.0, p.text_2);
                    ui.add(egui::Label::new(RichText::new(t("This repeat was made in another app and has parts CalendarCraft can’t edit, so it is kept as it is. Pick another repeat to replace it.")).font(font(text::T12)).color(p.text_2)).wrap());
                });
                if let Some(rule) = &text {
                    ui.add(egui::Label::new(RichText::new(rule).font(egui::FontId::monospace(11.0)).color(p.text_2)).wrap().selectable(true));
                }
            }
        });
        ui.end_row();
        return changed;
    };
    let before = c.clone();

    // Every [n] [days|weeks|months|years]
    label(ui, p, t("Every"));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S8;
        if theme::text_field(ui, p, egui::TextEdit::singleline(&mut form.interval).desired_width(32.0).hint_text("1")).changed() {
            if let Some(n) = parse_number(&form.interval) {
                c.interval = n;
            }
        }
        let n = parse_number(&form.interval).unwrap_or(c.interval);
        let unit_w = (field_w - 32.0 - pad - space::S8).clamp(80.0, 140.0);
        theme::select(ui, p, egui::ComboBox::from_id_salt(ui.id().with("repeat-unit")).selected_text(unit_label(c.freq, n)).width(unit_w), |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            for f in [Freq::Daily, Freq::Weekly, Freq::Monthly, Freq::Yearly] {
                if ui.selectable_label(c.freq == f, unit_label(f, n)).clicked() {
                    c.freq = f;
                    if c.days.is_empty() {
                        c.days = vec![start.weekday()];
                    }
                }
            }
        });
    });
    ui.end_row();

    match c.freq {
        Freq::Weekly => {
            label(ui, p, t("Days"));
            ui.vertical(|ui| {
                    ui.set_max_width(col_w);
                    // Seven chips on one row, narrower on a phone.
                    let gap = 4.0;
                    let chip_w = ((col_w - 6.0 * gap - 4.0) / 7.0).clamp(24.0, 48.0);
                    // Short names when they fit, else one letter (a phone in some languages).
                    let widest = WEEKDAYS.iter().map(|d| ui.fonts(|f| f.layout_no_wrap(i18n::weekday_short(*d).to_string(), font(text::T12), p.text).size().x)).fold(0.0_f32, f32::max);
                    let letters = widest + 8.0 > chip_w;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(gap, gap);
                        ui.spacing_mut().button_padding = Vec2::new(2.0, 4.0);
                        for day in WEEKDAYS {
                            let on = c.days.contains(&day);
                            let name = if letters { i18n::weekday_letter(day) } else { i18n::weekday_short(day).to_string() };
                            let r = chip(ui, p, &name, on, chip_w).on_hover_text(i18n::weekday_name(day));
                            if r.clicked() {
                                if on && c.days.len() == 1 {
                                    form.need_day = true;
                                } else if on {
                                    c.days.retain(|d| *d != day);
                                } else {
                                    c.days.push(day);
                                    c.days = WEEKDAYS.into_iter().filter(|d| c.days.contains(d)).collect();
                                    form.need_day = false;
                                }
                            }
                        }
                    });
                    if form.need_day {
                        ui.label(RichText::new(t("Pick at least one day.")).font(font(text::T12)).color(p.danger));
                    }
            });
            ui.end_row();
        }
        Freq::Monthly | Freq::Yearly => {
            label(ui, p, t("Day"));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::S8;
                let day_label = if c.freq == Freq::Yearly { format!("{} {}", start.day(), i18n::month_name(start.month())) } else { t("Day {n}").replace("{n}", &start.day().to_string()) };
                let current = match c.on {
                    MonthlyOn::Day => day_label.clone(),
                    MonthlyOn::Nth(n, _) => rrule::ordinal_label(n),
                };
                let ord_w = combo_w(col_w, if matches!(c.on, MonthlyOn::Day) { field_w } else { 100.0 });
                theme::select(ui, p, egui::ComboBox::from_id_salt(ui.id().with("repeat-nth")).selected_text(current).width(ord_w), |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    if ui.selectable_label(c.on == MonthlyOn::Day, day_label.as_str()).clicked() {
                        c.on = MonthlyOn::Day;
                    }
                    let wd = match c.on {
                        MonthlyOn::Nth(_, d) => d,
                        MonthlyOn::Day => start.weekday(),
                    };
                    for n in [1, 2, 3, 4, 5, -1] {
                        if ui.selectable_label(matches!(c.on, MonthlyOn::Nth(k, _) if k == n), rrule::ordinal_label(n)).clicked() {
                            c.on = MonthlyOn::Nth(n, wd);
                        }
                    }
                });
                if let MonthlyOn::Nth(n, wd) = c.on {
                    let wd_w = (col_w - ord_w - 2.0 * pad - space::S8 - 8.0).clamp(80.0, 130.0);
                    theme::select(ui, p, egui::ComboBox::from_id_salt(ui.id().with("repeat-wd")).selected_text(i18n::weekday_name(wd)).width(wd_w), |ui| {
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                        for d in WEEKDAYS {
                            if ui.selectable_label(d == wd, i18n::weekday_name(d)).clicked() {
                                c.on = MonthlyOn::Nth(n, d);
                            }
                        }
                    });
                }
            });
            ui.end_row();
        }
        _ => {}
    }

    // Ends [Never | After n times | On date]
    label(ui, p, t("Ends"));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S8;
        let current = match c.end {
            End::Never => t("Never"),
            End::After(_) => t("After"),
            End::On(_) => t("On date"),
        };
        let end_w = combo_w(col_w, 96.0);
        theme::select(ui, p, egui::ComboBox::from_id_salt(ui.id().with("repeat-end")).selected_text(current).width(end_w), |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            if ui.selectable_label(c.end == End::Never, t("Never")).clicked() {
                c.end = End::Never;
            }
            if ui.selectable_label(matches!(c.end, End::After(_)), t("After")).clicked() && !matches!(c.end, End::After(_)) {
                c.end = End::After(parse_number(&form.count).unwrap_or(10));
                form.count = match c.end {
                    End::After(n) => n.to_string(),
                    _ => String::new(),
                };
            }
            if ui.selectable_label(matches!(c.end, End::On(_)), t("On date")).clicked() && !matches!(c.end, End::On(_)) {
                let d = NaiveDate::parse_from_str(form.until.trim(), "%Y-%m-%d").ok().filter(|d| *d >= start).unwrap_or_else(|| start.checked_add_months(chrono::Months::new(3)).unwrap_or(start));
                c.end = End::On(d);
                form.until = d.to_string();
            }
        });
        match c.end {
            End::After(_) => {
                if theme::text_field(ui, p, egui::TextEdit::singleline(&mut form.count).desired_width(36.0).hint_text("10")).changed() {
                    if let Some(n) = parse_number(&form.count) {
                        c.end = End::After(n);
                    }
                }
                label(ui, p, t("times"));
            }
            End::On(_) => {
                let w = (col_w - end_w - 2.0 * pad - space::S8 - 8.0).clamp(80.0, 110.0);
                if theme::text_field(ui, p, egui::TextEdit::singleline(&mut form.until).desired_width(w).hint_text("2026-12-31")).changed() {
                    if let Ok(d) = NaiveDate::parse_from_str(form.until.trim(), "%Y-%m-%d") {
                        c.end = End::On(d);
                    }
                }
            }
            End::Never => {}
        }
    });
    ui.end_row();

    if c != before {
        ev.rrule = Some(c.to_rule(start).to_text());
        form.mark_synced(ev);
        changed = true;
    }
    if summary_row {
        label(ui, p, "");
        ui.vertical(|ui| {
            ui.set_max_width(col_w);
            if let Some(s) = summary_of(ev) {
                ui.add(egui::Label::new(RichText::new(s).font(font(text::T12)).color(p.text_2)).wrap());
            }
        });
        ui.end_row();
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(rule: Option<&str>) -> CalendarEvent {
        CalendarEvent { date: NaiveDate::from_ymd_opt(2026, 11, 27).unwrap(), rrule: rule.map(String::from), ..Default::default() }
    }

    #[test]
    fn the_form_opens_on_custom_only_for_custom_rules() {
        assert!(!RepeatForm::for_event(&ev(None)).open);
        assert!(!RepeatForm::for_event(&ev(Some("FREQ=WEEKLY"))).open);
        let f = RepeatForm::for_event(&ev(Some("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,FR;COUNT=6")));
        assert!(f.open);
        assert_eq!((f.interval.as_str(), f.count.as_str()), ("2", "6"));
        assert!(RepeatForm::for_event(&ev(Some("FREQ=DAILY;BYHOUR=9"))).open, "read-only rules show as custom");
    }

    #[test]
    fn typed_values_are_checked_before_saving() {
        let e = ev(Some("FREQ=WEEKLY;BYDAY=MO;UNTIL=20261231"));
        let mut f = RepeatForm::for_event(&e);
        assert_eq!(f.until, "2026-12-31");
        assert!(f.error(&e).is_none());
        f.interval = "0".into();
        assert!(f.error(&e).is_some());
        f.interval = "2".into();
        f.until = "31.12.".into();
        assert!(f.error(&e).is_some());
        let early = ev(Some("FREQ=WEEKLY;BYDAY=MO;UNTIL=20261101"));
        assert!(RepeatForm::for_event(&early).error(&early).is_some());
    }

    #[test]
    fn summary_for_the_details_view() {
        crate::i18n::set_language("en");
        assert_eq!(summary_of(&ev(Some("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE,FR;UNTIL=20261231"))).as_deref(), Some("Every 2 weeks on Mon, Wed, Fri until 31 Dec 2026"));
        let berlin = CalendarEvent { zone: EventZone::Tz("Europe/Berlin".into()), ..ev(Some("FREQ=DAILY;UNTIL=20261231T225959Z")) };
        assert_eq!(summary_of(&berlin).as_deref(), Some("Every day until 31 Dec 2026"));
        let ny = CalendarEvent { zone: EventZone::Tz("America/New_York".into()), ..ev(Some("FREQ=DAILY;UNTIL=20270101T045959Z")) };
        assert_eq!(summary_of(&ny).as_deref(), Some("Every day until 31 Dec 2026"));
        assert_eq!(summary_of(&ev(None)), None);
    }
}
