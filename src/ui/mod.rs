//! CalendarCraft's window: a Fantastical-style layout with a sidebar (quick entry, mini
//! calendar, calendars, the day-by-day list) and the calendar (Day, Week, Month, Quarter, Year),
//! event details in a popover and new events in a dialog. Narrow windows get a phone layout.

mod popups;
mod side;
mod views;

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use chrono::{Datelike, Duration, Local, NaiveDate, NaiveTime};
use eframe::egui::{self, Color32, Rect};
use serde::{Deserialize, Serialize};

use crate::host::{self, HostConfig, HostInbox, HostMsg};
use crate::i18n::{self, t};
use crate::ics;
use crate::model::{AccountConfig, AccountKind, CalendarEvent, CalendarSource, CalendarStore, ViewMode};
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
        Self { theme: "system".into(), language: "auto".into(), animations: "system".into(), view: ViewMode::Month, sidebar: true, day_start: 7 }
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
    /// Open the new-event dialog.
    pub new_event: bool,
    /// Use the sample data, ignoring what is stored.
    pub demo: bool,
    pub language: Option<String>,
}

/// The `.ics` document the host opened (artcraftCloud's drive).
#[derive(Debug, Clone)]
pub struct FileDoc {
    pub name: String,
    pub account_id: String,
    pub calendar_id: String,
    pub dirty: bool,
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

pub struct CalendarCraftApp {
    pub store: CalendarStore,
    pub launch: Launch,
    pub selected_date: NaiveDate,
    pub view_mode: ViewMode,
    pub search_query: String,
    pub quick_input: String,
    pub prefs: UiPrefs,
    pub palette: Palette,
    applied_style: Option<(bool, bool, String)>,
    fonts_ready: bool,

    pub inspector: Option<Inspector>,
    pub new_event: Option<NewEvent>,
    pub settings_open: bool,
    pub settings_tab: u8,
    pub account_form: AccountForm,
    pub mcp_open: bool,
    pub mcp_request: String,
    pub mcp_response: String,
    pub calendars_open: bool,
    pub mobile_menu: bool,

    pub banner: Option<(String, bool)>,
    banner_until: f64,
    pub status: HashMap<String, String>,
    pub busy: usize,

    inbox: Option<Inbox>,
    host_inbox: HostInbox,
    pub host: HostConfig,
    pub secrets: BTreeMap<String, String>,
    pub file_doc: Option<FileDoc>,
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

    dirty_store: bool,
    last_saved: f64,
    now: f64,
    frame: u32,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    shot_requested: bool,
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

impl CalendarCraftApp {
    pub fn new(store: Option<CalendarStore>, launch: Launch) -> Self {
        let mut prefs: UiPrefs = host::load(host::KEY_UI).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        if let Some(t) = &launch.theme {
            prefs.theme = t.clone();
        }
        if let Some(l) = &launch.language {
            prefs.language = l.clone();
        }
        let host_cfg = host::host_config();
        let hosted = host::hosted();
        let anchor = launch.date.unwrap_or_else(today);
        let mut store = match store {
            Some(s) => s,
            None if launch.demo => CalendarStore::sample_preloaded(anchor),
            None => match host::load(host::KEY_STORE).and_then(|s| CalendarStore::from_json_str(&s).ok()) {
                Some(s) => s,
                // A host that signs people in starts empty (their server calendars come in);
                // a plain page shows the sample so there is something to look at.
                None if hosted || !host_cfg.accounts.is_empty() => CalendarStore::empty(),
                None => CalendarStore::sample_preloaded(anchor),
            },
        };
        if !launch.demo {
            if let Some(cache) = host::load(host::KEY_CACHE).and_then(|s| serde_json::from_str::<Vec<CalendarEvent>>(&s).ok()) {
                let known: Vec<String> = store.events.iter().map(|e| e.id.clone()).collect();
                store.events.extend(cache.into_iter().filter(|e| !known.contains(&e.id)));
            }
        }
        store.accounts.retain(|a| a.kind != AccountKind::File);
        let secrets: BTreeMap<String, String> = host::load(host::KEY_SECRETS).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
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
            quick_input: String::new(),
            palette: theme::palette(false),
            prefs,
            applied_style: None,
            fonts_ready: false,
            inspector: None,
            new_event: None,
            settings_open: false,
            settings_tab: 0,
            account_form: AccountForm::default(),
            mcp_open: false,
            mcp_request: sample_mcp,
            mcp_response: String::new(),
            calendars_open: true,
            mobile_menu: false,
            banner: None,
            banner_until: 0.0,
            status: HashMap::new(),
            busy: 0,
            inbox: None,
            host_inbox: Arc::new(Mutex::new(Vec::new())),
            host: host_cfg,
            secrets,
            file_doc: None,
            loaded: HashMap::new(),
            mini_month: first_of_month(anchor),
            agenda_scroll_to: Some(anchor),
            agenda_center: anchor,
            agenda_offset: 0.0,
            timeline_scrolled: false,
            scroll_accum: 0.0,
            dirty_store: false,
            last_saved: 0.0,
            now: 0.0,
            frame: 0,
            shot_requested: false,
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
        let gone: Vec<String> = self.store.accounts.iter().filter(|a| a.host_managed && !host_ids.contains(&a.id)).map(|a| a.id.clone()).collect();
        for id in gone {
            self.remove_account(&id);
        }
    }

