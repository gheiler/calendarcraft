//! Time zones end to end: iCloud-style `.ics` text in, the times the calendar shows out, in
//! Europe/Berlin and America/New_York, and back out to `.ics` unchanged.
//!
//! Regression for 0.3.3, where every store was written with a hidden `America/Los_Angeles`
//! display zone (iCloud events showed 9 hours early in Berlin) and `TZID`s that are not IANA
//! names (`GMT+0200`) were read as UTC.

use calendarcraft::ics;
use calendarcraft::model::{AppSettings, CalendarEvent, CalendarStore};
use calendarcraft::tz::{resolve_tzid, DisplayTz, EventZone};
use chrono::{NaiveDate, NaiveTime};
use chrono_tz::{America::New_York, Europe::Berlin};

const BERLIN: DisplayTz = DisplayTz::Iana(Berlin);
const NEW_YORK: DisplayTz = DisplayTz::Iana(New_York);

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn hm(h: u32, m: u32) -> Option<NaiveTime> {
    NaiveTime::from_hms_opt(h, m, 0)
}

/// (start, end) as shown on `day` in `display`, or `None` when the event is not on that day.
fn shown(ev: &CalendarEvent, day: NaiveDate, display: DisplayTz) -> Option<(NaiveTime, NaiveTime)> {
    let p = ev.projected(day, display)?;
    Some((p.start_time?, p.end_time?))
}

/// The `VTIMEZONE` iCloud sends for Europe/Berlin.
const ICLOUD_BERLIN_VTIMEZONE: &str = "BEGIN:VTIMEZONE\r
TZID:Europe/Berlin\r
X-LIC-LOCATION:Europe/Berlin\r
BEGIN:DAYLIGHT\r
TZOFFSETFROM:+0100\r
TZOFFSETTO:+0200\r
TZNAME:CEST\r
DTSTART:19810329T020000\r
RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU\r
END:DAYLIGHT\r
BEGIN:STANDARD\r
TZOFFSETFROM:+0200\r
TZOFFSETTO:+0100\r
TZNAME:CET\r
DTSTART:19961027T030000\r
RRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU\r
END:STANDARD\r
END:VTIMEZONE\r
";

fn icloud(vtimezones: &str, vevents: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Apple Inc.//iPhone OS 18.1//EN\r\nCALSCALE:GREGORIAN\r\n{vtimezones}{vevents}END:VCALENDAR\r\n"
    )
}

fn one(text: &str) -> CalendarEvent {
    let cal = ics::parse(text, "c").expect("parses");
    assert_eq!(cal.events.len(), 1, "{text}");
    cal.events.into_iter().next().unwrap()
}

#[test]
fn icloud_tzid_berlin_with_vtimezone() {
    let ev = one(&icloud(
        ICLOUD_BERLIN_VTIMEZONE,
        "BEGIN:VEVENT\r
CREATED:20261120T081500Z\r
DTEND;TZID=Europe/Berlin:20261126T110000\r
DTSTAMP:20261120T081512Z\r
DTSTART;TZID=Europe/Berlin:20261126T100000\r
SEQUENCE:0\r
SUMMARY:Zahnarzt\r
UID:6E1B9C3A-1F0B-4C51-9A57-0D7C3F1A2B11\r
END:VEVENT\r
",
    ));
    assert_eq!(ev.zone, EventZone::Tz("Europe/Berlin".into()));
    let day = d(2026, 11, 26);
    assert_eq!(shown(&ev, day, BERLIN), Some((hm(10, 0).unwrap(), hm(11, 0).unwrap())));
    assert_eq!(shown(&ev, day, NEW_YORK), Some((hm(4, 0).unwrap(), hm(5, 0).unwrap())));
    // Same in summer (CEST, +2): 10:00 Berlin is 04:00 New York (EDT, -4).
    let summer = one(&icloud(
        ICLOUD_BERLIN_VTIMEZONE,
        "BEGIN:VEVENT\r\nUID:s\r\nDTSTART;TZID=Europe/Berlin:20260715T100000\r\nDTEND;TZID=Europe/Berlin:20260715T103000\r\nSUMMARY:S\r\nEND:VEVENT\r\n",
    ));
    assert_eq!(shown(&summer, d(2026, 7, 15), BERLIN), Some((hm(10, 0).unwrap(), hm(10, 30).unwrap())));
    assert_eq!(shown(&summer, d(2026, 7, 15), NEW_YORK), Some((hm(4, 0).unwrap(), hm(4, 30).unwrap())));
}

