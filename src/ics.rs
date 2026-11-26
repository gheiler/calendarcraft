//! iCalendar (RFC 5545) reading and writing, built on `craft-ical`.
//!
//! Times:
//! - `…Z` (UTC) is kept as UTC and converted into the display zone when shown.
//! - `TZID` uses a `VTIMEZONE` in the text when one is present, otherwise an IANA name
//!   (`chrono-tz`).
//! - Floating times (no zone) and all-day dates are shown as written, in every zone.
//!
//! Written events keep that distinction: UTC stays `…Z`, floating stays a bare date-time,
//! a named zone is written with `TZID` and a `VTIMEZONE`, and all-day values use `VALUE=DATE`.

use chrono::{Datelike, Duration, NaiveDateTime, NaiveTime, Utc};
use craft_ical::{
    fold, format_rrule, param_value, parse_rrule,
    values::{self, escape_text, format_date, format_local, format_utc, unescape_text, When},
    write_vtimezone, ContentLine, Lines, TimeZones,
};

use crate::model::CalendarEvent;
use crate::recur;
use crate::tz::{self, DisplayTz, EventZone, IanaRules, ZoneLookup};

/// What an `.ics` file holds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IcsCalendar {
    /// `X-WR-CALNAME`
    pub name: Option<String>,
    /// `X-APPLE-CALENDAR-COLOR` or `COLOR`
    pub color: Option<String>,
    pub events: Vec<CalendarEvent>,
}

/// A duration in minutes, for alarms and tests that count in minutes.
/// The underlying parser (`craft-ical`) returns seconds.
pub fn parse_duration(text: &str) -> Option<i64> {
    values::parse_duration(text).map(|secs| secs / 60)
}

/// Read every `VEVENT` of an `.ics` text. Timed values keep their zone; convert them with
/// [`CalendarEvent::projected`] / [`covers_in`] for display.
pub fn parse(text: &str, calendar_id: &str) -> Result<IcsCalendar, String> {
    if !text
        .as_bytes()
        .windows(15)
        .any(|w| w.eq_ignore_ascii_case(b"BEGIN:VCALENDAR"))
    {
        return Err("not an iCalendar file (no BEGIN:VCALENDAR)".into());
    }
    let zones = TimeZones::collect(text);
    let lookup = ZoneLookup::new(&zones);
    let mut cal = IcsCalendar::default();
    let mut stack: Vec<String> = Vec::new();
    let mut current: Option<Vec<ContentLine>> = None;
    let mut alarm_trigger: Option<String> = None;
    // (UID, RECURRENCE-ID) of each changed occurrence, to take it out of its series below.
    let mut overrides: Vec<(String, ContentLine)> = Vec::new();
    for raw in Lines::new(text) {
        let Some(line) = ContentLine::parse(&raw) else {
            continue;
        };
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
                        if let Some(ev) = build_event(
                            &props,
                            alarm_trigger.take(),
                            calendar_id,
                            cal.events.len(),
                            &lookup,
                        ) {
                            if let Some(rid) = props.iter().find(|l| l.name == "RECURRENCE-ID") {
                                overrides.push((ev.uid.clone(), rid.clone()));
                            }
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
                "X-WR-CALNAME" | "NAME" => cal.name = Some(unescape_text(&line.value)),
                "X-APPLE-CALENDAR-COLOR" | "COLOR" => {
                    cal.color = Some(line.value.trim().to_string())
                }
                _ => {}
            },
            _ => {}
        }
    }
    // A changed occurrence (RECURRENCE-ID) replaces the one its series would make there, so the
    // series must not show that one too. Kept apart from EXDATE: it is not written back.
    for (uid, rid) in overrides {
        let Some(when) = when_of(&rid) else { continue };
        if let Some(master) = cal
            .events
            .iter_mut()
            .find(|e| e.uid == uid && e.rrule.is_some())
        {
            let wall = wall_in(&rid, when, &master.zone, &lookup);
            if !master.overridden.contains(&wall) {
                master.overridden.push(wall);
            }
        }
    }
    let _ = zones.stopped_after();
    Ok(cal)
}

fn when_of(line: &ContentLine) -> Option<When> {
    When::parse(line.value.trim())
}

