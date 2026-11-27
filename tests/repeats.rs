//! Custom repeats end to end: rules the form writes (several weekdays, "the 2nd Tuesday",
//! "the last Friday", an end date or a count) shown on the right days in Berlin across the
//! daylight-saving switches, with exclusions, and written to `.ics` / CalDAV and read back
//! with the rule intact, `UNTIL` in the form RFC 5545 asks for.

use calendarcraft::ics;
use calendarcraft::model::CalendarEvent;
use calendarcraft::rrule::{Custom, End, MonthlyOn, RRule};
use calendarcraft::tz::{DisplayTz, EventZone};
use chrono::{Duration, NaiveDate, NaiveTime, Weekday};
use chrono_tz::{America::New_York, Europe::Berlin};

const BERLIN: DisplayTz = DisplayTz::Iana(Berlin);
const NEW_YORK: DisplayTz = DisplayTz::Iana(New_York);

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn hm(h: u32, m: u32) -> Option<NaiveTime> {
    NaiveTime::from_hms_opt(h, m, 0)
}

fn berlin_event(date: NaiveDate, rule: &str) -> CalendarEvent {
    CalendarEvent {
        id: "r1".into(),
        calendar_id: "c".into(),
        title: "Repeat".into(),
        uid: "repeat-1@calendarcraft".into(),
        date,
        start_time: hm(9, 0),
        end_time: hm(10, 0),
        zone: EventZone::Tz("Europe/Berlin".into()),
        rrule: Some(rule.into()),
        ..Default::default()
    }
}

/// Days in `from..=to` the event shows on in `display`, with the shown start time.
fn shown_days(ev: &CalendarEvent, from: NaiveDate, to: NaiveDate, display: DisplayTz) -> Vec<(NaiveDate, NaiveTime)> {
    let mut out = Vec::new();
    let mut day = from;
    while day <= to {
        if let Some(p) = ev.projected(day, display).filter(|p| p.date == day) {
            out.push((day, p.start_time.unwrap()));
        }
        day += Duration::days(1);
    }
    out
}

fn back(text: &str) -> CalendarEvent {
    let cal = ics::parse(text, "c").expect("parses");
    assert_eq!(cal.events.len(), 1);
    cal.events.into_iter().next().unwrap()
}

fn rrule_line(text: &str) -> &str {
    let event = &text[text.find("BEGIN:VEVENT").expect("has an event")..];
    event.lines().find(|l| l.starts_with("RRULE:")).expect("has RRULE").trim_end()
}

#[test]
fn several_weekdays_across_the_october_dst_switch_with_an_exdate() {
    // Mon/Wed/Fri at 09:00 Berlin from Mon 19 Oct 2026; clocks go back on Sun 25 Oct.
    let mut ev = berlin_event(d(2026, 10, 19), "FREQ=WEEKLY;BYDAY=MO,WE,FR;UNTIL=20261030");
    ev.exdates.push(d(2026, 10, 28).and_time(NaiveTime::from_hms_opt(9, 0, 0).unwrap()));
    let days: Vec<NaiveDate> = shown_days(&ev, d(2026, 10, 1), d(2026, 11, 30), BERLIN).into_iter().map(|(d, t)| {
        assert_eq!(Some(t), hm(9, 0), "09:00 in Berlin on {d}, summer and winter");
        d
    }).collect();
    assert_eq!(days, vec![d(2026, 10, 19), d(2026, 10, 21), d(2026, 10, 23), d(2026, 10, 26), d(2026, 10, 30)]);
    // New York switches a week later: 03:00 before 25 Oct, 04:00 for one week, 03:00 after.
    let ny = shown_days(&ev, d(2026, 10, 1), d(2026, 11, 30), NEW_YORK);
    assert_eq!(ny.first().map(|x| x.1), hm(3, 0));
    assert_eq!(ny.iter().find(|x| x.0 == d(2026, 10, 26)).map(|x| x.1), hm(4, 0));
}

#[test]
fn monthly_second_tuesday_and_last_friday_across_dst() {
    let tue = berlin_event(d(2026, 2, 10), "FREQ=MONTHLY;BYDAY=2TU;COUNT=4");
    let got: Vec<(NaiveDate, NaiveTime)> = shown_days(&tue, d(2026, 1, 1), d(2026, 12, 31), BERLIN);
    assert_eq!(got.iter().map(|x| x.0).collect::<Vec<_>>(), vec![d(2026, 2, 10), d(2026, 3, 10), d(2026, 4, 14), d(2026, 5, 12)]);
    assert!(got.iter().all(|x| Some(x.1) == hm(9, 0)));
    let fri = berlin_event(d(2026, 9, 25), "FREQ=MONTHLY;BYDAY=-1FR");
    let got: Vec<NaiveDate> = shown_days(&fri, d(2026, 9, 1), d(2027, 1, 31), BERLIN).into_iter().map(|x| x.0).collect();
    assert_eq!(got, vec![d(2026, 9, 25), d(2026, 10, 30), d(2026, 11, 27), d(2026, 12, 25), d(2027, 1, 29)]);
}

