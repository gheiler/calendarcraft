pub mod mcp;
pub mod model;
pub mod nlp;
pub mod ui;

pub use model::CalendarStore;
pub use ui::CalendarCraftApp;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// WebAssembly entrypoint so CalendarCraft runs in the browser and preloads JSON config
/// from `localStorage["calendarcraft_config"]` or built-in preconfigured accounts.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn wasm_start() -> Result<(), JsValue> {
    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async move {
        let store = load_wasm_store();
        let document = web_sys::window()
            .and_then(|w| w.document())
            .expect("No window/document");
        let canvas = document
            .get_element_by_id("calendarcraft_canvas")
            .and_then(|el| el.dyn_into::<web_sys::HtmlCanvasElement>().ok());

        if let Some(canvas) = canvas {
            let _ = eframe::WebRunner::new()
                .start(
                    canvas,
                    web_options,
                    Box::new(|_cc| Ok(Box::new(CalendarCraftApp::new(store, None, None)))),
                )
                .await;
        }
    });
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn load_wasm_store() -> CalendarStore {
    if let Some(win) = web_sys::window() {
        if let Ok(Some(storage)) = win.local_storage() {
            if let Ok(Some(raw)) = storage.get_item("calendarcraft_config") {
                if let Ok(parsed) = CalendarStore::from_json_str(&raw) {
                    return parsed;
                }
            }
        }
    }
    CalendarStore::default()
}
