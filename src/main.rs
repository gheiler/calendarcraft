#[cfg(not(target_arch = "wasm32"))]
mod native_cli {
    use calendarcraft::mcp::handle_mcp_request;
    use calendarcraft::{CalendarCraftApp, CalendarStore};
    use clap::Parser;
    use eframe::egui;
    use std::io::{self, BufRead, Write};

    #[derive(Parser, Debug, Clone)]
    #[command(
        name = "calendarcraft",
        about = "Clean-room Rust calendar app (macOS + WebAssembly + MCP server + preloaded config)"
    )]
    pub struct Args {
        /// Path to preloaded JSON configuration file (accounts, calendars, sets, events, tasks)
        #[arg(short, long)]
        pub config: Option<String>,

        /// Write a sample preloaded configuration file to the given path and exit
        #[arg(long)]
        pub init_config: Option<String>,

        /// Run as a stdio Model Context Protocol (MCP) JSON-RPC server
        #[arg(long)]
        pub mcp: bool,

        /// Start normally, draw ~120 frames without stealing focus, save PNG screenshot and exit
        #[arg(long)]
        pub screenshot: Option<String>,
    }

    pub fn run_stdio_mcp(config_path: Option<&str>) -> io::Result<()> {
        let stdin = io::stdin();
        let mut stdout = io::stdout();
        let (mut store, resolved_path) = CalendarStore::load_from_path_or_default(config_path);

        for line in stdin.lock().lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some( p ) = resolved_path.as_deref() {
                let (reloaded, _) = CalendarStore::load_from_path_or_default(Some(p));
                store = reloaded;
            }
            let resp = match serde_json::from_str::<serde_json::Value>(trimmed) {
                Ok(req) => handle_mcp_request(&mut store, &req),
                Err(err) => Some(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": { "code": -32700, "message": format!("Parse error: {err}") }
                })),
            };
            if let Some( p ) = resolved_path.as_deref() {
                let _ = store.save_to_path(p);
            }
            if let Some(reply) = resp {
                writeln!(stdout, "{}", serde_json::to_string(&reply).unwrap_or_default())?;
                stdout.flush()?;
            }
        }
        Ok(())
    }

    pub fn run() -> eframe::Result {
        let args = Args::parse();

        if let Some(ref out_cfg) = args.init_config {
            let sample = CalendarStore::default();
            if let Err(e) = sample.save_to_path(out_cfg) {
                eprintln!("Failed to write sample config: {e}");
                std::process::exit(1);
            }
            println!("Wrote preloaded sample configuration to {out_cfg}");
            return Ok(());
        }

        if args.mcp {
            if let Err(e) = run_stdio_mcp(args.config.as_deref()) {
                eprintln!("MCP server error: {e}");
                std::process::exit(1);
            }
            return Ok(());
        }

        let (store, resolved_cfg) = CalendarStore::load_from_path_or_default(args.config.as_deref());
        let is_screenshot = args.screenshot.is_some();

        let mut viewport = egui::ViewportBuilder::default()
            .with_title("CalendarCraft")
            .with_inner_size([1320.0, 840.0])
            .with_min_inner_size([900.0, 600.0]);

        if is_screenshot {
            viewport = viewport.with_active(false);
            #[cfg(target_os = "windows")]
            {
                viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnBottom);
            }
        }

        #[allow(unused_mut)]
        let mut options = eframe::NativeOptions {
            viewport,
            ..Default::default()
        };

        #[cfg(target_os = "macos")]
        if is_screenshot {
            use winit::platform::macos::EventLoopBuilderExtMacOS;
            options.event_loop_builder = Some(Box::new(|builder| {
                builder.with_activate_ignoring_other_apps(false);
            }));
        }

        let screenshot_path = args.screenshot.clone();
        eframe::run_native(
            "CalendarCraft",
            options,
            Box::new(move |_cc| {
                Ok(Box::new(CalendarCraftApp::new(
                    store,
                    resolved_cfg,
                    screenshot_path,
                )))
            }),
        )
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    native_cli::run()
}

#[cfg(target_arch = "wasm32")]
fn main() {}