    // -----------------------------------------------------------------------------------------
    // Helpers

    pub fn cal_color(&self, cal_id: &str) -> Color32 {
        self.store.calendar_by_id(cal_id).map(|c| theme::color_of(&c.color_hex)).unwrap_or(theme::PURPLE)
    }

    pub fn account_of(&self, cal_id: &str) -> Option<&AccountConfig> {
        let cal = self.store.calendar_by_id(cal_id)?;
        self.store.accounts.iter().find(|a| a.id == cal.account_id)
    }

    pub fn calendar_writable(&self, cal_id: &str) -> bool {
        let Some(cal) = self.store.calendar_by_id(cal_id) else { return false };
        !cal.read_only && self.account_of(cal_id).is_none_or(|a| a.kind != AccountKind::Ics)
    }

    /// Calendars new events can go to, the default first.
    pub fn writable_calendars(&self) -> Vec<CalendarSource> {
        let mut list: Vec<CalendarSource> = self.store.calendars.iter().filter(|c| self.calendar_writable(&c.id)).cloned().collect();
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
        let host_cal = self.store.calendars.iter().find(|c| self.account_of(&c.id).is_some_and(|a| a.host_managed) && self.calendar_writable(&c.id));
        host_cal.or_else(|| self.store.calendars.iter().find(|c| self.calendar_writable(&c.id))).map(|c| c.id.clone())
    }

    pub fn events_on(&self, day: NaiveDate) -> Vec<CalendarEvent> {
        self.store.events_on_date(day, &self.search_query).into_iter().cloned().collect()
    }

