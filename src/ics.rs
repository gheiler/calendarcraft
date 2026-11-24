//! iCalendar (RFC 5545) reading and writing for the parts CalendarCraft shows: `VEVENT`s with
//! their summary, times, location, description, URL, attendees, first alarm and repeat rule.
//!
//! Times: `…Z` (UTC) values are converted into the local zone; values with a `TZID` and floating
//! values are taken as local wall-clock time (no time-zone database is bundled, to keep the
//! WebAssembly small). Written events use UTC for timed events and `VALUE=DATE` for all-day ones.

use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};

use crate::model::CalendarEvent;

/// What an `.ics` file holds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IcsCalendar {
    /// `X-WR-CALNAME`
    pub name: Option<String>,
    /// `X-APPLE-CALENDAR-COLOR` or `COLOR`
    pub color: Option<String>,
    pub events: Vec<CalendarEvent>,
}

/// One content line: `NAME;PARAM=V:VALUE`.
#[derive(Debug, Clone)]
struct Line {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

impl Line {
    fn param(&self, key: &str) -> Option<&str> {
        self.params.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
    }
}

/// Undo line folding (a line break followed by a space or tab continues the line).
fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix(' ').or_else(|| line.strip_prefix('\t')) {
            if let Some(last) = out.last_mut() {
                last.push_str(rest);
                continue;
            }
        }
        if !line.is_empty() {
            out.push(line.to_string());
        }
    }
    out
}

fn parse_line(line: &str) -> Option<Line> {
    // The value starts at the first ':' outside double quotes.
    let mut in_quotes = false;
    let mut split = None;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            ':' if !in_quotes => {
                split = Some(i);
                break;
            }
            _ => {}
        }
    }
    let at = split?;
    let (head, value) = (&line[..at], &line[at + 1..]);
    let mut parts = Vec::new();
    let mut current = String::new();
    in_quotes = false;
    for c in head.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            ';' if !in_quotes => parts.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    parts.push(current);
    let name = parts.first()?.trim().to_ascii_uppercase();
    let params = parts[1..]
        .iter()
        .filter_map(|p| p.split_once('=').map(|(k, v)| (k.trim().to_ascii_uppercase(), v.trim().to_string())))
        .collect();
    Some(Line { name, params, value: value.to_string() })
}

pub fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            other => out.push(other),
        }
    }
    out
}

/// A parsed `DTSTART`/`DTEND` value.
#[derive(Debug, Clone, Copy, PartialEq)]
enum When {
    Date(NaiveDate),
    Local(NaiveDateTime),
}

fn parse_when(line: &Line, to_local: &dyn Fn(DateTime<Utc>) -> NaiveDateTime) -> Option<When> {
    let v = line.value.trim();
    let is_date = line.param("VALUE").is_some_and(|x| x.eq_ignore_ascii_case("DATE")) || (v.len() == 8 && !v.contains('T'));
    if is_date {
        return NaiveDate::parse_from_str(&v[..8.min(v.len())], "%Y%m%d").ok().map(When::Date);
    }
    if let Some(utc) = v.strip_suffix('Z').or_else(|| v.strip_suffix('z')) {
        let naive = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").ok()?;
        return Some(When::Local(to_local(Utc.from_utc_datetime(&naive))));
    }
    NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S").ok().map(When::Local)
}

/// `-PT15M`, `PT1H30M`, `P1D`, `-P1DT2H` → minutes (negative before).
pub fn parse_duration(text: &str) -> Option<i64> {
    let t = text.trim();
    let (sign, rest) = match t.strip_prefix('-') {
        Some(r) => (-1, r),
        None => (1, t.strip_prefix('+').unwrap_or(t)),
    };
    let rest = rest.strip_prefix('P')?;
    let mut minutes = 0i64;
    let mut number = String::new();
    let mut in_time = false;
    for c in rest.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => number.push(c),
            _ => {
                let n: i64 = number.parse().ok()?;
                number.clear();
                minutes += match (c, in_time) {
                    ('W', _) => n * 7 * 24 * 60,
                    ('D', _) => n * 24 * 60,
                    ('H', true) => n * 60,
                    ('M', true) => n,
                    ('S', true) => n / 60,
                    _ => return None,
                };
            }
        }
    }
    Some(sign * minutes)
}

fn local_from_utc(utc: DateTime<Utc>) -> NaiveDateTime {
    utc.with_timezone(&Local).naive_local()
}

/// Read every `VEVENT` of an `.ics` text, in the local time zone.
pub fn parse(text: &str, calendar_id: &str) -> Result<IcsCalendar, String> {
    parse_with(text, calendar_id, &local_from_utc)
}

