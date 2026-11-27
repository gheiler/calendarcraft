//! Repeat rules (`RRULE`, RFC 5545 §3.3.10) as CalendarCraft reads, edits and writes them.
//!
//! [`RRule`] holds a rule without losing anything: every part it does not model (`BYHOUR`,
//! `BYWEEKNO`, `X-…`, a value it can't read) is kept as written and put back by
//! [`RRule::to_text`]. [`Custom`] is the part the event form's "Custom…" repeat edits: how
//! often, which weekdays, which day of the month, and when it ends. A rule the form can't edit
//! is never rewritten by it; it is shown with its summary and kept as it is.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Weekday};
use craft_ical::values::{format_date, format_local, When};

use crate::i18n::{self, t};

/// The days of the week, Monday first (the week CalendarCraft shows starts on Monday).
pub const WEEKDAYS: [Weekday; 7] = [Weekday::Mon, Weekday::Tue, Weekday::Wed, Weekday::Thu, Weekday::Fri, Weekday::Sat, Weekday::Sun];

/// Longest rule read, in bytes, and most parts of one rule.
const MAX_RULE: usize = 4096;
const MAX_PARTS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Freq {
    Secondly,
    Minutely,
    Hourly,
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl Freq {
    pub fn parse(s: &str) -> Option<Freq> {
        Some(match s.trim().to_ascii_uppercase().as_str() {
            "SECONDLY" => Freq::Secondly,
            "MINUTELY" => Freq::Minutely,
            "HOURLY" => Freq::Hourly,
            "DAILY" => Freq::Daily,
            "WEEKLY" => Freq::Weekly,
            "MONTHLY" => Freq::Monthly,
            "YEARLY" => Freq::Yearly,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Freq::Secondly => "SECONDLY",
            Freq::Minutely => "MINUTELY",
            Freq::Hourly => "HOURLY",
            Freq::Daily => "DAILY",
            Freq::Weekly => "WEEKLY",
            Freq::Monthly => "MONTHLY",
            Freq::Yearly => "YEARLY",
        }
    }

    /// Whether occurrences are worked out for this frequency (one per day at most).
    pub fn by_date(self) -> bool {
        matches!(self, Freq::Daily | Freq::Weekly | Freq::Monthly | Freq::Yearly)
    }
}

/// `MO`, `TU`… as iCalendar writes a weekday.
pub fn weekday_code(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "MO",
        Weekday::Tue => "TU",
        Weekday::Wed => "WE",
        Weekday::Thu => "TH",
        Weekday::Fri => "FR",
        Weekday::Sat => "SA",
        Weekday::Sun => "SU",
    }
}

pub fn weekday_of_code(s: &str) -> Option<Weekday> {
    WEEKDAYS.into_iter().find(|d| weekday_code(*d).eq_ignore_ascii_case(s.trim()))
}

/// One `BYDAY` item: `MO`, `2TU`, `-1FR`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DayNum {
    /// The position in the month (or year): 1 is the first, -1 the last.
    pub nth: Option<i8>,
    pub day: Weekday,
}

impl DayNum {
    fn parse(item: &str) -> Option<DayNum> {
        let item = item.trim();
        let split = item.len().checked_sub(2)?;
        let day = weekday_of_code(item.get(split..)?)?;
        let num = item.get(..split)?;
        let nth = if num.is_empty() {
            None
        } else {
            let n: i8 = num.trim_start_matches('+').parse().ok()?;
            if n == 0 || !(-53..=53).contains(&n) {
                return None;
            }
            Some(n)
        };
        Some(DayNum { nth, day })
    }

    fn text(self) -> String {
        match self.nth {
            Some(n) => format!("{n}{}", weekday_code(self.day)),
            None => weekday_code(self.day).to_string(),
        }
    }
}

/// A repeat rule with every part kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RRule {
    pub freq: Freq,
    /// 1 when the rule has no `INTERVAL`.
    pub interval: u32,
    pub count: Option<u32>,
    pub until: Option<When>,
    pub by_day: Vec<DayNum>,
    pub by_month_day: Vec<i8>,
    pub by_month: Vec<u8>,
    pub by_set_pos: Vec<i16>,
    pub wkst: Option<Weekday>,
    /// Parts kept as written: ones not modelled here, and known ones whose value didn't read.
    pub other: Vec<String>,
}

