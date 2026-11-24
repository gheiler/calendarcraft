//! The page around CalendarCraft: saved options (`localStorage` on the web, files natively), the
//! `craft-web-host` bridge when a host frames the app with `?host=parent` (artcraftCloud opens
//! `.ics` files from its drive this way), and the account list a host can hand over before the
//! app starts (`window.calendarcraftHost`).

use serde::Deserialize;
use std::sync::{Arc, Mutex};

/// Keys in storage. `calendarcraft.secrets` and `calendarcraft.cache` stay on this device (hosts
/// that copy options to a server should skip them).
pub const KEY_UI: &str = "calendarcraft.ui";
pub const KEY_STORE: &str = "calendarcraft.store";
pub const KEY_SECRETS: &str = "calendarcraft.secrets";
pub const KEY_CACHE: &str = "calendarcraft.cache";

/// What a host hands over before the app starts.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HostConfig {
    #[serde(default)]
    pub accounts: Vec<HostAccount>,
    /// Same-origin proxy for servers the browser cannot reach (CORS): `x-calendar-url` names the
    /// target.
    #[serde(default, rename = "remoteProxy")]
    pub remote_proxy: Option<String>,
    #[serde(default)]
    pub user: Option<HostUser>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HostAccount {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub email: String,
    /// The calendar home (CalDAV), same-origin; the host signs the requests.
    pub url: String,
    #[serde(default)]
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HostUser {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub email: String,
}

/// Messages from the host.
#[derive(Debug, Clone)]
pub enum HostMsg {
    /// `calendarcraft:open`: a document to show.
    Open { name: String, bytes: Vec<u8> },
    /// `window.calendarcraft.execute(command, args)`, e.g. `file.save` for the host's autosave,
    /// or `calendar.subscribe` with `{ url, name }`.
    Command(String, serde_json::Value),
}

pub type HostInbox = Arc<Mutex<Vec<HostMsg>>>;

// ---------------------------------------------------------------------------------------------
// Storage

#[cfg(target_arch = "wasm32")]
fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

#[cfg(target_arch = "wasm32")]
pub fn load(key: &str) -> Option<String> {
    local_storage()?.get_item(key).ok().flatten()
}

#[cfg(target_arch = "wasm32")]
pub fn save(key: &str, value: &str) {
    if let Some(s) = local_storage() {
        if s.get_item(key).ok().flatten().as_deref() != Some(value) {
            let _ = s.set_item(key, value);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn dir() -> std::path::PathBuf {
    if let Ok(d) = std::env::var("CALENDARCRAFT_HOME") {
        return d.into();
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    #[cfg(target_os = "macos")]
    return std::path::Path::new(&home).join("Library/Application Support/CalendarCraft");
    #[cfg(not(target_os = "macos"))]
    return std::path::Path::new(&home).join(".config/calendarcraft");
}

#[cfg(not(target_arch = "wasm32"))]
pub fn load(key: &str) -> Option<String> {
    std::fs::read_to_string(dir().join(format!("{key}.json"))).ok()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn save(key: &str, value: &str) {
    let d = dir();
    let _ = std::fs::create_dir_all(&d);
    let path = d.join(format!("{key}.json"));
    if std::fs::read_to_string(&path).ok().as_deref() == Some(value) {
        return;
    }
    let _ = std::fs::write(&path, value);
    #[cfg(unix)]
    if key == KEY_SECRETS {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
}

// ---------------------------------------------------------------------------------------------
// Page parameters and the host bridge

#[cfg(target_arch = "wasm32")]
pub fn query(key: &str) -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    let params = web_sys::UrlSearchParams::new_with_str(&search).ok()?;
    params.get(key)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn query(_key: &str) -> Option<String> {
    None
}

/// Framed by a host that speaks the bridge.
pub fn hosted() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        let search = web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default();
        search.trim_start_matches('?').split('&').any(|p| p == "host=parent")
    }
    #[cfg(not(target_arch = "wasm32"))]
    false
}

#[cfg(target_arch = "wasm32")]
pub fn host_config() -> HostConfig {
    let Some(window) = web_sys::window() else { return HostConfig::default() };
    let Ok(value) = js_sys::Reflect::get(&window, &wasm_bindgen::JsValue::from_str("calendarcraftHost")) else { return HostConfig::default() };
    if value.is_undefined() || value.is_null() {
        return HostConfig::default();
    }
    js_sys::JSON::stringify(&value).ok().and_then(|s| s.as_string()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn host_config() -> HostConfig {
    HostConfig::default()
}

#[cfg(target_arch = "wasm32")]
mod bridge {
    use super::*;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;

    fn parent() -> Option<web_sys::Window> {
        let w = web_sys::window()?;
        let p = w.parent().ok().flatten()?;
        // Not framed: parent is the window itself.
        (!js_sys::Object::is(&p, &w)).then_some(p)
    }

    fn origin() -> String {
        web_sys::window().and_then(|w| w.location().origin().ok()).unwrap_or_default()
    }

    fn message(kind: &str, fields: &[(&str, JsValue)]) -> js_sys::Object {
        let m = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&m, &"type".into(), &kind.into());
        for (k, v) in fields {
            let _ = js_sys::Reflect::set(&m, &(*k).into(), v);
        }
        m
    }

    pub fn post(kind: &str, fields: &[(&str, JsValue)], transfer: Option<&js_sys::ArrayBuffer>) {
        let Some(p) = parent() else { return };
        let m = message(kind, fields);
        match transfer {
            Some(buf) => {
                let list = js_sys::Array::of1(buf);
                let _ = p.post_message_with_transfer(&m, &origin(), &list);
            }
            None => {
                let _ = p.post_message(&m, &origin());
            }
        }
    }

    pub fn install(inbox: HostInbox, wake: Arc<dyn Fn() + Send + Sync>) {
        let Some(window) = web_sys::window() else { return };
        let own = origin();
        let inbox2 = inbox.clone();
        let wake2 = wake.clone();
        let on_message = Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
            if event.origin() != own {
                return;
            }
            if let (Some(src), Some(p)) = (event.source(), parent()) {
                if !js_sys::Object::is(&src, &p) {
                    return;
                }
            }
            let data = event.data();
            let ty = js_sys::Reflect::get(&data, &"type".into()).ok().and_then(|v| v.as_string()).unwrap_or_default();
            if ty == "calendarcraft:open" {
                let name = js_sys::Reflect::get(&data, &"name".into()).ok().and_then(|v| v.as_string()).unwrap_or_else(|| "Calendar.ics".into());
                let bytes = js_sys::Reflect::get(&data, &"bytes".into()).ok().map(|b| js_sys::Uint8Array::new(&b).to_vec()).unwrap_or_default();
                if let Ok(mut q) = inbox2.lock() {
                    q.push(HostMsg::Open { name, bytes });
                }
                wake2();
            }
        }) as Box<dyn FnMut(web_sys::MessageEvent)>);
        let _ = window.add_event_listener_with_callback("message", on_message.as_ref().unchecked_ref());
        on_message.forget();

        // Page API: window.calendarcraft.execute("file.save") (the host's autosave uses it).
        let api = js_sys::Object::new();
        let execute = Closure::wrap(Box::new(move |command: JsValue, args: JsValue| -> JsValue {
            let Some(cmd) = command.as_string() else { return JsValue::FALSE };
            let args = if args.is_object() {
                js_sys::JSON::stringify(&args).ok().and_then(|s| s.as_string()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
            } else {
                serde_json::Value::Null
            };
            if let Ok(mut q) = inbox.lock() {
                q.push(HostMsg::Command(cmd, args));
            }
            wake();
            JsValue::TRUE
        }) as Box<dyn FnMut(JsValue, JsValue) -> JsValue>);
        let _ = js_sys::Reflect::set(&api, &"execute".into(), execute.as_ref());
        let _ = js_sys::Reflect::set(&api, &"version".into(), &env!("CARGO_PKG_VERSION").into());
        execute.forget();
        let _ = js_sys::Reflect::set(&window, &"calendarcraft".into(), &api);
    }
}

/// What the app shows, as JSON in `window.calendarcraft.state`, for hosts, agents and tests.
pub fn publish_state(json: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        let Some(window) = web_sys::window() else { return };
        if let Ok(api) = js_sys::Reflect::get(&window, &"calendarcraft".into()) {
            if api.is_object() {
                let _ = js_sys::Reflect::set(&api, &"state".into(), &json.into());
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = json;
}

/// Listen for the host's messages and tell it the app is ready.
pub fn start_bridge(inbox: HostInbox, wake: Arc<dyn Fn() + Send + Sync>) {
    #[cfg(target_arch = "wasm32")]
    {
        bridge::install(inbox, wake);
        if hosted() {
            bridge::post("calendarcraft:ready", &[], None);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (inbox, wake);
    }
}

/// `calendarcraft:save` with the document's bytes.
pub fn post_save(name: &str, bytes: &[u8]) {
    #[cfg(target_arch = "wasm32")]
    {
        let arr = js_sys::Uint8Array::from(bytes);
        let buf = arr.buffer();
        bridge::post("calendarcraft:save", &[("name", name.into()), ("bytes", buf.clone().into())], Some(&buf));
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (name, bytes);
}

pub fn post_dirty(dirty: bool) {
    #[cfg(target_arch = "wasm32")]
    bridge::post("calendarcraft:dirty", &[("dirty", dirty.into())], None);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = dirty;
}

pub fn post_failed(error: &str) {
    #[cfg(target_arch = "wasm32")]
    bridge::post("calendarcraft:failed", &[("error", error.into())], None);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = error;
}