#[test]
fn utc_times_are_converted() {
    let ev = one(&icloud(
        "",
        "BEGIN:VEVENT\r\nUID:z\r\nDTSTART:20261126T090000Z\r\nDTEND:20261126T093000Z\r\nSUMMARY:Call\r\nEND:VEVENT\r\n",
    ));
    assert_eq!(ev.zone, EventZone::Utc);
    let day = d(2026, 11, 26);
    assert_eq!(shown(&ev, day, BERLIN), Some((hm(10, 0).unwrap(), hm(10, 30).unwrap())));
    assert_eq!(shown(&ev, day, NEW_YORK), Some((hm(4, 0).unwrap(), hm(4, 30).unwrap())));
}

#[test]
fn floating_times_read_the_same_everywhere() {
    let ev = one(&icloud(
        "",
        "BEGIN:VEVENT\r\nUID:f\r\nDTSTART:20261126T100000\r\nDTEND:20261126T110000\r\nSUMMARY:Yoga\r\nEND:VEVENT\r\n",
    ));
    assert_eq!(ev.zone, EventZone::Floating);
    let day = d(2026, 11, 26);
    assert_eq!(shown(&ev, day, BERLIN), Some((hm(10, 0).unwrap(), hm(11, 0).unwrap())));
    assert_eq!(shown(&ev, day, NEW_YORK), Some((hm(10, 0).unwrap(), hm(11, 0).unwrap())));
}

#[test]
fn all_day_dates_stay_on_their_day_everywhere() {
    let ev = one(&icloud(
        "",
        "BEGIN:VEVENT\r\nUID:a\r\nDTSTART;VALUE=DATE:20261127\r\nDTEND;VALUE=DATE:20261128\r\nSUMMARY:Holiday\r\nEND:VEVENT\r\n",
    ));
    assert!(ev.all_day);
    for display in [BERLIN, NEW_YORK] {
        assert!(!ev.covers_in(d(2026, 11, 26), display));
        assert!(ev.covers_in(d(2026, 11, 27), display));
        assert!(!ev.covers_in(d(2026, 11, 28), display));
    }
}

/// A weekly Monday 09:00 Berlin meeting across the 29 March 2026 switch to summer time.
/// New York switched three weeks earlier (8 March), so the gap goes from 5 to 6 hours.
const WEEKLY: &str = "BEGIN:VEVENT\r
UID:weekly-1@icloud\r
DTSTART;TZID=Europe/Berlin:20260316T090000\r
DTEND;TZID=Europe/Berlin:20260316T093000\r
RRULE:FREQ=WEEKLY;BYDAY=MO;COUNT=6\r
EXDATE;TZID=Europe/Berlin:20260406T090000\r
SUMMARY:Weekly\r
END:VEVENT\r
";

#[test]
fn weekly_recurrence_across_the_march_dst_switch() {
    let ev = one(&icloud(ICLOUD_BERLIN_VTIMEZONE, WEEKLY));
    for day in [d(2026, 3, 16), d(2026, 3, 23), d(2026, 3, 30), d(2026, 4, 13)] {
        assert_eq!(shown(&ev, day, BERLIN), Some((hm(9, 0).unwrap(), hm(9, 30).unwrap())), "{day}");
    }
    assert_eq!(shown(&ev, d(2026, 3, 16), NEW_YORK), Some((hm(4, 0).unwrap(), hm(4, 30).unwrap())));
    assert_eq!(shown(&ev, d(2026, 3, 23), NEW_YORK), Some((hm(4, 0).unwrap(), hm(4, 30).unwrap())));
    assert_eq!(shown(&ev, d(2026, 3, 30), NEW_YORK), Some((hm(3, 0).unwrap(), hm(3, 30).unwrap())));
    // EXDATE takes 6 April out; COUNT=6 still counts it, so the last one is 20 April.
    assert_eq!(shown(&ev, d(2026, 4, 6), BERLIN), None);
    assert!(shown(&ev, d(2026, 4, 20), BERLIN).is_some());
    assert_eq!(shown(&ev, d(2026, 4, 27), BERLIN), None);
}