/// [`parse`] with the UTC → local conversion given (tests use a fixed offset).
pub fn parse_with(text: &str, calendar_id: &str, to_local: &dyn Fn(DateTime<Utc>) -> NaiveDateTime) -> Result<IcsCalendar, String> {
    let lines = unfold(text);
    if !lines.iter().any(|l| l.trim().eq_ignore_ascii_case("BEGIN:VCALENDAR")) {
        return Err("not an iCalendar file (no BEGIN:VCALENDAR)".into());
    }
    let mut cal = IcsCalendar::default();
    let mut stack: Vec<String> = Vec::new();
    let mut current: Option<Vec<Line>> = None;
    let mut alarm_trigger: Option<String> = None;
    for raw in &lines {
        let Some(line) = parse_line(raw) else { continue };
        match line.name.as_str() {
            "BEGIN" => {
                let what = line.value.trim().to_ascii_uppercase();
                if what == "VEVENT" && stack.last().map(String::as_str) == Some("VCALENDAR") {
                    current = Some(Vec::new());
                    alarm_trigger = None;
                }
                stack.push(what);
                continue;
            }
            "END" => {
                let what = line.value.trim().to_ascii_uppercase();
                stack.pop();
                if what == "VEVENT" {
                    if let Some(props) = current.take() {
                        if let Some(ev) = build_event(&props, alarm_trigger.take(), calendar_id, cal.events.len(), to_local) {
                            cal.events.push(ev);
                        }
                    }
                }
                continue;
            }
            _ => {}
        }
        match stack.last().map(String::as_str) {
            Some("VEVENT") => {
                if let Some(props) = current.as_mut() {
                    props.push(line);
                }
            }
            Some("VALARM") => {
                if line.name == "TRIGGER" && alarm_trigger.is_none() {
                    alarm_trigger = Some(line.value.clone());
                }
            }
            Some("VCALENDAR") => match line.name.as_str() {
                "X-WR-CALNAME" | "NAME" => cal.name = Some(unescape(&line.value)),
                "X-APPLE-CALENDAR-COLOR" | "COLOR" => cal.color = Some(line.value.trim().to_string()),
                _ => {}
            },
            _ => {}
        }
    }
    Ok(cal)
}

fn build_event(
    props: &[Line],
    trigger: Option<String>,
    calendar_id: &str,
    index: usize,
    to_local: &dyn Fn(DateTime<Utc>) -> NaiveDateTime,
) -> Option<CalendarEvent> {
    let get = |name: &str| props.iter().find(|l| l.name == name);
    // A modified single occurrence (RECURRENCE-ID) is shown as its own event.
    let start = parse_when(get("DTSTART")?, to_local)?;
    let end = get("DTEND").and_then(|l| parse_when(l, to_local));
    let duration = get("DURATION").and_then(|l| parse_duration(&l.value));
    let uid = get("UID").map(|l| l.value.trim().to_string()).unwrap_or_default();
    let mut ev = CalendarEvent {
        id: if uid.is_empty() { format!("ics_{index}") } else { format!("ics_{}", short_hash(&uid)) },
        calendar_id: calendar_id.to_string(),
        title: get("SUMMARY").map(|l| unescape(&l.value)).unwrap_or_else(|| "(No title)".into()),
        uid,
        location: get("LOCATION").map(|l| unescape(&l.value)).filter(|s| !s.trim().is_empty()),
        notes: get("DESCRIPTION").map(|l| unescape(&l.value)).unwrap_or_default(),
        conference_url: get("URL").map(|l| l.value.trim().to_string()).filter(|s| !s.is_empty()),
        rrule: get("RRULE").map(|l| l.value.trim().to_string()),
        attendees: props
            .iter()
            .filter(|l| l.name == "ATTENDEE")
            .map(|l| l.value.trim().trim_start_matches("mailto:").trim_start_matches("MAILTO:").to_string())
            .collect(),
        alert_minutes_before: trigger.and_then(|t| parse_duration(&t)).map(|m| m.unsigned_abs() as u32),
        ..Default::default()
    };
    if let Some(rid) = get("RECURRENCE-ID") {
        ev.id = format!("{}_{}", ev.id, short_hash(&rid.value));
        ev.rrule = None;
    }
    let exdates: Vec<String> = props.iter().filter(|l| l.name == "EXDATE").flat_map(|l| l.value.split(',').map(|v| v[..8.min(v.len())].to_string()).collect::<Vec<_>>()).collect();
    if let (Some(rule), false) = (&ev.rrule, exdates.is_empty()) {
        ev.rrule = Some(format!("{rule};X-EXDATE={}", exdates.join(",")));
    }
    match start {
        When::Date(day) => {
            ev.all_day = true;
            ev.date = day;
            let last = match end {
                Some(When::Date(e)) if e > day => e - Duration::days(1),
                Some(When::Local(e)) if e.date() > day => e.date() - Duration::days(1),
                _ => match duration {
                    Some(m) if m > 24 * 60 => day + Duration::minutes(m) - Duration::days(1),
                    _ => day,
                },
            };
            ev.end_date = (last > day).then_some(last);
        }
        When::Local(at) => {
            ev.date = at.date();
            ev.start_time = Some(at.time());
            let finish = match end {
                Some(When::Local(e)) => Some(e),
                Some(When::Date(e)) => e.and_hms_opt(0, 0, 0),
                None => duration.map(|m| at + Duration::minutes(m)),
            };
            if let Some(f) = finish.filter(|f| *f > at) {
                // An event ending at midnight belongs to the day before.
                let last_day = if f.time() == NaiveTime::MIN { f.date() - Duration::days(1) } else { f.date() };
                ev.end_time = Some(if f.time() == NaiveTime::MIN && last_day > at.date() { NaiveTime::from_hms_opt(23, 59, 0).unwrap() } else { f.time() });
                if last_day > at.date() {
                    ev.end_date = Some(last_day);
                } else if f.time() == NaiveTime::MIN {
                    ev.end_time = NaiveTime::from_hms_opt(23, 59, 0);
                }
            }
        }
    }
    Some(ev)
}

