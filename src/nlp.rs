use crate::model::{CalendarEvent, CalendarSource};
use chrono::{Datelike, Duration, NaiveDate, NaiveTime, Weekday};

#[derive(Debug, Clone)]
pub struct ParsedQuickEntry {
    pub title: String,
    pub date: NaiveDate,
    pub start_time: Option<NaiveTime>,
    pub end_time: Option<NaiveTime>,
    pub all_day: bool,
    pub location: Option<String>,
    pub calendar_id: Option<String>,
    pub alert_minutes_before: Option<u32>,
    pub is_task: bool,
    /// The sentence named a day (today, tomorrow, friday, a date…); otherwise `date` is the anchor.
    pub date_given: bool,
}

/// Clean-room natural-language quick-entry parser inspired by calendar sentence input.
/// Supports examples like:
/// - "Lunch with Maya tomorrow at 12:30pm-1:30pm at Blue Bottle /Personal alert 15m"
/// - "Sprint Planning next mon 9am for 90m @ Boardroom #Engineering"
/// - "todo Submit quarterly forecast friday 5pm /Engineering"
/// - "Company Offsite 2026-12-04 all day @ Napa"
pub fn parse_quick_entry(
    input: &str,
    anchor_date: NaiveDate,
    calendars: &[CalendarSource],
    default_calendar_id: &str,
) -> Option<ParsedQuickEntry> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut is_task = false;
    let mut working = trimmed.to_string();

    for prefix in ["todo ", "task ", "reminder ", "[] "] {
        if working.to_lowercase().starts_with(prefix) {
            is_task = true;
            working = working[prefix.len()..].trim().to_string();
            break;
        }
    }

    // Extract calendar selector `/CalName` or `#CalName`
    let mut chosen_cal: Option<String> = None;
    let mut tokens: Vec<String> = working.split_whitespace().map(|s| s.to_string()).collect();
    let mut kept_tokens = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];
        if (tok.starts_with('/') || tok.starts_with('#')) && tok.len() > 1 {
            let query = tok[1..].to_lowercase();
            if let Some(cal) = calendars
                .iter()
                .find(|c| c.name.to_lowercase().starts_with(&query) || c.id.to_lowercase() == query)
            {
                chosen_cal = Some(cal.id.clone());
                i += 1;
                continue;
            }
        }
        kept_tokens.push(tok.clone());
        i += 1;
    }
    tokens = kept_tokens;

    // Extract alert `alert 15m` or `remind 30m`
    let mut alert_minutes_before: Option<u32> = Some(10);
    let mut filtered = Vec::new();
    i = 0;
    while i < tokens.len() {
        let low = tokens[i].to_lowercase();
        if (low == "alert" || low == "remind") && i + 1 < tokens.len() {
            if let Some(mins) = parse_duration_minutes(&tokens[i + 1]) {
                alert_minutes_before = Some(mins);
                i += 2;
                continue;
            }
        }
        filtered.push(tokens[i].clone());
        i += 1;
    }
    tokens = filtered;

    // Extract duration `for 45m` / `for 2h`
    let mut explicit_duration_mins: Option<i64> = None;
    let mut filtered = Vec::new();
    i = 0;
    while i < tokens.len() {
        let low = tokens[i].to_lowercase();
        if low == "for" && i + 1 < tokens.len() {
            if let Some(mins) = parse_duration_minutes(&tokens[i + 1]) {
                explicit_duration_mins = Some(mins as i64);
                i += 2;
                continue;
            }
        }
        filtered.push(tokens[i].clone());
        i += 1;
    }
    tokens = filtered;

    // Extract date and time tokens
    let mut resolved_date = anchor_date;
    let mut date_given = false;
    let mut start_time: Option<NaiveTime> = None;
    let mut end_time: Option<NaiveTime> = None;
    let mut all_day = false;

    let mut remaining = Vec::new();
    i = 0;
    while i < tokens.len() {
        let low = tokens[i].to_lowercase().trim_matches(',').to_string();

        // "all day"
        if low == "all" && i + 1 < tokens.len() && tokens[i + 1].to_lowercase() == "day" {
            all_day = true;
            i += 2;
            continue;
        }
        if low == "all-day" {
            all_day = true;
            i += 1;
            continue;
        }

        // "today", "tomorrow", "tmrw"
        if low == "today" || low == "tod" {
            date_given = true;
            resolved_date = anchor_date;
            i += 1;
            continue;
        }
        if low == "tomorrow" || low == "tmrw" || low == "tom" {
            date_given = true;
            resolved_date = anchor_date + Duration::days(1);
            i += 1;
            continue;
        }

        // "in X days"
        if low == "in" && i + 2 < tokens.len() {
            if let Ok(n) = tokens[i + 1].parse::<i64>() {
                let unit = tokens[i + 2].to_lowercase();
                if unit.starts_with("day") {
                    date_given = true;
                    resolved_date = anchor_date + Duration::days(n);
                    i += 3;
                    continue;
                } else if unit.starts_with("week") {
                    date_given = true;
                    resolved_date = anchor_date + Duration::days(n * 7);
                    i += 3;
                    continue;
                }
            }
        }

        // "next <weekday>" or "on <weekday>"
        if (low == "next" || low == "on") && i + 1 < tokens.len() {
            if let Some(wd) = parse_weekday(&tokens[i + 1]) {
                date_given = true;
                resolved_date = anchor_date.next_weekday_occurrence(wd, low == "next");
                i += 2;
                continue;
            }
            if low == "on" {
                if let Ok(d) = NaiveDate::parse_from_str(&tokens[i + 1], "%Y-%m-%d") {
                    date_given = true;
                    resolved_date = d;
                    i += 2;
                    continue;
                }
            }
        }

        // Direct ISO date YYYY-MM-DD
        if let Ok(d) = NaiveDate::parse_from_str(&low, "%Y-%m-%d") {
            date_given = true;
            resolved_date = d;
            i += 1;
            continue;
        }

        // Direct weekday
        if let Some(wd) = parse_weekday(&low) {
            date_given = true;
            resolved_date = anchor_date.next_weekday_occurrence(wd, false);
            i += 1;
            continue;
        }

        // "at <time>" or "from <time> to <time>"
        if low == "from" && i + 3 < tokens.len() && tokens[i + 2].to_lowercase() == "to" {
            if let (Some(t1), Some(t2)) = (parse_time_token(&tokens[i + 1]), parse_time_token(&tokens[i + 3])) {
                start_time = Some(t1);
                end_time = Some(t2);
                i += 4;
                continue;
            }
        }

        if low == "at" && i + 1 < tokens.len() {
            if let Some((t1, t2_opt)) = parse_time_or_range(&tokens[i + 1]) {
                start_time = Some(t1);
                if t2_opt.is_some() {
                    end_time = t2_opt;
                }
                i += 2;
                continue;
            }
        }

        if let Some((t1, t2_opt)) = parse_time_or_range(&low) {
            start_time = Some(t1);
            if t2_opt.is_some() {
                end_time = t2_opt;
            }
            i += 1;
            continue;
        }

        remaining.push(tokens[i].clone());
        i += 1;
    }

    // Extract location via `@ Location` or trailing `at Location`
    let joined = remaining.join(" ");
    let (title_raw, location) = if let Some(idx) = joined.find(" @ ") {
        let t = joined[..idx].trim().to_string();
        let loc = joined[idx + 3..].trim().to_string();
        (t, if loc.is_empty() { None } else { Some(loc) })
    } else if let Some(idx) = joined.rfind(" at ") {
        let t = joined[..idx].trim().to_string();
        let loc = joined[idx + 4..].trim().to_string();
        (t, if loc.is_empty() { None } else { Some(loc) })
    } else if let Some(idx) = joined.rfind(" in ") {
        let candidate_loc = joined[idx + 4..].trim();
        if !candidate_loc.is_empty() && candidate_loc.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
            (joined[..idx].trim().to_string(), Some(candidate_loc.to_string()))
        } else {
            (joined.trim().to_string(), None)
        }
    } else {
        (joined.trim().to_string(), None)
    };

    let title = if title_raw.is_empty() {
        "New Event".to_string()
    } else {
        title_raw
    };

    if all_day {
        start_time = None;
        end_time = None;
    } else if let Some(st) = start_time {
        if end_time.is_none() {
            let dur = explicit_duration_mins.unwrap_or(60);
            let mins_total = (st.hour() as i64 * 60 + st.minute() as i64 + dur).clamp(0, 23 * 60 + 59);
            end_time = NaiveTime::from_hms_opt((mins_total / 60) as u32, (mins_total % 60) as u32, 0);
        }
    } else if !is_task {
        // Default timed event at 10:00-11:00 if neither time nor all-day specified
        start_time = NaiveTime::from_hms_opt(10, 0, 0);
        let dur = explicit_duration_mins.unwrap_or(60);
        let end_m = (10 * 60 + dur).clamp(0, 23 * 60 + 59);
        end_time = NaiveTime::from_hms_opt((end_m / 60) as u32, (end_m % 60) as u32, 0);
    }

    Some(ParsedQuickEntry {
        date_given,
        title,
        date: resolved_date,
        start_time,
        end_time,
        all_day,
        location,
        calendar_id: Some(chosen_cal.unwrap_or_else(|| default_calendar_id.to_string())),
        alert_minutes_before,
        is_task,
    })
}

