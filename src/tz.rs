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

    /// The zone new events are written in: the chosen display zone, or for `Local` the
    /// device's own IANA zone (floating only when the device does not say which zone it is in).
    pub fn from_settings(name: &str) -> Self {
        match DisplayTz::parse(name) {
            DisplayTz::Local => system_zone()
                .map(|tz| EventZone::Tz(tz.name().to_string()))
                .unwrap_or(EventZone::Floating),
            DisplayTz::Iana(tz) => EventZone::Tz(tz.name().to_string()),
        }
    }
}

/// IANA zones offered in Settings: `Area/City` names, without the `Etc/` and legacy
/// (`US/…`, `Canada/…`) aliases, plus `UTC`.
pub fn pickable_zones() -> impl Iterator<Item = &'static str> {
    const AREAS: &[&str] = &[
        "Africa/", "America/", "Antarctica/", "Asia/", "Atlantic/", "Australia/", "Europe/",
        "Indian/", "Pacific/",
    ];
    std::iter::once("UTC").chain(
        chrono_tz::TZ_VARIANTS
            .iter()
            .map(|tz| tz.name())
            .filter(|name| AREAS.iter().any(|a| name.starts_with(a))),
    )
}

/// The device's IANA zone (`/etc/localtime` on macOS and Linux, `Intl` in a browser).
pub fn system_zone() -> Option<Tz> {
    iana_time_zone::get_timezone()
        .ok()
        .and_then(|name| resolve_tzid(&name))
}

/// The IANA zone a `TZID` means, for the names calendar programs write:
/// - IANA names (`Europe/Berlin`), in any letter case;
/// - path-style names with a vendor prefix (`/mozilla.org/20050126_1/Europe/Berlin`,
///   `/softwarestudio.org/Olson_20011030_5/America/New_York`);
/// - common Windows names (`W. Europe Standard Time`), as Outlook and Exchange invites carry;
/// - a fixed offset written as a name (`GMT+0200`, `UTC-05:00`, `GMT`), as some Apple and
///   Exchange events carry: whole hours become `Etc/GMT∓N`.
///
/// `None` when the name means nothing here; the caller then uses the `VTIMEZONE` in the text.
pub fn resolve_tzid(tzid: &str) -> Option<Tz> {
    let name = tzid.trim().trim_matches('"').trim();
    if name.is_empty() || name.len() > 128 {
        return None;
    }
    if let Ok(tz) = name.parse::<Tz>() {
        return Some(tz);
    }
    if let Some(tz) = chrono_tz::TZ_VARIANTS
        .iter()
        .find(|tz| tz.name().eq_ignore_ascii_case(name))
    {
        return Some(*tz);
    }
    // A vendor path in front of an IANA name: try each tail after a '/'.
    if name.contains('/') {
        for (i, _) in name.match_indices('/') {
            let tail = &name[i + 1..];
            if tail.contains('/') || tail.eq_ignore_ascii_case("UTC") {
                if let Ok(tz) = tail.parse::<Tz>() {
                    return Some(tz);
                }
            }
        }
    }
    if let Some((_, iana)) = WINDOWS_ZONES
        .iter()
        .find(|(windows, _)| windows.eq_ignore_ascii_case(name))
    {
        return iana.parse::<Tz>().ok();
    }
    fixed_offset_zone(name)
}

/// `GMT+0200`, `UTC+02:00`, `GMT-5`, `UTC`, `GMT`, `Z` → an `Etc/` zone with that fixed offset
/// (whole hours only; the `Etc/GMT` signs are inverted on purpose, POSIX style).
fn fixed_offset_zone(name: &str) -> Option<Tz> {
    let upper = name.to_ascii_uppercase();
    let rest = upper
        .strip_prefix("GMT")
        .or_else(|| upper.strip_prefix("UTC"))
        .or_else(|| (upper == "Z").then_some(""))?;
    let rest = rest.trim();
    if rest.is_empty() || rest == "0" || rest == "+0000" || rest == "-0000" {
        return Some(Tz::UTC);
    }
    let (sign, digits) = match rest.as_bytes().first()? {
        b'+' => (1, &rest[1..]),
        b'-' => (-1, &rest[1..]),
        _ => return None,
    };
    let digits: String = digits.chars().filter(|c| *c != ':').collect();
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (hours, minutes): (i32, i32) = match digits.len() {
        1 | 2 => (digits.parse().ok()?, 0),
        3 => (digits[..1].parse().ok()?, digits[1..].parse().ok()?),
        4 => (digits[..2].parse().ok()?, digits[2..].parse().ok()?),
        _ => return None,
    };
    if minutes != 0 || hours > 14 {
        return None;
    }
    if hours == 0 {
        return Some(Tz::UTC);
    }
    // Etc/GMT-2 is two hours EAST of UTC.
    let etc = if sign > 0 {
        format!("Etc/GMT-{hours}")
    } else {
        format!("Etc/GMT+{hours}")
    };
    etc.parse::<Tz>().ok()
}

