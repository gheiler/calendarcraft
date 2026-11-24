use calendarcraft::mcp::handle_mcp_request;
use calendarcraft::model::{CalendarStore, ViewMode};
use calendarcraft::nlp::parse_quick_entry;
use chrono::NaiveDate;
use serde_json::json;

#[test]
fn test_natural_language_quick_entry_and_tasks() {
    let anchor = NaiveDate::from_ymd_opt(2026, 11, 24).unwrap();
    let store = CalendarStore::sample_preloaded(anchor);

    let parsed = parse_quick_entry(
        "Design Critique tomorrow at 2:30pm-3:45pm @ Studio Desk /Client",
        anchor,
        &store.calendars,
        "cal_work",
    )
    .expect("should parse quick entry");

    assert_eq!(parsed.title, "Design Critique");
    assert_eq!(parsed.date, NaiveDate::from_ymd_opt(2026, 11, 25).unwrap());
    assert_eq!(parsed.location.as_deref(), Some("Studio Desk"));
    assert_eq!(parsed.calendar_id.as_deref(), Some("cal_studio"));
    assert!(!parsed.is_task);

    let task_parsed = parse_quick_entry(
        "todo Finalize Q4 roadmap friday 4pm /Engineering",
        anchor,
        &store.calendars,
        "cal_work",
    )
    .expect("should parse task");
    assert!(task_parsed.is_task);
    assert_eq!(task_parsed.title, "Finalize Q4 roadmap");
    assert_eq!(task_parsed.calendar_id.as_deref(), Some("cal_work"));
}

#[test]
fn test_mcp_protocol_and_config_preload() {
    let anchor = NaiveDate::from_ymd_opt(2026, 11, 24).unwrap();
    let mut store = CalendarStore::sample_preloaded(anchor);
    assert_eq!(store.settings.default_view, ViewMode::Month);

    // 1. initialize
    let init_resp = handle_mcp_request(
        &mut store,
        &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }),
    )
    .expect("initialize response");
    assert_eq!(init_resp["result"]["serverInfo"]["name"], "calendarcraft-mcp");

    // 2. tools/list
    let list_resp = handle_mcp_request(
        &mut store,
        &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
    )
    .expect("tools/list response");
    assert!(list_resp["result"]["tools"].as_array().unwrap().len() >= 8);

    // 3. configure_account via MCP
    let before_accounts = store.accounts.len();
    let acc_resp = handle_mcp_request(
        &mut store,
        &json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "configure_account",
                "arguments": {
                    "name": "SnowUI Cloud",
                    "provider": "CalDAV",
                    "email": "team@snowui.design",
                    "color_hex": "#9333EA"
                }
            }
        }),
    )
    .expect("configure_account response");
    assert_eq!(acc_resp["result"]["isError"], false);
    assert_eq!(store.accounts.len(), before_accounts + 1);

    // 4. JSON roundtrip preload
    let exported = store.to_json_pretty().expect("serialize config");
    let reloaded = CalendarStore::from_json_str(&exported).expect("deserialize config");
    assert_eq!(reloaded.accounts.len(), store.accounts.len());
}
