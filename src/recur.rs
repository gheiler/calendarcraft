//! Recurrence expansion: whether an event occurs on a given day (floating / all-day) or at a
//! given instant (timed, zoned).
//!
//! The rule text is read by [`crate::rrule::RRule`], which keeps every part. This module works
//! out which days a rule produces, period by period as RFC 5545 §3.3.10 has it: `BYDAY` with
//! several weekdays or with positions (`2TU`, `-1FR`), `BYMONTHDAY` (also from the end),
//! `BYMONTH`, `BYSETPOS`, `WKST`, `INTERVAL`, `COUNT` and `UNTIL`, plus excluded dates (`EXDATE`,
//! and the `;X-EXDATE=…` CalendarCraft used to keep on the rule). The start (`DTSTART`) is
//! always the first occurrence. Parts that pick times within a day (`BYHOUR`…) or weeks of the
//! year (`BYWEEKNO`, `BYYEARDAY`) are kept in the rule but not used here, and rules more often
//! than daily are shown once.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Weekday};
use craft_ical::values::When;

use crate::rrule::{self, Freq, RRule};
use crate::tz::{self, DisplayTz, EventZone};

/// A parsed rule plus the exclusions and a UTC `UNTIL` instant when the text had one.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub rec: RRule,
    pub except: Vec<NaiveDateTime>,
    /// When `UNTIL` was a UTC date-time, the last instant that is still included.
    pub until_utc: Option<i64>,
}

/// `None` for text without a rule CalendarCraft can expand (no `FREQ`, or more often than daily).
pub fn parse(rule: &str) -> Option<Rule> {
    let (core, except) = split_exdates(rule);
    let rec = RRule::parse(core).filter(|r| r.freq.by_date())?;
    let until_utc = match rec.until {
        Some(When::Utc(t)) => Some(t.and_utc().timestamp()),
        _ => None,
    };
    Some(Rule { rec, except, until_utc })
}

/// Strip the historical `;X-EXDATE=YYYYMMDD,…` suffix CalendarCraft stored on the rule.
pub fn split_exdates(rule: &str) -> (&str, Vec<NaiveDateTime>) {
    if let Some((core, rest)) = rule.split_once(";X-EXDATE=") {
        let dates = rest
            .split(',')
            .filter_map(|d| {
                let d = d.trim();
                NaiveDate::parse_from_str(&d[..8.min(d.len())], "%Y%m%d")
                    .ok()
                    .map(|d| d.and_time(chrono::NaiveTime::MIN))
            })
            .collect();
        (core, dates)
    } else {
        (rule, Vec::new())
    }
}

/// Most periods (days, weeks, months or years) walked for one question.
const MAX_PERIODS: i64 = 200_000;

fn months_between(a: NaiveDate, b: NaiveDate) -> i64 {
    (i64::from(b.year()) - i64::from(a.year())) * 12 + i64::from(b.month()) - i64::from(a.month())
}

fn week_start(day: NaiveDate, wkst: Weekday) -> NaiveDate {
    let back = (i64::from(day.weekday().num_days_from_monday()) - i64::from(wkst.num_days_from_monday())).rem_euclid(7);
    day - Duration::days(back)
}

fn add_months(year: i32, month: u32, n: i64) -> Option<(i32, u32)> {
    let total = i64::from(year) * 12 + i64::from(month) - 1 + n;
    Some((i32::try_from(total.div_euclid(12)).ok()?, u32::try_from(total.rem_euclid(12)).ok()? + 1))
}

/// The days of one month the rule picks, before `BYSETPOS`; `default_day` when nothing narrows it.
fn month_days(rec: &RRule, year: i32, month: u32, default_day: u32) -> Vec<NaiveDate> {
    let Some(len) = rrule::days_in_month(year, month) else { return Vec::new() };
    let by_md: Vec<NaiveDate> = rec.by_month_day.iter().filter_map(|n| rrule::month_day(year, month, *n)).collect();
    let mut by_wd: Vec<NaiveDate> = Vec::new();
    for dn in &rec.by_day {
        match dn.nth {
            Some(n) => by_wd.extend(rrule::nth_weekday(year, month, dn.day, n)),
            None => by_wd.extend((1..=len).filter_map(|d| NaiveDate::from_ymd_opt(year, month, d)).filter(|d| d.weekday() == dn.day)),
        }
    }
    let mut out = match (rec.by_month_day.is_empty(), rec.by_day.is_empty()) {
        (true, true) => NaiveDate::from_ymd_opt(year, month, default_day).into_iter().collect(),
        (false, true) => by_md,
        (true, false) => by_wd,
        (false, false) => by_md.into_iter().filter(|d| by_wd.contains(d)).collect(),
    };
    out.sort();
    out.dedup();
    out
}