/// Windows zone names → IANA (the CLDR "001" territory mapping), for the zones people meet most.
const WINDOWS_ZONES: &[(&str, &str)] = &[
    ("W. Europe Standard Time", "Europe/Berlin"),
    ("Romance Standard Time", "Europe/Paris"),
    ("Central Europe Standard Time", "Europe/Budapest"),
    ("Central European Standard Time", "Europe/Warsaw"),
    ("GMT Standard Time", "Europe/London"),
    ("Greenwich Standard Time", "Atlantic/Reykjavik"),
    ("E. Europe Standard Time", "Europe/Chisinau"),
    ("FLE Standard Time", "Europe/Kiev"),
    ("GTB Standard Time", "Europe/Bucharest"),
    ("Russian Standard Time", "Europe/Moscow"),
    ("Turkey Standard Time", "Europe/Istanbul"),
    ("Israel Standard Time", "Asia/Jerusalem"),
    ("Arabian Standard Time", "Asia/Dubai"),
    ("India Standard Time", "Asia/Kolkata"),
    ("China Standard Time", "Asia/Shanghai"),
    ("Singapore Standard Time", "Asia/Singapore"),
    ("Tokyo Standard Time", "Asia/Tokyo"),
    ("Korea Standard Time", "Asia/Seoul"),
    ("AUS Eastern Standard Time", "Australia/Sydney"),
    ("New Zealand Standard Time", "Pacific/Auckland"),
    ("Eastern Standard Time", "America/New_York"),
    ("Central Standard Time", "America/Chicago"),
    ("Mountain Standard Time", "America/Denver"),
    ("US Mountain Standard Time", "America/Phoenix"),
    ("Pacific Standard Time", "America/Los_Angeles"),
    ("Alaskan Standard Time", "America/Anchorage"),
    ("Hawaiian Standard Time", "Pacific/Honolulu"),
    ("Atlantic Standard Time", "America/Halifax"),
    ("E. South America Standard Time", "America/Sao_Paulo"),
    ("Argentina Standard Time", "America/Buenos_Aires"),
    ("Central Standard Time (Mexico)", "America/Mexico_City"),
    ("South Africa Standard Time", "Africa/Johannesburg"),
    ("Egypt Standard Time", "Africa/Cairo"),
    ("UTC", "UTC"),
    ("Coordinated Universal Time", "UTC"),
];

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

    /// The instant a wall clock in this zone refers to. A time the clocks skip is read with the
    /// offset before the change (02:30 on the spring-forward night is 03:30 summer time, as
    /// RFC 5545 §3.3.5 says); a time that comes twice is the first one.
    pub fn instant_of(self, wall: NaiveDateTime) -> i64 {
        let found = match self {
            DisplayTz::Local => Local.from_local_datetime(&wall).earliest().map(|dt| dt.timestamp()),
            DisplayTz::Iana(tz) => tz.from_local_datetime(&wall).earliest().map(|dt| dt.timestamp()),
        };
        found.unwrap_or_else(|| {
            let as_utc = wall.and_utc().timestamp();
            // Offsets change at most a few times a year: half a day earlier is before the gap.
            let before = self.offset_at(as_utc.saturating_sub(12 * 3600));
            as_utc.saturating_sub(i64::from(before))
        })
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

/// The zone a stored `TZID` is read in. Events parsed by this version always carry an IANA
/// name; older caches may hold the raw `TZID` (`GMT+0200`), which [`resolve_tzid`] still maps.
/// A name nothing knows is read in the device's zone (like a floating time), never as UTC.
fn stored_zone(name: &str) -> DisplayTz {
    resolve_tzid(name)
        .map(DisplayTz::Iana)
        .unwrap_or(DisplayTz::Local)
}

/// The instant of a wall clock in a stored event zone.
pub fn instant_in_event_zone(zone: &EventZone, wall: NaiveDateTime) -> i64 {
    match zone {
        EventZone::Utc => wall.and_utc().timestamp(),
        EventZone::Floating => wall.and_utc().timestamp(), // unused for display conversion
        EventZone::Tz(name) => stored_zone(name).instant_of(wall),
    }
}

/// The wall clock of `instant` in a stored event zone.
pub fn wall_in_event_zone(zone: &EventZone, instant: i64) -> NaiveDateTime {
    match zone {
        EventZone::Utc => instant_to_utc(instant).naive_utc(),
        EventZone::Floating => instant_to_utc(instant).naive_utc(),
        EventZone::Tz(name) => stored_zone(name).wall_at(instant),
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
