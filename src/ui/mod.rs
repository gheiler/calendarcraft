//! CalendarCraft's window: a Fantastical-style layout with a sidebar (quick entry, mini
//! calendar, calendars, the day-by-day list) and the calendar (Day, Week, Month, Quarter, Year),
//! event details in a popover and new events in a dialog. Narrow windows get a phone layout.

mod files;
mod icons;
mod popups;
mod search;
mod sets;
mod side;
mod views;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use chrono::{Datelike, Duration, Local, NaiveDate, NaiveTime};
use eframe::egui::{self, Color32, Rect};
use serde::{Deserialize, Serialize};

use crate::host::{self, HostConfig, HostInbox, HostMsg};
use crate::i18n::{self, t};
use crate::ics;
use crate::model::{
    AccountConfig, AccountKind, CalendarEvent, CalendarSource, CalendarStore, ViewMode,
};
use crate::sync::{self, Endpoint, Inbox, SyncMsg};
use crate::theme::{self, Palette, ThemeChoice};

/// Options kept between runs (`calendarcraft.ui`). artcraftCloud writes `theme` and `language`
/// here from its Settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiPrefs {
    pub theme: String,
    /// `auto` or a language code (`en`, `de`, `es`, `fr`, `it`, `pt-br`).
    pub language: String,
    /// `system`, `on` or `off`.
    pub animations: String,
    pub view: ViewMode,
    pub sidebar: bool,
    /// First hour shown when a day or week opens.
    pub day_start: u8,
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            language: "auto".into(),
            animations: "system".into(),
            view: ViewMode::Month,
            sidebar: true,
            day_start: 7,
        }
    }
}

/// How the app was started (command line natively, the page address on the web).
#[derive(Debug, Clone, Default)]
pub struct Launch {
    pub config_path: Option<String>,
    pub screenshot: Option<String>,
    pub view: Option<ViewMode>,
    pub theme: Option<String>,
    pub date: Option<NaiveDate>,
    /// Open the details popover of this event.
    pub open_event: Option<String>,
    /// Select this event (highlighted, details not opened). For screenshots.
    pub select_event: Option<String>,
    /// Ask before deleting the selected or open event. For screenshots.
    pub confirm_delete: bool,
    /// Open the new-event dialog.
    pub new_event: bool,
    /// Use the sample data, ignoring what is stored.
    pub demo: bool,
    pub language: Option<String>,
    /// Open Settings on this tab (0 General, 1 Accounts, 2 Sets, 3 MCP).
    pub settings: Option<u8>,
    /// Show or hide the sidebar, overriding the saved choice.
    pub sidebar: Option<bool>,
    /// Make the sample "Personal Cloud" a CalDAV account with a long URL and no saved
    /// password (for screenshots of the account list and the missing-password error).
    pub caldav_sample: bool,
    /// Open the edit form of this account in Settings › Accounts (for screenshots).
    pub edit_account: Option<String>,
    /// Add two sample calendar sets to the demo data (for screenshots).
    pub sample_sets: bool,
    /// Start with this text in the search field (for screenshots).
    pub search: Option<String>,
}

/// The `.ics` document the host opened (artcraftCloud's drive).
#[derive(Debug, Clone)]
pub struct FileDoc {
    pub name: String,
    pub account_id: String,
    pub calendar_id: String,
    pub dirty: bool,
    /// Drive path (file id) when the host opened an existing file; none for Untitled.
    pub path: Option<String>,
    pub etag: Option<String>,
}

/// The details popover.
#[derive(Debug, Clone)]
pub struct Inspector {
    pub event_id: String,
    /// The event's rectangle on screen, to place the popover beside it.
    pub anchor: Option<Rect>,
    pub draft: CalendarEvent,
    pub date_text: String,
    pub start_text: String,
    pub end_text: String,
    pub confirm_delete: bool,
}

/// The new-event dialog.
#[derive(Debug, Clone)]
pub struct NewEvent {
    pub sentence: String,
    /// The day the dialog was opened for: the event's day unless the sentence names one.
    pub day: NaiveDate,
    pub draft: CalendarEvent,
    pub date_text: String,
    pub start_text: String,
    pub end_text: String,
    pub focus_done: bool,
}

#[derive(Debug, Clone, Default)]
pub struct AccountForm {
    pub kind_ics: bool,
    pub name: String,
    pub url: String,
    pub username: String,
    pub password: String,
    pub error: Option<String>,
}

/// Editing an existing account in Settings › Accounts. The id stays the same, so its
/// calendars, colours and stored password stay attached. An empty password keeps the
/// stored one.
#[derive(Debug, Clone, Default)]
pub struct AccountEdit {
    pub id: String,
    pub name: String,
    pub url: String,
    pub username: String,
    pub password: String,
    pub error: Option<String>,
}

/// Shown on a CalDAV account that would be asked without a password.
pub const MISSING_PASSWORD: &str = "No saved password for this account. Enter it again in Settings › Accounts.";

/// Whether a request for `acc` would go out without the password it needs: a CalDAV
/// account with a user name, signed by this app (not by the host or its proxy), and no
/// password in the store.
pub fn password_missing(acc: &AccountConfig, has_password: bool, through_host: bool) -> bool {
    acc.kind == AccountKind::Caldav
        && !acc.host_managed
        && !through_host
        && !has_password
        && acc.username.as_deref().is_some_and(|u| !u.trim().is_empty())
}

pub struct CalendarCraftApp {
    pub store: CalendarStore,
    pub launch: Launch,
    pub selected_date: NaiveDate,
    pub view_mode: ViewMode,
    pub search_query: String,
    search_ui: search::SearchUi,
    /// Settings › Sets: the set being made or changed, and the one asked about deleting.
    pub set_form: Option<sets::SetForm>,
    pub set_delete: Option<String>,
    pub quick_input: String,
    pub prefs: UiPrefs,
    pub palette: Palette,
    applied_style: Option<(bool, bool, String)>,
    fonts_ready: bool,

    pub inspector: Option<Inspector>,
    /// The event a single click picked (highlighted; Delete/Backspace removes it after asking).
    pub selected_event: Option<String>,
    /// The event the delete confirmation is asking about.
    pub delete_confirm: Option<String>,
    pub new_event: Option<NewEvent>,
    pub settings_open: bool,
    pub settings_tab: u8,
    /// Last drawn size of each dialog (see `settle`).
    pub popup_sizes: HashMap<&'static str, egui::Vec2>,
    pub account_form: AccountForm,
    pub mcp_open: bool,
    pub mcp_request: String,
    pub mcp_response: String,
    pub calendars_open: bool,
    pub mobile_menu: bool,

    pub banner: Option<(String, bool)>,
    banner_until: f64,
    pub status: HashMap<String, String>,
    /// Accounts whose error details are unfolded in Settings.
    pub error_details: HashSet<String>,
    pub busy: usize,

    inbox: Option<Inbox>,
    host_inbox: HostInbox,
    pub host: HostConfig,
    pub secrets: BTreeMap<String, String>,
    /// The password store has been read (until then a missing password is not an error yet).
    pub secrets_ready: bool,
    pub account_edit: Option<AccountEdit>,
    pub file_doc: Option<FileDoc>,
    /// File-backed calendars in a host, keyed by calendar id, referenced by drive path.
    pub file_cals: HashMap<String, files::FileCal>,
    pending_files: HashMap<String, files::PendingFile>,
    next_file_req: u64,
    pub file_conflict: Option<files::FileConflict>,
    pub migration: Option<files::MigrationNotice>,
    last_file_sync: f64,
    last_file_autosave: f64,
    file_sync_requested: bool,
    loaded: HashMap<String, (NaiveDate, NaiveDate)>,

    /// The mini calendar's month (follows the selection unless paged).
    pub mini_month: NaiveDate,
    /// Scroll the list to this day on the next frame (the selection moved elsewhere).
    pub agenda_scroll_to: Option<NaiveDate>,
    pub agenda_center: NaiveDate,
    pub agenda_offset: f32,
    /// Scroll the day/week grid to the morning once.
    pub timeline_scrolled: bool,
    pub scroll_accum: f32,