/// The days of period `k` (the start's period plus `k` intervals), sorted, `BYSETPOS` applied.
fn period_days(rec: &RRule, start: NaiveDate, k: i64) -> Vec<NaiveDate> {
    let step = k * i64::from(rec.interval.max(1));
    let month_ok = |d: &NaiveDate| rec.by_month.is_empty() || rec.by_month.contains(&(d.month() as u8));
    let mut days: Vec<NaiveDate> = match rec.freq {
        Freq::Daily => {
            let day = start + Duration::days(step);
            let md_ok = rec.by_month_day.is_empty() || rec.by_month_day.iter().any(|n| rrule::month_day(day.year(), day.month(), *n) == Some(day));
            let wd_ok = rec.by_day.is_empty() || rec.by_day.iter().any(|d| d.day == day.weekday());
            if md_ok && wd_ok && month_ok(&day) { vec![day] } else { Vec::new() }
        }
        Freq::Weekly => {
            let first = week_start(start, rec.wkst.unwrap_or(Weekday::Mon)) + Duration::days(7 * step);
            (0..7)
                .map(|i| first + Duration::days(i))
                .filter(|d| if rec.by_day.is_empty() { d.weekday() == start.weekday() } else { rec.by_day.iter().any(|b| b.day == d.weekday()) })
                .filter(month_ok)
                .collect()
        }
        Freq::Monthly => match add_months(start.year(), start.month(), step) {
            Some((y, m)) if rec.by_month.is_empty() || rec.by_month.contains(&(m as u8)) => month_days(rec, y, m, start.day()),
            _ => Vec::new(),
        },
        Freq::Yearly => {
            let Ok(year) = i32::try_from(i64::from(start.year()) + step) else { return Vec::new() };
            if !rec.by_month.is_empty() {
                let mut months = rec.by_month.clone();
                months.sort();
                months.dedup();
                months.into_iter().flat_map(|m| month_days(rec, year, u32::from(m), start.day())).collect()
            } else if !rec.by_month_day.is_empty() {
                (1..=12).flat_map(|m| month_days(rec, year, m, start.day())).collect()
            } else if !rec.by_day.is_empty() {
                let (Some(first), Some(last)) = (NaiveDate::from_ymd_opt(year, 1, 1), NaiveDate::from_ymd_opt(year, 12, 31)) else { return Vec::new() };
                let mut out = Vec::new();
                for dn in &rec.by_day {
                    match dn.nth {
                        Some(n) => out.extend(rrule::nth_in(first, last, dn.day, n)),
                        None => {
                            let mut d = rrule::nth_in(first, last, dn.day, 1);
                            while let Some(x) = d.filter(|x| *x <= last) {
                                out.push(x);
                                d = Some(x + Duration::days(7));
                            }
                        }
                    }
                }
                out.sort();
                out.dedup();
                out
            } else {
                NaiveDate::from_ymd_opt(year, start.month(), start.day()).into_iter().collect()
            }
        }
        _ => Vec::new(),
    };
    if !rec.by_set_pos.is_empty() && !days.is_empty() {
        let n = days.len() as i64;
        let mut picked: Vec<NaiveDate> = rec
            .by_set_pos
            .iter()
            .filter_map(|p| {
                let i = if *p > 0 { i64::from(*p) - 1 } else { n + i64::from(*p) };
                usize::try_from(i).ok().and_then(|i| days.get(i).copied())
            })
            .collect();
        picked.sort();
        picked.dedup();
        days = picked;
    }
    days
}

/// The first period worth walking to reach `from` (0 when `COUNT` needs them all).
fn first_period(rec: &RRule, start: NaiveDate, from: NaiveDate) -> i64 {
    if rec.count.is_some() || from <= start {
        return 0;
    }
    let interval = i64::from(rec.interval.max(1));
    let units = match rec.freq {
        Freq::Daily => (from - start).num_days(),
        Freq::Weekly => {
            let wkst = rec.wkst.unwrap_or(Weekday::Mon);
            (week_start(from, wkst) - week_start(start, wkst)).num_days() / 7
        }
        Freq::Monthly => months_between(start, from),
        Freq::Yearly => i64::from(from.year()) - i64::from(start.year()),
        _ => 0,
    };
    (units / interval - 1).max(0)
}