fn list<T>(value: &str, item: impl Fn(&str) -> Option<T>) -> Option<Vec<T>> {
    let items: Vec<&str> = value.split(',').collect();
    if items.len() > MAX_PARTS {
        return None;
    }
    items.into_iter().map(|s| item(s.trim())).collect()
}

fn when_text(w: When) -> String {
    match w {
        When::Date(d) => format_date(d),
        When::Local(t) => format_local(t),
        When::Utc(t) => format!("{}Z", format_local(t)),
    }
}

impl RRule {
    pub fn new(freq: Freq) -> RRule {
        RRule { freq, interval: 1, count: None, until: None, by_day: Vec::new(), by_month_day: Vec::new(), by_month: Vec::new(), by_set_pos: Vec::new(), wkst: None, other: Vec::new() }
    }

    /// `FREQ=WEEKLY;BYDAY=MO,WE` (an `RRULE:` in front is let through); `None` without a
    /// readable `FREQ`.
    pub fn parse(text: &str) -> Option<RRule> {
        let text = text.trim();
        if text.len() > MAX_RULE {
            return None;
        }
        let text = match text.get(..6) {
            Some(name) if name.eq_ignore_ascii_case("RRULE:") => text.get(6..).unwrap_or(""),
            _ => text,
        };
        let mut freq = None;
        let mut r = RRule::new(Freq::Weekly);
        let mut seen: Vec<String> = Vec::new();
        for part in text.split(';').map(str::trim).filter(|p| !p.is_empty()).take(MAX_PARTS) {
            let Some((key, value)) = part.split_once('=') else {
                r.other.push(part.to_string());
                continue;
            };
            let key = key.trim().to_ascii_uppercase();
            let value = value.trim();
            if seen.contains(&key) {
                r.other.push(part.to_string());
                continue;
            }
            let read = match key.as_str() {
                "FREQ" => Freq::parse(value).map(|f| freq = Some(f)),
                "INTERVAL" => value.parse::<u32>().ok().filter(|n| *n > 0).map(|n| r.interval = n),
                "COUNT" => value.parse::<u32>().ok().filter(|n| *n > 0).map(|n| r.count = Some(n)),
                "UNTIL" => When::parse(value).map(|w| r.until = Some(w)),
                "BYDAY" => list(value, DayNum::parse).map(|v| r.by_day = v),
                "BYMONTHDAY" => list(value, |s| s.trim_start_matches('+').parse::<i8>().ok().filter(|n| *n != 0 && (-31..=31).contains(n))).map(|v| r.by_month_day = v),
                "BYMONTH" => list(value, |s| s.parse::<u8>().ok().filter(|n| (1..=12).contains(n))).map(|v| r.by_month = v),
                "BYSETPOS" => list(value, |s| s.trim_start_matches('+').parse::<i16>().ok().filter(|n| *n != 0 && (-366..=366).contains(n))).map(|v| r.by_set_pos = v),
                "WKST" => weekday_of_code(value).map(|d| r.wkst = Some(d)),
                _ => None,
            };
            if read.is_some() {
                seen.push(key);
            } else {
                r.other.push(part.to_string());
            }
        }
        r.freq = freq?;
        Some(r)
    }

    /// The rule as an `RRULE` value.
    pub fn to_text(&self) -> String {
        let mut out = format!("FREQ={}", self.freq.as_str());
        let join = |v: Vec<String>| v.join(",");
        if self.interval > 1 {
            out.push_str(&format!(";INTERVAL={}", self.interval));
        }
        if !self.by_month.is_empty() {
            out.push_str(&format!(";BYMONTH={}", join(self.by_month.iter().map(|m| m.to_string()).collect())));
        }
        if !self.by_month_day.is_empty() {
            out.push_str(&format!(";BYMONTHDAY={}", join(self.by_month_day.iter().map(|m| m.to_string()).collect())));
        }
        if !self.by_day.is_empty() {
            out.push_str(&format!(";BYDAY={}", join(self.by_day.iter().map(|d| d.text()).collect())));
        }
        if !self.by_set_pos.is_empty() {
            out.push_str(&format!(";BYSETPOS={}", join(self.by_set_pos.iter().map(|m| m.to_string()).collect())));
        }
        if let Some(c) = self.count {
            out.push_str(&format!(";COUNT={c}"));
        }
        if let Some(u) = self.until {
            out.push_str(&format!(";UNTIL={}", when_text(u)));
        }
        if let Some(w) = self.wkst {
            out.push_str(&format!(";WKST={}", weekday_code(w)));
        }
        for part in &self.other {
            out.push(';');
            out.push_str(part);
        }
        out
    }