    pub fn select_date(&mut self, day: NaiveDate) {
        if day != self.selected_date {
            self.selected_date = day;
            self.agenda_scroll_to = Some(day);
            self.mini_month = first_of_month(day);
            self.timeline_scrolled = false;
        }
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

    pub fn step(&mut self, dir: i64) {
        let d = self.selected_date;
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
        format!("{}-{:x}@calendarcraft", now.format("%Y%m%dT%H%M%S"), now.timestamp_subsec_nanos() ^ (self.store.events.len() as u32).wrapping_mul(2654435761))
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
        let Some(ev) = self.store.events.iter().find(|e| e.id == id).cloned() else { return };
        self.store.events.retain(|e| e.id != id);
        if self.inspector.as_ref().is_some_and(|i| i.event_id == id) {
            self.inspector = None;
        }
        self.push_delete(&ev);
        self.dirty_store = true;
        self.flash(format!("{} \"{}\"", t("Deleted"), ev.title), false);
    }

    fn mark_doc_dirty(&mut self, cal_id: &str) {
        if let Some(doc) = self.file_doc.as_mut() {
            if doc.calendar_id == cal_id && !doc.dirty {
                doc.dirty = true;
                host::post_dirty(true);
            }
        }
    }

    fn push_put(&mut self, ev: &CalendarEvent) {
        let Some(acc) = self.account_of(&ev.calendar_id).cloned() else { return };
        match acc.kind {
            AccountKind::File => self.mark_doc_dirty(&ev.calendar_id),
            AccountKind::Caldav => {
                let Some(cal_url) = self.store.calendar_by_id(&ev.calendar_id).and_then(|c| c.url.clone()) else { return };
                let (Some(ep), Some(inbox)) = (self.endpoint(&acc), self.inbox.clone()) else { return };
                let ev = ev.clone();
                let acc_id = acc.id.clone();
                self.busy += 1;
                spawn_task(async move {
                    match sync::put_event(&ep, &cal_url, &ev).await {
                        Ok((href, etag)) => inbox.push(SyncMsg::Saved { event_id: ev.id.clone(), href, etag }),
                        Err(what) => inbox.push(SyncMsg::Failed { account_id: acc_id, what }),
                    }
                });
            }
            _ => {}
        }
    }

    fn push_delete(&mut self, ev: &CalendarEvent) {
        let Some(acc) = self.account_of(&ev.calendar_id).cloned() else { return };
        match acc.kind {
            AccountKind::File => self.mark_doc_dirty(&ev.calendar_id),
            AccountKind::Caldav => {
                let Some(href) = ev.href.clone() else { return };
                let (Some(ep), Some(inbox)) = (self.endpoint(&acc), self.inbox.clone()) else { return };
                let etag = ev.etag.clone();
                let id = ev.id.clone();
                let acc_id = acc.id.clone();
                self.busy += 1;
                spawn_task(async move {
                    match sync::delete_event(&ep, &href, etag.as_deref()).await {
                        Ok(()) => inbox.push(SyncMsg::Deleted { event_id: id }),
                        Err(what) => inbox.push(SyncMsg::Failed { account_id: acc_id, what }),
                    }
                });
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------------------------
    // Accounts and syncing

    pub fn endpoint(&self, acc: &AccountConfig) -> Option<Endpoint> {
        let base = acc.server_url.clone()?;
        let auth = match (&acc.username, self.secrets.get(&acc.id)) {
            (Some(u), Some(p)) if !acc.host_managed => Some((u.clone(), p.clone())),
            _ => None,
        };
        // In a browser, servers on other origins go through the host's proxy when it has one.
        let proxy = if cfg!(target_arch = "wasm32") && !acc.host_managed && (acc.via_proxy || !base.starts_with('/')) { self.host.remote_proxy.clone() } else { None };
        Some(Endpoint { base, auth, proxy, host_managed: acc.host_managed })
    }

    /// Find the calendars of every server account and read their events.
    pub fn refresh_all(&mut self) {
        let ids: Vec<String> = self.store.accounts.iter().filter(|a| a.enabled && matches!(a.kind, AccountKind::Caldav | AccountKind::Ics)).map(|a| a.id.clone()).collect();
        self.loaded.clear();
        for id in ids {
            self.refresh_account(&id);
        }
    }

    pub fn refresh_account(&mut self, account_id: &str) {
        let Some(acc) = self.store.accounts.iter().find(|a| a.id == account_id).cloned() else { return };
        let (Some(ep), Some(inbox)) = (self.endpoint(&acc), self.inbox.clone()) else { return };
        self.status.remove(account_id);
        self.busy += 1;
        let acc_id = acc.id.clone();
        match acc.kind {
            AccountKind::Caldav => spawn_task(async move {
                match sync::discover(&ep).await {
                    Ok(calendars) => inbox.push(SyncMsg::Calendars { account_id: acc_id, calendars }),
                    Err(what) => inbox.push(SyncMsg::Failed { account_id: acc_id, what }),
                }
            }),
            AccountKind::Ics => {
                let cal_id = self.ensure_feed_calendar(&acc);
                let url = acc.server_url.clone().unwrap_or_default();
                spawn_task(async move {
                    match sync::fetch_feed(&ep, &url, &cal_id).await {
                        Ok(cal) => inbox.push(SyncMsg::Events { calendar_id: cal_id, events: cal.events, name: cal.name, color: cal.color }),
                        Err(what) => inbox.push(SyncMsg::Failed { account_id: acc_id, what }),
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
        self.store.calendars.push(CalendarSource { id: id.clone(), account_id: acc.id.clone(), name: acc.name.clone(), color_hex: color, visible: true, read_only: true, url: acc.server_url.clone(), ctag: None });
        self.add_to_all_set(&id);
        id
    }

    fn add_to_all_set(&mut self, cal_id: &str) {
        for set in &mut self.store.calendar_sets {
            if set.id == "set_all" && !set.calendar_ids.iter().any(|c| c == cal_id) {
                set.calendar_ids.push(cal_id.to_string());
            }
        }
    }

    pub fn next_color(&self) -> String {
        let used: Vec<String> = self.store.calendars.iter().map(|c| c.color_hex.to_ascii_uppercase()).collect();
        theme::CALENDAR_COLORS.iter().map(|(_, c)| c.to_string()).find(|c| !used.contains(c)).unwrap_or_else(|| theme::CALENDAR_COLORS[self.store.calendars.len() % theme::CALENDAR_COLORS.len()].1.to_string())
    }

    fn load_events(&mut self, cal_id: &str) {
        let Some(cal) = self.store.calendar_by_id(cal_id).cloned() else { return };
        let Some(acc) = self.account_of(cal_id).cloned() else { return };
        if acc.kind != AccountKind::Caldav {
            return;
        }
        let (Some(url), Some(ep), Some(inbox)) = (cal.url.clone(), self.endpoint(&acc), self.inbox.clone()) else { return };
        let (from, to) = sync::window_around(self.selected_date);
        self.loaded.insert(cal_id.to_string(), (from, to));
        let id = cal.id.clone();
        let acc_id = acc.id.clone();
        self.busy += 1;
        spawn_task(async move {
            match sync::fetch_events(&ep, &url, &id, from, to).await {
                Ok(events) => inbox.push(SyncMsg::Events { calendar_id: id, events, name: None, color: None }),
                Err(what) => inbox.push(SyncMsg::Failed { account_id: acc_id, what }),
            }
        });
    }

    /// Read again when the selection moves outside what was read.
    fn ensure_range(&mut self) {
        let cal_ids: Vec<String> = self.store.calendars.iter().filter(|c| c.url.is_some() && self.account_of(&c.id).is_some_and(|a| a.kind == AccountKind::Caldav && a.enabled)).map(|c| c.id.clone()).collect();
        for id in cal_ids {
            let inside = self.loaded.get(&id).is_some_and(|(from, to)| self.selected_date - Duration::days(62) >= *from && self.selected_date + Duration::days(62) <= *to);
            if !inside && self.loaded.contains_key(&id) {
                self.load_events(&id);
            }
        }
    }

    fn handle_sync(&mut self, msg: SyncMsg) {
        self.busy = self.busy.saturating_sub(1);
        match msg {
            SyncMsg::Calendars { account_id, calendars } => {
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
                            self.store.calendars.push(CalendarSource { id: id.clone(), account_id: account_id.clone(), name: rc.name.clone(), color_hex: color, visible: true, read_only: rc.read_only, url: Some(rc.url.clone()), ctag: rc.ctag.clone() });
                            self.add_to_all_set(&id);
                        }
                    }
                    fresh.push(id);
                }
                // Calendars deleted on the server go here too.
                let stale: Vec<String> = self.store.calendars.iter().filter(|c| c.account_id == account_id && !fresh.contains(&c.id)).map(|c| c.id.clone()).collect();
                self.store.calendars.retain(|c| !stale.contains(&c.id));
                self.store.events.retain(|e| !stale.contains(&e.calendar_id));
                for id in fresh {
                    self.load_events(&id);
                }
                self.dirty_store = true;
            }
            SyncMsg::Events { calendar_id, events, name, color } => {
                // Keep events created here that the server has not confirmed yet.
                let is_caldav = self.account_of(&calendar_id).is_some_and(|a| a.kind == AccountKind::Caldav);
                self.store.events.retain(|e| e.calendar_id != calendar_id || (e.href.is_none() && is_caldav));
                let pending: Vec<String> = self.store.events.iter().map(|e| e.uid.clone()).collect();
                self.store.events.extend(events.into_iter().filter(|e| e.uid.is_empty() || !pending.contains(&e.uid)));
                if let Some(c) = self.store.calendars.iter_mut().find(|c| c.id == calendar_id) {
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
            SyncMsg::Saved { event_id, href, etag } => {
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
                let name = self.store.accounts.iter().find(|a| a.id == account_id).map(|a| a.name.clone()).unwrap_or_default();
                self.status.insert(account_id.clone(), what.clone());
                self.flash(format!("{name}: {what}"), true);
                // A refused change: read the server's version back.
                let cals: Vec<String> = self.store.calendars.iter().filter(|c| c.account_id == account_id).map(|c| c.id.clone()).collect();
                if what.contains("changed on the server") {
                    for c in cals {
                        self.load_events(&c);
                    }
                }
            }
        }
    }

    pub fn add_account_from_form(&mut self) {
        let f = self.account_form.clone();
        let url = f.url.trim().to_string();
        if !(url.starts_with("https://") || url.starts_with("http://") || url.starts_with("webcal://")) {
            self.account_form.error = Some(t("Enter the server address, starting with https://").into());
            return;
        }
        let id = format!("acc_{}", ics::short_hash(&format!("{url}{}", f.username)));
        let kind = if f.kind_ics || url.starts_with("webcal://") || url.to_ascii_lowercase().ends_with(".ics") { AccountKind::Ics } else { AccountKind::Caldav };
        let name = if f.name.trim().is_empty() { crate::net::host_of(&url) } else { f.name.trim().to_string() };
        self.store.accounts.retain(|a| a.id != id);
        self.store.accounts.push(AccountConfig {
            id: id.clone(),
            name,
            provider: if kind == AccountKind::Ics { "ICS".into() } else { "CalDAV".into() },
            email: f.username.trim().to_string(),
            server_url: Some(url),
            enabled: true,
            kind,
            username: Some(f.username.trim().to_string()).filter(|u| !u.is_empty()),
            host_managed: false,
            via_proxy: false,
        });
        if !f.password.is_empty() {
            self.secrets.insert(id.clone(), f.password.clone());
            host::save(host::KEY_SECRETS, &serde_json::to_string(&self.secrets).unwrap_or_default());
        }
        self.account_form = AccountForm::default();
        self.dirty_store = true;
        self.refresh_account(&id);
    }

    pub fn remove_account(&mut self, id: &str) {
        let cals: Vec<String> = self.store.calendars.iter().filter(|c| c.account_id == id).map(|c| c.id.clone()).collect();
        self.store.events.retain(|e| !cals.contains(&e.calendar_id));
        self.store.calendars.retain(|c| c.account_id != id);
        self.store.accounts.retain(|a| a.id != id);
        for set in &mut self.store.calendar_sets {
            set.calendar_ids.retain(|c| !cals.contains(c));
        }
        if self.secrets.remove(id).is_some() {
            host::save(host::KEY_SECRETS, &serde_json::to_string(&self.secrets).unwrap_or_default());
        }
        self.dirty_store = true;
    }

    pub fn add_local_calendar(&mut self, name: &str) {
        if !self.store.accounts.iter().any(|a| a.id == "acc_local") {
            self.store.accounts.push(AccountConfig { id: "acc_local".into(), name: t("On this device").into(), provider: "Local".into(), enabled: true, kind: AccountKind::Local, ..Default::default() });
        }
        let id = format!("cal_local_{}", ics::short_hash(&format!("{name}{}", self.store.calendars.len())));
        let color = self.next_color();
        self.store.calendars.push(CalendarSource { id: id.clone(), account_id: "acc_local".into(), name: name.to_string(), color_hex: color, visible: true, ..Default::default() });
        self.add_to_all_set(&id);
        self.dirty_store = true;
    }

    // -----------------------------------------------------------------------------------------
    // The host's document

    fn open_document(&mut self, name: String, bytes: Vec<u8>) {
        let text = String::from_utf8_lossy(&bytes).to_string();
        let acc_id = "acc_file".to_string();
        let cal_id = "cal_file".to_string();
        let parsed = if text.trim().is_empty() { Ok(ics::IcsCalendar::default()) } else { ics::parse(&text, &cal_id) };
        match parsed {
            Ok(cal) => {
                self.store.events.retain(|e| e.calendar_id != cal_id);
                self.store.calendars.retain(|c| c.id != cal_id);
                self.store.accounts.retain(|a| a.id != acc_id);
                self.store.accounts.insert(0, AccountConfig { id: acc_id.clone(), name: name.clone(), provider: t("File").into(), enabled: true, kind: AccountKind::File, ..Default::default() });
                let color = cal.color.as_deref().and_then(crate::caldav::normalize_color).unwrap_or_else(|| "#B899EB".into());
                let display = cal.name.clone().unwrap_or_else(|| name.trim_end_matches(".ics").to_string());
                self.store.calendars.insert(0, CalendarSource { id: cal_id.clone(), account_id: acc_id.clone(), name: display, color_hex: color, visible: true, ..Default::default() });
                self.add_to_all_set(&cal_id);
                let first = cal.events.iter().map(|e| e.date).filter(|d| *d >= today()).min().or_else(|| cal.events.iter().map(|e| e.date).max());
                let count = cal.events.len();
                self.store.events.extend(cal.events);
                self.file_doc = Some(FileDoc { name: name.clone(), account_id: acc_id, calendar_id: cal_id, dirty: false });
                if let Some(day) = first {
                    self.select_date(day);
                }
                self.flash(format!("{name}: {count} {}", t("events")), false);
            }
            Err(err) => {
                host::post_failed(&err);
                self.flash(format!("{name}: {err}"), true);
            }
        }
    }

    /// `file.save`: send the document back to the host.
    pub fn save_document(&mut self) {
        let Some(doc) = self.file_doc.clone() else { return };
        let cal_name = self.store.calendar_by_id(&doc.calendar_id).map(|c| c.name.clone());
        let events: Vec<&CalendarEvent> = self.store.events.iter().filter(|e| e.calendar_id == doc.calendar_id).collect();
        let text = ics::write_calendar(cal_name.as_deref(), events);
        host::post_save(&doc.name, text.as_bytes());
        if let Some(d) = self.file_doc.as_mut() {
            d.dirty = false;
        }
        host::post_dirty(false);
    }

    fn handle_host(&mut self, msg: HostMsg) {
        match msg {
            HostMsg::Open { name, bytes } => self.open_document(name, bytes),
            HostMsg::Command(cmd) => match cmd.as_str() {
                "file.save" => self.save_document(),
                "view.today" => self.select_date(today()),
                "sync.refresh" => self.refresh_all(),
                _ => {}
            },
        }
    }

    // -----------------------------------------------------------------------------------------
    // Saving options and data

    fn persist(&mut self) {
        if !self.dirty_store || self.now - self.last_saved < 0.6 {
            return;
        }
        self.dirty_store = false;
        self.last_saved = self.now;
        host::save(host::KEY_UI, &serde_json::to_string(&self.prefs).unwrap_or_default());
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
        let server_cals: Vec<String> = self.store.calendars.iter().filter(|c| self.account_of(&c.id).is_some_and(|a| matches!(a.kind, AccountKind::Caldav | AccountKind::Ics))).map(|c| c.id.clone()).collect();
        let mut keep = self.store.clone();
        keep.accounts.retain(|a| a.kind != AccountKind::File);
        keep.calendars.retain(|c| c.account_id != "acc_file");
        keep.events.retain(|e| e.calendar_id != "cal_file" && !server_cals.contains(&e.calendar_id));
        if let Ok(json) = serde_json::to_string(&keep) {
            host::save(host::KEY_STORE, &json);
        }
        let cache: Vec<&CalendarEvent> = self.store.events.iter().filter(|e| server_cals.contains(&e.calendar_id)).collect();
        host::save(host::KEY_CACHE, &serde_json::to_string(&cache).unwrap_or_default());
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
            ThemeChoice::System => ctx.input(|i| i.raw.system_theme).map(|t| t == egui::Theme::Dark).unwrap_or(false),
        };
        let animations = match self.prefs.animations.as_str() {
            "off" => false,
            "on" => true,
            _ => !prefers_reduced_motion(),
        };
        let key = (dark, animations, self.prefs.language.clone());
        if self.applied_style.as_ref() != Some(&key) {
            self.palette = theme::palette(dark);
            ctx.options_mut(|o| o.theme_preference = if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
            theme::apply(ctx, &self.palette, animations);
            i18n::set_language(&self.prefs.language);
            self.applied_style = Some(key);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.launch.screenshot.clone() else { return };
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
            if let Err(err) = image::save_buffer(&path, &rgba, img.width() as u32, img.height() as u32, image::ColorType::Rgba8) {
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
        host::start_bridge(self.host_inbox.clone(), Arc::new(move || c.request_repaint()));
        self.refresh_all();
        if let Some(id) = self.launch.open_event.clone() {
            self.open_inspector(&id, None);
        }
        if self.launch.new_event {
            self.open_new_event(self.selected_date, None);
        }
    }

    pub fn is_mobile(&self, ctx: &egui::Context) -> bool {
        ctx.screen_rect().width() < 720.0
    }

    pub fn keyboard(&mut self, ctx: &egui::Context) {
        let typing = ctx.wants_keyboard_input();
        let (esc, n, t_key, left, right, save) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Escape),
                i.modifiers.command && i.key_pressed(egui::Key::N),
                !i.modifiers.any() && i.key_pressed(egui::Key::T),
                !i.modifiers.any() && i.key_pressed(egui::Key::ArrowLeft),
                !i.modifiers.any() && i.key_pressed(egui::Key::ArrowRight),
                i.modifiers.command && i.key_pressed(egui::Key::S),
            )
        });
        if esc {
            if self.new_event.is_some() {
                self.new_event = None;
            } else if self.inspector.is_some() {
                self.inspector = None;
            } else {
                self.settings_open = false;
            }
        }
        if n {
            self.open_new_event(self.selected_date, None);
        }
        if save {
            self.save_document();
        }
        if !typing && self.inspector.is_none() && self.new_event.is_none() {
            if t_key {
                self.select_date(today());
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
    web_sys::window().and_then(|w| w.match_media("(prefers-reduced-motion: reduce)").ok().flatten()).is_some_and(|m| m.matches())
}

#[cfg(not(target_arch = "wasm32"))]
fn prefers_reduced_motion() -> bool {
    false
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
    let next = if m == 12 { NaiveDate::from_ymd_opt(y + 1, 1, 1) } else { NaiveDate::from_ymd_opt(y, m + 1, 1) };
    next.map(|n| (n - Duration::days(1)).day()).unwrap_or(30)
}

/// Monday of the week holding `d`.
pub fn week_start(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}

pub fn parse_hhmm(text: &str) -> Option<NaiveTime> {
    let t = text.trim();
    NaiveTime::parse_from_str(t, "%H:%M").ok().or_else(|| crate::nlp::parse_time_token(t))
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
            self.handle_sync(m);
        }
        let host_msgs: Vec<HostMsg> = self.host_inbox.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
        for m in host_msgs {
            self.handle_host(m);
        }
        if self.banner.is_some() && self.now > self.banner_until {
            self.banner = None;
        }
        self.ensure_range();
        self.keyboard(ctx);

        if self.is_mobile(ctx) {
            self.mobile_layout(ctx);
        } else {
            self.desktop_layout(ctx);
        }
        self.popups(ctx);
        self.persist();
        if self.dirty_store {
            ctx.request_repaint_after(std::time::Duration::from_millis(700));
        }
    }
}