#[test]
fn a_moved_occurrence_replaces_the_one_from_the_series() {
    let ev_text = format!(
        "{WEEKLY}BEGIN:VEVENT\r
UID:weekly-1@icloud\r
RECURRENCE-ID;TZID=Europe/Berlin:20260330T090000\r
DTSTART;TZID=Europe/Berlin:20260330T110000\r
DTEND;TZID=Europe/Berlin:20260330T113000\r
SUMMARY:Weekly (moved)\r
END:VEVENT\r
"
    );
    let cal = ics::parse(&icloud(ICLOUD_BERLIN_VTIMEZONE, &ev_text), "c").unwrap();
    assert_eq!(cal.events.len(), 2);
    let day = d(2026, 3, 30);
    let on_day: Vec<_> = cal.events.iter().filter_map(|e| shown(e, day, BERLIN)).collect();
    assert_eq!(on_day, vec![(hm(11, 0).unwrap(), hm(11, 30).unwrap())]);
    // The rest of the series is still there, and the moved one is not written as an EXDATE.
    assert!(shown(&cal.events[0], d(2026, 3, 23), BERLIN).is_some());
    assert!(!ics::write_single(&cal.events[0]).contains("20260330"));
}

#[test]
fn apple_and_exchange_style_tzids() {
    // A fixed offset as the TZID (with its own VTIMEZONE): 10:00 at +02:00 is 08:00 UTC,
    // 09:00 in Berlin in November.
    let gmt = one(&icloud(
        "BEGIN:VTIMEZONE\r\nTZID:GMT+0200\r\nBEGIN:STANDARD\r\nDTSTART:16010101T000000\r\nTZOFFSETFROM:+0200\r\nTZOFFSETTO:+0200\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\n",
        "BEGIN:VEVENT\r\nUID:g\r\nDTSTART;TZID=GMT+0200:20261126T100000\r\nDTEND;TZID=GMT+0200:20261126T110000\r\nSUMMARY:G\r\nEND:VEVENT\r\n",
    ));
    assert_eq!(shown(&gmt, d(2026, 11, 26), BERLIN), Some((hm(9, 0).unwrap(), hm(10, 0).unwrap())));
    assert_eq!(shown(&gmt, d(2026, 11, 26), NEW_YORK), Some((hm(3, 0).unwrap(), hm(4, 0).unwrap())));

    // A Windows name with no VTIMEZONE, a vendor path, and a custom name whose VTIMEZONE
    // says where it is: all three are Berlin.
    for (tzid, vtz) in [
        ("W. Europe Standard Time", String::new()),
        ("/mozilla.org/20050126_1/Europe/Berlin", String::new()),
        (
            "Mitteleuropäische Zeit",
            ICLOUD_BERLIN_VTIMEZONE
                .replace("TZID:Europe/Berlin", "TZID:Mitteleuropäische Zeit"),
        ),
    ] {
        let ev = one(&icloud(
            &vtz,
            &format!("BEGIN:VEVENT\r\nUID:w\r\nDTSTART;TZID=\"{tzid}\":20260715T100000\r\nDTEND;TZID=\"{tzid}\":20260715T110000\r\nSUMMARY:W\r\nEND:VEVENT\r\n"),
        ));
        assert_eq!(ev.zone, EventZone::Tz("Europe/Berlin".into()), "{tzid}");
        assert_eq!(shown(&ev, d(2026, 7, 15), NEW_YORK), Some((hm(4, 0).unwrap(), hm(5, 0).unwrap())), "{tzid}");
    }
}

