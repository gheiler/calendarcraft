//! Named time zones and the zone the calendar is shown in.
//!
//! `craft-ical` has no time-zone database: a `VTIMEZONE` in the text answers for itself, and
//! anything else (an IANA `TZID`, the zone the user picked) is resolved here through `chrono-tz`.

use chrono::{DateTime, FixedOffset, Local, NaiveDate, NaiveDateTime, Offset, TimeZone, Utc};
use chrono_tz::Tz;
use craft_ical::{Transition, ZoneRules};

/// The zone a timed event's wall clock is written in.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum EventZone {
    /// No zone: the wall clock is the same wherever it is shown.
    #[default]
    Floating,
    /// Written as UTC (`…Z`).
    Utc,
    /// A named zone (`TZID=Europe/Berlin`).
    Tz(String),
}

impl EventZone {
    pub fn is_floating(&self) -> bool {
        matches!(self, EventZone::Floating)
    }

    pub fn tzid(&self) -> Option<&str> {
        match self {
            EventZone::Tz(name) => Some(name.as_str()),
            _ => None,
        }
    }

    /// The user's chosen display zone: an IANA name becomes a `TZID`, `Local` stays floating.
    pub fn from_settings(name: &str) -> Self {
        match DisplayTz::parse(name) {
            DisplayTz::Local => EventZone::Floating,
            DisplayTz::Iana(tz) => EventZone::Tz(tz.name().to_string()),
        }
    }
}

/// The zone dates and times are shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayTz {
    /// The device's local zone.
    Local,
    /// A named IANA zone.
    Iana(Tz),
}

impl DisplayTz {
    /// `Local`, `system`, empty, or an unknown name → [`DisplayTz::Local`].
    pub fn parse(name: &str) -> Self {
        let name = name.trim();
        if name.is_empty()
            || name.eq_ignore_ascii_case("local")
            || name.eq_ignore_ascii_case("system")
        {
            return DisplayTz::Local;
        }
        name.parse::<Tz>()
            .map(DisplayTz::Iana)
            .unwrap_or(DisplayTz::Local)
    }

    pub fn name(self) -> &'static str {
        match self {
            DisplayTz::Local => "Local",
            DisplayTz::Iana(tz) => tz.name(),
        }
    }

    /// Seconds east of UTC at `instant` (seconds since the Unix epoch).
    pub fn offset_at(self, instant: i64) -> i32 {
        let utc = instant_to_utc(instant);
        match self {
            DisplayTz::Local => utc.with_timezone(&Local).offset().fix().local_minus_utc(),
            DisplayTz::Iana(tz) => utc.with_timezone(&tz).offset().fix().local_minus_utc(),
        }
    }

    /// The wall clock at `instant` in this zone.
    pub fn wall_at(self, instant: i64) -> NaiveDateTime {
        let utc = instant_to_utc(instant);
        match self {
            DisplayTz::Local => utc.with_timezone(&Local).naive_local(),
            DisplayTz::Iana(tz) => utc.with_timezone(&tz).naive_local(),
        }
    }

    /// The instant a wall clock in this zone refers to (gaps use the offset after; folds use the first).
    pub fn instant_of(self, wall: NaiveDateTime) -> i64 {
        match self {
            DisplayTz::Local => match Local.from_local_datetime(&wall) {
                chrono::LocalResult::Single(dt) | chrono::LocalResult::Ambiguous(dt, _) => {
                    dt.timestamp()
                }
                chrono::LocalResult::None => wall.and_utc().timestamp(),
            },
            DisplayTz::Iana(tz) => match tz.from_local_datetime(&wall) {
                chrono::LocalResult::Single(dt) | chrono::LocalResult::Ambiguous(dt, _) => {
                    dt.timestamp()
                }
                chrono::LocalResult::None => wall.and_utc().timestamp(),
            },
        }
    }

    /// Midnight at the start of `day` in this zone, as an instant.
    pub fn start_of_day(self, day: NaiveDate) -> i64 {
        self.instant_of(day.and_time(chrono::NaiveTime::MIN))
    }
}

fn instant_to_utc(instant: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(instant, 0).unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
}

/// Look up a named zone: a `VTIMEZONE` in the text first, then an IANA name (and `X-LIC-LOCATION`).
pub struct ZoneLookup<'a> {
    ics: &'a craft_ical::TimeZones,
}