/// The first day of period `k` (counted as in [`period_days`]).
fn period_reaches(rec: &RRule, start: NaiveDate, k: i64) -> NaiveDate {
    let step = k * i64::from(rec.interval.max(1));
    let far = NaiveDate::MAX;
    match rec.freq {
        Freq::Daily => start.checked_add_signed(Duration::days(step)).unwrap_or(far),
        Freq::Weekly => week_start(start, rec.wkst.unwrap_or(Weekday::Mon)).checked_add_signed(Duration::days(7 * step)).unwrap_or(far),
        Freq::Monthly => add_months(start.year(), start.month(), step).and_then(|(y, m)| NaiveDate::from_ymd_opt(y, m, 1)).unwrap_or(far),
        Freq::Yearly => i32::try_from(i64::from(start.year()) + step).ok().and_then(|y| NaiveDate::from_ymd_opt(y, 1, 1)).unwrap_or(far),
        _ => far,
    }
}

/// Every day an occurrence starts on, from `from` through `to` (inclusive), in order: the start
/// itself first, then what the rule makes, up to `COUNT` and `UNTIL`. Exclusions not applied.
pub fn occurrence_days(rec: &RRule, start: NaiveDate, from: NaiveDate, to: NaiveDate) -> Vec<NaiveDate> {
    let last = match rec.until_date() {
        Some(u) => to.min(u),
        None => to,
    };
    let mut out = Vec::new();
    if start > last {
        return out;
    }
    if start >= from {
        out.push(start);
    }
    if !rec.freq.by_date() {
        return out;
    }
    let mut n = 1u32;
    let k0 = first_period(rec, start, from);
    for k in k0..k0 + MAX_PERIODS {
        if period_reaches(rec, start, k) > last {
            break;
        }
        for d in period_days(rec, start, k) {
            if d <= start {
                continue;
            }
            if d > last {
                return out;
            }
            n += 1;
            if rec.count.is_some_and(|c| n > c) {
                return out;
            }
            if d >= from {
                out.push(d);
            }
        }
    }
    out
}

fn excluded_date(rule: &Rule, day: NaiveDate) -> bool {
    rule.except.iter().any(|t| t.date() == day)
}

/// Whether an event starting on `start` with this rule has an occurrence starting on `day`
/// (floating or all-day dates: no zone is applied).
pub fn occurs_on(rule: &Rule, start: NaiveDate, day: NaiveDate) -> bool {
    day >= start && !excluded_date(rule, day) && occurrence_days(&rule.rec, start, day, day).contains(&day)
}

/// Each occurrence start in the event's own wall clock, until `end_day` (inclusive, event-zone dates)
/// or `COUNT` / `UNTIL`.
pub fn occurrence_walls(rule: &Rule, start: NaiveDateTime, end_day: NaiveDate) -> Vec<NaiveDateTime> {
    occurrence_walls_between(rule, start, start.date(), end_day)
}

/// [`occurrence_walls`] that start from `from_day` on (event-zone dates).
pub fn occurrence_walls_between(rule: &Rule, start: NaiveDateTime, from_day: NaiveDate, end_day: NaiveDate) -> Vec<NaiveDateTime> {
    // COUNT counts the occurrences the rule makes; an excluded one still uses up its place
    // (RFC 5545 §3.8.5.1: EXDATE is taken out of the set after the rule made it).
    occurrence_days(&rule.rec, start.date(), from_day, end_day)
        .into_iter()
        .map(|d| d.and_time(start.time()))
        .filter(|wall| {
            !rule.except.iter().any(|ex| {
                if ex.time() == chrono::NaiveTime::MIN && start.time() != chrono::NaiveTime::MIN {
                    ex.date() == wall.date()
                } else {
                    ex == wall || ex.date() == wall.date() && ex.time() == chrono::NaiveTime::MIN
                }
            })
        })
        .collect()
}

/// Whether a timed occurrence whose start instant is `start_instant` falls so that its display
/// interval overlaps the display-zone civil day `day`.
pub fn occurrence_hits_display_day(
    start_instant: i64,
    duration_secs: i64,
    day: NaiveDate,
    display: DisplayTz,
    all_day: bool,
) -> bool {
    if all_day {
        return display.wall_at(start_instant).date() == day;
    }
    let end_instant = start_instant.saturating_add(duration_secs.max(0));
    let day_start = display.start_of_day(day);
    let day_end = display.start_of_day(day + Duration::days(1));
    start_instant < day_end && end_instant > day_start
}

/// Start instant of a wall-clock time in the event's zone.
pub fn start_instant(zone: &EventZone, wall: NaiveDateTime) -> i64 {
    match zone {
        EventZone::Utc => wall.and_utc().timestamp(),
        EventZone::Floating => wall.and_utc().timestamp(),
        EventZone::Tz(_) => tz::instant_in_event_zone(zone, wall),
    }
}

