use crate::model::{AccountConfig, CalendarEvent, CalendarSource, CalendarStore, TaskItem};
use crate::nlp::{parse_quick_entry, parse_time_token};
use chrono::{Local, NaiveDate};
use serde_json::{json, Value};

/// Clean-room Model Context Protocol (MCP) JSON-RPC 2.0 handler.
/// Operates on a `CalendarStore` (in-memory or persisted to a shared config JSON file).
pub fn handle_mcp_request(store: &mut CalendarStore, req: &Value) -> Option<Value> {
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let id = req.get("id").cloned();

    // Notifications have no `id` and expect no response
    if id.is_none() || method.starts_with("notifications/") {
        return None;
    }
    let req_id = id.unwrap_or(Value::Null);
    let params = req.get("params").cloned().unwrap_or_else(|| json!({}));

    match method {
        "initialize" => Some(json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": { "listChanged": false },
                    "resources": { "subscribe": false, "listChanged": false }
                },
                "serverInfo": {
                    "name": "calendarcraft-mcp",
                    "version": "0.1.0"
                }
            }
        })),
        "ping" => Some(json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {}
        })),
        "resources/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "resources": [
                    {
                        "uri": "calendar://config",
                        "name": "CalendarCraft Active Configuration",
                        "description": "Full preloaded accounts, calendars, sets, events, and tasks",
                        "mimeType": "application/json"
                    }
                ]
            }
        })),
        "resources/read" => {
            let uri = params
                .get("uri")
                .and_then(|u| u.as_str())
                .unwrap_or("calendar://config");
            let text = store.to_json_pretty().unwrap_or_else(|_| "{}".to_string());
            Some(json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "contents": [
                        {
                            "uri": uri,
                            "mimeType": "application/json",
                            "text": text
                        }
                    ]
                }
            }))
        }
        "tools/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "tools": mcp_tools_schema()
            }
        })),
        "tools/call" => {
            let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));

            match call_tool(store, tool_name, &args) {
                Ok(payload) => Some(json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {
                        "content": [
                            {
                                "type": "text",
                                "text": serde_json::to_string_pretty(&payload).unwrap_or_default()
                            }
                        ],
                        "isError": false
                    }
                })),
                Err(err_msg) => Some(json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {
                        "content": [
                            {
                                "type": "text",
                                "text": err_msg
                            }
                        ],
                        "isError": true
                    }
                })),
            }
        }
        _ => Some(json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "error": {
                "code": -32601,
                "message": format!("Method not found: {method}")
            }
        })),
    }
}

