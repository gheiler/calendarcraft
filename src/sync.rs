//! Talking to calendar servers: find an account's calendars, read events in a date range, write
//! and delete events (CalDAV), and read `.ics` subscriptions. Results come back as [`SyncMsg`]s
//! in an inbox the UI drains every frame.

use std::sync::{Arc, Mutex};

use base64::Engine;
use chrono::{Duration, NaiveDate};

use crate::caldav::{self, DavResponse};
use crate::ics;
use crate::model::CalendarEvent;
use crate::net::{self, HttpRequest, HttpResponse};

/// Where an account lives and how requests get there.
#[derive(Debug, Clone, Default)]
pub struct Endpoint {
    /// The account URL: a CalDAV server, principal or calendar home, or an `.ics` feed. For the
    /// host's own server this is a same-origin path (`/caldav/<address>/`).
    pub base: String,
    pub auth: Option<(String, String)>,
    /// Send through this same-origin proxy (`x-calendar-url` names the target), for servers the
    /// browser cannot reach directly because of CORS.
    pub proxy: Option<String>,
    /// The host signs requests itself (cookie on the same origin): mark them as the app's own.
    pub host_managed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteCalendar {
    pub url: String,
    pub name: String,
    pub color: Option<String>,
    pub ctag: Option<String>,
    pub read_only: bool,
}

#[derive(Debug, Clone)]
pub enum SyncMsg {
    Calendars { account_id: String, calendars: Vec<RemoteCalendar> },
    Events { calendar_id: String, events: Vec<CalendarEvent>, name: Option<String>, color: Option<String> },
    Saved { event_id: String, href: String, etag: Option<String> },
    Deleted { event_id: String },
    Failed { account_id: String, what: String },
}

/// Messages from background work, and a way to wake the UI.
#[derive(Clone)]
pub struct Inbox {
    pub items: Arc<Mutex<Vec<SyncMsg>>>,
    pub wake: Arc<dyn Fn() + Send + Sync>,
}

impl Inbox {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { items: Arc::new(Mutex::new(Vec::new())), wake }
    }
    pub fn push(&self, msg: SyncMsg) {
        if let Ok(mut items) = self.items.lock() {
            items.push(msg);
        }
        (self.wake)();
    }
    pub fn drain(&self) -> Vec<SyncMsg> {
        self.items.lock().map(|mut items| std::mem::take(&mut *items)).unwrap_or_default()
    }
}

impl Endpoint {
    fn request(&self, method: &str, url: &str, depth: Option<&str>, body: Option<String>, extra: &[(&str, String)]) -> HttpRequest {
        let mut headers: Vec<(String, String)> = Vec::new();
        if let Some(d) = depth {
            headers.push(("Depth".into(), d.into()));
        }
        if body.is_some() && !extra.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")) {
            headers.push(("Content-Type".into(), "application/xml; charset=utf-8".into()));
        }
        for (k, v) in extra {
            headers.push(((*k).into(), v.clone()));
        }
        let auth = self.auth.as_ref().map(|(user, pass)| format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"))));
        let url = match &self.proxy {
            Some(proxy) => {
                headers.push(("x-calendar-url".into(), url.to_string()));
                if let Some(a) = auth {
                    headers.push(("x-calendar-auth".into(), a));
                }
                headers.push(("x-requested-with".into(), "artcraft".into()));
                proxy.clone()
            }
            None => {
                if let Some(a) = auth {
                    headers.push(("Authorization".into(), a));
                }
                if self.host_managed {
                    headers.push(("x-requested-with".into(), "artcraft".into()));
                }
                url.to_string()
            }
        };
        HttpRequest { method: method.into(), url, headers, body }
    }

    async fn send(&self, method: &str, url: &str, depth: Option<&str>, body: Option<String>, extra: &[(&str, String)]) -> Result<(String, HttpResponse), String> {
        let mut at = url.to_string();
        for _ in 0..5 {
            let resp = net::send(self.request(method, &at, depth, body.clone(), extra)).await?;
            match (resp.status, &resp.location) {
                (301 | 302 | 303 | 307 | 308, Some(loc)) => at = caldav::resolve(&at, loc),
                (401, _) => return Err("the server did not accept the user name and password".into()),
                (403, _) => return Err("the server refused access".into()),
                _ => return Ok((at, resp)),
            }
        }
        Err("too many redirects".into())
    }

