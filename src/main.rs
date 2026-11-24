#[cfg(not(target_arch = "wasm32"))]
mod native_cli {
    use calendarcraft::mcp::handle_mcp_request;
    use calendarcraft::model::ViewMode;
    use calendarcraft::{CalendarCraftApp, CalendarStore, Launch};
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

        /// Start normally, draw ~90 frames without stealing focus, save PNG screenshot and exit
        #[arg(long)]
        pub screenshot: Option<String>,

        /// Open in this view: day, week, month, quarter or year
        #[arg(long)]
        pub view: Option<String>,

        /// light, dark or system
        #[arg(long)]
        pub theme: Option<String>,

        /// Show this day (YYYY-MM-DD)
        #[arg(long)]
        pub date: Option<String>,

        /// Interface language (en, de, es, fr, it, pt-br or auto)
        #[arg(long)]
        pub lang: Option<String>,

        /// Use the sample calendars instead of what is stored
        #[arg(long)]
        pub demo: bool,

        /// Open the details of this event (for screenshots)
        #[arg(long)]
        pub open_event: Option<String>,

        /// Open the new-event dialog (for screenshots)
        #[arg(long)]
        pub new_event: bool,

        /// Window size in points, e.g. 1320x840 (390x844 shows the phone layout)
        #[arg(long)]
        pub size: Option<String>,
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

        let (store, resolved_cfg) = match args.config.as_deref() {
            Some(path) => {
                let (s, p) = CalendarStore::load_from_path_or_default(Some(path));
                (Some(s), p)
            }
            None => (None, None),
        };
        let is_screenshot = args.screenshot.is_some();
        let size = args
            .size
            .as_deref()
            .and_then(|s| s.split_once('x'))
            .and_then(|(w, h)| Some([w.trim().parse::<f32>().ok()?, h.trim().parse::<f32>().ok()?]))
            .unwrap_or([1320.0, 840.0]);

        let mut viewport = egui::ViewportBuilder::default()
            .with_title("CalendarCraft")
            .with_inner_size(size)
            .with_min_inner_size([360.0, 560.0]);

        if is_screenshot {
            viewport = viewport.with_active(false);
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

        let launch = Launch {
            config_path: resolved_cfg,
            screenshot: args.screenshot.clone(),
            view: args.view.as_deref().and_then(|v| serde_json::from_value::<ViewMode>(serde_json::Value::String(v.to_ascii_lowercase())).ok()),
            theme: args.theme.clone(),
            date: args.date.as_deref().and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()),
            open_event: args.open_event.clone(),
            new_event: args.new_event,
            demo: args.demo,
            language: args.lang.clone(),
        };
        eframe::run_native("CalendarCraft", options, Box::new(move |_cc| Ok(Box::new(CalendarCraftApp::new(store, launch)))))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    native_cli::run()
}

#[cfg(target_arch = "wasm32")]
fn main() {
    let _ = calendarcraft::wasm_start();
}