pub fn call_tool(store: &mut CalendarStore, name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "list_accounts" => Ok(json!({
            "accounts": store.accounts,
            "calendars": store.calendars,
            "calendar_sets": store.calendar_sets,
            "active_calendar_set": store.settings.active_calendar_set
        })),
        "configure_account" => {
            let acc_name = args
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or("Missing `name`")?;
            let provider = args
                .get("provider")
                .and_then(|v| v.as_str())
                .unwrap_or("CalDAV");
            let email = args
                .get("email")
                .and_then(|v| v.as_str())
                .ok_or("Missing `email`")?;
            let server_url = args
                .get("server_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let cal_name = args
                .get("default_calendar_name")
                .and_then(|v| v.as_str())
                .unwrap_or(acc_name);
            let color_hex = args
                .get("color_hex")
                .and_then(|v| v.as_str())
                .unwrap_or("#06B6D4");

            let acc_id = store.next_id("acc");
            let cal_id = store.next_id("cal");
            let account = AccountConfig {
                id: acc_id.clone(),
                name: acc_name.to_string(),
                provider: provider.to_string(),
                email: email.to_string(),
                server_url,
                enabled: true,
                ..Default::default()
            };
            let calendar = CalendarSource {
                id: cal_id.clone(),
                account_id: acc_id,
                name: cal_name.to_string(),
                color_hex: color_hex.to_string(),
                visible: true,
                read_only: false,
                ..Default::default()
            };
            store.accounts.push(account.clone());
            store.calendars.push(calendar.clone());
            Ok(json!({
                "status": "created",
                "account": account,
                "calendar": calendar
            }))
        }
        "list_events" => {
            let start_filter = args
                .get("start_date")
                .and_then(|s| s.as_str())
                .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
            let end_filter = args
                .get("end_date")
                .and_then(|s| s.as_str())
                .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
            let cal_filter = args.get("calendar_id").and_then(|s| s.as_str());
            let query = args
                .get("query")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_lowercase();

            let events: Vec<&CalendarEvent> = store
                .events
                .iter()
                .filter(|ev| start_filter.is_none_or(|sd| ev.date >= sd))
                .filter(|ev| end_filter.is_none_or(|ed| ev.date <= ed))
                .filter(|ev| cal_filter.is_none_or(|cid| ev.calendar_id == cid))
                .filter(|ev| {
                    query.is_empty()
                        || ev.title.to_lowercase().contains(&query)
                        || ev
                            .location
                            .as_deref()
                            .unwrap_or("")
                            .to_lowercase()
                            .contains(&query)
                        || ev.notes.to_lowercase().contains(&query)
                })
                .collect();

            Ok(json!({
                "count": events.len(),
                "events": events
            }))
        }
        "quick_add_natural" => {
            let sentence = args
                .get("sentence")
                .and_then(|v| v.as_str())
                .ok_or("Missing `sentence`")?;
            let anchor = Local::now().date_naive();
            let default_cal = store.settings.default_calendar_id.clone();
            let parsed = parse_quick_entry(sentence, anchor, &store.calendars, &default_cal)
                .ok_or("Could not parse quick-add sentence")?;

            if parsed.is_task {
                let task = TaskItem {
                    id: store.next_id("task"),
                    calendar_id: parsed.calendar_id.unwrap_or(default_cal),
                    title: parsed.title,
                    due_date: Some(parsed.date),
                    due_time: parsed.start_time,
                    completed: false,
                    priority: 2,
                };
                store.tasks.push(task.clone());
                Ok(json!({ "kind": "task", "created": task }))
            } else {
                let ev = parsed.into_event(store.next_id("ev"), &default_cal);
                store.events.push(ev.clone());
                Ok(json!({ "kind": "event", "created": ev }))
            }
        }
        "create_event" => {
            let title = args
                .get("title")
                .and_then(|v| v.as_str())
                .ok_or("Missing `title`")?;
            let date_str = args
                .get("date")
                .and_then(|v| v.as_str())
                .ok_or("Missing `date` (YYYY-MM-DD)")?;
            let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                .map_err(|e| format!("Invalid date {date_str}: {e}"))?;
            let all_day = args
                .get("all_day")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let start_time = args
                .get("start_time")
                .and_then(|v| v.as_str())
                .and_then(parse_time_token);
            let end_time = args
                .get("end_time")
                .and_then(|v| v.as_str())
                .and_then(parse_time_token);
            let calendar_id = args
                .get("calendar_id")
                .and_then(|v| v.as_str())
                .unwrap_or(&store.settings.default_calendar_id)
                .to_string();
            let location = args
                .get("location")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let notes = args
                .get("notes")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let ev = CalendarEvent {
                id: store.next_id("ev"),
                calendar_id,
                title: title.to_string(),
                date,
                start_time: if all_day { None } else { start_time },
                end_time: if all_day { None } else { end_time },
                all_day,
                location,
                conference_url: args
                    .get("conference_url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                notes,
                attendees: Vec::new(),
                alert_minutes_before: Some(10),
                zone: crate::tz::EventZone::from_settings(&store.settings.timezone),
                ..Default::default()
            };
            store.events.push(ev.clone());
            Ok(json!({ "status": "created", "event": ev }))
        }
        "delete_event" => {
            let event_id = args
                .get("event_id")
                .and_then(|v| v.as_str())
                .ok_or("Missing `event_id`")?;
            let before = store.events.len();
            store.events.retain(|e| e.id != event_id);
            Ok(json!({
                "deleted": before != store.events.len(),
                "event_id": event_id
            }))
        }
        "list_tasks" => Ok(json!({
            "count": store.tasks.len(),
            "tasks": store.tasks
        })),
        "toggle_task" => {
            let task_id = args
                .get("task_id")
                .and_then(|v| v.as_str())
                .ok_or("Missing `task_id`")?;
            let task = store
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id)
                .ok_or_else(|| format!("Task not found: {task_id}"))?;
            task.completed = !task.completed;
            Ok(json!({ "task": task }))
        }
        "set_calendar_set" => {
            let set_id = args
                .get("set_id")
                .and_then(|v| v.as_str())
                .ok_or("Missing `set_id`")?;
            if set_id == crate::model::ALL_SET_ID || set_id == "all" {
                store.settings.active_calendar_set = None;
                return Ok(json!({ "active_calendar_set": crate::model::ALL_SET_ID }));
            }
            if !store.user_sets().any(|s| s.id == set_id) {
                return Err(format!("Calendar set not found: {set_id}"));
            }
            store.settings.active_calendar_set = Some(set_id.to_string());
            Ok(json!({ "active_calendar_set": set_id }))
        }
        "preload_configuration" => {
            let cfg_val = args
                .get("config")
                .ok_or("Missing `config` JSON object")?;
            let mut new_store: CalendarStore = serde_json::from_value(cfg_val.clone())
                .map_err(|e| format!("Invalid configuration schema: {e}"))?;
            new_store.normalize_calendar_sets();
            *store = new_store;
            Ok(json!({
                "status": "reloaded",
                "accounts": store.accounts.len(),
                "calendars": store.calendars.len(),
                "events": store.events.len(),
                "tasks": store.tasks.len()
            }))
        }
        _ => Err(format!("Unknown tool: {name}")),
    }
}

