use chrono::{Datelike, Duration, Local, NaiveDate, NaiveTime, Weekday};
pub use crate::nlp::NaiveDateExt;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ViewMode {
    Day,
    Week,
    Month,
    Quarter,
    Year,
}

impl Default for ViewMode {
    fn default() -> Self {
        Self::Month
    }
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
            timezone: "America/Los_Angeles".to_string(),
            default_calendar_id: "cal_work".to_string(),
            active_calendar_set: Some("set_all".to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountConfig {
    pub id: String,
    pub name: String,
    pub provider: String, // e.g., "iCloud", "Google", "Exchange", "CalDAV"
    pub email: String,
    #[serde(default)]
    pub server_url: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarSource {
    pub id: String,
    pub account_id: String,
    pub name: String,
    pub color_hex: String,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub read_only: bool,
}

impl CalendarSource {
    pub fn rgb(&self) -> [u8; 3] {
        parse_hex_color(&self.color_hex).unwrap_or([59, 130, 246])
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarSet {
    pub id: String,
    pub name: String,
    pub calendar_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarEvent {
    pub id: String,
    pub calendar_id: String,
    pub title: String,
    pub date: NaiveDate,
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
    pub fn from_json_str(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| format!("Invalid config JSON: {e}"))
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

        if let Some(ref set_id) = self.settings.active_calendar_set {
            if let Some(set) = self.calendar_sets.iter().find(|s| &s.id == set_id) {
                if !set.calendar_ids.is_empty() {
                    return set.calendar_ids.iter().any(|id| id == calendar_id);
                }
            }
        }
        true
    }

    pub fn events_on_date(&self, date: NaiveDate, filter_query: &str) -> Vec<&CalendarEvent> {
        let q = filter_query.trim().to_lowercase();
        let mut list: Vec<&CalendarEvent> = self
            .events
            .iter()
            .filter(|ev| ev.date == date && self.is_calendar_visible(&ev.calendar_id))
            .filter(|ev| {
                if q.is_empty() {
                    true
                } else {
                    ev.title.to_lowercase().contains(&q)
                        || ev
                            .location
                            .as_deref()
                            .unwrap_or("")
                            .to_lowercase()
                            .contains(&q)
                        || ev.notes.to_lowercase().contains(&q)
                }
            })
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
        let count = self.events.len() + self.tasks.len() + self.accounts.len() + self.calendars.len() + 1;
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
            },
            AccountConfig {
                id: "acc_personal".to_string(),
                name: "Personal Cloud".to_string(),
                provider: "iCloud CalDAV".to_string(),
                email: "alex@icloud.com".to_string(),
                server_url: Some("https://caldav.icloud.com".to_string()),
                enabled: true,
            },
            AccountConfig {
                id: "acc_studio".to_string(),
                name: "Design Studio".to_string(),
                provider: "Fastmail CalDAV".to_string(),
                email: "studio@craft.design".to_string(),
                server_url: Some("https://caldav.fastmail.com".to_string()),
                enabled: true,
            },
        ];

        let calendars = vec![
            CalendarSource {
                id: "cal_work".to_string(),
                account_id: "acc_work".to_string(),
                name: "Engineering".to_string(),
                color_hex: "#3B82F6".to_string(),
                visible: true,
                read_only: false,
            },
            CalendarSource {
                id: "cal_product".to_string(),
                account_id: "acc_work".to_string(),
                name: "Product Launches".to_string(),
                color_hex: "#8B5CF6".to_string(),
                visible: true,
                read_only: false,
            },
            CalendarSource {
                id: "cal_personal".to_string(),
                account_id: "acc_personal".to_string(),
                name: "Personal".to_string(),
                color_hex: "#10B981".to_string(),
                visible: true,
                read_only: false,
            },
            CalendarSource {
                id: "cal_family".to_string(),
                account_id: "acc_personal".to_string(),
                name: "Family & Travel".to_string(),
                color_hex: "#F59E0B".to_string(),
                visible: true,
                read_only: false,
            },
            CalendarSource {
                id: "cal_studio".to_string(),
                account_id: "acc_studio".to_string(),
                name: "Client Reviews".to_string(),
                color_hex: "#EC4899".to_string(),
                visible: true,
                read_only: false,
            },
        ];

        let calendar_sets = vec![
            CalendarSet {
                id: "set_all".to_string(),
                name: "All Calendars".to_string(),
                calendar_ids: calendars.iter().map(|c| c.id.clone()).collect(),
            },
            CalendarSet {
                id: "set_focus".to_string(),
                name: "Work Focus".to_string(),
                calendar_ids: vec![
                    "cal_work".to_string(),
                    "cal_product".to_string(),
                    "cal_studio".to_string(),
                ],
            },
            CalendarSet {
                id: "set_home".to_string(),
                name: "Home & Personal".to_string(),
                calendar_ids: vec!["cal_personal".to_string(), "cal_family".to_string()],
            },
        ];

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
            },
            CalendarEvent {
                id: "ev_3".to_string(),
                calendar_id: "cal_personal".to_string(),
                title: "Morning Run & Espresso".to_string(),
                date: anchor,
                start_time: hm(8, 0),
                end_time: hm(9, 0),
                all_day: false,
                location: Some("Waterfront Trail".to_string()),
                conference_url: None,
                notes: "5km tempo run.".to_string(),
                attendees: vec![],
                alert_minutes_before: Some(15),
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