/// What a `TZID` in the text means.
enum ZoneSrc {
    /// An IANA zone (the name itself, a known alias, the `VTIMEZONE`'s `X-LIC-LOCATION`, or the
    /// device's zone when the `VTIMEZONE` keeps the same clock).
    Iana(chrono_tz::Tz),
    /// Only the `VTIMEZONE` in the text knows it: times are converted to UTC with it.
    Ics(String),
    /// Nothing knows it: the time is taken as written (floating).
    Unknown,
}

fn resolve_zone(tzid: &str, year: i32, lookup: &ZoneLookup<'_>) -> ZoneSrc {
    if let Some(tz) = tz::resolve_tzid(tzid) {
        return ZoneSrc::Iana(tz);
    }
    let Some(def) = lookup.get_ics(tzid) else {
        return ZoneSrc::Unknown;
    };
    if let Some(tz) = tz::resolve_tzid(def.place()) {
        return ZoneSrc::Iana(tz);
    }
    // A name only the text defines (`Mitteleuropäische Zeit`, a custom Exchange name): when it
    // keeps the device's clock all year, it is the device's zone, so repeats follow its DST.
    if let Some(system) = tz::system_zone() {
        let same = (1..=12).all(|month| {
            [1, 15].iter().all(|day| {
                let Some(at) = chrono::NaiveDate::from_ymd_opt(year, month, *day)
                    .and_then(|d| d.and_hms_opt(12, 0, 0))
                else {
                    return true;
                };
                let instant = at.and_utc().timestamp();
                lookup.offset_at(tzid, instant) == Some(DisplayTz::Iana(system).offset_at(instant))
            })
        });
        if same {
            return ZoneSrc::Iana(system);
        }
    }
    ZoneSrc::Ics(tzid.to_string())
}

/// The zone a start written like this is kept in.
fn zone_of(line: &ContentLine, when: When, lookup: &ZoneLookup<'_>) -> EventZone {
    match when {
        When::Utc(_) => EventZone::Utc,
        When::Date(_) => EventZone::Floating,
        When::Local(t) => match line.param("TZID").filter(|z| !z.trim().is_empty()) {
            None => EventZone::Floating,
            Some(tzid) => match resolve_zone(tzid, t.year(), lookup) {
                ZoneSrc::Iana(tz) => EventZone::Tz(tz.name().to_string()),
                ZoneSrc::Ics(_) => EventZone::Utc,
                ZoneSrc::Unknown => EventZone::Floating,
            },
        },
    }
}

/// The instant a value names, when it names one (`None` for dates and floating times).
fn instant_of(when: When, tzid: Option<&str>, lookup: &ZoneLookup<'_>) -> Option<i64> {
    match when {
        When::Utc(t) => Some(t.and_utc().timestamp()),
        When::Date(_) => None,
        When::Local(t) => {
            let tzid = tzid.filter(|z| !z.trim().is_empty())?;
            match resolve_zone(tzid, t.year(), lookup) {
                ZoneSrc::Iana(tz) => Some(DisplayTz::Iana(tz).instant_of(t)),
                ZoneSrc::Ics(name) => lookup.instant_of(&name, t),
                ZoneSrc::Unknown => None,
            }
        }
    }
}

/// A value of `line` as a wall clock in `target` (the zone the event is kept in). A value
/// already in that zone is kept exactly as written; a value in another zone is converted
/// through its instant; a floating value or a date is taken as written.
fn wall_in(line: &ContentLine, when: When, target: &EventZone, lookup: &ZoneLookup<'_>) -> NaiveDateTime {
    let own = zone_of(line, when, lookup);
    match (when, &own) {
        // Written in the kept zone: exactly as written (a zone only the VTIMEZONE knows is
        // kept as UTC, so its local times still go through the conversion below).
        (When::Utc(t), EventZone::Utc) | (When::Local(t), EventZone::Tz(_) | EventZone::Floating)
            if &own == target =>
        {
            return t;
        }
        _ => {}
    }
    if let When::Date(d) = when {
        return d.and_time(NaiveTime::MIN);
    }
    match (instant_of(when, line.param("TZID"), lookup), target) {
        (Some(instant), EventZone::Utc | EventZone::Tz(_)) => tz::wall_in_event_zone(target, instant),
        // A zoned value in a floating event: its wall clock in the device's zone.
        (Some(instant), EventZone::Floating) => DisplayTz::Local.wall_at(instant),
        (None, _) => when.wall(),
    }
}