impl ParsedQuickEntry {
    pub fn into_event(self, id: String, fallback_cal: &str) -> CalendarEvent {
        CalendarEvent {
            id,
            calendar_id: self.calendar_id.unwrap_or_else(|| fallback_cal.to_string()),
            title: self.title,
            date: self.date,
            start_time: self.start_time,
            end_time: self.end_time,
            all_day: self.all_day,
            location: self.location,
            conference_url: None,
            notes: String::new(),
            attendees: Vec::new(),
            alert_minutes_before: self.alert_minutes_before,
            ..Default::default()
        }
    }
}

fn parse_duration_minutes(tok: &str) -> Option<u32> {
    let s = tok.trim().to_lowercase();
    if let Some(num) = s.strip_suffix("min").or_else(|| s.strip_suffix("mins")).or_else(|| s.strip_suffix('m')) {
        num.parse::<u32>().ok()
    } else if let Some(num) = s.strip_suffix("hr").or_else(|| s.strip_suffix("hrs")).or_else(|| s.strip_suffix('h')) {
        num.parse::<f32>().ok().map(|h| (h * 60.0) as u32)
    } else {
        s.parse::<u32>().ok()
    }
}

fn parse_weekday(tok: &str) -> Option<Weekday> {
    match tok.trim_matches(',').to_lowercase().as_str() {
        "mon" | "monday" => Some(Weekday::Mon),
        "tue" | "tues" | "tuesday" => Some(Weekday::Tue),
        "wed" | "wednesday" => Some(Weekday::Wed),
        "thu" | "thur" | "thurs" | "thursday" => Some(Weekday::Thu),
        "fri" | "friday" => Some(Weekday::Fri),
        "sat" | "saturday" => Some(Weekday::Sat),
        "sun" | "sunday" => Some(Weekday::Sun),
        _ => None,
    }
}