#[test]
fn the_form_rule_is_written_with_until_in_utc_and_read_back() {
    let start = d(2026, 11, 2);
    let custom = Custom { end: End::On(d(2026, 12, 31)), days: vec![Weekday::Mon, Weekday::Wed, Weekday::Fri], interval: 2, ..Custom::default_for(start) };
    let rule = custom.to_rule(start).to_text();
    assert_eq!(rule, "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE,FR;UNTIL=20261231");
    let ev = berlin_event(start, &rule);
    let text = ics::write_single(&ev);
    // The end of 31 December in Berlin is 22:59:59 UTC.
    assert_eq!(rrule_line(&text), "RRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE,FR;UNTIL=20261231T225959Z");
    let again = back(&text);
    assert_eq!(shown_days(&again, d(2026, 12, 20), d(2027, 1, 10), BERLIN).len(), 2, "28 and 30 Dec, nothing after 31 Dec");
    // Written again unchanged.
    assert_eq!(rrule_line(&ics::write_single(&again)), rrule_line(&text));
    // The form reads it back as the same custom repeat, ending on 31 December.
    let parsed = RRule::parse(again.rrule.as_deref().unwrap()).unwrap();
    let to_berlin = |w: craft_ical::values::When| calendarcraft::tz::wall_in_event_zone(&again.zone, w.wall().and_utc().timestamp()).date();
    let form = Custom::from_rule(&parsed, start, &to_berlin).unwrap();
    assert_eq!(form.end, End::On(d(2026, 12, 31)));
    assert_eq!(form.to_rule(start), parsed, "an untouched end keeps its UTC value");
}

#[test]
fn until_forms_for_all_day_and_floating_events() {
    let all_day = CalendarEvent { all_day: true, start_time: None, end_time: None, zone: EventZone::Floating, ..berlin_event(d(2026, 11, 27), "FREQ=MONTHLY;BYDAY=-1FR;UNTIL=20270331T225959Z") };
    assert_eq!(rrule_line(&ics::write_single(&all_day)), "RRULE:FREQ=MONTHLY;BYDAY=-1FR;UNTIL=20270331");
    let floating = CalendarEvent { zone: EventZone::Floating, ..berlin_event(d(2026, 11, 27), "FREQ=DAILY;UNTIL=20261205") };
    assert_eq!(rrule_line(&ics::write_single(&floating)), "RRULE:FREQ=DAILY;UNTIL=20261205T235959");
    let utc = CalendarEvent { zone: EventZone::Utc, ..berlin_event(d(2026, 11, 27), "FREQ=DAILY;UNTIL=20261205") };
    assert_eq!(rrule_line(&ics::write_single(&utc)), "RRULE:FREQ=DAILY;UNTIL=20261205T235959Z");
}

#[test]
fn rules_the_form_cannot_edit_are_kept_as_they_are() {
    for rule in [
        "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1",
        "FREQ=DAILY;BYHOUR=9,17",
        "FREQ=YEARLY;BYWEEKNO=20;BYDAY=MO;X-APPLE-THING=1",
        "FREQ=MONTHLY;BYMONTHDAY=1,15;COUNT=10",
    ] {
        let ev = berlin_event(d(2026, 11, 2), rule);
        let text = ics::write_single(&ev);
        assert_eq!(rrule_line(&text), format!("RRULE:{rule}"));
        assert_eq!(back(&text).rrule.as_deref(), Some(rule));
        assert!(Custom::from_rule(&RRule::parse(rule).unwrap(), ev.date, &|w| w.wall().date()).is_none(), "{rule} is shown read-only");
    }
    // An unknown frequency is not even parsed, and still written back as it was.
    let ev = berlin_event(d(2026, 11, 2), "FREQ=FORTNIGHTLY");
    assert_eq!(rrule_line(&ics::write_single(&ev)), "RRULE:FREQ=FORTNIGHTLY");
}

#[test]
fn yearly_nth_weekday_from_the_form() {
    let start = d(2026, 11, 26);
    let c = Custom { freq: calendarcraft::rrule::Freq::Yearly, on: MonthlyOn::Nth(4, Weekday::Thu), ..Custom::default_for(start) };
    let ev = berlin_event(start, &c.to_rule(start).to_text());
    let got: Vec<NaiveDate> = shown_days(&ev, d(2026, 1, 1), d(2028, 12, 31), BERLIN).into_iter().map(|x| x.0).collect();
    assert_eq!(got, vec![d(2026, 11, 26), d(2027, 11, 25), d(2028, 11, 23)]);
}