    async fn propfind(&self, url: &str, depth: &str, body: &str) -> Result<(String, Vec<DavResponse>), String> {
        let (at, resp) = self.send("PROPFIND", url, Some(depth), Some(body.to_string()), &[]).await?;
        if resp.status != 207 {
            return Err(format!("{} answered {} to PROPFIND", net::host_of(&at), resp.status));
        }
        Ok((at.clone(), caldav::parse_multistatus(&resp.body)?))
    }
}

fn calendars_in(at: &str, list: &[DavResponse]) -> Vec<RemoteCalendar> {
    list.iter()
        .filter(|r| r.is_calendar && r.events)
        .map(|r| RemoteCalendar {
            url: caldav::resolve(at, &r.href),
            name: r.displayname.clone().unwrap_or_else(|| {
                let trimmed = r.href.trim_end_matches('/');
                percent_decode(trimmed.rsplit('/').next().unwrap_or("Calendar"))
            }),
            color: r.color.as_deref().and_then(caldav::normalize_color),
            ctag: r.ctag.clone(),
            read_only: r.read_only,
        })
        .collect()
}

/// Find the calendars of a CalDAV account. `base` may be the server (with `/.well-known/caldav`),
/// a principal, a calendar home, or one calendar.
pub async fn discover(ep: &Endpoint) -> Result<Vec<RemoteCalendar>, String> {
    let base = ep.base.clone();
    let mut first_error = None;
    match ep.propfind(&base, "1", caldav::PROPFIND_CALENDARS).await {
        Ok((at, list)) => {
            let cals = calendars_in(&at, &list);
            if !cals.is_empty() {
                return Ok(cals);
            }
        }
        Err(e) => first_error = Some(e),
    }
    let mut starts = vec![base.clone()];
    if let Some(i) = base.find("://") {
        let origin_end = base[i + 3..].find('/').map(|j| i + 3 + j).unwrap_or(base.len());
        starts.push(format!("{}/.well-known/caldav", &base[..origin_end]));
    }
    for start in starts {
        let Ok((at, list)) = ep.propfind(&start, "0", caldav::PROPFIND_PRINCIPAL).await else { continue };
        let Some(principal) = list.iter().find_map(|r| r.principal.clone()) else { continue };
        let principal = caldav::resolve(&at, &principal);
        let Ok((at, list)) = ep.propfind(&principal, "0", caldav::PROPFIND_HOME).await else { continue };
        let Some(home) = list.iter().find_map(|r| r.home.clone()) else { continue };
        let home = caldav::resolve(&at, &home);
        let (at, list) = ep.propfind(&home, "1", caldav::PROPFIND_CALENDARS).await?;
        return Ok(calendars_in(&at, &list));
    }
    Err(first_error.unwrap_or_else(|| "no calendars found at that address".into()))
}

fn utc_bound(day: NaiveDate) -> String {
    format!("{}T000000Z", day.format("%Y%m%d"))
}

/// The id CalendarCraft gives an event read from `href` (the `n`th in the resource).
pub fn remote_id(href: &str, n: usize) -> String {
    if n == 0 {
        format!("dav_{}", ics::short_hash(href))
    } else {
        format!("dav_{}_{n}", ics::short_hash(href))
    }
}

/// Events of one calendar that touch `[from, to)`.
pub async fn fetch_events(ep: &Endpoint, cal_url: &str, calendar_id: &str, from: NaiveDate, to: NaiveDate) -> Result<Vec<CalendarEvent>, String> {
    let body = caldav::report_range(&utc_bound(from), &utc_bound(to));
    let (at, resp) = ep.send("REPORT", cal_url, Some("1"), Some(body), &[]).await?;
    if resp.status != 207 {
        return Err(format!("{} answered {} to REPORT", net::host_of(&at), resp.status));
    }
    let mut out = Vec::new();
    for item in caldav::parse_multistatus(&resp.body)? {
        let Some(data) = &item.calendar_data else { continue };
        let href = caldav::resolve(&at, &item.href);
        let Ok(parsed) = ics::parse(data, calendar_id) else { continue };
        let several = parsed.events.len() > 1;
        for (n, mut ev) in parsed.events.into_iter().enumerate() {
            ev.id = remote_id(&href, n);
            ev.href = Some(href.clone());
            ev.etag = item.etag.clone();
            ev.locked = several;
            out.push(ev);
        }
    }
    Ok(out)
}

