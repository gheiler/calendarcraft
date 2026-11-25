#![allow(clippy::too_many_arguments)]

pub mod caldav;
pub mod host;
pub mod i18n;
pub mod ics;
pub mod mcp;
pub mod model;
pub mod net;
pub mod nlp;
pub mod recur;
pub mod secrets;
pub mod sync;
pub mod theme;
pub mod ui;

pub use model::CalendarStore;
pub use ui::{CalendarCraftApp, Launch};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// WebAssembly entry point. Page parameters: `?host=parent` (framed by a host that speaks the
/// craft-web-host bridge), `view=month|quarter|…`, `theme=light|dark|system`, `date=2026-11-24`,
/// `demo=1` (sample data), `lang=de`, `open=<event id>`, `new=1`, `settings=general|accounts`,
/// `sidebar=0|1`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn wasm_start() -> Result<(), JsValue> {
    use crate::host::query;
    let launch = Launch {
        view: query("view").and_then(|v| serde_json::from_value(serde_json::Value::String(v)).ok()),
        theme: query("theme"),
        date: query("date").and_then(|d| chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok()),
        demo: query("demo").is_some(),
        language: query("lang"),
        open_event: query("open"),
        new_event: query("new").is_some(),
        settings: query("settings").map(|t| u8::from(t == "accounts" || t == "1")),
        sidebar: query("sidebar").map(|v| v != "0"),
        ..Default::default()
    };
    wasm_bindgen_futures::spawn_local(async move {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
        let Some(canvas) = document.get_element_by_id("calendarcraft_canvas").and_then(|el| el.dyn_into::<web_sys::HtmlCanvasElement>().ok()) else { return };
        let result = eframe::WebRunner::new().start(canvas, eframe::WebOptions::default(), Box::new(move |_cc| Ok(Box::new(CalendarCraftApp::new(None, launch))))).await;
        if let Some(loading) = document.get_element_by_id("loading") {
            match result {
                Ok(()) => loading.remove(),
                Err(e) => loading.set_text_content(Some(&format!("CalendarCraft could not start: {e:?}"))),
            }
        }
    });
    Ok(())
}