#[test]
fn a_zone_only_the_vtimezone_knows_is_converted_with_it() {
    // +05:30 under a made-up name: 10:00 there is 04:30 UTC, 05:30 in Berlin (CET).
    let ev = one(&icloud(
        "BEGIN:VTIMEZONE\r\nTZID:Custom Office\r\nBEGIN:STANDARD\r\nDTSTART:19700101T000000\r\nTZOFFSETFROM:+0530\r\nTZOFFSETTO:+0530\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\n",
        "BEGIN:VEVENT\r\nUID:c\r\nDTSTART;TZID=Custom Office:20261126T100000\r\nDTEND;TZID=Custom Office:20261126T110000\r\nSUMMARY:C\r\nEND:VEVENT\r\n",
    ));
    assert_eq!(shown(&ev, d(2026, 11, 26), BERLIN), Some((hm(5, 30).unwrap(), hm(6, 30).unwrap())));
}

#[test]
fn dtend_in_another_zone_is_converted() {
    // A flight: leaves Berlin 10:00 (09:00 UTC), lands New York 12:30 (17:30 UTC).
    let ev = one(&icloud(
        ICLOUD_BERLIN_VTIMEZONE,
        "BEGIN:VEVENT\r\nUID:fl\r\nDTSTART;TZID=Europe/Berlin:20261126T100000\r\nDTEND;TZID=America/New_York:20261126T123000\r\nSUMMARY:LH400\r\nEND:VEVENT\r\n",
    ));
    assert_eq!(shown(&ev, d(2026, 11, 26), BERLIN), Some((hm(10, 0).unwrap(), hm(18, 30).unwrap())));
    assert_eq!(shown(&ev, d(2026, 11, 26), NEW_YORK), Some((hm(4, 0).unwrap(), hm(12, 30).unwrap())));
}

#[test]
fn writing_back_keeps_the_tzid_and_the_wall_clock() {
    let ev = one(&icloud(ICLOUD_BERLIN_VTIMEZONE, WEEKLY));
    let text = ics::write_single(&ev);
    assert!(text.contains("DTSTART;TZID=Europe/Berlin:20260316T090000"), "{text}");
    assert!(text.contains("DTEND;TZID=Europe/Berlin:20260316T093000"), "{text}");
    assert!(text.contains("BEGIN:VTIMEZONE"));
    let again = one(&text);
    assert_eq!(again.zone, ev.zone);
    for day in [d(2026, 3, 16), d(2026, 3, 30)] {
        assert_eq!(shown(&again, day, NEW_YORK), shown(&ev, day, NEW_YORK));
    }
    // UTC stays UTC.
    let utc = one(&icloud("", "BEGIN:VEVENT\r\nUID:z\r\nDTSTART:20261126T090000Z\r\nDTEND:20261126T093000Z\r\nSUMMARY:Z\r\nEND:VEVENT\r\n"));
    assert!(ics::write_single(&utc).contains("DTSTART:20261126T090000Z"));
}

#[test]
fn editing_in_another_display_zone_keeps_the_event_zone() {
    let ev = one(&icloud(ICLOUD_BERLIN_VTIMEZONE, WEEKLY));
    // The form shows New York times...
    let mut form = ev.shown_in(NEW_YORK);
    assert_eq!((form.date, form.start_time), (d(2026, 3, 16), hm(4, 0)));
    // ...saving untouched gives the same event back.
    let mut same = form.clone();
    same.times_from_display(NEW_YORK);
    assert_eq!((same.date, same.start_time, same.end_time), (ev.date, ev.start_time, ev.end_time));
    assert_eq!(same.zone, ev.zone);
    // Moving it an hour later in New York is an hour later in Berlin, still TZID=Europe/Berlin.
    form.start_time = hm(5, 0);
    form.end_time = hm(5, 30);
    form.times_from_display(NEW_YORK);
    assert_eq!((form.start_time, form.end_time), (hm(10, 0), hm(10, 30)));
    assert!(ics::write_single(&form).contains("DTSTART;TZID=Europe/Berlin:20260316T100000"));
}