pub fn mcp_tools_schema() -> Value {
    json!([
        {
            "name": "list_accounts",
            "description": "List all preloaded calendar accounts, calendars, and calendar sets.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        },
        {
            "name": "configure_account",
            "description": "Add a new calendar account (Google, iCloud, Exchange, CalDAV) and default calendar.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "provider": { "type": "string" },
                    "email": { "type": "string" },
                    "server_url": { "type": "string" },
                    "default_calendar_name": { "type": "string" },
                    "color_hex": { "type": "string" }
                },
                "required": ["name", "email"]
            }
        },
        {
            "name": "list_events",
            "description": "Query calendar events filtered by date range, calendar_id, or search text.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "start_date": { "type": "string", "description": "YYYY-MM-DD" },
                    "end_date": { "type": "string", "description": "YYYY-MM-DD" },
                    "calendar_id": { "type": "string" },
                    "query": { "type": "string" }
                }
            }
        },
        {
            "name": "quick_add_natural",
            "description": "Create an event or task from natural language (e.g. 'Team Sync tomorrow 2pm-3pm @ Room 4B /Engineering').",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "sentence": { "type": "string" }
                },
                "required": ["sentence"]
            }
        },
        {
            "name": "create_event",
            "description": "Create a structured calendar event.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "date": { "type": "string", "description": "YYYY-MM-DD" },
                    "start_time": { "type": "string", "description": "HH:MM or 2:30pm" },
                    "end_time": { "type": "string", "description": "HH:MM or 3:30pm" },
                    "all_day": { "type": "boolean" },
                    "calendar_id": { "type": "string" },
                    "location": { "type": "string" },
                    "conference_url": { "type": "string" },
                    "notes": { "type": "string" }
                },
                "required": ["title", "date"]
            }
        },
        {
            "name": "delete_event",
            "description": "Delete an event by its event_id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "event_id": { "type": "string" }
                },
                "required": ["event_id"]
            }
        },
        {
            "name": "list_tasks",
            "description": "List all tasks/reminders in CalendarCraft.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        },
        {
            "name": "toggle_task",
            "description": "Toggle completion state of a task by task_id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string" }
                },
                "required": ["task_id"]
            }
        },
        {
            "name": "set_calendar_set",
            "description": "Show only one calendar set (an id from list_accounts' calendar_sets), or set_all for every calendar.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "set_id": { "type": "string" }
                },
                "required": ["set_id"]
            }
        },
        {
            "name": "preload_configuration",
            "description": "Replace the entire CalendarStore with a preloaded configuration JSON object.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "config": { "type": "object" }
                },
                "required": ["config"]
            }
        }
    ])
}