    /// The Month strip: its top edge in week rows from 5 Jan 1970 (NaN until first drawn), where
    /// it glides to, the selection it last followed, the month in focus and its top week.
    pub month_pos: f64,
    pub month_target: Option<f64>,
    pub month_synced: NaiveDate,
    pub month_focus: NaiveDate,
    pub month_top_week: NaiveDate,
    pub month_input_at: f64,
    pub month_settle: bool,
    /// The list was scrolled by hand to this day (the Month strip follows).
    pub agenda_moved: Option<NaiveDate>,
    /// The first day the list shows at its top.
    pub agenda_top: Option<NaiveDate>,
    pub last_view: Option<ViewMode>,
    /// The sidebar over the calendar (narrow windows, or peeked from the left edge).
    pub sidebar_overlay: bool,
    pub sidebar_peek: bool,
    pub edge_since: Option<f64>,
    /// The search field opened from its icon in a narrow toolbar.
    pub search_open: bool,
    /// Enter was pressed in the quick entry this frame.
    pub quick_enter: bool,
    motion: bool,

    dirty_store: bool,
    last_saved: f64,
    /// The day the window/Dock icon shows.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    icon_day: Option<NaiveDate>,
    now: f64,
    frame: u32,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    shot_requested: bool,
    published: u64,
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

/// What a click on an event does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventClick {
    Select,
    Open,
}

/// The first click selects an event; a double-click, or a click on the event already
/// selected, opens it.
pub fn event_click(selected: Option<&str>, id: &str, double: bool) -> EventClick {
    if double || selected == Some(id) {
        EventClick::Open
    } else {
        EventClick::Select
    }
}

