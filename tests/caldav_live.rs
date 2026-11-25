//! Round trip against a real CalDAV server. Skipped unless CALDAV_TEST_URL is set, e.g.
//! CALDAV_TEST_URL=http://127.0.0.1:8091/dav/cal/jonas/ CALDAV_TEST_USER=jonas@example.test \
//! CALDAV_TEST_PASS=... cargo test --test caldav_live -- --nocapture
#![cfg(not(target_arch = "wasm32"))]

use calendarcraft::model::CalendarEvent;
use calendarcraft::sync::{self, Endpoint};
use chrono::{NaiveDate, NaiveTime};

#[test]
fn discover_put_report_delete() {
    let Ok(url) = std::env::var("CALDAV_TEST_URL") else {
        eprintln!("CALDAV_TEST_URL not set; skipping");
        return;
    };
    let user = std::env::var("CALDAV_TEST_USER").unwrap_or_default();
    let pass = std::env::var("CALDAV_TEST_PASS").unwrap_or_default();
    let ep = Endpoint { base: url, auth: Some((user, pass)), proxy: None, host_managed: false, account_id: None };
    pollster::block_on(async {
        let cals = sync::discover(&ep).await.expect("discover");
        println!("calendars: {cals:#?}");
        let cal = cals.iter().find(|c| !c.read_only).expect("a writable calendar");
        let day = NaiveDate::from_ymd_opt(2026, 11, 26).unwrap();
        let ev = CalendarEvent {
            id: "live_test".into(),
            calendar_id: "live".into(),
            title: "CalendarCraft live test".into(),
            date: day,
            start_time: NaiveTime::from_hms_opt(15, 0, 0),
            end_time: NaiveTime::from_hms_opt(16, 0, 0),
            uid: format!("calendarcraft-live-{}", std::process::id()),
            ..Default::default()
        };
        let (href, etag) = sync::put_event(&ep, &cal.url, &ev).await.expect("put");
        println!("put {href} etag {etag:?}");
        let got = sync::fetch_events(&ep, &cal.url, "live", day, day.succ_opt().unwrap()).await.expect("report");
        let found = got.iter().find(|e| e.title == ev.title).expect("event comes back from REPORT");
        assert_eq!(found.start_time, ev.start_time);
        sync::delete_event(&ep, &href, found.etag.as_deref().or(etag.as_deref())).await.expect("delete");
        let after = sync::fetch_events(&ep, &cal.url, "live", day, day.succ_opt().unwrap()).await.expect("report");
        assert!(after.iter().all(|e| e.title != ev.title));
    });
}
