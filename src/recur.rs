//! Recurrence expansion on top of `craft-ical`'s [`Recurrence`]: whether an event occurs on a
//! given day (floating / all-day) or at a given instant (timed, zoned).
//!
//! Parsing and formatting of the rule text itself is done by `craft-ical`. This module only
//! decides which days or instants the rule produces, including `COUNT`, `UNTIL` and the
//! excluded dates CalendarCraft used to keep as `;X-EXDATE=…`.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Weekday};
use craft_ical::{parse_rrule, values::When, Freq, Recurrence};

use crate::tz::{self, DisplayTz, EventZone};

/// A parsed rule plus the exclusions and a UTC `UNTIL` instant when the text had one.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub rec: Recurrence,
    pub except: Vec<NaiveDateTime>,
    /// When `UNTIL` was a UTC date-time, the last instant that is still included.
    pub until_utc: Option<i64>,
}

pub fn parse(rule: &str) -> Option<Rule> {
    let (core, extra_except) = split_exdates(rule);
    let rec = parse_rrule(core)?;
    let until_utc = until_utc_instant(core);
    let mut except = extra_except;
    except.extend(exdates_from_rule(core));
    Some(Rule {
        rec,
        except,
        until_utc,
    })
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

fn exdates_from_rule(_rule: &str) -> Vec<NaiveDateTime> {
    Vec::new()
}

fn until_utc_instant(rule: &str) -> Option<i64> {
    for part in rule.split(';') {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        if !k.trim().eq_ignore_ascii_case("UNTIL") {
            continue;
        }
        if let Some(When::Utc(t)) = When::parse(v.trim()) {
            return Some(t.and_utc().timestamp());
        }
    }
    None
}

fn months_between(a: NaiveDate, b: NaiveDate) -> i64 {
    (b.year() as i64 - a.year() as i64) * 12 + b.month() as i64 - a.month() as i64
}

fn nth_weekday(year: i32, month: u32, weekday: Weekday, nth: i8) -> Option<NaiveDate> {
    let first = NaiveDate::from_ymd_opt(year, month, 1)?;
    if nth > 0 {
        let offset = (i64::from(weekday.num_days_from_monday())
            - i64::from(first.weekday().num_days_from_monday()))
        .rem_euclid(7);
        let day = 1 + offset + 7 * (i64::from(nth) - 1);
        NaiveDate::from_ymd_opt(year, month, u32::try_from(day).ok()?)
    } else {
        let last = first
            .checked_add_months(chrono::Months::new(1))?
            .pred_opt()?;
        let back = (i64::from(last.weekday().num_days_from_monday())
            - i64::from(weekday.num_days_from_monday()))
        .rem_euclid(7);
        let day = last.checked_sub_days(chrono::Days::new(
            u64::try_from(back + 7 * (i64::from(nth.saturating_neg()) - 1)).ok()?,
        ))?;
        (day.month() == month).then_some(day)
    }
}

/// Whether the pattern (ignoring COUNT and exclusions) has an occurrence starting on `day`.
fn on_pattern(rec: &Recurrence, start: NaiveDate, day: NaiveDate) -> bool {
    if day < start {
        return false;
    }
    let interval = rec.interval.max(1) as i64;
    match rec.freq {
        Freq::Daily => {
            let weekday_ok = rec.by_day.is_empty() || rec.by_day.contains(&day.weekday());
            weekday_ok && (day - start).num_days() % interval == 0
        }
        Freq::Weekly => {
            let weekday_ok = if rec.by_day.is_empty() {
                day.weekday() == start.weekday()
            } else {
                rec.by_day.contains(&day.weekday())
            };
            let week0 = start - Duration::days(start.weekday().num_days_from_monday() as i64);
            let week = day - Duration::days(day.weekday().num_days_from_monday() as i64);
            weekday_ok && ((week - week0).num_days() / 7) % interval == 0
        }
        Freq::Monthly => {
            if months_between(start, day) % interval != 0 {
                return false;
            }
            if let Some(nth) = rec.nth {
                let weekdays: &[Weekday] = if rec.by_day.is_empty() {
                    // The start's weekday, once the start is known.
                    return nth_weekday(day.year(), day.month(), start.weekday(), nth) == Some(day);
                } else {
                    &rec.by_day
                };
                return weekdays
                    .iter()
                    .any(|wd| nth_weekday(day.year(), day.month(), *wd, nth) == Some(day));
            }
            day.day() == start.day()
        }
        Freq::Yearly => {
            if (day.year() - start.year()) as i64 % interval != 0 {
                return false;
            }
            if let Some(nth) = rec.nth {
                let wd = rec.by_day.first().copied().unwrap_or(start.weekday());
                return day.month() == start.month()
                    && nth_weekday(day.year(), day.month(), wd, nth) == Some(day);
            }
            day.day() == start.day() && day.month() == start.month()
        }
    }
}

fn excluded_date(rule: &Rule, day: NaiveDate) -> bool {
    rule.except.iter().any(|t| t.date() == day)
}

/// Whether an event starting on `start` with this rule has an occurrence starting on `day`
/// (floating or all-day dates: no zone is applied).
pub fn occurs_on(rule: &Rule, start: NaiveDate, day: NaiveDate) -> bool {
    if day < start || rule.rec.until.is_some_and(|u| day > u) || excluded_date(rule, day) {
        return false;
    }
    if !on_pattern(&rule.rec, start, day) {
        return false;
    }
    match rule.rec.count {
        None => true,
        Some(count) => count_up_to(&rule.rec, start, day) <= count,
    }
}

fn count_up_to(rec: &Recurrence, start: NaiveDate, day: NaiveDate) -> u32 {
    let mut n = 0u32;
    let mut d = start;
    while d <= day {
        if on_pattern(rec, start, d) {
            n = n.saturating_add(1);
        }
        d += Duration::days(1);
        if (d - start).num_days() > 3660 {
            break;
        }
    }
    n
}

/// Next pattern day on or after `from`, or `None` when the search is exhausted.
fn next_pattern_day(rec: &Recurrence, start: NaiveDate, from: NaiveDate) -> Option<NaiveDate> {
    let mut d = from.max(start);
    for _ in 0..3660 {
        if on_pattern(rec, start, d) {
            return Some(d);
        }
        d += Duration::days(1);
    }
    None
}

/// Each occurrence start in the event's own wall clock, until `end_day` (inclusive, event-zone dates)
/// or `COUNT` / `UNTIL`.
pub fn occurrence_walls(
    rule: &Rule,
    start: NaiveDateTime,
    end_day: NaiveDate,
) -> Vec<NaiveDateTime> {
    let mut out = Vec::new();
    let mut day = start.date();
    let mut n = 0u32;
    while let Some(d) = next_pattern_day(&rule.rec, start.date(), day) {
        if d > end_day {
            break;
        }
        if rule.rec.until.is_some_and(|u| d > u) {
            break;
        }
        let wall = d.and_time(start.time());
        if !rule.except.iter().any(|ex| {
            if ex.time() == chrono::NaiveTime::MIN && start.time() != chrono::NaiveTime::MIN {
                ex.date() == d
            } else {
                *ex == wall || ex.date() == d && ex.time() == chrono::NaiveTime::MIN
            }
        }) {
            n += 1;
            if rule.rec.count.is_some_and(|c| n > c) {
                break;
            }
            out.push(wall);
        }
        day = d + Duration::days(1);
        if (day - start.date()).num_days() > 3660 {
            break;
        }
    }
    out
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
}
