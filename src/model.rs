pub use crate::nlp::NaiveDateExt;
use crate::tz::{DisplayTz, EventZone};
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, Weekday};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum ViewMode {
    Day,
    Week,
    #[default]
    Month,
    Quarter,
    Year,
}

impl ViewMode {
    pub fn label(&self) -> &'static str {
        match self {
            ViewMode::Day => "Day",
            ViewMode::Week => "Week",
            ViewMode::Month => "Month",
            ViewMode::Quarter => "Quarter",
            ViewMode::Year => "Year",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default)]
    pub default_view: ViewMode,
    #[serde(default = "default_dark_mode")]
    pub dark_mode: bool,
    #[serde(default = "default_start_hour")]
    pub work_day_start: u8,
    #[serde(default = "default_end_hour")]
    pub work_day_end: u8,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    #[serde(default = "default_calendar_id")]
    pub default_calendar_id: String,
    #[serde(default)]
    pub active_calendar_set: Option<String>,
    /// How often to check file-backed calendars in a host, in seconds. `0` means the default (120).
    #[serde(default)]
    pub file_sync_interval_secs: u32,
    /// Set once the person picks a zone in Settings. Before 0.3.4 every new store was written
    /// with `America/Los_Angeles` (a leftover sample default no one could see or change); a
    /// store with that zone and no pick is moved back to the device's zone when it loads.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub timezone_picked: bool,
}

/// The zone every store got by mistake before 0.3.4 (see [`AppSettings::timezone_picked`]).
const OLD_DEFAULT_TIMEZONE: &str = "America/Los_Angeles";

impl AppSettings {
    /// Undo the old hidden `America/Los_Angeles` default (only when no zone was ever picked).
    pub fn repair_timezone(&mut self) -> bool {
        if !self.timezone_picked && self.timezone == OLD_DEFAULT_TIMEZONE {
            self.timezone = default_timezone();
            return true;
        }
        false
    }
}

fn default_dark_mode() -> bool {
    true
}
fn default_start_hour() -> u8 {
    8
}
fn default_end_hour() -> u8 {
    18
}
fn default_timezone() -> String {
    "Local".to_string()
}
fn default_calendar_id() -> String {
    "cal_work".to_string()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            default_view: ViewMode::Month,
            dark_mode: true,
            work_day_start: 8,
            work_day_end: 18,
            timezone: default_timezone(),
            default_calendar_id: "cal_work".to_string(),
            active_calendar_set: None,
            file_sync_interval_secs: 0,
            timezone_picked: false,
        }
    }
}