/// Convert a zoned/UTC occurrence into display-zone walls (start, end).
pub fn display_range(
    zone: &EventZone,
    start_wall: NaiveDateTime,
    end_wall: NaiveDateTime,
    display: DisplayTz,
) -> (NaiveDateTime, NaiveDateTime) {
    match zone {
        EventZone::Floating => (start_wall, end_wall),
        EventZone::Utc | EventZone::Tz(_) => {
            let start = start_instant(zone, start_wall);
            let dur = start_instant(zone, end_wall).saturating_sub(start);
            let start_d = display.wall_at(start);
            let end_d = display.wall_at(start.saturating_add(dur));
            (start_d, end_d)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn weekly_by_day_with_interval_and_count() {
        let r = parse("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE;COUNT=3").unwrap();
        let start = d(2026, 11, 2); // a Monday
        assert!(occurs_on(&r, start, d(2026, 11, 2)));
        assert!(occurs_on(&r, start, d(2026, 11, 4)));
        assert!(!occurs_on(&r, start, d(2026, 11, 9)), "odd week");
        assert!(occurs_on(&r, start, d(2026, 11, 16)));
        assert!(!occurs_on(&r, start, d(2026, 11, 18)), "past COUNT");
    }

    #[test]
    fn monthly_yearly_until_and_exdates() {
        let r = parse("FREQ=MONTHLY;UNTIL=20270301T000000Z;X-EXDATE=20261215").unwrap();
        let start = d(2026, 10, 15);
        assert!(occurs_on(&r, start, d(2026, 11, 15)));
        assert!(!occurs_on(&r, start, d(2026, 12, 15)));
        assert!(occurs_on(&r, start, d(2027, 1, 15)));
        assert!(!occurs_on(&r, start, d(2027, 3, 15)));
        let y = parse("FREQ=YEARLY").unwrap();
        assert!(occurs_on(&y, d(2020, 2, 3), d(2026, 2, 3)));
        assert!(parse("FREQ=SECONDLY").is_none());
    }

    #[test]
    fn until_utc_is_kept_as_an_instant() {
        let r = parse("FREQ=DAILY;UNTIL=20260329T010000Z").unwrap();
        assert_eq!(
            r.until_utc,
            Some(
                NaiveDateTime::parse_from_str("2026-03-29 01:00:00", "%Y-%m-%d %H:%M:%S")
                    .unwrap()
                    .and_utc()
                    .timestamp()
            )
        );
    }

    fn days(rule: &str, start: NaiveDate, from: NaiveDate, to: NaiveDate) -> Vec<NaiveDate> {
        let r = parse(rule).unwrap();
        occurrence_days(&r.rec, start, from, to).into_iter().filter(|x| !excluded_date(&r, *x)).collect()
    }

    #[test]
    fn weekly_on_several_days_with_an_interval() {
        // Every 2 weeks on Mon, Wed, Fri from Wed 4 Nov 2026.
        let got = days("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE,FR", d(2026, 11, 4), d(2026, 11, 1), d(2026, 11, 30));
        assert_eq!(got, vec![d(2026, 11, 4), d(2026, 11, 6), d(2026, 11, 16), d(2026, 11, 18), d(2026, 11, 20), d(2026, 11, 30)]);
        // WKST=SU moves the week boundary: Sun 8 Nov starts the odd week.
        let su = days("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,SU;WKST=SU", d(2026, 11, 2), d(2026, 11, 1), d(2026, 11, 22));
        assert_eq!(su, vec![d(2026, 11, 2), d(2026, 11, 15), d(2026, 11, 16)]);
        let mo = days("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,SU", d(2026, 11, 2), d(2026, 11, 1), d(2026, 11, 22));
        assert_eq!(mo, vec![d(2026, 11, 2), d(2026, 11, 8), d(2026, 11, 16), d(2026, 11, 22)]);
    }

    #[test]
    fn the_start_is_always_the_first_occurrence() {
        // A Tuesday start of a Mon/Wed rule: the Tuesday counts, and COUNT includes it.
        let got = days("FREQ=WEEKLY;BYDAY=MO,WE;COUNT=3", d(2026, 11, 3), d(2026, 11, 1), d(2026, 12, 31));
        assert_eq!(got, vec![d(2026, 11, 3), d(2026, 11, 4), d(2026, 11, 9)]);
    }

    #[test]
    fn monthly_positions_and_days_from_the_end() {
        let second_tue = days("FREQ=MONTHLY;BYDAY=2TU", d(2026, 1, 13), d(2026, 1, 1), d(2026, 6, 30));
        assert_eq!(second_tue, vec![d(2026, 1, 13), d(2026, 2, 10), d(2026, 3, 10), d(2026, 4, 14), d(2026, 5, 12), d(2026, 6, 9)]);
        let last_fri = days("FREQ=MONTHLY;BYDAY=-1FR;COUNT=4", d(2026, 1, 30), d(2026, 1, 1), d(2027, 12, 31));
        assert_eq!(last_fri, vec![d(2026, 1, 30), d(2026, 2, 27), d(2026, 3, 27), d(2026, 4, 24)]);
        let fifth_mon = days("FREQ=MONTHLY;BYDAY=5MO", d(2026, 3, 30), d(2026, 3, 1), d(2026, 12, 31));
        assert_eq!(fifth_mon, vec![d(2026, 3, 30), d(2026, 6, 29), d(2026, 8, 31), d(2026, 11, 30)]);
        let last_day = days("FREQ=MONTHLY;BYMONTHDAY=-1", d(2026, 1, 31), d(2026, 1, 1), d(2026, 4, 30));
        assert_eq!(last_day, vec![d(2026, 1, 31), d(2026, 2, 28), d(2026, 3, 31), d(2026, 4, 30)]);
        // The 31st skips the shorter months (RFC 5545 has no "last day" fallback).
        let on_31 = days("FREQ=MONTHLY", d(2026, 1, 31), d(2026, 1, 1), d(2026, 5, 31));
        assert_eq!(on_31, vec![d(2026, 1, 31), d(2026, 3, 31), d(2026, 5, 31)]);
        let workday = days("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1", d(2026, 1, 30), d(2026, 1, 1), d(2026, 5, 31));
        assert_eq!(workday, vec![d(2026, 1, 30), d(2026, 2, 27), d(2026, 3, 31), d(2026, 4, 30), d(2026, 5, 29)]);
        let two = days("FREQ=MONTHLY;BYMONTHDAY=1,15", d(2026, 1, 1), d(2026, 1, 1), d(2026, 2, 28));
        assert_eq!(two, vec![d(2026, 1, 1), d(2026, 1, 15), d(2026, 2, 1), d(2026, 2, 15)]);
    }

    #[test]
    fn yearly_rules() {
        let thanksgiving = days("FREQ=YEARLY;BYMONTH=11;BYDAY=4TH", d(2026, 11, 26), d(2026, 1, 1), d(2029, 12, 31));
        assert_eq!(thanksgiving, vec![d(2026, 11, 26), d(2027, 11, 25), d(2028, 11, 23), d(2029, 11, 22)]);
        let leap = days("FREQ=YEARLY", d(2024, 2, 29), d(2024, 1, 1), d(2032, 12, 31));
        assert_eq!(leap, vec![d(2024, 2, 29), d(2028, 2, 29), d(2032, 2, 29)]);
        // Far from the start without COUNT: walked from near `from`, not from the start.
        assert_eq!(days("FREQ=YEARLY", d(1990, 5, 1), d(2026, 1, 1), d(2026, 12, 31)), vec![d(2026, 5, 1)]);
        assert_eq!(days("FREQ=DAILY;INTERVAL=3", d(2000, 1, 1), d(2026, 1, 1), d(2026, 1, 7)), vec![d(2026, 1, 2), d(2026, 1, 5)]);
    }

    #[test]
    fn until_and_exclusions() {
        let got = days("FREQ=WEEKLY;BYDAY=MO,TH;UNTIL=20261119;X-EXDATE=20261112", d(2026, 11, 2), d(2026, 11, 1), d(2026, 12, 31));
        assert_eq!(got, vec![d(2026, 11, 2), d(2026, 11, 5), d(2026, 11, 9), d(2026, 11, 16), d(2026, 11, 19)]);
        let r = parse("FREQ=WEEKLY;BYDAY=MO,TH;COUNT=4;X-EXDATE=20261105").unwrap();
        let start = NaiveDateTime::new(d(2026, 11, 2), chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap());
        // The excluded Thursday still uses up one of the four.
        let walls: Vec<NaiveDate> = occurrence_walls(&r, start, d(2026, 12, 31)).into_iter().map(|w| w.date()).collect();
        assert_eq!(walls, vec![d(2026, 11, 2), d(2026, 11, 9), d(2026, 11, 12)]);
        assert!(occurs_on(&r, d(2026, 11, 2), d(2026, 11, 12)));
        assert!(!occurs_on(&r, d(2026, 11, 2), d(2026, 11, 5)));
        assert!(!occurs_on(&r, d(2026, 11, 2), d(2026, 11, 16)));
        assert!(parse("FREQ=HOURLY").is_none(), "shown once, not expanded");
    }
}