fn parse_time_or_range(tok: &str) -> Option<(NaiveTime, Option<NaiveTime>)> {
    let clean = tok.trim_matches(',');
    if let Some((left, right)) = clean.split_once('-') {
        let t1 = parse_time_token(left)?;
        let t2 = parse_time_token(right)?;
        Some((t1, Some(t2)))
    } else {
        let t1 = parse_time_token(clean)?;
        Some((t1, None))
    }
}

pub fn parse_time_token(tok: &str) -> Option<NaiveTime> {
    let s = tok.trim().to_lowercase();
    if s == "noon" {
        return NaiveTime::from_hms_opt(12, 0, 0);
    }
    if s == "midnight" {
        return NaiveTime::from_hms_opt(0, 0, 0);
    }

    let (body, meridiem) = if let Some(b) = s.strip_suffix("am") {
        (b, Some(false))
    } else if let Some(b) = s.strip_suffix("pm") {
        (b, Some(true))
    } else {
        (s.as_str(), None)
    };

    if let Some((h_str, m_str)) = body.split_once(':') {
        let mut h = h_str.parse::<u32>().ok()?;
        let m = m_str.parse::<u32>().ok()?;
        if m >= 60 {
            return None;
        }
        if let Some(is_pm) = meridiem {
            if h == 0 || h > 12 {
                return None;
            }
            if is_pm && h < 12 {
                h += 12;
            } else if !is_pm && h == 12 {
                h = 0;
            }
        } else if h > 23 {
            return None;
        }
        NaiveTime::from_hms_opt(h, m, 0)
    } else if let Some(is_pm) = meridiem {
        let mut h = body.parse::<u32>().ok()?;
        if h == 0 || h > 12 {
            return None;
        }
        if is_pm && h < 12 {
            h += 12;
        } else if !is_pm && h == 12 {
            h = 0;
        }
        NaiveTime::from_hms_opt(h, 0, 0)
    } else {
        None
    }
}

use chrono::Timelike;

pub trait NaiveDateExt {
    fn next_weekday_occurrence(&self, target: Weekday, force_next_week: bool) -> NaiveDate;
}

impl NaiveDateExt for NaiveDate {
    fn next_weekday_occurrence(&self, target: Weekday, force_next_week: bool) -> NaiveDate {
        let cur = self.weekday().num_days_from_monday() as i64;
        let tgt = target.num_days_from_monday() as i64;
        let mut diff = (tgt - cur).rem_euclid(7);
        if diff == 0 && force_next_week {
            diff = 7;
        } else if force_next_week && diff < 7 {
            diff += 7;
        }
        *self + Duration::days(diff)
    }
}
