//! CalDAV (RFC 4791) requests and responses, without any networking: what to send to find an
//! account's calendars and read or write their events, and how to read the multistatus answers.
//! [`crate::sync`] runs these over HTTP.

use quick_xml::events::Event;
use quick_xml::Reader;

pub const PROPFIND_PRINCIPAL: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:"><d:prop><d:current-user-principal/></d:prop></d:propfind>"#;

pub const PROPFIND_HOME: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><c:calendar-home-set/></d:prop></d:propfind>"#;

pub const PROPFIND_CALENDARS: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:cs="http://calendarserver.org/ns/" xmlns:a="http://apple.com/ns/ical/">
<d:prop><d:resourcetype/><d:displayname/><cs:getctag/><d:sync-token/><a:calendar-color/><c:supported-calendar-component-set/><d:current-user-privilege-set/></d:prop></d:propfind>"#;

/// `calendar-query` for the events that touch `[start, end)` (UTC, `YYYYMMDDTHHMMSSZ`).
pub fn report_range(start: &str, end: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:getetag/><c:calendar-data/></d:prop>
<c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VEVENT"><c:time-range start="{start}" end="{end}"/></c:comp-filter></c:comp-filter></c:filter></c:calendar-query>"#
    )
}

/// One `<response>` of a multistatus answer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DavResponse {
    pub href: String,
    /// `200` when a propstat with a 2xx status held the props.
    pub ok: bool,
    pub is_calendar: bool,
    pub is_collection: bool,
    pub displayname: Option<String>,
    pub color: Option<String>,
    pub ctag: Option<String>,
    pub etag: Option<String>,
    pub calendar_data: Option<String>,
    /// `current-user-principal` / `calendar-home-set` hrefs.
    pub principal: Option<String>,
    pub home: Option<String>,
    /// The collection takes VEVENTs (no component set listed counts as yes).
    pub events: bool,
    pub read_only: bool,
}