fn safe_name(uid: &str) -> String {
    let clean: String = uid.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    if clean.trim_matches('-').is_empty() { ics::short_hash(uid) } else { clean }
}

/// Create or replace an event. Returns its href and new ETag (when the server sends one).
pub async fn put_event(ep: &Endpoint, cal_url: &str, ev: &CalendarEvent) -> Result<(String, Option<String>), String> {
    let url = match &ev.href {
        Some(h) => h.clone(),
        None => {
            let dir = if cal_url.ends_with('/') { cal_url.to_string() } else { format!("{cal_url}/") };
            format!("{dir}{}.ics", safe_name(&ev.uid_or_id()))
        }
    };
    let mut extra = vec![("Content-Type", "text/calendar; charset=utf-8".to_string())];
    match &ev.etag {
        Some(tag) if ev.href.is_some() => extra.push(("If-Match", tag.clone())),
        _ if ev.href.is_none() => extra.push(("If-None-Match", "*".to_string())),
        _ => {}
    }
    let (_, resp) = ep.send("PUT", &url, None, Some(ics::write_single(ev)), &extra).await?;
    match resp.status {
        200..=299 => Ok((url, resp.etag)),
        412 => Err("the event changed on the server; it was reloaded".into()),
        code => Err(format!("{} answered {code} when saving \"{}\"", net::host_of(&url), ev.title)),
    }
}

pub async fn delete_event(ep: &Endpoint, href: &str, etag: Option<&str>) -> Result<(), String> {
    let extra: Vec<(&str, String)> = etag.map(|t| vec![("If-Match", t.to_string())]).unwrap_or_default();
    let (_, resp) = ep.send("DELETE", href, None, None, &extra).await?;
    match resp.status {
        200..=299 | 404 => Ok(()),
        412 => Err("the event changed on the server; it was reloaded".into()),
        code => Err(format!("{} answered {code} when deleting", net::host_of(href))),
    }
}

/// Read an `.ics` subscription (`webcal://` is read over HTTPS).
pub async fn fetch_feed(ep: &Endpoint, url: &str, calendar_id: &str) -> Result<ics::IcsCalendar, String> {
    let url = match url.strip_prefix("webcal://") {
        Some(rest) => format!("https://{rest}"),
        None => url.to_string(),
    };
    let (at, resp) = ep.send("GET", &url, None, None, &[]).await?;
    if !resp.ok() {
        return Err(format!("{} answered {}", net::host_of(&at), resp.status));
    }
    let mut cal = ics::parse(&resp.body, calendar_id)?;
    for (n, ev) in cal.events.iter_mut().enumerate() {
        ev.id = format!("feed_{}_{}", ics::short_hash(calendar_id), if ev.uid.is_empty() { n.to_string() } else { ics::short_hash(&format!("{}{}", ev.uid, ev.date)) });
    }
    Ok(cal)
}

/// The range read around `focus`: half a year back, a year ahead.
pub fn window_around(focus: NaiveDate) -> (NaiveDate, NaiveDate) {
    (focus - Duration::days(186), focus + Duration::days(400))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxied_requests_name_the_target_and_keep_the_password_off_the_host() {
        let ep = Endpoint { base: "https://dav.example.org/".into(), auth: Some(("ada".into(), "pw".into())), proxy: Some("/api/calendar/remote".into()), host_managed: false };
        let req = ep.request("PROPFIND", "https://dav.example.org/cal/", Some("1"), Some("<x/>".into()), &[]);
        assert_eq!(req.url, "/api/calendar/remote");
        let h = |k: &str| req.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.clone());
        assert_eq!(h("x-calendar-url").as_deref(), Some("https://dav.example.org/cal/"));
        assert_eq!(h("x-calendar-auth").as_deref(), Some("Basic YWRhOnB3"));
        assert_eq!(h("authorization"), None);
        assert_eq!(h("depth").as_deref(), Some("1"));
    }

    #[test]
    fn names_and_ids() {
        assert_eq!(safe_name("abc@x.org"), "abc-x-org");
        assert_eq!(percent_decode("ada%40example.test"), "ada@example.test");
        assert_ne!(remote_id("/a.ics", 0), remote_id("/a.ics", 1));
    }
}
