#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut cfg_path: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        if (args[i] == "--config" || args[i] == "-c") && i + 1 < args.len() {
            cfg_path = Some(args[i + 1].clone());
            i += 2;
        } else {
            i += 1;
        }
    }

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let (mut store, resolved_path) =
        calendarcraft::CalendarStore::load_from_path_or_default(cfg_path.as_deref());

    use std::io::{BufRead, Write};
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(p) = resolved_path.as_deref() {
            let (reloaded, _) = calendarcraft::CalendarStore::load_from_path_or_default(Some(p));
            store = reloaded;
        }
        let resp = match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(req) => calendarcraft::mcp::handle_mcp_request(&mut store, &req),
            Err(err) => Some(serde_json::json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": { "code": -32700, "message": format!("Parse error: {err}") }
            })),
        };
        if let Some(p) = resolved_path.as_deref() {
            let _ = store.save_to_path(p);
        }
        if let Some(reply) = resp {
            let _ = writeln!(stdout, "{}", serde_json::to_string(&reply).unwrap_or_default());
            let _ = stdout.flush();
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {}