    /// The day `UNTIL` names, as written (a UTC time gives its UTC date).
    pub fn until_date(&self) -> Option<NaiveDate> {
        self.until.map(|w| w.wall().date())
    }
}

/// Day `n` of a month counted from its end when negative (-1 is the last day).
pub fn month_day(year: i32, month: u32, n: i8) -> Option<NaiveDate> {
    let len = days_in_month(year, month)? as i32;
    let day = if n > 0 { i32::from(n) } else { len + 1 + i32::from(n) };
    if day < 1 || day > len {
        return None;
    }
    NaiveDate::from_ymd_opt(year, month, day as u32)
}

pub fn days_in_month(year: i32, month: u32) -> Option<u32> {
    let first = NaiveDate::from_ymd_opt(year, month, 1)?;
    let next = first.checked_add_months(chrono::Months::new(1))?;
    Some((next - first).num_days() as u32)
}

/// The `nth` `weekday` of a month (1 the first, -1 the last).
pub fn nth_weekday(year: i32, month: u32, weekday: Weekday, nth: i8) -> Option<NaiveDate> {
    let first = NaiveDate::from_ymd_opt(year, month, 1)?;
    let last = first + Duration::days(i64::from(days_in_month(year, month)?) - 1);
    nth_in(first, last, weekday, nth)
}

/// The `nth` `weekday` between `first` and `last` (inclusive).
pub fn nth_in(first: NaiveDate, last: NaiveDate, weekday: Weekday, nth: i8) -> Option<NaiveDate> {
    let day = if nth > 0 {
        let ahead = (i64::from(weekday.num_days_from_monday()) - i64::from(first.weekday().num_days_from_monday())).rem_euclid(7);
        first + Duration::days(ahead + 7 * (i64::from(nth) - 1))
    } else {
        let back = (i64::from(last.weekday().num_days_from_monday()) - i64::from(weekday.num_days_from_monday())).rem_euclid(7);
        last - Duration::days(back + 7 * (-i64::from(nth) - 1))
    };
    (day >= first && day <= last).then_some(day)
}

/// The position of `date`'s weekday in its month: 1–5.
pub fn nth_of(date: NaiveDate) -> i8 {
    ((date.day() - 1) / 7 + 1) as i8
}

/// Whether `date` is the last such weekday of its month.
pub fn is_last_of_month(date: NaiveDate) -> bool {
    days_in_month(date.year(), date.month()).is_some_and(|len| date.day() + 7 > len)
}

/// Which day of the month (or of the rule's month, yearly) a custom repeat falls on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonthlyOn {
    /// The event's own day of the month.
    Day,
    /// The `nth` weekday (1–5, -1 the last).
    Nth(i8, Weekday),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Never,
    After(u32),
    On(NaiveDate),
}

/// What the "Custom…" repeat edits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Custom {
    /// Daily, weekly, monthly or yearly.
    pub freq: Freq,
    pub interval: u32,
    /// Weekly: the days, Monday first; never empty.
    pub days: Vec<Weekday>,
    /// Monthly and yearly.
    pub on: MonthlyOn,
    pub end: End,
    /// Kept from the rule it was read from.
    pub wkst: Option<Weekday>,
    /// The rule's own `UNTIL` and the day it was shown as, so an unchanged end is written back
    /// exactly as it was read.
    pub until_src: Option<(NaiveDate, When)>,
}

/// The repeats offered before "Custom…": `(rule, label)`.
pub const PRESETS: &[(&str, &str)] = &[("", "Never"), ("FREQ=DAILY", "Every day"), ("FREQ=WEEKLY", "Every week"), ("FREQ=MONTHLY", "Every month"), ("FREQ=YEARLY", "Every year")];

fn sorted_days(days: impl IntoIterator<Item = Weekday>) -> Vec<Weekday> {
    let set: Vec<Weekday> = days.into_iter().collect();
    WEEKDAYS.into_iter().filter(|d| set.contains(d)).collect()
}