/// Read a `207 Multi-Status` body. Namespaces are matched by local name, which is enough for the
/// properties CalendarCraft asks for.
pub fn parse_multistatus(xml: &str) -> Result<Vec<DavResponse>, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut out = Vec::new();
    let mut path: Vec<String> = Vec::new();
    let mut current: Option<DavResponse> = None;
    let mut text = String::new();
    // Props of the current propstat, applied when its status says 2xx.
    let mut pending = DavResponse::default();
    let mut privileges: Vec<String> = Vec::new();
    let mut components: Vec<String> = Vec::new();
    let mut saw_components = false;
    loop {
        let event = reader.read_event().map_err(|e| format!("bad XML from the server: {e}"))?;
        match event {
            Event::Start(e) => {
                let name = local(e.name().as_ref());
                text.clear();
                match name.as_str() {
                    "response" => {
                        current = Some(DavResponse { events: true, ..Default::default() });
                        privileges.clear();
                    }
                    "propstat" => {
                        pending = DavResponse::default();
                        components.clear();
                        saw_components = false;
                    }
                    "supported-calendar-component-set" => saw_components = true,
                    _ => {}
                }
                path.push(name);
            }
            Event::Empty(e) => {
                let name = local(e.name().as_ref());
                let parent = path.last().map(String::as_str).unwrap_or("");
                match (parent, name.as_str()) {
                    ("resourcetype", "calendar") => pending.is_calendar = true,
                    ("resourcetype", "collection") => pending.is_collection = true,
                    ("supported-calendar-component-set", "comp") => {
                        saw_components = true;
                        for attr in e.attributes().flatten() {
                            if local(attr.key.as_ref()) == "name" {
                                components.push(String::from_utf8_lossy(&attr.value).to_ascii_uppercase());
                            }
                        }
                    }
                    ("privilege", p) => privileges.push(p.to_string()),
                    (_, "supported-calendar-component-set") => saw_components = true,
                    _ => {}
                }
            }
            Event::Text(t) => {
                text.push_str(&t.unescape().map_err(|e| e.to_string())?);
            }
            Event::CData(t) => text.push_str(&String::from_utf8_lossy(&t.into_inner())),
            Event::End(_) => {
                let name = path.pop().unwrap_or_default();
                let parent = path.last().cloned().unwrap_or_default();
                let value = text.trim().to_string();
                match name.as_str() {
                    "href" => match parent.as_str() {
                        "response" => {
                            if let Some(c) = current.as_mut() {
                                c.href = value.clone();
                            }
                        }
                        "current-user-principal" => pending.principal = Some(value.clone()),
                        "calendar-home-set" => pending.home = Some(value.clone()),
                        _ => {}
                    },
                    "displayname" => pending.displayname = Some(value.clone()).filter(|v| !v.is_empty()),
                    "calendar-color" => pending.color = Some(value.clone()).filter(|v| !v.is_empty()),
                    "getctag" => pending.ctag = Some(value.clone()).filter(|v| !v.is_empty()),
                    "sync-token" if pending.ctag.is_none() => pending.ctag = Some(value.clone()).filter(|v| !v.is_empty()),
                    "getetag" => pending.etag = Some(value.clone()).filter(|v| !v.is_empty()),
                    "calendar-data" => pending.calendar_data = Some(text.clone()).filter(|v| !v.trim().is_empty()),
                    "status" if parent == "propstat" => {
                        let good = value.split_whitespace().nth(1).is_some_and(|code| code.starts_with('2'));
                        if good {
                            if let Some(c) = current.as_mut() {
                                merge(c, &pending);
                                if saw_components {
                                    c.events = components.iter().any(|x| x == "VEVENT");
                                }
                            }
                        }
                    }
                    "status" if parent == "response" => {
                        if let Some(c) = current.as_mut() {
                            c.ok = value.split_whitespace().nth(1).is_some_and(|code| code.starts_with('2'));
                        }
                    }
                    "response" => {
                        if let Some(mut c) = current.take() {
                            if !privileges.is_empty() {
                                c.read_only = !privileges.iter().any(|p| matches!(p.as_str(), "write" | "write-content" | "all" | "bind"));
                            }
                            out.push(c);
                        }
                    }
                    _ => {}
                }
                text.clear();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

fn merge(into: &mut DavResponse, from: &DavResponse) {
    into.ok = true;
    into.is_calendar |= from.is_calendar;
    into.is_collection |= from.is_collection;
    macro_rules! take {
        ($f:ident) => {
            if from.$f.is_some() {
                into.$f = from.$f.clone();
            }
        };
    }
    take!(displayname);
    take!(color);
    take!(ctag);
    take!(etag);
    take!(calendar_data);
    take!(principal);
    take!(home);
}

fn local(name: &[u8]) -> String {
    let s = String::from_utf8_lossy(name);
    s.rsplit(':').next().unwrap_or(&s).to_string()
}

/// Resolve `href` (absolute URL or absolute path) against `base`.
pub fn resolve(base: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    let origin_end = base.find("://").map(|i| base[i + 3..].find('/').map(|j| i + 3 + j).unwrap_or(base.len()));
    match (origin_end, href.starts_with('/')) {
        (Some(end), true) => format!("{}{}", &base[..end], href),
        (_, true) => href.to_string(),
        _ => {
            let dir = match base.rfind('/') {
                Some(i) => &base[..=i],
                None => "",
            };
            format!("{dir}{href}")
        }
    }
}

/// `#RRGGBBAA` (Apple) → `#RRGGBB`.
pub fn normalize_color(color: &str) -> Option<String> {
    let c = color.trim();
    let hex = c.strip_prefix('#')?;
    (hex.len() >= 6 && hex[..6].chars().all(|x| x.is_ascii_hexdigit())).then(|| format!("#{}", &hex[..6].to_ascii_uppercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shaped like Stalwart 1.2's answers.
    const CALENDARS: &str = r#"<?xml version="1.0" encoding="UTF-8"?><D:multistatus xmlns:D="DAV:" xmlns:A="urn:ietf:params:xml:ns:caldav" xmlns:B="http://calendarserver.org/ns/" xmlns:C="http://apple.com/ns/ical/"><D:response><D:href>/dav/cal/ada%40example.test/</D:href><D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype><D:displayname>Ada</D:displayname></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat><D:propstat><D:prop><B:getctag/></D:prop><D:status>HTTP/1.1 404 Not Found</D:status></D:propstat></D:response><D:response><D:href>/dav/cal/ada%40example.test/default/</D:href><D:propstat><D:prop><D:resourcetype><D:collection/><A:calendar/></D:resourcetype><D:displayname>Stalwart Calendar</D:displayname><B:getctag>"42"</B:getctag><C:calendar-color>#B899EBFF</C:calendar-color><A:supported-calendar-component-set><A:comp name="VEVENT"/><A:comp name="VTODO"/></A:supported-calendar-component-set><D:current-user-privilege-set><D:privilege><D:read/></D:privilege><D:privilege><D:write/></D:privilege></D:current-user-privilege-set></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response><D:response><D:href>/dav/cal/ada%40example.test/tasks/</D:href><D:propstat><D:prop><D:resourcetype><D:collection/><A:calendar/></D:resourcetype><A:supported-calendar-component-set><A:comp name="VTODO"/></A:supported-calendar-component-set></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response></D:multistatus>"#;

    #[test]
    fn finds_calendars_their_names_colours_and_rights() {
        let all = parse_multistatus(CALENDARS).unwrap();
        assert_eq!(all.len(), 3);
        assert!(!all[0].is_calendar && all[0].is_collection);
        assert_eq!(all[0].ctag, None, "404 propstat ignored");
        let cal = &all[1];
        assert!(cal.is_calendar && cal.events && !cal.read_only);
        assert_eq!(cal.displayname.as_deref(), Some("Stalwart Calendar"));
        assert_eq!(cal.ctag.as_deref(), Some("\"42\""));
        assert_eq!(normalize_color(cal.color.as_deref().unwrap()).as_deref(), Some("#B899EB"));
        assert!(!all[2].events, "a task list is not shown");
    }

    #[test]
    fn reads_event_data_and_principals() {
        let xml = r#"<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav"><d:response><d:href>/c/x.ics</d:href><d:propstat><d:prop><d:getetag>"e1"</d:getetag><cal:calendar-data>BEGIN:VCALENDAR&#13;
SUMMARY:Tom &amp; Jerry&#13;
END:VCALENDAR</cal:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response><d:response><d:href>/p/</d:href><d:propstat><d:prop><d:current-user-principal><d:href>/principals/ada/</d:href></d:current-user-principal><cal:calendar-home-set><d:href>/cal/ada/</d:href></cal:calendar-home-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>"#;
        let all = parse_multistatus(xml).unwrap();
        assert_eq!(all[0].etag.as_deref(), Some("\"e1\""));
        assert!(all[0].calendar_data.as_deref().unwrap().contains("SUMMARY:Tom & Jerry"));
        assert_eq!(all[1].principal.as_deref(), Some("/principals/ada/"));
        assert_eq!(all[1].home.as_deref(), Some("/cal/ada/"));
    }

    #[test]
    fn hrefs_resolve_against_the_server() {
        assert_eq!(resolve("https://dav.example.org/a/b/", "/cal/x/"), "https://dav.example.org/cal/x/");
        assert_eq!(resolve("https://dav.example.org/a/b/", "x.ics"), "https://dav.example.org/a/b/x.ics");
        assert_eq!(resolve("/caldav/ada/", "/caldav/ada/default/"), "/caldav/ada/default/");
        assert_eq!(resolve("https://a.org/", "https://b.org/c"), "https://b.org/c");
    }
}