impl<'a> ZoneLookup<'a> {
    pub fn new(ics: &'a craft_ical::TimeZones) -> Self {
        Self { ics }
    }

    pub fn get_ics(&self, tzid: &str) -> Option<&craft_ical::VTimezone> {
        self.ics.get(tzid)
    }

    /// Seconds east of UTC at `instant` for `tzid`.
    pub fn offset_at(&self, tzid: &str, instant: i64) -> Option<i32> {
        if let Some(zone) = self.ics.get(tzid) {
            if self.ics.spend(zone.cost()) {
                return Some(zone.offset_at(instant));
            }
        }
        if let Some(place) = self.ics.get(tzid).map(|z| z.place().to_string()) {
            if !place.is_empty() {
                if let Ok(tz) = place.parse::<Tz>() {
                    return Some(DisplayTz::Iana(tz).offset_at(instant));
                }
            }
        }
        tzid.parse::<Tz>()
            .ok()
            .map(|tz| DisplayTz::Iana(tz).offset_at(instant))
    }

    /// The instant of a wall-clock time written in `tzid`.
    pub fn instant_of(&self, tzid: &str, wall: NaiveDateTime) -> Option<i64> {
        if let Some(zone) = self.ics.get(tzid) {
            if self.ics.spend(zone.cost()) {
                return Some(zone.instant(wall));
            }
        }
        if let Some(place) = self.ics.get(tzid).map(|z| z.place().to_string()) {
            if !place.is_empty() {
                if let Ok(tz) = place.parse::<Tz>() {
                    return Some(DisplayTz::Iana(tz).instant_of(wall));
                }
            }
        }
        tzid.parse::<Tz>()
            .ok()
            .map(|tz| DisplayTz::Iana(tz).instant_of(wall))
    }
}

/// Resolve a stored event zone after parse (IANA only; `VTIMEZONE` was already applied).
pub fn instant_in_event_zone(zone: &EventZone, wall: NaiveDateTime) -> i64 {
    match zone {
        EventZone::Utc => wall.and_utc().timestamp(),
        EventZone::Floating => wall.and_utc().timestamp(), // unused for display conversion
        EventZone::Tz(name) => name
            .parse::<Tz>()
            .map(|tz| DisplayTz::Iana(tz).instant_of(wall))
            .unwrap_or_else(|_| wall.and_utc().timestamp()),
    }
}

pub fn wall_in_event_zone(zone: &EventZone, instant: i64) -> NaiveDateTime {
    match zone {
        EventZone::Utc => instant_to_utc(instant).naive_utc(),
        EventZone::Floating => instant_to_utc(instant).naive_utc(),
        EventZone::Tz(name) => name
            .parse::<Tz>()
            .map(|tz| DisplayTz::Iana(tz).wall_at(instant))
            .unwrap_or_else(|_| instant_to_utc(instant).naive_utc()),
    }
}

/// `ZoneRules` for writing a `VTIMEZONE` from an IANA zone.
pub struct IanaRules(pub Tz);

impl ZoneRules for IanaRules {
    fn offset_at(&self, instant: i64) -> i32 {
        DisplayTz::Iana(self.0).offset_at(instant)
    }

    fn transitions(&self, from: i64, to: i64) -> Vec<Transition> {
        let mut out = Vec::new();
        if to <= from {
            return out;
        }
        let step: i64 = 3600;
        let mut prev_at = from;
        let mut prev = self.offset_at(from);
        let mut at = from.saturating_add(step);
        while at <= to {
            let cur = self.offset_at(at);
            if cur != prev {
                let change = exact_change(self, prev_at, at, prev, cur);
                out.push(Transition {
                    at: change,
                    before: prev,
                    after: cur,
                });
                prev = cur;
            }
            prev_at = at;
            at = at.saturating_add(step);
            if at < prev_at {
                break;
            }
        }
        out
    }
}

fn exact_change(zone: &IanaRules, mut lo: i64, mut hi: i64, before: i32, after: i32) -> i64 {
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if zone.offset_at(mid) == before {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let _ = after;
    hi
}

/// Offset of a [`FixedOffset`], in seconds, for tests.
pub fn fixed_offset_seconds(east: i32) -> FixedOffset {
    FixedOffset::east_opt(east).unwrap_or(FixedOffset::east_opt(0).expect("zero offset"))
}