fn valid_nth(n: i64) -> Option<i8> {
    ((1..=5).contains(&n) || n == -1).then_some(n as i8)
}

impl Custom {
    /// A new custom repeat: every week on the event's weekday.
    pub fn default_for(start: NaiveDate) -> Custom {
        Custom { freq: Freq::Weekly, interval: 1, days: vec![start.weekday()], on: MonthlyOn::Day, end: End::Never, wkst: None, until_src: None }
    }

    /// The form's view of `rule` for an event starting on `start`, or `None` when the rule has
    /// parts the form can't edit. `until_day` turns the rule's `UNTIL` into the day it ends
    /// on in the event's zone.
    pub fn from_rule(rule: &RRule, start: NaiveDate, until_day: &dyn Fn(When) -> NaiveDate) -> Option<Custom> {
        if !rule.other.is_empty() || (rule.count.is_some() && rule.until.is_some()) {
            return None;
        }
        let end = match (rule.count, rule.until) {
            (Some(n), _) => End::After(n),
            (_, Some(u)) => End::On(until_day(u)),
            _ => End::Never,
        };
        let until_src = rule.until.map(|u| (until_day(u), u));
        let mut c = Custom { freq: rule.freq, interval: rule.interval.max(1), days: vec![start.weekday()], on: MonthlyOn::Day, end, wkst: rule.wkst, until_src };
        let plain_days = rule.by_day.iter().all(|d| d.nth.is_none());
        match rule.freq {
            Freq::Daily => {
                if !rule.by_month.is_empty() || !rule.by_month_day.is_empty() || !rule.by_set_pos.is_empty() || !plain_days {
                    return None;
                }
                if !rule.by_day.is_empty() {
                    // Every day, only on some weekdays, is every week on those days.
                    if rule.interval > 1 {
                        return None;
                    }
                    c.freq = Freq::Weekly;
                    c.days = sorted_days(rule.by_day.iter().map(|d| d.day));
                }
            }
            Freq::Weekly => {
                if !rule.by_month.is_empty() || !rule.by_month_day.is_empty() || !rule.by_set_pos.is_empty() || !plain_days {
                    return None;
                }
                if !rule.by_day.is_empty() {
                    c.days = sorted_days(rule.by_day.iter().map(|d| d.day));
                }
            }
            Freq::Monthly | Freq::Yearly => {
                let month_ok = match rule.freq {
                    Freq::Monthly => rule.by_month.is_empty(),
                    _ => rule.by_month.is_empty() || rule.by_month == [start.month() as u8],
                };
                if !month_ok {
                    return None;
                }
                c.on = match (rule.by_day.as_slice(), rule.by_month_day.as_slice(), rule.by_set_pos.as_slice()) {
                    ([], [], []) => MonthlyOn::Day,
                    ([], [d], []) if i64::from(*d) == i64::from(start.day()) => MonthlyOn::Day,
                    ([DayNum { nth: Some(n), day }], [], []) => MonthlyOn::Nth(valid_nth(i64::from(*n))?, *day),
                    ([DayNum { nth: None, day }], [], [pos]) => MonthlyOn::Nth(valid_nth(i64::from(*pos))?, *day),
                    _ => return None,
                };
                // A yearly nth weekday needs its month; BYMONTH without one is "every day of it".
                if rule.freq == Freq::Yearly && matches!(c.on, MonthlyOn::Nth(..)) && rule.by_month.is_empty() {
                    return None;
                }
                if rule.freq == Freq::Yearly && matches!(c.on, MonthlyOn::Day) && !rule.by_month.is_empty() && rule.by_month_day.is_empty() {
                    // FREQ=YEARLY;BYMONTH=11 alone is every day of November.
                    return None;
                }
            }
            _ => return None,
        }
        Some(c)
    }