#[test]
fn local_and_file_calendars_round_trip_through_ics() {
    let ev = CalendarEvent {
        id: "e1".into(),
        calendar_id: "c".into(),
        title: "Local".into(),
        uid: "local-1".into(),
        date: d(2026, 11, 26),
        start_time: hm(10, 0),
        end_time: hm(11, 0),
        zone: EventZone::Tz("Europe/Berlin".into()),
        ..Default::default()
    };
    let text = ics::write_calendar(Some("Mine"), [&ev]);
    let back = one(&text);
    assert_eq!(shown(&back, d(2026, 11, 26), BERLIN), Some((hm(10, 0).unwrap(), hm(11, 0).unwrap())));
    // And through the store's JSON (local calendars).
    let json = serde_json::to_string(&ev).unwrap();
    let from_json: CalendarEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(shown(&from_json, d(2026, 11, 26), NEW_YORK), Some((hm(4, 0).unwrap(), hm(5, 0).unwrap())));
}

#[test]
fn old_raw_tzids_in_a_cache_still_show_right() {
    // 0.3.3 kept `GMT+0200` as written and then read it as UTC.
    let ev = CalendarEvent {
        title: "Cached".into(),
        date: d(2026, 11, 26),
        start_time: hm(10, 0),
        end_time: hm(11, 0),
        zone: EventZone::Tz("GMT+0200".into()),
        ..Default::default()
    };
    assert_eq!(shown(&ev, d(2026, 11, 26), BERLIN), Some((hm(9, 0).unwrap(), hm(10, 0).unwrap())));
}

#[test]
fn the_display_zone_defaults_to_the_device() {
    assert_eq!(AppSettings::default().timezone, "Local");
    assert_eq!(CalendarStore::empty().settings.timezone, "Local");
    assert_eq!(CalendarStore::sample_preloaded(d(2026, 11, 26)).settings.timezone, "Local");
    // A store written by 0.3.3 (hidden America/Los_Angeles) is repaired when it loads...
    let mut old = CalendarStore::empty();
    old.settings.timezone = "America/Los_Angeles".into();
    let json = serde_json::to_string(&old).unwrap();
    let loaded = CalendarStore::from_json_str(&json).unwrap();
    assert_eq!(loaded.settings.timezone, "Local");
    // ...but a zone someone picked is kept.
    old.settings.timezone_picked = true;
    let json = serde_json::to_string(&old).unwrap();
    assert_eq!(CalendarStore::from_json_str(&json).unwrap().settings.timezone, "America/Los_Angeles");
}

#[test]
fn tzid_names_resolve() {
    let name = |s: &str| resolve_tzid(s).map(|z| z.name().to_string());
    assert_eq!(name("Europe/Berlin").as_deref(), Some("Europe/Berlin"));
    assert_eq!(name("europe/berlin").as_deref(), Some("Europe/Berlin"));
    assert_eq!(name("GMT+0200").as_deref(), Some("Etc/GMT-2"));
    assert_eq!(name("UTC-05:00").as_deref(), Some("Etc/GMT+5"));
    assert!(matches!(name("GMT").as_deref(), Some("GMT" | "UTC")));
    assert_eq!(name("Eastern Standard Time").as_deref(), Some("America/New_York"));
    assert_eq!(name("GMT+0530"), None, "not whole hours: the VTIMEZONE answers");
    assert_eq!(name("Nowhere"), None);
}

#[test]
fn spring_forward_gap_uses_the_offset_before() {
    let wall = d(2026, 3, 29).and_hms_opt(2, 30, 0).unwrap();
    let instant = BERLIN.instant_of(wall);
    // Read with CET (+1): 01:30 UTC, which is 03:30 CEST.
    assert_eq!(instant, d(2026, 3, 29).and_hms_opt(1, 30, 0).unwrap().and_utc().timestamp());
    assert_eq!(BERLIN.wall_at(instant).time(), NaiveTime::from_hms_opt(3, 30, 0).unwrap());
}
