//! Repeat rules (`RRULE`) for showing repeating events: FREQ DAILY / WEEKLY / MONTHLY / YEARLY
//! with INTERVAL, COUNT, UNTIL, BYDAY (weekly) and the excluded dates CalendarCraft keeps as
//! `;X-EXDATE=YYYYMMDD,…`. Other parts (BYSETPOS, BYMONTHDAY lists…) fall back to the start day's
//! pattern.

use chrono::{Datelike, Duration, NaiveDate, Weekday};

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub freq: Freq,
    pub interval: u32,
    pub count: Option<u32>,
    pub until: Option<NaiveDate>,
    pub by_day: Vec<Weekday>,
    pub except: Vec<NaiveDate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

pub fn parse(rule: &str) -> Option<Rule> {
    let mut out = Rule { freq: Freq::Daily, interval: 1, count: None, until: None, by_day: Vec::new(), except: Vec::new() };
    let mut freq = None;
    for part in rule.split(';') {
        let Some((k, v)) = part.split_once('=') else { continue };
        match k.trim().to_ascii_uppercase().as_str() {
            "FREQ" => {
                freq = match v.trim().to_ascii_uppercase().as_str() {
                    "DAILY" => Some(Freq::Daily),
                    "WEEKLY" => Some(Freq::Weekly),
                    "MONTHLY" => Some(Freq::Monthly),
                    "YEARLY" => Some(Freq::Yearly),
                    _ => None,
                }
            }
            "INTERVAL" => out.interval = v.trim().parse().unwrap_or(1).max(1),
            "COUNT" => out.count = v.trim().parse().ok(),
            "UNTIL" => out.until = NaiveDate::parse_from_str(&v.trim()[..8.min(v.trim().len())], "%Y%m%d").ok(),
            "BYDAY" => {
                out.by_day = v
                    .split(',')
                    .filter_map(|d| {
                        let d = d.trim();
                        match &d[d.len().saturating_sub(2)..] {
                            "MO" => Some(Weekday::Mon),
                            "TU" => Some(Weekday::Tue),
                            "WE" => Some(Weekday::Wed),
                            "TH" => Some(Weekday::Thu),
                            "FR" => Some(Weekday::Fri),
                            "SA" => Some(Weekday::Sat),
                            "SU" => Some(Weekday::Sun),
                            _ => None,
                        }
                    })
                    .collect()
            }
            "X-EXDATE" => out.except = v.split(',').filter_map(|d| NaiveDate::parse_from_str(d.trim(), "%Y%m%d").ok()).collect(),
            _ => {}
        }
    }
    out.freq = freq?;
    Some(out)
}

fn months_between(a: NaiveDate, b: NaiveDate) -> i64 {
    (b.year() as i64 - a.year() as i64) * 12 + b.month() as i64 - a.month() as i64
}

/// Whether the pattern (ignoring COUNT) has an occurrence starting on `day`.
fn on_pattern(rule: &Rule, start: NaiveDate, day: NaiveDate) -> bool {
    if day < start {
        return false;
    }
    let interval = rule.interval as i64;
    match rule.freq {
        Freq::Daily => (day - start).num_days() % interval == 0,
        Freq::Weekly => {
            let days: &[Weekday] = if rule.by_day.is_empty() { &[] } else { &rule.by_day };
            let weekday_ok = if days.is_empty() { day.weekday() == start.weekday() } else { days.contains(&day.weekday()) };
            let week0 = start - Duration::days(start.weekday().num_days_from_monday() as i64);
            let week = day - Duration::days(day.weekday().num_days_from_monday() as i64);
            weekday_ok && ((week - week0).num_days() / 7) % interval == 0
        }
        Freq::Monthly => day.day() == start.day() && months_between(start, day) % interval == 0,
        Freq::Yearly => day.day() == start.day() && day.month() == start.month() && (day.year() - start.year()) as i64 % interval == 0,
    }
}

/// Whether an event starting on `start` with this rule has an occurrence starting on `day`.
pub fn occurs_on(rule: &Rule, start: NaiveDate, day: NaiveDate) -> bool {
    if day < start || rule.until.is_some_and(|u| day > u) || rule.except.contains(&day) {
        return false;
    }
    if !on_pattern(rule, start, day) {
        return false;
    }
    match rule.count {
        None => true,
        Some(count) => {
            // Count occurrences from the start up to `day` (counts are small in practice).
            let mut n = 0u32;
            let mut d = start;
            while d <= day {
                if on_pattern(rule, start, d) {
                    n += 1;
                    if n > count {
                        return false;
                    }
                }
                d += Duration::days(1);
                if (d - start).num_days() > 3660 {
                    break;
                }
            }
            n <= count
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
}