/// How an account's calendars are reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    /// Kept in this app only (the sample data, or calendars made here).
    #[default]
    Local,
    /// A CalDAV server (Stalwart, iCloud, Fastmail, Nextcloud, Google…), read and written.
    Caldav,
    /// An `.ics` subscription URL, read only.
    Ics,
    /// The `.ics` file opened from the host (artcraftCloud's drive); saved back through the bridge.
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccountConfig {
    pub id: String,
    pub name: String,
    pub provider: String, // e.g., "iCloud", "Google", "Exchange", "CalDAV"
    pub email: String,
    #[serde(default)]
    pub server_url: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub kind: AccountKind,
    /// Sign-in name for CalDAV (often the email address). The password is never stored here.
    #[serde(default)]
    pub username: Option<String>,
    /// The host signs requests itself (artcraftCloud's same-origin `/caldav/` proxy): no password.
    #[serde(default)]
    pub host_managed: bool,
    /// Go through the host's proxy (browsers block most CalDAV servers by CORS).
    #[serde(default)]
    pub via_proxy: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CalendarSource {
    pub id: String,
    pub account_id: String,
    pub name: String,
    pub color_hex: String,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub read_only: bool,
    /// The calendar collection on the server (CalDAV) or the feed (ICS).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The collection's `getctag` / sync token when last read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctag: Option<String>,
    /// Host drive path (file id) when this calendar is stored as an `.ics` file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
    /// Last seen ETag / revision of that file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_etag: Option<String>,
}

impl CalendarSource {
    pub fn rgb(&self) -> [u8; 3] {
        parse_hex_color(&self.color_hex).unwrap_or([59, 130, 246])
    }
}

/// The built-in "All" set. It is computed (every visible calendar), never stored; older stores
/// kept it as a list, which [`CalendarStore::normalize_calendar_sets`] drops.
pub const ALL_SET_ID: &str = "set_all";

/// The sample sets older versions created, with the calendars they held. Dropped on load
/// unless they were changed to hold other calendars.
const DEMO_SETS: &[(&str, &[&str])] = &[
    ("set_focus", &["cal_work", "cal_product", "cal_studio"]),
    ("set_home", &["cal_personal", "cal_family"]),
];

/// A named group of calendars the user made (Settings › Sets). Choosing one shows only its
/// calendars; "All" is not one of these.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalendarSet {
    pub id: String,
    pub name: String,
    pub calendar_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct CalendarEvent {
    pub id: String,
    pub calendar_id: String,
    pub title: String,
    pub date: NaiveDate,
    /// Last day of an event that spans days (inclusive); `None` for one-day events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_date: Option<NaiveDate>,
    /// iCalendar UID (made from `id` when empty).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub uid: String,
    /// The event's resource on a CalDAV server, and its ETag when read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    /// A repeat rule kept as text (`RRULE` value) so it survives a round trip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rrule: Option<String>,
    /// Read only here: one of several events in the same server resource (a repeating series
    /// with changed occurrences), which CalendarCraft cannot write back without losing the rest.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub locked: bool,
    pub start_time: Option<NaiveTime>,
    pub end_time: Option<NaiveTime>,
    #[serde(default)]
    pub all_day: bool,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub conference_url: Option<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub attendees: Vec<String>,
    #[serde(default)]
    pub alert_minutes_before: Option<u32>,
    /// Wall-clock zone of `date` / `start_time` / `end_time`. Omitted in older stores (floating).
    #[serde(default, skip_serializing_if = "EventZone::is_floating")]
    pub zone: EventZone,
    /// Excluded occurrence starts, in the event's own wall clock.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exdates: Vec<NaiveDateTime>,
    /// Occurrence starts (event's wall clock) that a changed copy (`RECURRENCE-ID`) in the
    /// same resource replaces: not shown from the series, and not written as `EXDATE`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overridden: Vec<NaiveDateTime>,
}

impl CalendarEvent {
    fn parsed_rule(&self) -> Option<crate::recur::Rule> {
        let mut rule = crate::recur::parse(self.rrule.as_deref()?)?;
        for ex in self.exdates.iter().chain(&self.overridden) {
            if !rule.except.contains(ex) {
                rule.except.push(*ex);
            }
        }
        Some(rule)
    }

    fn start_wall(&self) -> NaiveDateTime {
        self.date
            .and_time(self.start_time.unwrap_or(NaiveTime::MIN))
    }

    fn end_wall(&self) -> NaiveDateTime {
        let start = self.start_wall();
        if self.all_day {
            let last = self.end_date.unwrap_or(self.date).max(self.date);
            return (last + Duration::days(1)).and_time(NaiveTime::MIN);
        }
        let end_day = self.end_date.unwrap_or(self.date);
        let end = match self.end_time {
            Some(t) => end_day.and_time(t),
            None => start + Duration::hours(1),
        };
        if end <= start {
            start + Duration::minutes(30)
        } else {
            end
        }
    }

    fn exclusive_end_date(start: NaiveDateTime, end: NaiveDateTime) -> Option<NaiveDate> {
        let last = if end.time() == NaiveTime::MIN {
            end.date() - Duration::days(1)
        } else {
            end.date()
        };
        (last > start.date()).then_some(last)
    }

    fn occurrence_clone(&self, start: NaiveDateTime, end: NaiveDateTime) -> CalendarEvent {
        let mut ev = self.clone();
        ev.date = start.date();
        ev.start_time = if self.all_day {
            None
        } else {
            Some(start.time())
        };
        ev.end_time = if self.all_day { None } else { Some(end.time()) };
        ev.end_date = Self::exclusive_end_date(start, end);
        ev.rrule = None;
        ev.exdates.clear();
        ev.overridden.clear();
        ev.zone = EventZone::Floating;
        ev
    }

    /// The event (its first occurrence) with date and times as they read in `display`, for an
    /// edit form. Zone, rule and exclusions stay as they are; put edited times back with
    /// [`CalendarEvent::times_from_display`].
    pub fn shown_in(&self, display: DisplayTz) -> CalendarEvent {
        let mut ev = self.clone();
        if self.all_day || self.start_time.is_none() || self.zone.is_floating() {
            return ev;
        }
        let (start, end) =
            crate::recur::display_range(&self.zone, self.start_wall(), self.end_wall(), display);
        ev.date = start.date();
        ev.start_time = Some(start.time());
        ev.end_time = Some(end.time());
        ev.end_date = (end.date() > start.date()).then_some(end.date());
        ev
    }

    /// Turn date and times typed in `display` back into the event's own zone, so a saved event
    /// keeps its `TZID` and its instant (the inverse of [`CalendarEvent::shown_in`]).
    pub fn times_from_display(&mut self, display: DisplayTz) {
        if self.all_day || self.start_time.is_none() || self.zone.is_floating() {
            return;
        }
        let start = self.start_wall();
        let end = self.end_wall();
        let to_event = |wall: NaiveDateTime| {
            crate::tz::wall_in_event_zone(&self.zone, display.instant_of(wall))
        };
        let (s, e) = (to_event(start), to_event(end));
        self.date = s.date();
        self.start_time = Some(s.time());
        self.end_time = Some(e.time());
        self.end_date = (e.date() > s.date()).then_some(e.date());
    }

    /// Whether the event shows on `day`, treating written dates as local (floating / all-day,
    /// or a display zone of [`DisplayTz::Local`]). Prefer [`covers_in`] when a zone is known.
    pub fn covers(&self, day: NaiveDate) -> bool {
        self.covers_in(day, DisplayTz::Local)
    }

    /// Whether `day` is the first day of an occurrence.
    pub fn starts_on(&self, day: NaiveDate) -> bool {
        self.starts_on_in(day, DisplayTz::Local)
    }

    pub fn starts_on_in(&self, day: NaiveDate, display: DisplayTz) -> bool {
        self.projected(day, display)
            .is_some_and(|ev| ev.date == day)
    }

    /// Whether the event shows on the civil date `day` in `display`.
    pub fn covers_in(&self, day: NaiveDate, display: DisplayTz) -> bool {
        self.projected(day, display).is_some()
    }

    /// A copy whose date and times are the occurrence that falls on `day` in `display`.
    /// The copy is floating (already in the display zone) and is not meant to be written back.
    pub fn projected(&self, day: NaiveDate, display: DisplayTz) -> Option<CalendarEvent> {
        if self.all_day || self.zone.is_floating() {
            return self.project_floating(day);
        }
        let start = self.start_wall();
        let length = self.end_wall() - start;
        let day_start = display.start_of_day(day);
        let day_end = display.start_of_day(day + Duration::days(1));
        let pad = length.num_seconds().abs().saturating_add(2 * 86_400);
        let from_wall = crate::tz::wall_in_event_zone(&self.zone, day_start.saturating_sub(pad));
        let to_wall = crate::tz::wall_in_event_zone(&self.zone, day_end.saturating_add(pad));
        let rule = self.parsed_rule();
        let walls = match &rule {
            None => {
                if start.date() + Duration::days(2) < from_wall.date()
                    || start.date() > to_wall.date() + Duration::days(2)
                {
                    Vec::new()
                } else {
                    vec![start]
                }
            }
            Some(rule) => {
                let mut walls = crate::recur::occurrence_walls_between(
                    rule,
                    start,
                    from_wall.date() - Duration::days(2),
                    to_wall.date(),
                );
                if let Some(until) = rule.until_utc {
                    walls.retain(|w| crate::recur::start_instant(&self.zone, *w) <= until);
                }
                walls
                    .into_iter()
                    .filter(|w| w.date() + Duration::days(2) >= from_wall.date())
                    .collect()
            }
        };
        for wall in walls {
            let end = wall + length;
            let start_i = crate::recur::start_instant(&self.zone, wall);
            let end_i = crate::recur::start_instant(&self.zone, end).max(start_i);
            if !crate::recur::occurrence_hits_display_day(
                start_i,
                end_i.saturating_sub(start_i),
                day,
                display,
                false,
            ) {
                continue;
            }
            let (shown_start, shown_end) =
                crate::recur::display_range(&self.zone, wall, end, display);
            return Some(self.occurrence_clone(shown_start, shown_end));
        }
        None
    }

    fn project_floating(&self, day: NaiveDate) -> Option<CalendarEvent> {
        let span = self
            .end_date
            .filter(|end| *end > self.date)
            .map(|end| (end - self.date).num_days())
            .unwrap_or(0);
        let start_day = match self.parsed_rule() {
            None => {
                if day >= self.date && day <= self.date + Duration::days(span) {
                    self.date
                } else {
                    return None;
                }
            }
            Some(rule) => (0..=span)
                .map(|k| day - Duration::days(k))
                .find(|d| crate::recur::occurs_on(&rule, self.date, *d))?,
        };
        let start = start_day.and_time(self.start_time.unwrap_or(NaiveTime::MIN));
        let end = if self.all_day {
            (start_day + Duration::days(span) + Duration::days(1)).and_time(NaiveTime::MIN)
        } else {
            let end_day = start_day + Duration::days(span);
            match self.end_time {
                Some(t) => {
                    let end = end_day.and_time(t);
                    if end <= start {
                        start + Duration::minutes(30)
                    } else {
                        end
                    }
                }
                None => start + Duration::hours(1),
            }
        };
        Some(self.occurrence_clone(start, end))
    }

    /// Every occurrence that starts on or before `until` (display-zone dates), as floating
    /// copies in `display` (as [`projected`] makes them). One for a single event; for a series
    /// at most what [`crate::recur::occurrence_walls`] walks (about ten years from its start).
    pub fn occurrences_until(&self, display: DisplayTz, until: NaiveDate) -> Vec<CalendarEvent> {
        let start = self.start_wall();
        let length = self.end_wall() - start;
        let floating = self.all_day || self.zone.is_floating();
        let walls = match self.parsed_rule() {
            None => vec![start],
            Some(rule) => {
                let mut walls =
                    crate::recur::occurrence_walls(&rule, start, until + Duration::days(2));
                if !floating {
                    if let Some(limit) = rule.until_utc {
                        walls.retain(|w| crate::recur::start_instant(&self.zone, *w) <= limit);
                    }
                }
                walls
            }
        };
        walls
            .into_iter()
            .map(|wall| {
                let end = wall + length;
                let (s, e) = if floating {
                    (wall, end)
                } else {
                    crate::recur::display_range(&self.zone, wall, end, display)
                };
                self.occurrence_clone(s, e)
            })
            .filter(|o| o.date <= until)
            .collect()
    }

    pub fn uid_or_id(&self) -> String {
        if self.uid.is_empty() {
            format!("{}@calendarcraft", self.id)
        } else {
            self.uid.clone()
        }
    }

    /// Minutes from midnight, for ordering; all-day events first.
    pub fn sort_key(&self) -> (u8, u32) {
        use chrono::Timelike;
        if self.all_day {
            (0, 0)
        } else {
            (
                1,
                self.start_time
                    .map(|t| t.hour() * 60 + t.minute())
                    .unwrap_or(0),
            )
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskItem {
    pub id: String,
    pub calendar_id: String,
    pub title: String,
    pub due_date: Option<NaiveDate>,
    pub due_time: Option<NaiveTime>,
    #[serde(default)]
    pub completed: bool,
    #[serde(default = "default_priority")]
    pub priority: u8, // 1 = High, 2 = Medium, 3 = Low
}

fn default_priority() -> u8 {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarStore {
    #[serde(default)]
    pub settings: AppSettings,
    #[serde(default)]
    pub accounts: Vec<AccountConfig>,
    #[serde(default)]
    pub calendars: Vec<CalendarSource>,
    #[serde(default)]
    pub calendar_sets: Vec<CalendarSet>,
    #[serde(default)]
    pub events: Vec<CalendarEvent>,
    #[serde(default)]
    pub tasks: Vec<TaskItem>,
}

impl Default for CalendarStore {
    fn default() -> Self {
        Self::sample_preloaded(Local::now().date_naive())
    }
}

impl CalendarStore {
    /// No accounts or events: what a host that brings its own accounts starts from.
    pub fn empty() -> Self {
        Self {
            settings: AppSettings {
                default_calendar_id: String::new(),
                active_calendar_set: None,
                ..AppSettings::default()
            },
            accounts: Vec::new(),
            calendars: Vec::new(),
            calendar_sets: Vec::new(),
            events: Vec::new(),
            tasks: Vec::new(),
        }
    }

    pub fn from_json_str(json: &str) -> Result<Self, String> {
        let mut store: Self =
            serde_json::from_str(json).map_err(|e| format!("Invalid config JSON: {e}"))?;
        store.settings.repair_timezone();
        store.normalize_calendar_sets();
        Ok(store)
    }

    /// Bring calendar sets from older stores up to date: drop the stored "All" list (All is
    /// computed now) and the unchanged sample sets, merge duplicate ids, and fall back to All
    /// when the active set is gone. Sets that name real calendars are kept. Idempotent.
    pub fn normalize_calendar_sets(&mut self) {
        self.calendar_sets.retain(|s| {
            s.id != ALL_SET_ID
                && !DEMO_SETS.iter().any(|(id, ids)| {
                    s.id == *id && s.calendar_ids.iter().all(|c| ids.contains(&c.as_str()))
                })
        });
        let mut seen = std::collections::HashSet::new();
        self.calendar_sets.retain(|s| seen.insert(s.id.clone()));
        for set in &mut self.calendar_sets {
            let mut ids = std::collections::HashSet::new();
            set.calendar_ids.retain(|c| ids.insert(c.clone()));
        }
        if self.active_user_set().is_none() {
            self.settings.active_calendar_set = None;
        }
    }

    /// The sets the user made (never the built-in All).
    pub fn user_sets(&self) -> impl Iterator<Item = &CalendarSet> {
        self.calendar_sets.iter().filter(|s| s.id != ALL_SET_ID)
    }

    /// The chosen user set, or `None` when All is showing.
    pub fn active_user_set(&self) -> Option<&CalendarSet> {
        let id = self.settings.active_calendar_set.as_deref()?;
        self.user_sets().find(|s| s.id == id)
    }

    /// Calendars of `set` that still exist (a set can outlive a calendar removed elsewhere).
    pub fn set_calendars<'a>(&'a self, set: &'a CalendarSet) -> impl Iterator<Item = &'a CalendarSource> {
        self.calendars
            .iter()
            .filter(move |c| set.calendar_ids.contains(&c.id))
    }

    /// Drop a calendar from every set (when the calendar is removed).
    pub fn forget_calendar_in_sets(&mut self, calendar_id: &str) {
        for set in &mut self.calendar_sets {
            set.calendar_ids.retain(|id| id != calendar_id);
        }
    }

    pub fn to_json_pretty(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| format!("Serialize error: {e}"))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_from_path_or_default(path: Option<&str>) -> (Self, Option<String>) {
        if let Some(p) = path {
            match std::fs::read_to_string(p) {
                Ok(contents) => match Self::from_json_str(&contents) {
                    Ok(store) => return (store, Some(p.to_string())),
                    Err(err) => eprintln!("Warning: could not parse config {p}: {err}"),
                },
                Err(err) => eprintln!("Warning: could not read config {p}: {err}"),
            }
        }
        if let Ok(env_path) = std::env::var("CALENDARCRAFT_CONFIG") {
            if let Ok(contents) = std::fs::read_to_string(&env_path) {
                if let Ok(store) = Self::from_json_str(&contents) {
                    return (store, Some(env_path));
                }
            }
        }
        let default_file = "calendarcraft.config.json";
        if std::path::Path::new(default_file).exists() {
            if let Ok(contents) = std::fs::read_to_string(default_file) {
                if let Ok(store) = Self::from_json_str(&contents) {
                    return (store, Some(default_file.to_string()));
                }
            }
        }
        (Self::default(), None)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn save_to_path(&self, path: &str) -> Result<(), String> {
        let json = self.to_json_pretty()?;
        if let Some(parent) = std::path::Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        std::fs::write(path, json).map_err(|e| e.to_string())
    }

    pub fn is_calendar_visible(&self, calendar_id: &str) -> bool {
        let cal_vis = self
            .calendars
            .iter()
            .find(|c| c.id == calendar_id)
            .map(|c| {
                let acc_enabled = self
                    .accounts
                    .iter()
                    .find(|a| a.id == c.account_id)
                    .map(|a| a.enabled)
                    .unwrap_or(true);
                c.visible && acc_enabled
            })
            .unwrap_or(true);

        if !cal_vis {
            return false;
        }

        // A chosen set shows only its calendars, even when it has none left (the sidebar
        // says so) rather than silently showing everything.
        match self.active_user_set() {
            Some(set) => set.calendar_ids.iter().any(|id| id == calendar_id),
            None => true,
        }
    }

    pub fn events_on_date(&self, date: NaiveDate, filter_query: &str) -> Vec<&CalendarEvent> {
        let terms = crate::search::terms(filter_query);
        let display = DisplayTz::parse(&self.settings.timezone);
        let mut list: Vec<&CalendarEvent> = self
            .events
            .iter()
            .filter(|ev| ev.covers_in(date, display) && self.is_calendar_visible(&ev.calendar_id))
            .filter(|ev| terms.is_empty() || crate::search::matches(self, ev, &terms))
            .collect();

        list.sort_by(|a, b| match (a.all_day, b.all_day) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.start_time.cmp(&b.start_time),
        });
        list
    }

    pub fn calendar_by_id(&self, id: &str) -> Option<&CalendarSource> {
        self.calendars.iter().find(|c| c.id == id)
    }

    pub fn next_id(&self, prefix: &str) -> String {
        let count =
            self.events.len() + self.tasks.len() + self.accounts.len() + self.calendars.len() + 1;
        let ts = Local::now().timestamp_subsec_millis();
        format!("{prefix}_{count}_{ts}")
    }

    pub fn sample_preloaded(anchor: NaiveDate) -> Self {
        let monday_offset = anchor.weekday().num_days_from_monday() as i64;
        let week_start = anchor - Duration::days(monday_offset);

        let accounts = vec![
            AccountConfig {
                id: "acc_work".to_string(),
                name: "Acme Workspace".to_string(),
                provider: "Google Workspace".to_string(),
                email: "alex@acme.dev".to_string(),
                server_url: Some("https://apidata.googleusercontent.com/caldav/v2".to_string()),
                enabled: true,
                ..Default::default()
            },
            AccountConfig {
                id: "acc_personal".to_string(),
                name: "Personal Cloud".to_string(),
                provider: "iCloud CalDAV".to_string(),
                email: "alex@icloud.com".to_string(),
                server_url: Some("https://caldav.icloud.com".to_string()),
                enabled: true,
                ..Default::default()
            },
            AccountConfig {
                id: "acc_studio".to_string(),
                name: "Design Studio".to_string(),
                provider: "Fastmail CalDAV".to_string(),
                email: "studio@craft.design".to_string(),
                server_url: Some("https://caldav.fastmail.com".to_string()),
                enabled: true,
                ..Default::default()
            },
        ];

        let calendars = vec![
            CalendarSource {
                id: "cal_work".to_string(),
                account_id: "acc_work".to_string(),
                name: "Engineering".to_string(),
                color_hex: "#33D1FF".to_string(),
                visible: true,
                read_only: false,
                ..Default::default()
            },
            CalendarSource {
                id: "cal_product".to_string(),
                account_id: "acc_work".to_string(),
                name: "Product Launches".to_string(),
                color_hex: "#B899EB".to_string(),
                visible: true,
                read_only: false,
                ..Default::default()
            },
            CalendarSource {
                id: "cal_personal".to_string(),
                account_id: "acc_personal".to_string(),
                name: "Personal".to_string(),
                color_hex: "#71DD8C".to_string(),
                visible: true,
                read_only: false,
                ..Default::default()
            },
            CalendarSource {
                id: "cal_family".to_string(),
                account_id: "acc_personal".to_string(),
                name: "Family & Travel".to_string(),
                color_hex: "#FFB55B".to_string(),
                visible: true,
                read_only: false,
                ..Default::default()
            },
            CalendarSource {
                id: "cal_studio".to_string(),
                account_id: "acc_studio".to_string(),
                name: "Client Reviews".to_string(),
                color_hex: "#6BE6D3".to_string(),
                visible: true,
                read_only: false,
                ..Default::default()
            },
        ];

        // No sample sets: the sidebar shows set pills only once someone makes a set.
        let calendar_sets = Vec::new();

        let hm = |h: u32, m: u32| NaiveTime::from_hms_opt(h, m, 0);

        let events = vec![
            CalendarEvent {
                id: "ev_1".to_string(),
                calendar_id: "cal_product".to_string(),
                title: "Q4 Release Milestone".to_string(),
                date: anchor,
                start_time: None,
                end_time: None,
                all_day: true,
                location: Some("Remote & HQ".to_string()),
                conference_url: None,
                notes: "Feature freeze and release candidate sign-off.".to_string(),
                attendees: vec!["team@acme.dev".to_string()],
                alert_minutes_before: Some(60),
                ..Default::default()
            },
            CalendarEvent {
                id: "ev_2".to_string(),
                calendar_id: "cal_work".to_string(),
                title: "Architecture Sync".to_string(),
                date: week_start,
                start_time: hm(9, 30),
                end_time: hm(10, 30),
                all_day: false,
                location: Some("Room 4B".to_string()),
                conference_url: Some("https://meet.acme.dev/arch-sync".to_string()),
                notes: "Review Rust & WebAssembly shared core layout.".to_string(),
                attendees: vec!["maya@acme.dev".to_string(), "liam@acme.dev".to_string()],
                alert_minutes_before: Some(10),
                ..Default::default()
            },
            CalendarEvent {
                id: "ev_3".to_string(),
                calendar_id: "cal_personal".to_string(),
                title: "Morning Run & Espresso".to_string(),
                date: anchor,
                start_time: hm(8, 0),
                end_time: hm(9, 0),
                all_day: false,
                location: Some("Waterfront Trail\nPier 7\nSan Francisco, CA".to_string()),
                conference_url: None,
                notes: "5km tempo run.".to_string(),
                attendees: vec![],
                alert_minutes_before: Some(15),
                ..Default::default()
            },
            CalendarEvent {
                id: "ev_4".to_string(),
                calendar_id: "cal_work".to_string(),
                title: "MCP Protocol Integration".to_string(),
                date: anchor,
                start_time: hm(10, 30),
                end_time: hm(12, 0),
                all_day: false,
                location: Some("Studio Desk".to_string()),
                conference_url: Some("https://meet.acme.dev/mcp-demo".to_string()),
                notes: "Live test of JSON-RPC tool calls against preloaded accounts.".to_string(),
                attendees: vec!["alex@acme.dev".to_string(), "sam@acme.dev".to_string()],
                alert_minutes_before: Some(10),
                ..Default::default()
            },
            CalendarEvent {
                id: "ev_5".to_string(),
                calendar_id: "cal_studio".to_string(),
                title: "Design Critique: Typography & Grid".to_string(),
                date: anchor,
                start_time: hm(14, 0),
                end_time: hm(15, 15),
                all_day: false,
                location: Some("Figma Live".to_string()),
                conference_url: Some("https://meet.craft.design/critique".to_string()),
                notes: "Inspect week timeline contrast and event pill spacing.".to_string(),
                attendees: vec!["nina@craft.design".to_string()],
                alert_minutes_before: Some(5),
                ..Default::default()
            },
            CalendarEvent {
                id: "ev_6".to_string(),
                calendar_id: "cal_family".to_string(),
                title: "Dinner at Osteria".to_string(),
                date: anchor + Duration::days(1),
                start_time: hm(18, 30),
                end_time: hm(20, 0),
                all_day: false,
                location: Some("420 Market St".to_string()),
                conference_url: None,
                notes: "Reservation under Alex.".to_string(),
                attendees: vec!["jordan@icloud.com".to_string()],
                alert_minutes_before: Some(30),
                ..Default::default()
            },
            CalendarEvent {
                id: "ev_7".to_string(),
                calendar_id: "cal_product".to_string(),
                title: "Customer Advisory Demo".to_string(),
                date: week_start + Duration::days(3),
                start_time: hm(11, 0),
                end_time: hm(12, 30),
                all_day: false,
                location: Some("Boardroom A".to_string()),
                conference_url: Some("https://meet.acme.dev/advisory".to_string()),
                notes: "Walk through preloaded configuration workflows.".to_string(),
                attendees: vec!["pm@acme.dev".to_string()],
                alert_minutes_before: Some(15),
                ..Default::default()
            },
            CalendarEvent {
                id: "ev_8".to_string(),
                calendar_id: "cal_work".to_string(),
                title: "Sprint Retrospective".to_string(),
                date: week_start + Duration::days(4),
                start_time: hm(15, 0),
                end_time: hm(16, 0),
                all_day: false,
                location: Some("Zoom".to_string()),
                conference_url: Some("https://meet.acme.dev/retro".to_string()),
                notes: "Celebrate shipping native + WASM builds.".to_string(),
                attendees: vec!["team@acme.dev".to_string()],
                alert_minutes_before: Some(10),
                ..Default::default()
            },
            CalendarEvent {
                id: "ev_standup".to_string(),
                calendar_id: "cal_work".to_string(),
                title: "Stand-up".to_string(),
                date: week_start - Duration::days(21),
                start_time: hm(9, 0),
                end_time: hm(9, 15),
                location: Some("Huddle".to_string()),
                rrule: Some("FREQ=WEEKLY;BYDAY=MO,WE,FR".to_string()),
                alert_minutes_before: Some(5),
                ..Default::default()
            },
            CalendarEvent {
                id: "ev_offsite".to_string(),
                calendar_id: "cal_studio".to_string(),
                title: "Lisbon Offsite".to_string(),
                date: week_start + Duration::days(9),
                end_date: Some(week_start + Duration::days(11)),
                all_day: true,
                location: Some("Lisbon".to_string()),
                notes: "Workshops, design reviews and a team dinner.".to_string(),
                ..Default::default()
            },
            CalendarEvent {
                id: "ev_dentist".to_string(),
                calendar_id: "cal_personal".to_string(),
                title: "Dentist".to_string(),
                date: week_start - Duration::days(5),
                start_time: hm(16, 0),
                end_time: hm(16, 45),
                location: Some("Dr. Weiss".to_string()),
                ..Default::default()
            },
            CalendarEvent {
                id: "ev_party".to_string(),
                calendar_id: "cal_family".to_string(),
                title: "Holiday Party".to_string(),
                date: week_start + Duration::days(26),
                start_time: hm(19, 0),
                end_time: hm(23, 0),
                location: Some("Rooftop".to_string()),
                ..Default::default()
            },
            CalendarEvent {
                id: "ev_strategy".to_string(),
                calendar_id: "cal_product".to_string(),
                title: "Product Strategy Review".to_string(),
                date: week_start + Duration::days(44),
                start_time: hm(13, 0),
                end_time: hm(15, 0),
                ..Default::default()
            },
            CalendarEvent {
                id: "ev_bday".to_string(),
                calendar_id: "cal_family".to_string(),
                title: "Mia’s Birthday".to_string(),
                date: week_start + Duration::days(17),
                all_day: true,
                rrule: Some("FREQ=YEARLY".to_string()),
                ..Default::default()
            },
        ];

        let tasks = vec![
            TaskItem {
                id: "task_1".to_string(),
                calendar_id: "cal_work".to_string(),
                title: "Publish WASM web bundle & config template".to_string(),
                due_date: Some(anchor),
                due_time: hm(16, 0),
                completed: false,
                priority: 1,
            },
            TaskItem {
                id: "task_2".to_string(),
                calendar_id: "cal_personal".to_string(),
                title: "Book train tickets for weekend trip".to_string(),
                due_date: Some(anchor + Duration::days(1)),
                due_time: None,
                completed: false,
                priority: 2,
            },
            TaskItem {
                id: "task_3".to_string(),
                calendar_id: "cal_product".to_string(),
                title: "Verify MCP server tools/list schema".to_string(),
                due_date: Some(anchor),
                due_time: hm(11, 0),
                completed: true,
                priority: 1,
            },
        ];

        let _ = Weekday::Mon;
        Self {
            settings: AppSettings::default(),
            accounts,
            calendars,
            calendar_sets,
            events,
            tasks,
        }
    }
}

pub fn parse_hex_color(hex: &str) -> Option<[u8; 3]> {
    let clean = hex.trim().trim_start_matches('#');
    if clean.len() == 6 {
        let r = u8::from_str_radix(&clean[0..2], 16).ok()?;
        let g = u8::from_str_radix(&clean[2..4], 16).ok()?;
        let b = u8::from_str_radix(&clean[4..6], 16).ok()?;
        Some([r, g, b])
    } else {
        None
    }
}

#[cfg(test)]
mod set_tests {
    use super::*;

    fn set(id: &str, ids: &[&str]) -> CalendarSet {
        CalendarSet { id: id.into(), name: id.into(), calendar_ids: ids.iter().map(|s| s.to_string()).collect() }
    }

    #[test]
    fn old_stores_lose_all_and_unchanged_sample_sets() {
        let mut s = CalendarStore::sample_preloaded(NaiveDate::from_ymd_opt(2026, 11, 24).unwrap());
        s.calendar_sets = vec![
            set(ALL_SET_ID, &["cal_work", "cal_personal"]),
            set("set_focus", &["cal_work", "cal_product", "cal_studio"]),
            set("set_home", &[]),
            set("set_mine", &["cal_personal", "cal_personal"]),
        ];
        s.settings.active_calendar_set = Some("set_focus".into());
        let json = s.to_json_pretty().unwrap();
        let s = CalendarStore::from_json_str(&json).unwrap();
        assert_eq!(s.calendar_sets, vec![set("set_mine", &["cal_personal"])]);
        assert_eq!(s.settings.active_calendar_set, None, "the active set is gone: All");
        assert!(s.is_calendar_visible("cal_work"));
    }

    #[test]
    fn changed_sample_sets_are_kept() {
        let mut s = CalendarStore::empty();
        s.calendar_sets = vec![set("set_home", &["cal_personal", "icloud_home"])];
        s.settings.active_calendar_set = Some("set_home".into());
        s.normalize_calendar_sets();
        assert_eq!(s.calendar_sets.len(), 1);
        assert_eq!(s.settings.active_calendar_set.as_deref(), Some("set_home"));
    }

    #[test]
    fn an_empty_set_shows_nothing_and_all_shows_everything() {
        let mut s = CalendarStore::sample_preloaded(NaiveDate::from_ymd_opt(2026, 11, 24).unwrap());
        assert_eq!(s.user_sets().count(), 0, "no sample sets");
        assert!(s.is_calendar_visible("cal_work"));
        s.calendar_sets.push(set("work", &["cal_work"]));
        s.settings.active_calendar_set = Some("work".into());
        assert!(s.is_calendar_visible("cal_work"));
        assert!(!s.is_calendar_visible("cal_personal"));
        s.forget_calendar_in_sets("cal_work");
        assert!(!s.is_calendar_visible("cal_work"), "an emptied set does not fall back to all");
        s.settings.active_calendar_set = Some(ALL_SET_ID.into());
        assert!(s.is_calendar_visible("cal_personal"));
    }
}