fn end_wall_from_duration(start: NaiveDateTime, duration_minutes: i64) -> NaiveDateTime {
    start + Duration::minutes(duration_minutes)
}

fn build_event(
    props: &[ContentLine],
    trigger: Option<String>,
    calendar_id: &str,
    index: usize,
    lookup: &ZoneLookup<'_>,
) -> Option<CalendarEvent> {
    let get = |name: &str| props.iter().find(|l| l.name == name);
    let start_line = get("DTSTART")?;
    let start_when = when_of(start_line)?;
    let zone = zone_of(start_line, start_when, lookup);
    let start = wall_in(start_line, start_when, &zone, lookup);
    let end_line = get("DTEND");
    let end_when = end_line.and_then(when_of);
    let duration = get("DURATION").and_then(|l| parse_duration(&l.value));
    let uid = get("UID")
        .map(|l| l.value.trim().to_string())
        .unwrap_or_default();
    let (core_rrule, mut exdates) = get("RRULE")
        .map(|l| {
            let (core, extra) = recur::split_exdates(l.value.trim());
            (Some(core.to_string()), extra)
        })
        .unwrap_or((None, Vec::new()));
    for line in props.iter().filter(|l| l.name == "EXDATE") {
        for item in values::split_list(&line.value, 64) {
            if let Some(when) = When::parse(item.trim()) {
                exdates.push(wall_in(line, when, &zone, lookup));
            }
        }
    }
    let mut ev = CalendarEvent {
        id: if uid.is_empty() {
            format!("ics_{index}")
        } else {
            format!("ics_{}", short_hash(&uid))
        },
        calendar_id: calendar_id.to_string(),
        title: get("SUMMARY")
            .map(|l| unescape_text(&l.value))
            .unwrap_or_else(|| "(No title)".into()),
        uid,
        location: get("LOCATION")
            .map(|l| unescape_text(&l.value))
            .filter(|s| !s.trim().is_empty()),
        notes: get("DESCRIPTION")
            .map(|l| unescape_text(&l.value))
            .unwrap_or_default(),
        conference_url: get("URL")
            .map(|l| l.value.trim().to_string())
            .filter(|s| !s.is_empty()),
        rrule: core_rrule,
        exdates,
        zone: zone.clone(),
        attendees: props
            .iter()
            .filter(|l| l.name == "ATTENDEE")
            .map(|l| {
                l.value
                    .trim()
                    .trim_start_matches("mailto:")
                    .trim_start_matches("MAILTO:")
                    .to_string()
            })
            .collect(),
        alert_minutes_before: trigger
            .and_then(|t| parse_duration(&t))
            .map(|m| m.unsigned_abs() as u32),
        ..Default::default()
    };
    if let Some(rid) = get("RECURRENCE-ID") {
        ev.id = format!("{}_{}", ev.id, short_hash(&rid.value));
        ev.rrule = None;
        ev.exdates.clear();
        ev.overridden.clear();
    }
    match start_when {
        When::Date(day) => {
            ev.all_day = true;
            ev.zone = EventZone::Floating;
            ev.date = day;
            let last = match end_when {
                Some(When::Date(e)) if e > day => e - Duration::days(1),
                Some(When::Local(e) | When::Utc(e)) if e.date() > day => {
                    e.date() - Duration::days(1)
                }
                _ => match duration {
                    Some(m) if m > 24 * 60 => day + Duration::minutes(m) - Duration::days(1),
                    _ => day,
                },
            };
            ev.end_date = (last > day).then_some(last);
        }
        When::Local(_) | When::Utc(_) => {
            ev.date = start.date();
            ev.start_time = Some(start.time());
            let finish = if let (Some(when), Some(line)) = (end_when, end_line) {
                // DTEND may name another zone (a flight); keep it in the start's zone.
                Some(wall_in(line, when, &ev.zone, lookup))
            } else {
                duration.map(|m| end_wall_from_duration(start, m))
            };
            if let Some(f) = finish.filter(|f| *f > start) {
                let last_day = if f.time() == NaiveTime::MIN {
                    f.date() - Duration::days(1)
                } else {
                    f.date()
                };
                ev.end_time = Some(if f.time() == NaiveTime::MIN && last_day > start.date() {
                    NaiveTime::from_hms_opt(23, 59, 0).unwrap_or(NaiveTime::MIN)
                } else {
                    f.time()
                });
                if last_day > start.date() {
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

fn write_line(out: &mut String, line: &str) {
    fold(out, line);
}

fn dt_line(kind: &str, zone: &EventZone, wall: NaiveDateTime) -> String {
    match zone {
        EventZone::Utc => format!("{kind}:{}", format_utc(wall.and_utc().timestamp())),
        EventZone::Tz(tzid) => {
            format!("{kind};TZID={}:{}", param_value(tzid), format_local(wall))
        }
        EventZone::Floating => format!("{kind}:{}", format_local(wall)),
    }
}

/// One `VEVENT` block (without the calendar around it).
pub fn write_event(ev: &CalendarEvent, out: &mut String, stamp: &str) {
    write_line(out, "BEGIN:VEVENT");
    write_line(out, &format!("UID:{}", ev.uid_or_id()));
    write_line(out, &format!("DTSTAMP:{stamp}"));
    if ev.all_day || ev.start_time.is_none() {
        let last = ev.end_date.unwrap_or(ev.date).max(ev.date);
        write_line(out, &format!("DTSTART;VALUE=DATE:{}", format_date(ev.date)));
        write_line(
            out,
            &format!("DTEND;VALUE=DATE:{}", format_date(last + Duration::days(1))),
        );
    } else {
        let start = ev.date.and_time(ev.start_time.unwrap_or(NaiveTime::MIN));
        let end_day = ev.end_date.unwrap_or(ev.date);
        let end = match ev.end_time {
            Some(t) => end_day.and_time(t),
            None => start + Duration::hours(1),
        };
        let end = if end <= start {
            start + Duration::minutes(30)
        } else {
            end
        };
        write_line(out, &dt_line("DTSTART", &ev.zone, start));
        write_line(out, &dt_line("DTEND", &ev.zone, end));
    }
    write_line(out, &format!("SUMMARY:{}", escape_text(&ev.title)));
    if let Some(loc) = ev.location.as_deref().filter(|l| !l.is_empty()) {
        write_line(out, &format!("LOCATION:{}", escape_text(loc)));
    }
    if !ev.notes.is_empty() {
        write_line(out, &format!("DESCRIPTION:{}", escape_text(&ev.notes)));
    }
    if let Some(url) = ev.conference_url.as_deref().filter(|u| !u.is_empty()) {
        write_line(out, &format!("URL:{url}"));
    }
    if let Some(rule) = &ev.rrule {
        let (core, _) = recur::split_exdates(rule);
        let formatted = if ev.zone.is_floating() || ev.all_day {
            parse_rrule(core)
                .map(|r| {
                    let start = ev.date.and_time(ev.start_time.unwrap_or(NaiveTime::MIN));
                    if ev.all_day {
                        craft_ical::rrule::format_for(&r, start, true)
                    } else {
                        format_rrule(&r)
                    }
                })
                .unwrap_or_else(|| core.to_string())
        } else if let EventZone::Tz(_) | EventZone::Utc = ev.zone {
            parse_rrule(core)
                .map(|r| {
                    let start = ev.date.and_time(ev.start_time.unwrap_or(NaiveTime::MIN));
                    let until = recur::parse(core).and_then(|p| p.until_utc);
                    craft_ical::rrule::format_zoned(&r, start, until)
                })
                .unwrap_or_else(|| core.to_string())
        } else {
            core.to_string()
        };
        write_line(out, &format!("RRULE:{formatted}"));
    }
    if !ev.exdates.is_empty() {
        if ev.all_day {
            let dates = ev
                .exdates
                .iter()
                .map(|t| format_date(t.date()))
                .collect::<Vec<_>>()
                .join(",");
            write_line(out, &format!("EXDATE;VALUE=DATE:{dates}"));
        } else {
            for ex in &ev.exdates {
                write_line(out, &dt_line("EXDATE", &ev.zone, *ex));
            }
        }
    }
    for who in &ev.attendees {
        if who.contains('@') {
            write_line(out, &format!("ATTENDEE:mailto:{who}"));
        }
    }
    if let Some(m) = ev.alert_minutes_before {
        write_line(out, "BEGIN:VALARM");
        write_line(out, "ACTION:DISPLAY");
        write_line(out, &format!("DESCRIPTION:{}", escape_text(&ev.title)));
        write_line(out, &format!("TRIGGER:-PT{m}M"));
        write_line(out, "END:VALARM");
    }
    write_line(out, "END:VEVENT");
}

fn header(out: &mut String, name: Option<&str>) {
    write_line(out, "BEGIN:VCALENDAR");
    write_line(out, "VERSION:2.0");
    write_line(out, "PRODID:-//CalendarCraft//CalendarCraft 0.3//EN");
    write_line(out, "CALSCALE:GREGORIAN");
    if let Some(name) = name {
        write_line(out, &format!("X-WR-CALNAME:{}", escape_text(name)));
    }
}

fn write_zones_for(out: &mut String, events: &[&CalendarEvent]) {
    let mut seen = Vec::new();
    for ev in events {
        let Some(tzid) = ev.zone.tzid() else { continue };
        if seen.iter().any(|s: &String| s == tzid) {
            continue;
        }
        let Ok(tz) = tzid.parse::<chrono_tz::Tz>() else {
            continue;
        };
        let year = ev.date.year();
        let from = (year - 1).max(2);
        let to = (year + 5).min(9990);
        write_vtimezone(out, tzid, &IanaRules(tz), from, to);
        seen.push(tzid.to_string());
    }
}


/// A whole `.ics` file with these events.
pub fn write_calendar<'a>(
    name: Option<&str>,
    events: impl IntoIterator<Item = &'a CalendarEvent>,
) -> String {
    let stamp = format_utc(Utc::now().timestamp());
    let collected: Vec<&CalendarEvent> = events.into_iter().collect();
    let mut out = String::new();
    header(&mut out, name);
    write_zones_for(&mut out, &collected);
    for ev in collected {
        write_event(ev, &mut out, &stamp);
    }
    write_line(&mut out, "END:VCALENDAR");
    out
}

/// A one-event calendar object, as CalDAV stores it.
pub fn write_single(ev: &CalendarEvent) -> String {
    write_calendar(None, std::iter::once(ev))
}

/// Text helpers kept so the rest of the crate can escape values without taking a `craft-ical` dep.
pub fn escape(value: &str) -> String {
    escape_text(value)
}

pub fn unescape(value: &str) -> String {
    unescape_text(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tz::DisplayTz;
    use chrono::{NaiveDate, TimeZone};
    use chrono_tz::{America::New_York, Europe::Berlin};

    const SAMPLE: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nX-WR-CALNAME:Team\r\nBEGIN:VTIMEZONE\r\nTZID:Europe/Berlin\r\nBEGIN:STANDARD\r\nDTSTART:19701025T030000\r\nTZOFFSETFROM:+0200\r\nTZOFFSETTO:+0100\r\nRRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU\r\nEND:STANDARD\r\nBEGIN:DAYLIGHT\r\nDTSTART:19700329T020000\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0200\r\nRRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:a1@example\r\nDTSTART:20261124T090000Z\r\nDTEND:20261124T100000Z\r\nSUMMARY:Stand-up\\, daily\r\nLOCATION:Room 1\r\nDESCRIPTION:Line one\\nline two that is long enough to be folded over the seventy-five o\r\n ctet limit\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,TU\r\nBEGIN:VALARM\r\nTRIGGER:-PT15M\r\nACTION:DISPLAY\r\nEND:VALARM\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:b2@example\r\nDTSTART;VALUE=DATE:20261127\r\nDTEND;VALUE=DATE:20261130\r\nSUMMARY:Trip\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:c3\r\nDTSTART;TZID=Europe/Berlin:20261201T140000\r\nDURATION:PT45M\r\nSUMMARY:Review\r\nATTENDEE;CN=\"Ada: L\";ROLE=REQ-PARTICIPANT:mailto:ada@example.org\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn reads_events_times_and_text() {
        let cal = parse(SAMPLE, "c").unwrap();
        assert_eq!(cal.name.as_deref(), Some("Team"));
        assert_eq!(cal.events.len(), 3);
        let a = &cal.events[0];
        assert_eq!(a.title, "Stand-up, daily");
        assert_eq!(a.zone, EventZone::Utc);
        assert_eq!(a.start_time, NaiveTime::from_hms_opt(9, 0, 0));
        assert_eq!(a.end_time, NaiveTime::from_hms_opt(10, 0, 0));
        // 09:00Z in November is 10:00 in Berlin (CET, UTC+1).
        let shown = a
            .projected(
                NaiveDate::from_ymd_opt(2026, 11, 24).unwrap(),
                DisplayTz::Iana(Berlin),
            )
            .unwrap();
        assert_eq!(shown.start_time, NaiveTime::from_hms_opt(10, 0, 0));
        assert_eq!(shown.end_time, NaiveTime::from_hms_opt(11, 0, 0));
        assert!(
            a.notes.starts_with("Line one\nline two") && a.notes.ends_with("octet limit"),
            "{}",
            a.notes
        );
        assert_eq!(a.alert_minutes_before, Some(15));
        assert_eq!(a.rrule.as_deref(), Some("FREQ=WEEKLY;BYDAY=MO,TU"));
        let b = &cal.events[1];
        assert!(b.all_day);
        assert_eq!(b.date, NaiveDate::from_ymd_opt(2026, 11, 27).unwrap());
        assert_eq!(b.end_date, NaiveDate::from_ymd_opt(2026, 11, 29));
        let c = &cal.events[2];
        assert_eq!(c.zone, EventZone::Tz("Europe/Berlin".into()));
        assert_eq!(c.start_time, NaiveTime::from_hms_opt(14, 0, 0));
        assert_eq!(c.end_time, NaiveTime::from_hms_opt(14, 45, 0));
        assert_eq!(c.attendees, vec!["ada@example.org".to_string()]);
        // The same Berlin 14:00 is 08:00 in New York (EST) on 1 December 2026.
        let in_ny = c
            .projected(
                NaiveDate::from_ymd_opt(2026, 12, 1).unwrap(),
                DisplayTz::Iana(New_York),
            )
            .unwrap();
        assert_eq!(in_ny.start_time, NaiveTime::from_hms_opt(8, 0, 0));
    }

    #[test]
    fn round_trips_through_writing() {
        let cal = parse(SAMPLE, "c").unwrap();
        let text = write_calendar(Some("Team"), &cal.events);
        assert!(
            text.contains("BEGIN:VTIMEZONE") && text.contains("TZID:Europe/Berlin"),
            "named zones are written back with a VTIMEZONE"
        );
        assert!(
            text.contains("DTSTART:20261124T090000Z"),
            "UTC stays UTC: {text}"
        );
        assert!(text.contains("DTSTART;TZID=Europe/Berlin:20261201T140000"));
        assert!(text.contains("DTSTART;VALUE=DATE:20261127"));
        assert!(text.lines().all(|l| l.trim_end_matches('\r').len() <= 75));
        let again = parse(&text, "c").unwrap();
        assert_eq!(again.events.len(), 3);
        for (a, b) in cal.events.iter().zip(&again.events) {
            assert_eq!(a.title, b.title);
            assert_eq!(a.uid, b.uid);
            assert_eq!(a.all_day, b.all_day);
            assert_eq!(a.end_date, b.end_date);
            assert_eq!(a.notes, b.notes);
            assert_eq!(a.zone, b.zone);
            assert_eq!(a.start_time, b.start_time);
            assert_eq!(a.end_time, b.end_time);
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

    #[test]
    fn dst_transitions_berlin_and_new_york() {
        // Europe/Berlin 2026: 29 Mar 02:00 → 03:00, 25 Oct 03:00 → 02:00.
        // America/New_York 2026: 8 Mar 02:00 → 03:00, 1 Nov 02:00 → 01:00.
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:dst@ex\r\nDTSTART;TZID=Europe/Berlin:20260315T150000\r\nDTEND;TZID=Europe/Berlin:20260315T160000\r\nRRULE:FREQ=WEEKLY;BYDAY=SU;COUNT=12\r\nSUMMARY:Weekly 15:00 Berlin\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let ev = &parse(ics, "c").unwrap().events[0];
        let ny = DisplayTz::Iana(New_York);
        // 15 Mar 2026 is a Sunday, still CET (UTC+1) and already EDT (UTC-4): 15:00 Berlin = 10:00 New York.
        let mid_march = NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();
        let shown = ev.projected(mid_march, ny).unwrap();
        assert_eq!(shown.start_time, NaiveTime::from_hms_opt(10, 0, 0));
        // 29 Mar 2026 is a Sunday: Berlin is now CEST (UTC+2), New York still EDT: 15:00 Berlin = 09:00 New York.
        let after_eu = NaiveDate::from_ymd_opt(2026, 3, 29).unwrap();
        let shown = ev.projected(after_eu, ny).unwrap();
        assert_eq!(shown.start_time, NaiveTime::from_hms_opt(9, 0, 0));
        // 5 Apr: same offset pair as 29 Mar.
        let shown = ev
            .projected(NaiveDate::from_ymd_opt(2026, 4, 5).unwrap(), ny)
            .unwrap();
        assert_eq!(shown.start_time, NaiveTime::from_hms_opt(9, 0, 0));

        // America/New_York 2026: 8 Mar 02:00 → 03:00. A 15:00 New York weekly shown in Berlin
        // is 21:00 CET before the US change and 20:00 CET after (Berlin is already CEST by 15 Mar).
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:ny@ex\r\nDTSTART;TZID=America/New_York:20260301T150000\r\nDTEND;TZID=America/New_York:20260301T160000\r\nRRULE:FREQ=WEEKLY;BYDAY=SU;COUNT=8\r\nSUMMARY:Weekly 15:00 New York\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let ev = &parse(ics, "c").unwrap().events[0];
        let berlin = DisplayTz::Iana(Berlin);
        // 1 Mar 2026 is a Sunday: EST (UTC-5) and CET (UTC+1) → 15:00 NY = 21:00 Berlin.
        let shown = ev
            .projected(NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(), berlin)
            .unwrap();
        assert_eq!(shown.start_time, NaiveTime::from_hms_opt(21, 0, 0));
        // 15 Mar: New York is EDT (UTC-4), Berlin still CET (UTC+1) until 29 Mar → 15:00 NY = 20:00 Berlin.
        let shown = ev
            .projected(NaiveDate::from_ymd_opt(2026, 3, 15).unwrap(), berlin)
            .unwrap();
        assert_eq!(shown.start_time, NaiveTime::from_hms_opt(20, 0, 0));
    }

    #[test]
    fn rrule_until_with_tzid_and_exdate() {
        // UNTIL is 29 Mar 2026 15:00 Berlin (the first DST Sunday). The 29 Mar occurrence is included;
        // 5 Apr is not. 22 Mar is excluded via EXDATE in the same zone.
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:until@ex\r\nDTSTART;TZID=Europe/Berlin:20260308T150000\r\nDTEND;TZID=Europe/Berlin:20260308T160000\r\nRRULE:FREQ=WEEKLY;BYDAY=SU;UNTIL=20260329T140000Z\r\nEXDATE;TZID=Europe/Berlin:20260322T150000\r\nSUMMARY:Until DST\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let ev = &parse(ics, "c").unwrap().events[0];
        let berlin = DisplayTz::Iana(Berlin);
        assert!(ev.covers_in(NaiveDate::from_ymd_opt(2026, 3, 8).unwrap(), berlin));
        assert!(
            ev.projected(NaiveDate::from_ymd_opt(2026, 3, 22).unwrap(), berlin)
                .is_none(),
            "EXDATE drops 22 Mar"
        );
        assert!(ev.covers_in(NaiveDate::from_ymd_opt(2026, 3, 29).unwrap(), berlin));
        assert!(!ev.covers_in(NaiveDate::from_ymd_opt(2026, 4, 5).unwrap(), berlin));
    }

    #[test]
    fn all_day_events_do_not_shift_with_the_display_zone() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:day@ex\r\nDTSTART;VALUE=DATE:20260704\r\nDTEND;VALUE=DATE:20260706\r\nSUMMARY:Holiday\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let ev = &parse(ics, "c").unwrap().events[0];
        assert!(ev.all_day);
        assert_eq!(ev.date, NaiveDate::from_ymd_opt(2026, 7, 4).unwrap());
        assert_eq!(ev.end_date, NaiveDate::from_ymd_opt(2026, 7, 5));
        assert!(ev.covers_in(ev.date, DisplayTz::Iana(New_York)));
        assert!(ev.covers_in(ev.date, DisplayTz::Iana(Berlin)));
        assert!(ev.covers_in(
            NaiveDate::from_ymd_opt(2026, 7, 5).unwrap(),
            DisplayTz::Iana(Berlin)
        ));
        assert!(!ev.covers_in(
            NaiveDate::from_ymd_opt(2026, 7, 6).unwrap(),
            DisplayTz::Iana(Berlin)
        ));
        let text = write_single(ev);
        assert!(text.contains("DTSTART;VALUE=DATE:20260704"));
        assert!(!text.contains("TZID="));
    }

    #[test]
    fn floating_times_stay_the_same_in_every_zone() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:float@ex\r\nDTSTART:20260704T090000\r\nDTEND:20260704T100000\r\nSUMMARY:Local morning\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let ev = &parse(ics, "c").unwrap().events[0];
        assert_eq!(ev.zone, EventZone::Floating);
        let day = NaiveDate::from_ymd_opt(2026, 7, 4).unwrap();
        let in_ny = ev.projected(day, DisplayTz::Iana(New_York)).unwrap();
        let in_be = ev.projected(day, DisplayTz::Iana(Berlin)).unwrap();
        assert_eq!(in_ny.start_time, NaiveTime::from_hms_opt(9, 0, 0));
        assert_eq!(in_be.start_time, NaiveTime::from_hms_opt(9, 0, 0));
        let text = write_single(ev);
        assert!(text.contains("DTSTART:20260704T090000\r\n"), "{text}");
        assert!(!text.contains("DTSTART:20260704T090000Z"));
    }

    #[test]
    fn hostile_input_is_rejected_or_skipped_without_panic() {
        assert!(parse("", "c").is_err());
        assert!(parse("BEGIN:VEVENT\nEND:VEVENT", "c").is_err());
        let long = format!(
            "BEGIN:VCALENDAR\r\nX-JUNK:{}\r\nEND:VCALENDAR\r\n",
            "A".repeat(2_000_000)
        );
        let empty = parse(&long, "c").unwrap();
        assert!(empty.events.is_empty());
        let junk_zone = "BEGIN:VCALENDAR\r\nBEGIN:VTIMEZONE\r\nTZID:Nope\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:x\r\nDTSTART;TZID=Nope:not-a-date\r\nSUMMARY:Bad\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let cal = parse(junk_zone, "c").unwrap();
        assert!(cal.events.is_empty());
        let huge_rule = format!(
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:y\r\nDTSTART:20260101T090000Z\r\nRRULE:FREQ=WEEKLY;BYDAY={}\r\nSUMMARY:Ok\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
            "MO,".repeat(200)
        );
        let cal = parse(&huge_rule, "c").unwrap();
        assert_eq!(cal.events.len(), 1);
    }

    #[test]
    fn writes_a_valid_vtimezone_for_iana_names() {
        let ev = CalendarEvent {
            title: "Review".into(),
            date: NaiveDate::from_ymd_opt(2026, 12, 1).unwrap(),
            start_time: NaiveTime::from_hms_opt(14, 0, 0),
            end_time: NaiveTime::from_hms_opt(14, 45, 0),
            zone: EventZone::Tz("Europe/Berlin".into()),
            uid: "c3".into(),
            ..Default::default()
        };
        let text = write_single(&ev);
        assert!(text.contains("BEGIN:VTIMEZONE"));
        assert!(TimeZones::collect(&text).get("Europe/Berlin").is_some());
        // A wall time just after the October 2026 back-transition is CET, not CEST.
        let inst = Berlin
            .with_ymd_and_hms(2026, 10, 25, 2, 30, 0)
            .earliest()
            .or_else(|| Berlin.with_ymd_and_hms(2026, 10, 25, 2, 30, 0).latest())
            .unwrap();
        let zone = TimeZones::collect(&text)
            .get("Europe/Berlin")
            .unwrap()
            .offset_at(inst.timestamp());
        // 2:30 on the fold is the first (CEST, +2h) or we took latest; either is a valid offset.
        assert!(zone == 3600 || zone == 7200, "offset={zone}");
    }
}