/// FNV-1a, hex: a short stable id from a UID.
pub fn short_hash(text: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// Fold a content line at 75 octets (never inside a UTF-8 character).
fn fold(line: &str, out: &mut String) {
    let mut count = 0;
    for c in line.chars() {
        let len = c.len_utf8();
        if count + len > 75 {
            out.push_str("\r\n ");
            count = 1;
        }
        out.push(c);
        count += len;
    }
    out.push_str("\r\n");
}

fn utc_stamp(local: NaiveDateTime) -> String {
    let utc = match Local.from_local_datetime(&local).earliest() {
        Some(dt) => dt.with_timezone(&Utc).naive_utc(),
        None => local,
    };
    utc.format("%Y%m%dT%H%M%SZ").to_string()
}

/// One `VEVENT` block (without the calendar around it).
pub fn write_event(ev: &CalendarEvent, out: &mut String, stamp: &str) {
    let mut line = |s: String| fold(&s, out);
    line("BEGIN:VEVENT".into());
    line(format!("UID:{}", ev.uid_or_id()));
    line(format!("DTSTAMP:{stamp}"));
    if ev.all_day || ev.start_time.is_none() {
        let last = ev.end_date.unwrap_or(ev.date).max(ev.date);
        line(format!("DTSTART;VALUE=DATE:{}", ev.date.format("%Y%m%d")));
        line(format!("DTEND;VALUE=DATE:{}", (last + Duration::days(1)).format("%Y%m%d")));
    } else {
        let start = ev.date.and_time(ev.start_time.unwrap_or(NaiveTime::MIN));
        let end_day = ev.end_date.unwrap_or(ev.date);
        let end = match ev.end_time {
            Some(t) => end_day.and_time(t),
            None => start + Duration::hours(1),
        };
        let end = if end <= start { start + Duration::minutes(30) } else { end };
        line(format!("DTSTART:{}", utc_stamp(start)));
        line(format!("DTEND:{}", utc_stamp(end)));
    }
    line(format!("SUMMARY:{}", escape(&ev.title)));
    if let Some(loc) = ev.location.as_deref().filter(|l| !l.is_empty()) {
        line(format!("LOCATION:{}", escape(loc)));
    }
    if !ev.notes.is_empty() {
        line(format!("DESCRIPTION:{}", escape(&ev.notes)));
    }
    if let Some(url) = ev.conference_url.as_deref().filter(|u| !u.is_empty()) {
        line(format!("URL:{url}"));
    }
    if let Some(rule) = &ev.rrule {
        let (rule, ex) = match rule.split_once(";X-EXDATE=") {
            Some((r, ex)) => (r.to_string(), Some(ex.to_string())),
            None => (rule.clone(), None),
        };
        line(format!("RRULE:{rule}"));
        if let Some(ex) = ex {
            line(format!("EXDATE;VALUE=DATE:{ex}"));
        }
    }
    for who in &ev.attendees {
        if who.contains('@') {
            line(format!("ATTENDEE:mailto:{who}"));
        }
    }
    if let Some(m) = ev.alert_minutes_before {
        line("BEGIN:VALARM".into());
        line("ACTION:DISPLAY".into());
        line(format!("DESCRIPTION:{}", escape(&ev.title)));
        line(format!("TRIGGER:-PT{m}M"));
        line("END:VALARM".into());
    }
    line("END:VEVENT".into());
}

fn header(out: &mut String, name: Option<&str>) {
    fold("BEGIN:VCALENDAR", out);
    fold("VERSION:2.0", out);
    fold("PRODID:-//CalendarCraft//CalendarCraft 0.2//EN", out);
    fold("CALSCALE:GREGORIAN", out);
    if let Some(name) = name {
        fold(&format!("X-WR-CALNAME:{}", escape(name)), out);
    }
}

/// A whole `.ics` file with these events.
pub fn write_calendar<'a>(name: Option<&str>, events: impl IntoIterator<Item = &'a CalendarEvent>) -> String {
    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let mut out = String::new();
    header(&mut out, name);
    for ev in events {
        write_event(ev, &mut out, &stamp);
    }
    fold("END:VCALENDAR", &mut out);
    out
}