impl CalendarCraftApp {
    pub fn new(store: Option<CalendarStore>, launch: Launch) -> Self {
        let mut prefs: UiPrefs = host::load(host::KEY_UI)
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        if let Some(t) = &launch.theme {
            prefs.theme = t.clone();
        }
        if let Some(l) = &launch.language {
            prefs.language = l.clone();
        }
        if let Some(sb) = launch.sidebar {
            prefs.sidebar = sb;
        }
        let host_cfg = host::host_config();
        let hosted = host::hosted();
        let anchor = launch.date.unwrap_or_else(today);
        let mut store = match store {
            Some(s) => s,
            None if launch.demo => CalendarStore::sample_preloaded(anchor),
            None => match host::load(host::KEY_STORE)
                .and_then(|s| CalendarStore::from_json_str(&s).ok())
            {
                Some(s) => s,
                // A host that signs people in starts empty (their server calendars come in);
                // a plain page shows the sample so there is something to look at.
                None if hosted || !host_cfg.accounts.is_empty() => CalendarStore::empty(),
                None => CalendarStore::sample_preloaded(anchor),
            },
        };
        if !launch.demo {
            if let Some(cache) = host::load(host::KEY_CACHE)
                .and_then(|s| serde_json::from_str::<Vec<CalendarEvent>>(&s).ok())
            {
                let known: Vec<String> = store.events.iter().map(|e| e.id.clone()).collect();
                store
                    .events
                    .extend(cache.into_iter().filter(|e| !known.contains(&e.id)));
            }
        }
        Self::drop_ephemeral_file_accounts(&mut store);
        store.normalize_calendar_sets();
        if launch.demo && launch.sample_sets {
            store.calendar_sets.push(crate::model::CalendarSet {
                id: "set_work".into(),
                name: "Work".into(),
                calendar_ids: vec!["cal_work".into(), "cal_product".into(), "cal_studio".into()],
            });
            store.calendar_sets.push(crate::model::CalendarSet {
                id: "set_family".into(),
                name: "Family".into(),
                calendar_ids: vec!["cal_personal".into(), "cal_family".into()],
            });
        }
        if launch.demo && launch.caldav_sample {
            if let Some(acc) = store.accounts.iter_mut().find(|a| a.id == "acc_personal") {
                acc.kind = AccountKind::Caldav;
                acc.username = Some(acc.email.clone());
                acc.server_url = Some("https://p142-caldav.icloud.com/1234567890/calendars/home/family-and-travel-shared-with-everyone/".into());
            }
        }
        // Passwords are loaded in first_frame (and a leftover plaintext file is migrated then).
        let secrets: BTreeMap<String, String> = BTreeMap::new();
        let view_mode = launch.view.unwrap_or(prefs.view);
        i18n::set_language(&prefs.language);
        let sample_mcp = serde_json::to_string_pretty(&serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "quick_add_natural", "arguments": { "sentence": "Product review tomorrow at 2pm-3pm @ Boardroom" } }
        }))
        .unwrap_or_default();
        let mut app = Self {
            store,
            selected_date: anchor,
            view_mode,
            search_query: String::new(),
            search_ui: search::SearchUi::default(),
            set_form: None,
            set_delete: None,
            quick_input: String::new(),
            palette: theme::palette(false),
            prefs,
            applied_style: None,
            fonts_ready: false,
            inspector: None,
            selected_event: None,
            delete_confirm: None,
            new_event: None,
            settings_open: false,
            settings_tab: 0,
            popup_sizes: HashMap::new(),
            account_form: AccountForm::default(),
            mcp_open: false,
            mcp_request: sample_mcp,
            mcp_response: String::new(),
            calendars_open: true,
            mobile_menu: false,
            banner: None,
            banner_until: 0.0,
            status: HashMap::new(),
            secrets_ready: false,
            account_edit: None,
            error_details: HashSet::new(),
            busy: 0,
            inbox: None,
            host_inbox: Arc::new(Mutex::new(Vec::new())),
            host: host_cfg,
            secrets,
            file_doc: None,
            file_cals: HashMap::new(),
            pending_files: HashMap::new(),
            next_file_req: 0,
            file_conflict: None,
            migration: None,
            last_file_sync: 0.0,
            last_file_autosave: 0.0,
            file_sync_requested: false,
            loaded: HashMap::new(),
            mini_month: first_of_month(anchor),
            agenda_scroll_to: Some(anchor),
            agenda_center: anchor,
            agenda_offset: 0.0,
            timeline_scrolled: false,
            scroll_accum: 0.0,
            month_pos: f64::NAN,
            month_target: None,
            month_synced: anchor,
            month_focus: first_of_month(anchor),
            month_top_week: week_start(anchor),
            month_input_at: 0.0,
            month_settle: false,
            agenda_moved: None,
            agenda_top: None,
            last_view: None,
            sidebar_overlay: false,
            sidebar_peek: false,
            edge_since: None,
            search_open: false,
            quick_enter: false,
            motion: true,
            dirty_store: false,
            last_saved: 0.0,
            icon_day: None,
            now: 0.0,
            frame: 0,
            shot_requested: false,
            published: 0,
            launch,
        };
        app.adopt_host_accounts();
        app
    }

    /// Accounts the host provides (artcraftCloud: the signed-in person's Stalwart calendars).
    fn adopt_host_accounts(&mut self) {
        for h in self.host.accounts.clone() {
            let acc = AccountConfig {
                id: h.id.clone(),
                name: h.name.clone(),
                provider: h.provider.clone().unwrap_or_else(|| "CalDAV".into()),
                email: h.email.clone(),
                server_url: Some(h.url.clone()),
                enabled: true,
                kind: AccountKind::Caldav,
                username: None,
                host_managed: true,
                via_proxy: false,
            };
            match self.store.accounts.iter_mut().find(|a| a.id == h.id) {
                Some(existing) => {
                    let enabled = existing.enabled;
                    *existing = acc;
                    existing.enabled = enabled;
                }
                None => self.store.accounts.insert(0, acc),
            }
        }
        // Host accounts that are gone (another person signed in) leave with their calendars.
        let host_ids: Vec<String> = self.host.accounts.iter().map(|h| h.id.clone()).collect();
        let gone: Vec<String> = self
            .store
            .accounts
            .iter()
            .filter(|a| a.host_managed && !host_ids.contains(&a.id))
            .map(|a| a.id.clone())
            .collect();
        for id in gone {
            self.remove_account(&id);
        }
    }

    // -----------------------------------------------------------------------------------------
    // Helpers

    pub fn cal_color(&self, cal_id: &str) -> Color32 {
        self.store
            .calendar_by_id(cal_id)
            .map(|c| theme::color_of(&c.color_hex))
            .unwrap_or(theme::PURPLE)
    }

    pub fn account_of(&self, cal_id: &str) -> Option<&AccountConfig> {
        let cal = self.store.calendar_by_id(cal_id)?;
        self.store.accounts.iter().find(|a| a.id == cal.account_id)
    }

    pub fn calendar_writable(&self, cal_id: &str) -> bool {
        let Some(cal) = self.store.calendar_by_id(cal_id) else {
            return false;
        };
        !cal.read_only
            && self
                .account_of(cal_id)
                .is_none_or(|a| a.kind != AccountKind::Ics)
    }

    /// Calendars new events can go to, the default first.
    pub fn writable_calendars(&self) -> Vec<CalendarSource> {
        let mut list: Vec<CalendarSource> = self
            .store
            .calendars
            .iter()
            .filter(|c| self.calendar_writable(&c.id))
            .cloned()
            .collect();
        let preferred = self.default_calendar();
        list.sort_by_key(|c| (Some(&c.id) != preferred.as_ref()) as u8);
        list
    }

    pub fn default_calendar(&self) -> Option<String> {
        let wanted = &self.store.settings.default_calendar_id;
        if self.calendar_writable(wanted) && self.store.is_calendar_visible(wanted) {
            return Some(wanted.clone());
        }
        // The open document first, then the host's server, then anything writable.
        if let Some(doc) = &self.file_doc {
            return Some(doc.calendar_id.clone());
        }
        let host_cal = self.store.calendars.iter().find(|c| {
            self.account_of(&c.id).is_some_and(|a| a.host_managed) && self.calendar_writable(&c.id)
        });
        host_cal
            .or_else(|| {
                self.store
                    .calendars
                    .iter()
                    .find(|c| self.calendar_writable(&c.id))
            })
            .map(|c| c.id.clone())
    }

    pub fn events_on(&self, day: NaiveDate) -> Vec<CalendarEvent> {
        let display = crate::tz::DisplayTz::parse(&self.store.settings.timezone);
        self.store
            .events_on_date(day, "")
            .into_iter()
            .filter_map(|ev| ev.projected(day, display))
            .collect()
    }

    pub fn select_date(&mut self, day: NaiveDate) {
        if day != self.selected_date {
            self.selected_date = day;
            self.agenda_scroll_to = Some(day);
            self.mini_month = first_of_month(day);
            self.timeline_scrolled = false;
        }
    }

    /// Today: select it and, in Month, bring its month back into view even when the selection
    /// was already today (the strip may have been scrolled away).
    pub fn go_today(&mut self) {
        let now = today();
        self.select_date(now);
        if !self.month_pos.is_nan() {
            self.month_synced = now;
            self.month_target = Some(views::row_of(week_start(first_of_month(now))) as f64);
        }
        self.agenda_scroll_to = Some(now);
    }

    pub fn set_view(&mut self, mode: ViewMode) {
        self.view_mode = mode;
        self.prefs.view = mode;
        self.timeline_scrolled = false;
        self.dirty_store = true;
    }

    pub fn flash(&mut self, text: impl Into<String>, error: bool) {
        self.banner = Some((text.into(), error));
        self.banner_until = self.now + if error { 8.0 } else { 4.0 };
    }

    /// egui sizes grids, scroll areas and modals from the frame before; draw again until a
    /// dialog stops changing size, so it never rests half-measured (cut off at the bottom).
    pub fn settle(&mut self, ctx: &egui::Context, key: &'static str, size: egui::Vec2) {
        if self.popup_sizes.get(key) != Some(&size) {
            self.popup_sizes.insert(key, size);
            ctx.request_repaint();
        }
    }

    pub fn reduced_motion(&self) -> bool {
        !self.motion
    }

    pub fn step(&mut self, dir: i64) {
        let mut d = self.selected_date;
        if self.view_mode == ViewMode::Month && !self.month_pos.is_nan() {
            // From the month in view (the strip may have been scrolled away from the selection).
            let focus = self.month_focus;
            d = NaiveDate::from_ymd_opt(
                focus.year(),
                focus.month(),
                d.day().min(days_in_month(focus.year(), focus.month())),
            )
            .unwrap_or(focus);
        }
        let next = match self.view_mode {
            ViewMode::Day => d + Duration::days(dir),
            ViewMode::Week => d + Duration::days(7 * dir),
            ViewMode::Month => add_months(d, dir as i32),
            ViewMode::Quarter => add_months(d, 3 * dir as i32),
            ViewMode::Year => add_months(d, 12 * dir as i32),
        };
        self.select_date(next);
    }

    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    // -----------------------------------------------------------------------------------------
    // Changes

    fn new_uid(&self) -> String {
        let now = Local::now();
        format!(
            "{}-{:x}@calendarcraft",
            now.format("%Y%m%dT%H%M%S"),
            now.timestamp_subsec_nanos()
                ^ (self.store.events.len() as u32).wrapping_mul(2654435761)
        )
    }

    /// Add a new event, or replace an existing one with the same id, and send it where its
    /// calendar lives.
    pub fn commit_event(&mut self, mut ev: CalendarEvent) {
        if ev.uid.is_empty() {
            ev.uid = self.new_uid();
        }
        if ev.end_date.is_some_and(|e| e <= ev.date) {
            ev.end_date = None;
        }
        let previous = self.store.events.iter().find(|e| e.id == ev.id).cloned();
        // Moved to a calendar on another server: delete there, create here.
        if let Some(prev) = &previous {
            if prev.calendar_id != ev.calendar_id {
                self.push_delete(prev);
                ev.href = None;
                ev.etag = None;
            }
        }
        match self.store.events.iter_mut().find(|e| e.id == ev.id) {
            Some(slot) => *slot = ev.clone(),
            None => self.store.events.push(ev.clone()),
        }
        self.push_put(&ev);
        self.dirty_store = true;
    }

    pub fn delete_event(&mut self, id: &str) {
        let Some(ev) = self.store.events.iter().find(|e| e.id == id).cloned() else {
            return;
        };
        self.store.events.retain(|e| e.id != id);
        if self.inspector.as_ref().is_some_and(|i| i.event_id == id) {
            self.inspector = None;
        }
        if self.selected_event.as_deref() == Some(id) {
            self.selected_event = None;
        }
        if self.delete_confirm.as_deref() == Some(id) {
            self.delete_confirm = None;
        }
        self.push_delete(&ev);
        self.dirty_store = true;
        self.flash(format!("{} \"{}\"", t("Deleted"), ev.title), false);
    }

    fn mark_doc_dirty(&mut self, cal_id: &str) {
        self.mark_file_dirty(cal_id);
    }

    fn push_put(&mut self, ev: &CalendarEvent) {
        let Some(acc) = self.account_of(&ev.calendar_id).cloned() else {
            return;
        };
        match acc.kind {
            AccountKind::File => self.mark_doc_dirty(&ev.calendar_id),
            AccountKind::Caldav => {
                let Some(cal_url) = self
                    .store
                    .calendar_by_id(&ev.calendar_id)
                    .and_then(|c| c.url.clone())
                else {
                    return;
                };
                let (Some(ep), Some(inbox)) = (self.endpoint(&acc), self.inbox.clone()) else {
                    return;
                };
                let ev = ev.clone();
                let acc_id = acc.id.clone();
                self.busy += 1;
                spawn_task(async move {
                    match sync::put_event(&ep, &cal_url, &ev).await {
                        Ok((href, etag)) => inbox.push(SyncMsg::Saved {
                            event_id: ev.id.clone(),
                            href,
                            etag,
                        }),
                        Err(what) => inbox.push(SyncMsg::Failed {
                            account_id: acc_id,
                            what,
                        }),
                    }
                });
            }
            _ => {}
        }
    }

    fn push_delete(&mut self, ev: &CalendarEvent) {
        let Some(acc) = self.account_of(&ev.calendar_id).cloned() else {
            return;
        };
        match acc.kind {
            AccountKind::File => self.mark_doc_dirty(&ev.calendar_id),
            AccountKind::Caldav => {
                let Some(href) = ev.href.clone() else { return };
                let (Some(ep), Some(inbox)) = (self.endpoint(&acc), self.inbox.clone()) else {
                    return;
                };
                let etag = ev.etag.clone();
                let id = ev.id.clone();
                let acc_id = acc.id.clone();
                self.busy += 1;
                spawn_task(async move {
                    match sync::delete_event(&ep, &href, etag.as_deref()).await {
                        Ok(()) => inbox.push(SyncMsg::Deleted { event_id: id }),
                        Err(what) => inbox.push(SyncMsg::Failed {
                            account_id: acc_id,
                            what,
                        }),
                    }
                });
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------------------------
    // Accounts and syncing

    fn through_host(&self, acc: &AccountConfig) -> bool {
        // In a browser, servers on other origins go through the host's proxy when it has one.
        cfg!(target_arch = "wasm32")
            && !acc.host_managed
            && (acc.via_proxy || acc.server_url.as_deref().is_some_and(|b| !b.starts_with('/')))
            && self.host.remote_proxy.is_some()
    }

    /// The account needs a password this device does not have (or has not loaded yet).
    pub fn needs_password(&self, acc: &AccountConfig) -> bool {
        password_missing(acc, self.secrets.contains_key(&acc.id), self.through_host(acc))
    }

    /// Where to send requests for `acc`, or `None` when they cannot go out yet. A CalDAV
    /// account whose password is missing gets no endpoint, so nothing is sent without
    /// the Authorization header it needs.
    pub fn endpoint(&self, acc: &AccountConfig) -> Option<Endpoint> {
        let base = acc.server_url.clone()?;
        if self.needs_password(acc) {
            return None;
        }
        let through_host = self.through_host(acc);
        let auth = if through_host {
            None
        } else {
            match (&acc.username, self.secrets.get(&acc.id)) {
                (Some(u), Some(p)) if !acc.host_managed => Some((u.clone(), p.clone())),
                _ => None,
            }
        };
        let proxy = if through_host {
            self.host.remote_proxy.clone()
        } else {
            None
        };
        let account_id = if through_host {
            Some(acc.id.clone())
        } else {
            None
        };
        Some(Endpoint {
            base,
            auth,
            proxy,
            host_managed: acc.host_managed,
            account_id,
        })
    }

    /// Find the calendars of every server account and read their events.
    pub fn refresh_all(&mut self) {
        let ids: Vec<String> = self
            .store
            .accounts
            .iter()
            .filter(|a| a.enabled && matches!(a.kind, AccountKind::Caldav | AccountKind::Ics))
            .map(|a| a.id.clone())
            .collect();
        self.loaded.clear();
        for id in ids {
            self.refresh_account(&id);
        }
    }

    pub fn refresh_account(&mut self, account_id: &str) {
        let Some(acc) = self
            .store
            .accounts
            .iter()
            .find(|a| a.id == account_id)
            .cloned()
        else {
            return;
        };
        if acc.kind == AccountKind::File {
            let ids: Vec<String> = self
                .file_cals
                .values()
                .filter(|f| {
                    self.store
                        .calendars
                        .iter()
                        .any(|c| c.id == f.calendar_id && c.account_id == acc.id)
                })
                .map(|f| f.calendar_id.clone())
                .collect();
            if ids.is_empty() {
                self.request_file_sync(true);
            } else {
                for id in ids {
                    self.request_file_load(&id);
                }
            }
            return;
        }
        if self.needs_password(&acc) {
            // Wait for the store on start; after that, say what is missing.
            if self.secrets_ready {
                self.status.insert(acc.id.clone(), t(MISSING_PASSWORD).to_string());
            }
            return;
        }
        let (Some(ep), Some(inbox)) = (self.endpoint(&acc), self.inbox.clone()) else {
            return;
        };
        self.status.remove(account_id);
        self.busy += 1;
        let acc_id = acc.id.clone();
        match acc.kind {
            AccountKind::Caldav => spawn_task(async move {
                match sync::discover(&ep).await {
                    Ok(calendars) => inbox.push(SyncMsg::Calendars {
                        account_id: acc_id,
                        calendars,
                    }),
                    Err(what) => inbox.push(SyncMsg::Failed {
                        account_id: acc_id,
                        what,
                    }),
                }
            }),
            AccountKind::Ics => {
                let cal_id = self.ensure_feed_calendar(&acc);
                let url = acc.server_url.clone().unwrap_or_default();
                spawn_task(async move {
                    match sync::fetch_feed(&ep, &url, &cal_id).await {
                        Ok(cal) => inbox.push(SyncMsg::Events {
                            calendar_id: cal_id,
                            events: cal.events,
                            name: cal.name,
                            color: cal.color,
                        }),
                        Err(what) => inbox.push(SyncMsg::Failed {
                            account_id: acc_id,
                            what,
                        }),
                    }
                });
            }
            _ => self.busy -= 1,
        }
    }

    fn ensure_feed_calendar(&mut self, acc: &AccountConfig) -> String {
        if let Some(c) = self.store.calendars.iter().find(|c| c.account_id == acc.id) {
            return c.id.clone();
        }
        let id = format!("cal_{}", ics::short_hash(&acc.id));
        let color = self.next_color();
        self.store.calendars.push(CalendarSource {
            id: id.clone(),
            account_id: acc.id.clone(),
            name: acc.name.clone(),
            color_hex: color,
            visible: true,
            read_only: true,
            url: acc.server_url.clone(),
            ctag: None,
            ..Default::default()
        });
        id
    }

    pub fn next_color(&self) -> String {
        let used: Vec<String> = self
            .store
            .calendars
            .iter()
            .map(|c| c.color_hex.to_ascii_uppercase())
            .collect();
        theme::CALENDAR_COLORS
            .iter()
            .map(|(_, c)| c.to_string())
            .find(|c| !used.contains(c))
            .unwrap_or_else(|| {
                theme::CALENDAR_COLORS[self.store.calendars.len() % theme::CALENDAR_COLORS.len()]
                    .1
                    .to_string()
            })
    }

    fn load_events(&mut self, cal_id: &str) {
        let Some(cal) = self.store.calendar_by_id(cal_id).cloned() else {
            return;
        };
        let Some(acc) = self.account_of(cal_id).cloned() else {
            return;
        };
        if acc.kind != AccountKind::Caldav {
            return;
        }
        let (Some(url), Some(ep), Some(inbox)) =
            (cal.url.clone(), self.endpoint(&acc), self.inbox.clone())
        else {
            return;
        };
        let (from, to) = sync::window_around(self.selected_date);
        self.loaded.insert(cal_id.to_string(), (from, to));
        let id = cal.id.clone();
        let acc_id = acc.id.clone();
        self.busy += 1;
        spawn_task(async move {
            match sync::fetch_events(&ep, &url, &id, from, to).await {
                Ok(events) => inbox.push(SyncMsg::Events {
                    calendar_id: id,
                    events,
                    name: None,
                    color: None,
                }),
                Err(what) => inbox.push(SyncMsg::Failed {
                    account_id: acc_id,
                    what,
                }),
            }
        });
    }

    /// Read again when the selection moves outside what was read.
    fn ensure_range(&mut self) {
        let cal_ids: Vec<String> = self
            .store
            .calendars
            .iter()
            .filter(|c| {
                c.url.is_some()
                    && self
                        .account_of(&c.id)
                        .is_some_and(|a| a.kind == AccountKind::Caldav && a.enabled)
            })
            .map(|c| c.id.clone())
            .collect();
        for id in cal_ids {
            let inside = self.loaded.get(&id).is_some_and(|(from, to)| {
                self.selected_date - Duration::days(62) >= *from
                    && self.selected_date + Duration::days(62) <= *to
            });
            if !inside && self.loaded.contains_key(&id) {
                self.load_events(&id);
            }
        }
    }

    fn handle_sync(&mut self, msg: SyncMsg) {
        match &msg {
            SyncMsg::SecretsReady { .. } | SyncMsg::SecretSaved { .. } => {}
            _ => self.busy = self.busy.saturating_sub(1),
        }
        match msg {
            SyncMsg::SecretsReady { .. } | SyncMsg::SecretSaved { .. } => {}
            SyncMsg::Calendars {
                account_id,
                calendars,
            } => {
                let mut fresh = Vec::new();
                for rc in &calendars {
                    let id = format!("cal_{}", ics::short_hash(&rc.url));
                    match self.store.calendars.iter_mut().find(|c| c.id == id) {
                        Some(c) => {
                            c.name = rc.name.clone();
                            c.read_only = rc.read_only;
                            c.url = Some(rc.url.clone());
                            c.ctag = rc.ctag.clone();
                        }
                        None => {
                            let color = rc.color.clone().unwrap_or_else(|| self.next_color());
                            self.store.calendars.push(CalendarSource {
                                id: id.clone(),
                                account_id: account_id.clone(),
                                name: rc.name.clone(),
                                color_hex: color,
                                visible: true,
                                read_only: rc.read_only,
                                url: Some(rc.url.clone()),
                                ctag: rc.ctag.clone(),
                                ..Default::default()
                            });
                        }
                    }
                    fresh.push(id);
                }
                // Calendars deleted on the server go here too.
                let stale: Vec<String> = self
                    .store
                    .calendars
                    .iter()
                    .filter(|c| c.account_id == account_id && !fresh.contains(&c.id))
                    .map(|c| c.id.clone())
                    .collect();
                self.store.calendars.retain(|c| !stale.contains(&c.id));
                for id in &stale {
                    self.store.forget_calendar_in_sets(id);
                }
                self.store
                    .events
                    .retain(|e| !stale.contains(&e.calendar_id));
                for id in fresh {
                    self.load_events(&id);
                }
                self.dirty_store = true;
            }
            SyncMsg::Events {
                calendar_id,
                events,
                name,
                color,
            } => {
                // Keep events created here that the server has not confirmed yet.
                let is_caldav = self
                    .account_of(&calendar_id)
                    .is_some_and(|a| a.kind == AccountKind::Caldav);
                self.store
                    .events
                    .retain(|e| e.calendar_id != calendar_id || (e.href.is_none() && is_caldav));
                let pending: Vec<String> =
                    self.store.events.iter().map(|e| e.uid.clone()).collect();
                self.store.events.extend(
                    events
                        .into_iter()
                        .filter(|e| e.uid.is_empty() || !pending.contains(&e.uid)),
                );
                if let Some(c) = self
                    .store
                    .calendars
                    .iter_mut()
                    .find(|c| c.id == calendar_id)
                {
                    if let (Some(n), true) = (name, c.name.is_empty()) {
                        c.name = n;
                    }
                    if let Some(col) = color.and_then(|c| crate::caldav::normalize_color(&c)) {
                        c.color_hex = col;
                    }
                }
                if let Some(i) = &self.inspector {
                    if !self.store.events.iter().any(|e| e.id == i.event_id) {
                        self.inspector = None;
                    }
                }
                self.dirty_store = true;
            }
            SyncMsg::Saved {
                event_id,
                href,
                etag,
            } => {
                if let Some(e) = self.store.events.iter_mut().find(|e| e.id == event_id) {
                    e.href = Some(href);
                    e.etag = etag.clone();
                    // No ETag in the answer: read the calendar again to learn it.
                    if etag.is_none() {
                        let cal = e.calendar_id.clone();
                        self.load_events(&cal);
                    }
                }
                self.dirty_store = true;
            }
            SyncMsg::Deleted { .. } => {}
            SyncMsg::Failed { account_id, what } => {
                let name = self
                    .store
                    .accounts
                    .iter()
                    .find(|a| a.id == account_id)
                    .map(|a| a.name.clone())
                    .unwrap_or_default();
                self.status.insert(account_id.clone(), what.clone());
                let (summary, _) = sync::split_error(&what);
                self.flash(format!("{name}: {summary}"), true);
                // A refused change: read the server's version back.
                let cals: Vec<String> = self
                    .store
                    .calendars
                    .iter()
                    .filter(|c| c.account_id == account_id)
                    .map(|c| c.id.clone())
                    .collect();
                if what.contains("changed on the server") {
                    for c in cals {
                        self.load_events(&c);
                    }
                    self.flash(t("This event changed on the server and was reloaded."), true);
                }
            }
        }
    }

    fn load_secrets(&mut self) {
        let Some(inbox) = self.inbox.clone() else {
            return;
        };
        let hosted = host::hosted();
        let ids: Vec<String> = self.store.accounts.iter().map(|a| a.id.clone()).collect();
        let usernames: BTreeMap<String, String> = self
            .store
            .accounts
            .iter()
            .filter_map(|a| {
                let name = a
                    .username
                    .as_ref()
                    .map(|u| u.trim())
                    .filter(|u| !u.is_empty())
                    .map(|u| u.to_string())
                    .or_else(|| {
                        let email = a.email.trim();
                        (!email.is_empty()).then(|| email.to_string())
                    })?;
                Some((a.id.clone(), name))
            })
            .collect();
        spawn_task(async move {
            match crate::secrets::startup(hosted, &ids, &usernames).await {
                Ok(map) => inbox.push(SyncMsg::SecretsReady { map }),
                Err(what) => inbox.push(SyncMsg::Failed {
                    account_id: String::new(),
                    what,
                }),
            }
        });
    }

    fn save_password(&mut self, id: &str, username: &str, password: &str) {
        if host::hosted() {
            let Some(inbox) = self.inbox.clone() else {
                return;
            };
            let id = id.to_string();
            let username = username.to_string();
            let password = password.to_string();
            spawn_task(async move {
                match crate::secrets::put_hosted(&id, &username, &password).await {
                    Ok(()) => inbox.push(SyncMsg::SecretSaved { account_id: id }),
                    Err(what) => inbox.push(SyncMsg::Failed {
                        account_id: id,
                        what,
                    }),
                }
            });
            return;
        }
        self.secrets.insert(id.to_string(), password.to_string());
        let Some(inbox) = self.inbox.clone() else {
            self.refresh_account(id);
            return;
        };
        let saved_id = id.to_string();
        let password = password.to_string();
        spawn_task(async move {
            if let Err(what) = crate::secrets::set(&saved_id, &password).await {
                inbox.push(SyncMsg::Failed {
                    account_id: saved_id,
                    what,
                });
            }
        });
        self.refresh_account(id);
    }

    fn forget_password(&mut self, id: &str) {
        self.secrets.remove(id);
        let id = id.to_string();
        if host::hosted() {
            spawn_task(async move {
                let _ = crate::secrets::delete_hosted(&id).await;
            });
        } else {
            spawn_task(async move {
                let _ = crate::secrets::remove(&id).await;
            });
        }
    }

    pub fn add_account_from_form(&mut self) {
        let f = self.account_form.clone();
        let url = f.url.trim().to_string();
        if !(url.starts_with("https://")
            || url.starts_with("http://")
            || url.starts_with("webcal://"))
        {
            self.account_form.error =
                Some(t("Enter the server address, starting with https://").into());
            return;
        }
        let id = format!("acc_{}", ics::short_hash(&format!("{url}{}", f.username)));
        let kind = if f.kind_ics
            || url.starts_with("webcal://")
            || url.to_ascii_lowercase().ends_with(".ics")
        {
            AccountKind::Ics
        } else {
            AccountKind::Caldav
        };
        let name = if f.name.trim().is_empty() {
            crate::net::host_of(&url)
        } else {
            f.name.trim().to_string()
        };
        self.store.accounts.retain(|a| a.id != id);
        self.store.accounts.push(AccountConfig {
            id: id.clone(),
            name,
            provider: if kind == AccountKind::Ics {
                "ICS".into()
            } else {
                "CalDAV".into()
            },
            email: f.username.trim().to_string(),
            server_url: Some(url),
            enabled: true,
            kind,
            username: Some(f.username.trim().to_string()).filter(|u| !u.is_empty()),
            host_managed: false,
            via_proxy: false,
        });
        self.account_form = AccountForm::default();
        self.dirty_store = true;
        if !f.password.is_empty() {
            self.save_password(&id, f.username.trim(), &f.password);
        } else {
            self.refresh_account(&id);
        }
    }

    pub fn start_account_edit(&mut self, id: &str) {
        let Some(acc) = self.store.accounts.iter().find(|a| a.id == id) else {
            return;
        };
        self.account_edit = Some(AccountEdit {
            id: acc.id.clone(),
            name: acc.name.clone(),
            url: acc.server_url.clone().unwrap_or_default(),
            username: acc.username.clone().unwrap_or_default(),
            password: String::new(),
            error: None,
        });
    }

    /// Apply the edit form to the account it was opened for, keeping its id.
    pub fn save_account_edit(&mut self) {
        let Some(edit) = self.account_edit.clone() else {
            return;
        };
        let Some(acc) = self.store.accounts.iter_mut().find(|a| a.id == edit.id) else {
            self.account_edit = None;
            return;
        };
        if let Err(e) = apply_account_edit(acc, &edit) {
            if let Some(f) = self.account_edit.as_mut() {
                f.error = Some(e);
            }
            return;
        }
        let (id, username) = (acc.id.clone(), acc.username.clone().unwrap_or_default());
        self.account_edit = None;
        self.dirty_store = true;
        self.status.remove(&id);
        self.error_details.remove(&id);
        let cals: Vec<String> = self.store.calendars.iter().filter(|c| c.account_id == id).map(|c| c.id.clone()).collect();
        for c in &cals {
            self.loaded.remove(c);
        }
        if !edit.password.is_empty() {
            self.save_password(&id, &username, &edit.password);
        } else {
            self.refresh_account(&id);
        }
    }

    pub fn remove_account(&mut self, id: &str) {
        let cals: Vec<String> = self
            .store
            .calendars
            .iter()
            .filter(|c| c.account_id == id)
            .map(|c| c.id.clone())
            .collect();
        self.store.events.retain(|e| !cals.contains(&e.calendar_id));
        self.store.calendars.retain(|c| c.account_id != id);
        self.store.accounts.retain(|a| a.id != id);
        for set in &mut self.store.calendar_sets {
            set.calendar_ids.retain(|c| !cals.contains(c));
        }
        self.forget_password(id);
        self.dirty_store = true;
    }

    // -----------------------------------------------------------------------------------------
    // The host's document

    // -----------------------------------------------------------------------------------------
    // Saving options and data

    /// `window.calendarcraft.state`: the calendars and the events around the selected day.
    fn publish_state(&mut self) {
        let files_sig = (self.file_cals.len() as u64)
            .wrapping_mul(17)
            .wrapping_add(self.file_cals.values().filter(|f| f.dirty).count() as u64)
            .wrapping_add(self.file_conflict.is_some() as u64 * 31)
            .wrapping_add(self.migration.as_ref().map(|m| m.pending + m.saved.len()).unwrap_or(0) as u64);
        let sig = self.store.events.iter().fold(
            (self.store.calendars.len() as u64) << 32 | self.busy as u64,
            |acc, e| {
                acc.wrapping_mul(31).wrapping_add(
                    e.title.len() as u64 + e.date.num_days_from_ce() as u64 + e.id.len() as u64,
                )
            },
        ) ^ self.selected_date.num_days_from_ce() as u64 ^ files_sig;
        if sig == self.published {
            return;
        }
        self.published = sig;
        let from = self.selected_date - Duration::days(45);
        let to = self.selected_date + Duration::days(60);
        let events: Vec<serde_json::Value> = self
            .store
            .events
            .iter()
            .filter(|e| e.date >= from && e.date <= to)
            .map(|e| {
                serde_json::json!({
                    "id": e.id, "title": e.title, "date": e.date.to_string(), "calendar": e.calendar_id,
                    "start": e.start_time.map(|t| t.format("%H:%M").to_string()), "allDay": e.all_day,
                })
            })
            .collect();
        let calendars: Vec<serde_json::Value> = self
            .store
            .calendars
            .iter()
            .map(|c| serde_json::json!({"id": c.id, "name": c.name, "account": c.account_id}))
            .collect();
        let files = self.file_state_json();
        let state = serde_json::json!({
            "date": self.selected_date.to_string(),
            "busy": self.busy,
            "calendars": calendars,
            "events": events,
            "errors": self.status,
            "fileCalendars": files.get("fileCalendars").cloned().unwrap_or(serde_json::json!([])),
            "conflict": files.get("conflict").cloned().unwrap_or(serde_json::Value::Null),
            "migration": files.get("migration").cloned().unwrap_or(serde_json::Value::Null),
        });
        host::publish_state(&state.to_string());
    }

    fn persist(&mut self) {
        if !self.dirty_store || self.now - self.last_saved < 0.6 {
            return;
        }
        self.dirty_store = false;
        self.last_saved = self.now;
        host::save(
            host::KEY_UI,
            &serde_json::to_string(&self.prefs).unwrap_or_default(),
        );
        if self.launch.demo {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = self.launch.config_path.clone() {
            let _ = self.store.save_to_path(&path);
            return;
        }
        // Server and file events are not part of the saved store: the server events go to a
        // cache that stays on this device, the document goes back to the host.
        let server_cals: Vec<String> = self
            .store
            .calendars
            .iter()
            .filter(|c| {
                self.account_of(&c.id)
                    .is_some_and(|a| matches!(a.kind, AccountKind::Caldav | AccountKind::Ics))
            })
            .map(|c| c.id.clone())
            .collect();
        let file_cals: Vec<String> = self
            .store
            .calendars
            .iter()
            .filter(|c| crate::filesync::is_file_backed(c) || c.account_id == "acc_file")
            .map(|c| c.id.clone())
            .collect();
        let mut keep = self.store.clone();
        // Keep file-backed calendar *references* (path / etag), not their events or an Untitled doc.
        keep.accounts.retain(|a| {
            a.kind != AccountKind::File
                || keep.calendars.iter().any(|c| c.account_id == a.id && crate::filesync::is_file_backed(c))
        });
        keep.calendars.retain(|c| crate::filesync::is_file_backed(c) || (c.account_id != "acc_file" && c.id != "cal_file"));
        keep.events
            .retain(|e| !file_cals.contains(&e.calendar_id) && !server_cals.contains(&e.calendar_id));
        if let Ok(json) = serde_json::to_string(&keep) {
            host::save(host::KEY_STORE, &json);
        }
        let cache: Vec<&CalendarEvent> = self
            .store
            .events
            .iter()
            .filter(|e| server_cals.contains(&e.calendar_id))
            .collect();
        host::save(
            host::KEY_CACHE,
            &serde_json::to_string(&cache).unwrap_or_default(),
        );
    }

    pub fn mark_changed(&mut self) {
        self.dirty_store = true;
    }

    fn apply_style(&mut self, ctx: &egui::Context) {
        if !self.fonts_ready {
            theme::install_fonts(ctx);
            self.fonts_ready = true;
        }
        let choice = ThemeChoice::parse(&self.prefs.theme);
        let dark = match choice {
            ThemeChoice::Dark => true,
            ThemeChoice::Light => false,
            ThemeChoice::System => ctx
                .input(|i| i.raw.system_theme)
                .map(|t| t == egui::Theme::Dark)
                .unwrap_or(false),
        };
        let animations = match self.prefs.animations.as_str() {
            "off" => false,
            "on" => true,
            _ => !prefers_reduced_motion(),
        };
        let key = (dark, animations, self.prefs.language.clone());
        if self.applied_style.as_ref() != Some(&key) {
            self.palette = theme::palette(dark);
            ctx.options_mut(|o| {
                o.theme_preference = if dark {
                    egui::ThemePreference::Dark
                } else {
                    egui::ThemePreference::Light
                }
            });
            theme::apply(ctx, &self.palette, animations);
            i18n::set_language(&self.prefs.language);
            self.applied_style = Some(key);
            self.motion = animations;
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.launch.screenshot.clone() else {
            return;
        };
        ctx.request_repaint();
        let mut captured = None;
        ctx.input(|i| {
            for e in &i.raw.events {
                if let egui::Event::Screenshot { image, .. } = e {
                    captured = Some(image.clone());
                }
            }
        });
        if let Some(img) = captured {
            if let Some(parent) = std::path::Path::new(&path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let rgba: Vec<u8> = img.pixels.iter().flat_map(|p| p.to_array()).collect();
            if let Err(err) = image::save_buffer(
                &path,
                &rgba,
                img.width() as u32,
                img.height() as u32,
                image::ColorType::Rgba8,
            ) {
                eprintln!("screenshot: {err}");
            }
            std::process::exit(0);
        }
        if self.frame >= 90 && !self.shot_requested {
            self.shot_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
    }

    fn first_frame(&mut self, ctx: &egui::Context) {
        let c = ctx.clone();
        self.inbox = Some(Inbox::new(Arc::new(move || c.request_repaint())));
        let c = ctx.clone();
        host::start_bridge(
            self.host_inbox.clone(),
            Arc::new(move || c.request_repaint()),
        );
        self.load_secrets();
        if let Some(q) = self.launch.search.clone() {
            self.search_query = q;
            self.search_open = true;
        }
        if let Some(id) = self.launch.open_event.clone() {
            self.selected_event = Some(id.clone());
            self.open_inspector(&id, None);
        }
        if let Some(id) = self.launch.select_event.clone() {
            self.selected_event = Some(id);
        }
        if self.launch.confirm_delete {
            self.ask_delete_selected();
        }
        if self.launch.new_event {
            self.open_new_event(self.selected_date, None);
        }
        if let Some(tab) = self.launch.settings {
            self.settings_open = true;
            self.settings_tab = tab;
        }
        if let Some(id) = self.launch.edit_account.clone() {
            self.settings_open = true;
            self.settings_tab = 1;
            self.start_account_edit(&id);
        }
        self.start_host_file_setup();
    }

    /// A click on an event: the first selects it, a second click on the selected event or a
    /// double-click opens its details.
    pub fn click_event(&mut self, id: &str, rect: Rect, double: bool) {
        match event_click(self.selected_event.as_deref(), id, double) {
            EventClick::Open => {
                self.selected_event = Some(id.to_string());
                // Already open: keep the draft (a third click must not drop edits).
                if self.inspector.as_ref().is_none_or(|i| i.event_id != id) {
                    self.open_inspector(id, Some(rect));
                }
            }
            EventClick::Select => {
                self.selected_event = Some(id.to_string());
                if self.inspector.as_ref().is_some_and(|i| i.event_id != id) {
                    self.inspector = None;
                }
            }
        }
    }

    /// The event drawn as selected: the picked one, else the one whose details are open.
    pub fn highlighted_event(&self) -> Option<String> {
        self.selected_event
            .clone()
            .or_else(|| self.inspector.as_ref().map(|i| i.event_id.clone()))
    }

    /// Ask before deleting the selected event (or the one whose details are open).
    fn ask_delete_selected(&mut self) {
        let Some(id) = self.highlighted_event() else {
            return;
        };
        let Some(ev) = self.store.events.iter().find(|e| e.id == id) else {
            self.selected_event = None;
            return;
        };
        if !self.calendar_writable(&ev.calendar_id) || ev.locked {
            let why = if ev.locked {
                t("Part of a repeating series with changes; edit it in the app that made it.")
            } else {
                t("This calendar is read only.")
            };
            self.flash(why, true);
            return;
        }
        self.delete_confirm = Some(id);
    }

    /// Paint today's date on the window and Dock icon; again after midnight.
    #[cfg(not(target_arch = "wasm32"))]
    fn refresh_icon(&mut self, ctx: &egui::Context) {
        let now = Local::now();
        let day = now.date_naive();
        let became_active = ctx.input(|i| {
            i.events
                .iter()
                .any(|e| matches!(e, egui::Event::WindowFocused(true)))
        });
        if self.icon_day != Some(day) {
            self.icon_day = Some(day);
            if let Ok(icon) = crate::icon::load_for_date(day) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Icon(Some(Arc::new(icon))));
            }
            #[cfg(target_os = "macos")]
            crate::icon::set_dock_icon_for_date(day);
        } else if became_active {
            // macOS can drop a cargo-run Dock icon after the app is backgrounded;
            // putting it back when the window becomes active also catches a missed midnight.
            #[cfg(target_os = "macos")]
            crate::icon::set_dock_icon_for_date(day);
        }
        // Wake up just after midnight even if nothing else happens.
        let next = (day + Duration::days(1))
            .and_hms_opt(0, 0, 1)
            .and_then(|m| m.and_local_timezone(Local).earliest())
            .map(|m| (m - now).to_std().unwrap_or_default())
            .unwrap_or(std::time::Duration::from_secs(3600));
        ctx.request_repaint_after(next);
    }

    pub fn is_mobile(&self, ctx: &egui::Context) -> bool {
        ctx.screen_rect().width() < 720.0
    }

    pub fn keyboard(&mut self, ctx: &egui::Context) {
        let typing = ctx.wants_keyboard_input();
        let (esc, n, t_key, left, right, save, sidebar, delete) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Escape),
                // ⌘N natively; browsers keep Ctrl/⌘N for a new window, so N or C alone too.
                (i.modifiers.command && i.key_pressed(egui::Key::N))
                    || (!typing
                        && !i.modifiers.any()
                        && (i.key_pressed(egui::Key::N) || i.key_pressed(egui::Key::C))),
                !i.modifiers.any() && i.key_pressed(egui::Key::T),
                !i.modifiers.any() && i.key_pressed(egui::Key::ArrowLeft),
                !i.modifiers.any() && i.key_pressed(egui::Key::ArrowRight),
                i.modifiers.command && i.key_pressed(egui::Key::S),
                i.modifiers.command && i.key_pressed(egui::Key::Backslash),
                i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
            )
        });
        if esc {
            if self.delete_confirm.is_some() {
                self.delete_confirm = None;
            } else if self.new_event.is_some() {
                self.new_event = None;
            } else if self.inspector.is_some() {
                self.inspector = None;
            } else if self.selected_event.is_some() {
                self.selected_event = None;
            } else {
                self.settings_open = false;
            }
        }
        // Delete/Backspace removes the selected event after asking; never while typing.
        if delete
            && !typing
            && self.delete_confirm.is_none()
            && self.new_event.is_none()
            && !self.settings_open
        {
            self.ask_delete_selected();
        }
        if n {
            self.open_new_event(self.selected_date, None);
        }
        if save {
            self.save_document();
        }
        if sidebar && !self.is_mobile(ctx) {
            self.toggle_sidebar(ctx);
        }
        if !typing && self.inspector.is_none() && self.new_event.is_none() {
            if t_key {
                self.go_today();
            }
            if left {
                self.step(-1);
            }
            if right {
                self.step(1);
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn spawn_task(fut: impl std::future::Future<Output = ()> + 'static) {
    crate::net::spawn(fut);
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_task(fut: impl std::future::Future<Output = ()> + Send + 'static) {
    crate::net::spawn(fut);
}

#[cfg(target_arch = "wasm32")]
fn prefers_reduced_motion() -> bool {
    web_sys::window()
        .and_then(|w| {
            w.match_media("(prefers-reduced-motion: reduce)")
                .ok()
                .flatten()
        })
        .is_some_and(|m| m.matches())
}

#[cfg(not(target_arch = "wasm32"))]
fn prefers_reduced_motion() -> bool {
    false
}

/// Copy the edit form onto `acc`. The id, kind and calendars are left alone; the password is
/// not part of the account (an empty field keeps the stored one).
pub fn apply_account_edit(acc: &mut AccountConfig, edit: &AccountEdit) -> Result<(), String> {
    let url = edit.url.trim();
    if !(url.starts_with("https://") || url.starts_with("http://") || url.starts_with("webcal://")) {
        return Err(t("Enter the server address, starting with https://").into());
    }
    acc.server_url = Some(url.to_string());
    acc.name = if edit.name.trim().is_empty() { crate::net::host_of(url) } else { edit.name.trim().to_string() };
    if acc.kind == AccountKind::Caldav {
        let user = edit.username.trim();
        acc.username = Some(user.to_string()).filter(|u| !u.is_empty());
        if !user.is_empty() {
            acc.email = user.to_string();
        }
    }
    Ok(())
}

pub fn first_of_month(d: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap_or(d)
}

pub fn add_months(d: NaiveDate, n: i32) -> NaiveDate {
    let total = d.year() * 12 + d.month0() as i32 + n;
    let (y, m) = (total.div_euclid(12), total.rem_euclid(12) as u32 + 1);
    let last = days_in_month(y, m);
    NaiveDate::from_ymd_opt(y, m, d.day().min(last)).unwrap_or(d)
}

pub fn days_in_month(y: i32, m: u32) -> u32 {
    let next = if m == 12 {
        NaiveDate::from_ymd_opt(y + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(y, m + 1, 1)
    };
    next.map(|n| (n - Duration::days(1)).day()).unwrap_or(30)
}

/// Monday of the week holding `d`.
pub fn week_start(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}

pub fn parse_hhmm(text: &str) -> Option<NaiveTime> {
    let t = text.trim();
    NaiveTime::parse_from_str(t, "%H:%M")
        .ok()
        .or_else(|| crate::nlp::parse_time_token(t))
}

impl eframe::App for CalendarCraftApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.now = ctx.input(|i| i.time);
        if !self.fonts_ready {
            // Fonts registered with set_fonts take effect next frame; draw nothing until then
            // so the "semibold" family is bound before anything asks for it.
            theme::install_fonts(ctx);
            self.fonts_ready = true;
            ctx.request_repaint();
            return;
        }
        if self.frame == 0 {
            self.first_frame(ctx);
        }
        self.frame += 1;
        self.apply_style(ctx);
        #[cfg(not(target_arch = "wasm32"))]
        self.screenshot(ctx);

        let msgs = self.inbox.as_ref().map(|i| i.drain()).unwrap_or_default();
        for m in msgs {
            match m {
                SyncMsg::SecretsReady { map } => {
                    self.secrets = map;
                    self.secrets_ready = true;
                    self.refresh_all();
                }
                SyncMsg::SecretSaved { account_id } => self.refresh_account(&account_id),
                other => self.handle_sync(other),
            }
        }
        let host_msgs: Vec<HostMsg> = self
            .host_inbox
            .lock()
            .map(|mut q| std::mem::take(&mut *q))
            .unwrap_or_default();
        for m in host_msgs {
            self.handle_host_msg(m);
        }
        if self.banner.is_some() && self.now > self.banner_until {
            self.banner = None;
        }
        self.ensure_range();
        #[cfg(not(target_arch = "wasm32"))]
        self.refresh_icon(ctx);
        self.search_keys(ctx);
        self.keyboard(ctx);

        if self.is_mobile(ctx) {
            self.mobile_layout(ctx);
        } else {
            self.desktop_layout(ctx);
        }
        self.search_popup(ctx);
        self.popups(ctx);
        if host::hosted() {
            if self.file_sync_requested {
                self.file_sync_requested = false;
                self.request_file_sync(true);
            } else {
                self.request_file_sync(false);
            }
            self.maybe_autosave_files();
            let interval = crate::filesync::sync_interval_secs(self.store.settings.file_sync_interval_secs);
            ctx.request_repaint_after(std::time::Duration::from_secs(interval as u64));
        }
        self.publish_state();
        self.persist();
        if self.dirty_store {
            ctx.request_repaint_after(std::time::Duration::from_millis(700));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_account_edit, event_click, password_missing, AccountEdit, EventClick};
    use crate::model::{AccountConfig, AccountKind};

    fn icloud() -> AccountConfig {
        AccountConfig {
            id: "acc_icloud".into(),
            name: "iCloud".into(),
            kind: AccountKind::Caldav,
            username: Some("ada@icloud.com".into()),
            server_url: Some("https://caldav.icloud.com/".into()),
            ..Default::default()
        }
    }

    #[test]
    fn caldav_without_a_saved_password_is_not_asked() {
        let acc = icloud();
        assert!(password_missing(&acc, false, false));
        assert!(!password_missing(&acc, true, false));
        // The host's proxy or the host itself attaches the password.
        assert!(!password_missing(&acc, false, true));
        assert!(!password_missing(&AccountConfig { host_managed: true, ..icloud() }, false, false));
        // Subscriptions, local calendars and anonymous servers need none.
        assert!(!password_missing(&AccountConfig { kind: AccountKind::Ics, ..icloud() }, false, false));
        assert!(!password_missing(&AccountConfig { username: None, ..icloud() }, false, false));
        assert!(!password_missing(&AccountConfig { username: Some("  ".into()), ..icloud() }, false, false));
    }

    #[test]
    fn editing_an_account_keeps_its_id_and_checks_the_address() {
        let mut acc = icloud();
        let edit = AccountEdit {
            id: acc.id.clone(),
            name: "  Family iCloud ".into(),
            url: " https://p42-caldav.icloud.com/123/ ".into(),
            username: "ada@me.com".into(),
            password: String::new(),
            error: None,
        };
        apply_account_edit(&mut acc, &edit).unwrap();
        assert_eq!(acc.id, "acc_icloud");
        assert_eq!(acc.name, "Family iCloud");
        assert_eq!(acc.server_url.as_deref(), Some("https://p42-caldav.icloud.com/123/"));
        assert_eq!(acc.username.as_deref(), Some("ada@me.com"));
        assert_eq!(acc.kind, AccountKind::Caldav);

        let before = acc.clone();
        let bad = AccountEdit { url: "caldav.icloud.com".into(), ..edit.clone() };
        assert!(apply_account_edit(&mut acc, &bad).is_err());
        assert_eq!(acc.server_url, before.server_url, "a rejected edit changes nothing");

        let unnamed = AccountEdit { name: String::new(), ..edit };
        apply_account_edit(&mut acc, &unnamed).unwrap();
        assert_eq!(acc.name, "p42-caldav.icloud.com");
    }

    #[test]
    fn first_click_selects_second_click_or_double_click_opens() {
        assert_eq!(event_click(None, "a", false), EventClick::Select);
        assert_eq!(event_click(Some("b"), "a", false), EventClick::Select);
        assert_eq!(event_click(Some("a"), "a", false), EventClick::Open);
        assert_eq!(event_click(None, "a", true), EventClick::Open);
        assert_eq!(event_click(Some("b"), "a", true), EventClick::Open);
    }
}