    /// The rule this writes for an event starting on `start`.
    pub fn to_rule(&self, start: NaiveDate) -> RRule {
        let mut r = RRule::new(self.freq);
        r.interval = self.interval.max(1);
        r.wkst = self.wkst;
        match self.freq {
            Freq::Weekly => {
                let days = if self.days.is_empty() { vec![start.weekday()] } else { sorted_days(self.days.iter().copied()) };
                r.by_day = days.into_iter().map(|day| DayNum { nth: None, day }).collect();
            }
            Freq::Monthly | Freq::Yearly => {
                if let MonthlyOn::Nth(n, day) = self.on {
                    r.by_day = vec![DayNum { nth: Some(n), day }];
                    if self.freq == Freq::Yearly {
                        r.by_month = vec![start.month() as u8];
                    }
                }
            }
            _ => {}
        }
        match self.end {
            End::Never => {}
            End::After(n) => r.count = Some(n.max(1)),
            End::On(d) => {
                r.until = Some(match self.until_src {
                    Some((shown, src)) if shown == d => src,
                    _ => When::Date(d),
                })
            }
        }
        r
    }

    /// The preset this is the same as (index into [`PRESETS`]), when there is one.
    pub fn preset(&self, start: NaiveDate) -> Option<usize> {
        if self.interval != 1 || self.end != End::Never {
            return None;
        }
        match self.freq {
            Freq::Daily => Some(1),
            Freq::Weekly if self.days == [start.weekday()] => Some(2),
            Freq::Monthly if self.on == MonthlyOn::Day => Some(3),
            Freq::Yearly if self.on == MonthlyOn::Day => Some(4),
            _ => None,
        }
    }
}

/// The preset a rule's text shows as, for an event starting on `start`: an index into
/// [`PRESETS`] (0 is no repeat), or `None` for a custom rule.
pub fn preset_of(rule: Option<&str>, start: NaiveDate) -> Option<usize> {
    let Some(text) = rule.map(str::trim).filter(|r| !r.is_empty()) else { return Some(0) };
    let r = RRule::parse(text)?;
    // A rule's own UNTIL never makes it a preset, so the zone doesn't matter here.
    Custom::from_rule(&r, start, &|w| w.wall().date())?.preset(start)
}

fn ordinal(n: i8) -> &'static str {
    match n {
        1 => t("first"),
        2 => t("second"),
        3 => t("third"),
        4 => t("fourth"),
        5 => t("fifth"),
        -1 => t("last"),
        -2 => t("second to last"),
        _ => "",
    }
}