/// A one-event calendar object, as CalDAV stores it.
pub fn write_single(ev: &CalendarEvent) -> String {
    write_calendar(None, std::iter::once(ev))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn berlin(utc: DateTime<Utc>) -> NaiveDateTime {
        utc.with_timezone(&FixedOffset::east_opt(3600).unwrap()).naive_local()
    }

    const SAMPLE: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nX-WR-CALNAME:Team\r\nBEGIN:VTIMEZONE\r\nTZID:Europe/Berlin\r\nBEGIN:STANDARD\r\nDTSTART:19701025T030000\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:a1@example\r\nDTSTART:20261124T090000Z\r\nDTEND:20261124T100000Z\r\nSUMMARY:Stand-up\\, daily\r\nLOCATION:Room 1\r\nDESCRIPTION:Line one\\nline two that is long enough to be folded over the seventy-five o\r\n ctet limit\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,TU\r\nBEGIN:VALARM\r\nTRIGGER:-PT15M\r\nACTION:DISPLAY\r\nEND:VALARM\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:b2@example\r\nDTSTART;VALUE=DATE:20261127\r\nDTEND;VALUE=DATE:20261130\r\nSUMMARY:Trip\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:c3\r\nDTSTART;TZID=Europe/Berlin:20261201T140000\r\nDURATION:PT45M\r\nSUMMARY:Review\r\nATTENDEE;CN=\"Ada: L\";ROLE=REQ-PARTICIPANT:mailto:ada@example.org\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn reads_events_times_and_text() {
        let cal = parse_with(SAMPLE, "c", &berlin).unwrap();
        assert_eq!(cal.name.as_deref(), Some("Team"));
        assert_eq!(cal.events.len(), 3);
        let a = &cal.events[0];
        assert_eq!(a.title, "Stand-up, daily");
        assert_eq!(a.start_time, NaiveTime::from_hms_opt(10, 0, 0));
        assert_eq!(a.end_time, NaiveTime::from_hms_opt(11, 0, 0));
        assert!(a.notes.starts_with("Line one\nline two") && a.notes.ends_with("octet limit"), "{}", a.notes);
        assert_eq!(a.alert_minutes_before, Some(15));
        assert_eq!(a.rrule.as_deref(), Some("FREQ=WEEKLY;BYDAY=MO,TU"));
        let b = &cal.events[1];
        assert!(b.all_day);
        assert_eq!(b.date, NaiveDate::from_ymd_opt(2026, 11, 27).unwrap());
        assert_eq!(b.end_date, NaiveDate::from_ymd_opt(2026, 11, 29));
        let c = &cal.events[2];
        assert_eq!(c.start_time, NaiveTime::from_hms_opt(14, 0, 0));
        assert_eq!(c.end_time, NaiveTime::from_hms_opt(14, 45, 0));
        assert_eq!(c.attendees, vec!["ada@example.org".to_string()]);
    }

    #[test]
    fn round_trips_through_writing() {
        let cal = parse_with(SAMPLE, "c", &berlin).unwrap();
        let text = write_calendar(Some("Team"), &cal.events);
        assert!(text.lines().all(|l| l.trim_end_matches('\r').len() <= 75));
        let again = parse(&text, "c").unwrap();
        assert_eq!(again.events.len(), 3);
        for (a, b) in cal.events.iter().zip(&again.events) {
            assert_eq!(a.title, b.title);
            assert_eq!(a.uid, b.uid);
            assert_eq!(a.all_day, b.all_day);
            assert_eq!(a.end_date, b.end_date);
            assert_eq!(a.notes, b.notes);
            assert_eq!(a.rrule, b.rrule);
            assert_eq!(a.alert_minutes_before, b.alert_minutes_before);
        }
    }

    #[test]
    fn rejects_other_text_and_parses_durations() {
        assert!(parse("hello", "c").is_err());
        assert_eq!(parse_duration("-PT15M"), Some(-15));
        assert_eq!(parse_duration("P1DT2H"), Some(26 * 60));
        assert_eq!(parse_duration("-P1W"), Some(-7 * 24 * 60));
        assert_eq!(parse_duration("15M"), None);
    }
}