/// `first`…`fifth`, `last`, with a capital first letter, for the form's menu.
pub fn ordinal_label(n: i8) -> String {
    let o = ordinal(n);
    let mut c = o.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// `31 Dec 2026`.
pub fn short_date(d: NaiveDate) -> String {
    format!("{} {} {}", d.day(), i18n::month_short(d.month()), d.year())
}

fn day_list(days: &[Weekday]) -> String {
    if days.len() == 5 && WEEKDAYS[..5].iter().all(|d| days.contains(d)) {
        return t("weekdays").to_string();
    }
    if days.len() == 2 && days.contains(&Weekday::Sat) && days.contains(&Weekday::Sun) {
        return t("weekends").to_string();
    }
    sorted_days(days.iter().copied()).into_iter().map(i18n::weekday_short).collect::<Vec<_>>().join(", ")
}

fn nth_phrase(n: i8, day: Weekday) -> String {
    t("on the {nth} {day}").replace("{nth}", ordinal(n)).replace("{day}", i18n::weekday_name(day))
}

/// "Every 2 weeks on Mon, Wed, Fri until 31 Dec 2026", in the interface language.
/// `until_day` turns `UNTIL` into the day it ends on in the event's zone.
pub fn summary(rule: &RRule, start: NaiveDate, until_day: &dyn Fn(When) -> NaiveDate) -> String {
    let n = rule.interval.max(1);
    let every = |one: &'static str, many: &'static str| if n == 1 { t(one).to_string() } else { t(many).replace("{n}", &n.to_string()) };
    let mut out = match rule.freq {
        Freq::Daily => every("Every day", "Every {n} days"),
        Freq::Weekly => every("Every week", "Every {n} weeks"),
        Freq::Monthly => every("Every month", "Every {n} months"),
        Freq::Yearly => every("Every year", "Every {n} years"),
        Freq::Hourly => every("Every hour", "Every {n} hours"),
        Freq::Minutely => every("Every minute", "Every {n} minutes"),
        Freq::Secondly => every("Every second", "Every {n} seconds"),
    };
    let mut shown_all = rule.other.is_empty();
    let plain: Vec<Weekday> = rule.by_day.iter().filter(|d| d.nth.is_none()).map(|d| d.day).collect();
    let nths: Vec<String> = rule.by_day.iter().filter_map(|d| d.nth.map(|n| (n, d.day))).filter(|(n, _)| !ordinal(*n).is_empty()).map(|(n, d)| nth_phrase(n, d)).collect();
    if nths.len() != rule.by_day.iter().filter(|d| d.nth.is_some()).count() {
        shown_all = false;
    }
    let months = || rule.by_month.iter().map(|m| i18n::month_name(u32::from(*m))).collect::<Vec<_>>().join(", ");
    match rule.freq {
        Freq::Daily | Freq::Weekly | Freq::Hourly | Freq::Minutely | Freq::Secondly => {
            if !plain.is_empty() {
                out.push(' ');
                out.push_str(&t("on {days}").replace("{days}", &day_list(&plain)));
            } else if rule.freq == Freq::Weekly && rule.by_day.is_empty() && rule.interval > 1 {
                out.push(' ');
                out.push_str(&t("on {days}").replace("{days}", i18n::weekday_short(start.weekday())));
            }
            if !nths.is_empty() || !rule.by_month_day.is_empty() || !rule.by_set_pos.is_empty() {
                shown_all = false;
            }
            if !rule.by_month.is_empty() {
                out.push(' ');
                out.push_str(&t("in {months}").replace("{months}", &months()));
            }
        }
        Freq::Monthly | Freq::Yearly => {
            let set_nth = match (plain.as_slice(), rule.by_set_pos.as_slice()) {
                ([day], [pos]) if !ordinal(*pos as i8).is_empty() && (-5..=5).contains(pos) => Some(nth_phrase(*pos as i8, *day)),
                _ => None,
            };
            let on = if let Some(p) = set_nth {
                Some(p)
            } else if !nths.is_empty() {
                if !rule.by_set_pos.is_empty() || !plain.is_empty() {
                    shown_all = false;
                }
                Some(nths.join(", "))
            } else if !rule.by_month_day.is_empty() {
                let days = rule.by_month_day.iter().map(|d| if *d == -1 { t("the last day").to_string() } else { d.to_string() }).collect::<Vec<_>>().join(", ");
                if !plain.is_empty() || !rule.by_set_pos.is_empty() {
                    shown_all = false;
                }
                Some(t("on day {n}").replace("{n}", &days))
            } else if !plain.is_empty() {
                if !rule.by_set_pos.is_empty() {
                    shown_all = false;
                }
                Some(t("on {days}").replace("{days}", &day_list(&plain)))
            } else if !rule.by_set_pos.is_empty() {
                shown_all = false;
                None
            } else if rule.freq == Freq::Monthly {
                Some(t("on day {n}").replace("{n}", &start.day().to_string()))
            } else if rule.by_month.is_empty() {
                Some(t("on {date}").replace("{date}", &format!("{} {}", start.day(), i18n::month_name(start.month()))))
            } else {
                None
            };
            if let Some(on) = on {
                out.push(' ');
                out.push_str(&on);
            }
            if !rule.by_month.is_empty() {
                out.push(' ');
                out.push_str(&t("in {months}").replace("{months}", &months()));
            }
        }
    }
    match (rule.count, rule.until) {
        (Some(1), _) => out.push_str(t(", once")),
        (Some(c), _) => out.push_str(&t(", {n} times").replace("{n}", &c.to_string())),
        (None, Some(u)) => {
            out.push(' ');
            out.push_str(&t("until {date}").replace("{date}", &short_date(until_day(u))));
        }
        _ => {}
    }
    if !shown_all {
        out.push_str(t(" (and more rules)"));
    }
    out
}

/// A time of day at the end of `d`, for an `UNTIL` that has to be a date-time.
pub fn end_of_day(d: NaiveDate) -> NaiveDateTime {
    d.and_hms_opt(23, 59, 59).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn wall(w: When) -> NaiveDate {
        w.wall().date()
    }

    #[test]
    fn parse_and_write_round_trip() {
        for text in [
            "FREQ=DAILY",
            "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE,FR",
            "FREQ=MONTHLY;BYDAY=2TU",
            "FREQ=MONTHLY;BYDAY=-1FR;COUNT=6",
            "FREQ=MONTHLY;BYMONTHDAY=1,15,-1",
            "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1",
            "FREQ=YEARLY;BYMONTH=11;BYDAY=4TH",
            "FREQ=WEEKLY;BYDAY=MO;UNTIL=20261231T225959Z",
            "FREQ=WEEKLY;UNTIL=20261231",
            "FREQ=DAILY;UNTIL=20261231T235959",
            "FREQ=WEEKLY;INTERVAL=2;BYDAY=TU,TH;WKST=SU",
            "FREQ=HOURLY;INTERVAL=4",
            "FREQ=YEARLY;BYDAY=MO;BYWEEKNO=20",
            "FREQ=DAILY;BYHOUR=9,17;X-NAME=a",
        ] {
            let r = RRule::parse(text).unwrap_or_else(|| panic!("{text}"));
            assert_eq!(r.to_text(), text, "written back as read");
            assert_eq!(RRule::parse(&r.to_text()).as_ref(), Some(&r));
        }
    }

    #[test]
    fn parse_keeps_what_it_cannot_read() {
        let r = RRule::parse("rrule:freq=weekly;byday=mo,xx;interval=2;BYHOUR=9;INTERVAL=3").unwrap();
        assert_eq!(r.freq, Freq::Weekly);
        assert_eq!(r.interval, 2);
        assert!(r.by_day.is_empty());
        assert_eq!(r.other, vec!["byday=mo,xx", "BYHOUR=9", "INTERVAL=3"]);
        assert_eq!(r.to_text(), "FREQ=WEEKLY;INTERVAL=2;byday=mo,xx;BYHOUR=9;INTERVAL=3");
        assert!(RRule::parse("BYDAY=MO").is_none());
        assert!(RRule::parse("FREQ=SOMETIMES").is_none());
        assert_eq!(RRule::parse("FREQ=MONTHLY;BYDAY=+2TU").unwrap().by_day, vec![DayNum { nth: Some(2), day: Weekday::Tue }]);
    }

    #[test]
    fn presets_and_custom_views() {
        let fri = d(2026, 11, 27);
        assert_eq!(preset_of(None, fri), Some(0));
        assert_eq!(preset_of(Some("FREQ=DAILY"), fri), Some(1));
        assert_eq!(preset_of(Some("FREQ=WEEKLY"), fri), Some(2));
        assert_eq!(preset_of(Some("FREQ=WEEKLY;BYDAY=FR"), fri), Some(2), "the event's own weekday");
        assert_eq!(preset_of(Some("FREQ=WEEKLY;BYDAY=MO"), fri), None);
        assert_eq!(preset_of(Some("FREQ=MONTHLY;BYMONTHDAY=27"), fri), Some(3));
        assert_eq!(preset_of(Some("FREQ=YEARLY"), fri), Some(4));
        assert_eq!(preset_of(Some("FREQ=WEEKLY;COUNT=3"), fri), None);
        assert_eq!(preset_of(Some("FREQ=WEEKLY;INTERVAL=2"), fri), None);

        let c = Custom::from_rule(&RRule::parse("FREQ=WEEKLY;INTERVAL=2;BYDAY=FR,MO,WE;UNTIL=20261231").unwrap(), fri, &wall).unwrap();
        assert_eq!(c.freq, Freq::Weekly);
        assert_eq!(c.interval, 2);
        assert_eq!(c.days, vec![Weekday::Mon, Weekday::Wed, Weekday::Fri]);
        assert_eq!(c.end, End::On(d(2026, 12, 31)));
        assert_eq!(c.to_rule(fri).to_text(), "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE,FR;UNTIL=20261231");

        let m = Custom::from_rule(&RRule::parse("FREQ=MONTHLY;BYDAY=TU;BYSETPOS=2").unwrap(), fri, &wall).unwrap();
        assert_eq!(m.on, MonthlyOn::Nth(2, Weekday::Tue));
        assert_eq!(m.to_rule(fri).to_text(), "FREQ=MONTHLY;BYDAY=2TU");
        let y = Custom::from_rule(&RRule::parse("FREQ=YEARLY;BYMONTH=11;BYDAY=4TH").unwrap(), d(2026, 11, 26), &wall).unwrap();
        assert_eq!(y.on, MonthlyOn::Nth(4, Weekday::Thu));
        assert_eq!(y.to_rule(d(2026, 11, 26)).to_text(), "FREQ=YEARLY;BYMONTH=11;BYDAY=4TH");
        // Weekdays, the way some apps write them.
        let wd = Custom::from_rule(&RRule::parse("FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR").unwrap(), fri, &wall).unwrap();
        assert_eq!((wd.freq, wd.days.len()), (Freq::Weekly, 5));

        for unsupported in ["FREQ=MONTHLY;BYMONTHDAY=1,15", "FREQ=MONTHLY;BYMONTHDAY=3", "FREQ=DAILY;BYHOUR=9", "FREQ=HOURLY", "FREQ=YEARLY;BYMONTH=3", "FREQ=YEARLY;BYDAY=20MO", "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1", "FREQ=WEEKLY;COUNT=2;UNTIL=20261231", "FREQ=WEEKLY;BYDAY=1MO"] {
            assert!(Custom::from_rule(&RRule::parse(unsupported).unwrap(), fri, &wall).is_none(), "{unsupported}");
        }
    }

    #[test]
    fn an_unchanged_end_keeps_its_until_value() {
        let fri = d(2026, 11, 27);
        let r = RRule::parse("FREQ=WEEKLY;BYDAY=MO,FR;UNTIL=20261231T225959Z").unwrap();
        // Berlin: 22:59:59 UTC is the end of 31 December.
        let berlin = |w: When| (w.wall() + Duration::hours(1)).date();
        let mut c = Custom::from_rule(&r, fri, &berlin).unwrap();
        assert_eq!(c.end, End::On(d(2026, 12, 31)));
        assert_eq!(c.to_rule(fri), r);
        c.end = End::On(d(2027, 1, 15));
        assert_eq!(c.to_rule(fri).until, Some(When::Date(d(2027, 1, 15))));
        c.end = End::After(4);
        assert_eq!(c.to_rule(fri).to_text(), "FREQ=WEEKLY;BYDAY=MO,FR;COUNT=4");
    }

    #[test]
    fn summaries_read_like_a_sentence() {
        crate::i18n::set_language("en");
        let fri = d(2026, 11, 27);
        let s = |r: &str| summary(&RRule::parse(r).unwrap(), fri, &wall);
        assert_eq!(s("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE,FR;UNTIL=20261231"), "Every 2 weeks on Mon, Wed, Fri until 31 Dec 2026");
        assert_eq!(s("FREQ=DAILY;COUNT=10"), "Every day, 10 times");
        assert_eq!(s("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR"), "Every week on weekdays");
        assert_eq!(s("FREQ=MONTHLY"), "Every month on day 27");
        assert_eq!(s("FREQ=MONTHLY;BYDAY=2TU"), "Every month on the second Tuesday");
        assert_eq!(s("FREQ=MONTHLY;INTERVAL=3;BYDAY=-1FR;COUNT=1"), "Every 3 months on the last Friday, once");
        assert_eq!(s("FREQ=MONTHLY;BYMONTHDAY=1,15,-1"), "Every month on day 1, 15, the last day");
        assert_eq!(s("FREQ=YEARLY"), "Every year on 27 November");
        assert_eq!(s("FREQ=YEARLY;BYMONTH=11;BYDAY=4TH"), "Every year on the fourth Thursday in November");
        assert_eq!(s("FREQ=DAILY;BYHOUR=9"), "Every day (and more rules)");
        crate::i18n::set_language("de");
        assert_eq!(s("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE"), "Alle 2 Wochen am Mo, Mi");
        crate::i18n::set_language("en");
    }

    #[test]
    fn nth_weekdays() {
        assert_eq!(nth_weekday(2026, 11, Weekday::Thu, 4), Some(d(2026, 11, 26)));
        assert_eq!(nth_weekday(2026, 11, Weekday::Mon, -1), Some(d(2026, 11, 30)));
        assert_eq!(nth_weekday(2026, 11, Weekday::Mon, 5), Some(d(2026, 11, 30)));
        assert_eq!(nth_weekday(2026, 11, Weekday::Tue, 5), None);
        assert_eq!(nth_of(d(2026, 11, 27)), 4);
        assert!(is_last_of_month(d(2026, 11, 27)));
        assert!(!is_last_of_month(d(2026, 11, 20)));
        assert_eq!(month_day(2026, 2, -1), Some(d(2026, 2, 28)));
        assert_eq!(month_day(2026, 2, 30), None);
    }
}
