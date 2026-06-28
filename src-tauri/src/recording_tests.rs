use super::*;

#[test]
fn recording_status_serializes_lowercase() {
    assert_eq!(
        serde_json::to_string(&RecordingStatus::Idle).unwrap(),
        "\"idle\""
    );
    assert_eq!(
        serde_json::to_string(&RecordingStatus::Recording).unwrap(),
        "\"recording\""
    );
    assert_eq!(
        serde_json::to_string(&RecordingStatus::Processing).unwrap(),
        "\"processing\""
    );
}

#[test]
fn recording_status_deserializes_lowercase() {
    assert_eq!(
        serde_json::from_str::<RecordingStatus>("\"idle\"").unwrap(),
        RecordingStatus::Idle
    );
    assert_eq!(
        serde_json::from_str::<RecordingStatus>("\"recording\"").unwrap(),
        RecordingStatus::Recording
    );
    assert_eq!(
        serde_json::from_str::<RecordingStatus>("\"processing\"").unwrap(),
        RecordingStatus::Processing
    );
}

#[test]
fn recording_event_deserializes_from_browser_json() {
    let json_value = serde_json::json!({
        "type": "click",
        "timestamp": 1750000000000u64,
        "url": "https://example.com",
        "selector": "#login-btn",
        "selectorCandidates": ["#login-btn", "button.login"],
        "tagName": "button"
    });
    let event: RecordingEvent = serde_json::from_value(json_value).unwrap();
    assert_eq!(event.event_type, "click");
    assert_eq!(event.timestamp, 1750000000000);
    assert_eq!(event.url, "https://example.com");
    assert_eq!(event.selector, "#login-btn");
    assert_eq!(
        event.selector_candidates,
        vec!["#login-btn", "button.login"]
    );
    assert_eq!(event.tag_name, "button");
    assert!(event.value.is_none());
    assert!(event.screenshot.is_none());
}

#[test]
fn recording_event_with_value_deserializes() {
    let json_value = serde_json::json!({
        "type": "type",
        "timestamp": 1750000001000u64,
        "url": "https://example.com/login",
        "selector": "input[name='email']",
        "selectorCandidates": ["input[name='email']"],
        "tagName": "input",
        "value": "user@example.com"
    });
    let event: RecordingEvent = serde_json::from_value(json_value).unwrap();
    assert_eq!(event.event_type, "type");
    assert_eq!(event.value, Some("user@example.com".to_string()));
}

#[test]
fn recording_event_handles_missing_optional_fields() {
    let json_value = serde_json::json!({
        "type": "navigate",
        "timestamp": 1750000002000u64,
        "url": "https://example.com/dashboard"
    });
    let event: RecordingEvent = serde_json::from_value(json_value).unwrap();
    assert_eq!(event.event_type, "navigate");
    assert_eq!(event.selector, "");
    assert!(event.selector_candidates.is_empty());
    assert_eq!(event.tag_name, "");
}

#[test]
fn trace_file_roundtrip() {
    let trace = TraceFile {
        session_id: "test-session-123".to_string(),
        session_name: "my test recording".to_string(),
        start_url: "https://example.com".to_string(),
        started_at: "2026-06-21T10:00:00+00:00".to_string(),
        stopped_at: "2026-06-21T10:05:00+00:00".to_string(),
        events: vec![
            RecordingEvent {
                event_type: "click".to_string(),
                timestamp: 1750000000000,
                url: "https://example.com".to_string(),
                selector: "#btn".to_string(),
                selector_candidates: vec!["#btn".to_string(), "button.primary".to_string()],
                tag_name: "button".to_string(),
                value: None,
                screenshot: None,
            },
            RecordingEvent {
                event_type: "type".to_string(),
                timestamp: 1750000001000,
                url: "https://example.com".to_string(),
                selector: "#input".to_string(),
                selector_candidates: vec!["#input".to_string()],
                tag_name: "input".to_string(),
                value: Some("hello".to_string()),
                screenshot: None,
            },
        ],
    };

    let json = serde_json::to_string_pretty(&trace).unwrap();
    let deserialized: TraceFile = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.session_id, trace.session_id);
    assert_eq!(deserialized.session_name, trace.session_name);
    assert_eq!(deserialized.start_url, trace.start_url);
    assert_eq!(deserialized.started_at, trace.started_at);
    assert_eq!(deserialized.stopped_at, trace.stopped_at);
    assert_eq!(deserialized.events.len(), 2);
    assert_eq!(deserialized.events[0].event_type, "click");
    assert_eq!(deserialized.events[1].event_type, "type");
    assert_eq!(deserialized.events[1].value, Some("hello".to_string()));
}

#[test]
fn trace_file_uses_camel_case_json_keys() {
    let trace = TraceFile {
        session_id: "abc".to_string(),
        session_name: "test".to_string(),
        start_url: "https://example.com".to_string(),
        started_at: "2026-06-21T10:00:00Z".to_string(),
        stopped_at: "2026-06-21T10:05:00Z".to_string(),
        events: vec![],
    };

    let json_value: serde_json::Value = serde_json::to_value(&trace).unwrap();
    assert!(json_value.get("sessionId").is_some());
    assert!(json_value.get("sessionName").is_some());
    assert!(json_value.get("startUrl").is_some());
    assert!(json_value.get("startedAt").is_some());
    assert!(json_value.get("stoppedAt").is_some());
    // Should NOT have snake_case keys
    assert!(json_value.get("session_id").is_none());
    assert!(json_value.get("session_name").is_none());
}

#[test]
fn recording_event_uses_type_key_not_event_type() {
    let event = RecordingEvent {
        event_type: "click".to_string(),
        timestamp: 1000,
        url: "https://example.com".to_string(),
        selector: "#x".to_string(),
        selector_candidates: vec![],
        tag_name: "div".to_string(),
        value: None,
        screenshot: None,
    };

    let json_value: serde_json::Value = serde_json::to_value(&event).unwrap();
    // The field is renamed to "type" via #[serde(rename = "type")]
    assert!(json_value.get("type").is_some());
    assert!(json_value.get("event_type").is_none());
    assert!(json_value.get("eventType").is_none());
}

#[test]
fn trace_list_entry_uses_camel_case() {
    let entry = TraceListEntry {
        session_id: "abc".to_string(),
        session_name: "test".to_string(),
        start_url: "https://example.com".to_string(),
        started_at: "2026-06-21T10:00:00Z".to_string(),
        event_count: 5,
        path: "/some/path.trace.json".to_string(),
    };

    let json_value: serde_json::Value = serde_json::to_value(&entry).unwrap();
    assert!(json_value.get("sessionId").is_some());
    assert!(json_value.get("eventCount").is_some());
    assert_eq!(json_value["eventCount"], 5);
}

#[tokio::test]
async fn recorder_starts_idle() {
    let recorder = Recorder::new();
    assert_eq!(recorder.get_status().await, RecordingStatus::Idle);
}

#[tokio::test]
async fn start_recording_fails_without_browser() {
    let recorder = Recorder::new();
    let browser = crate::external_browser::ExternalBrowser::new();
    let http = reqwest::Client::new();

    let result = recorder.start_recording("test", &browser, &http).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("not running"),
        "Expected 'not running' error, got: {err}"
    );
}

#[tokio::test]
async fn stop_recording_fails_when_idle() {
    let recorder = Recorder::new();
    let http = reqwest::Client::new();
    let temp_dir = tempfile::TempDir::new().unwrap();

    let result = recorder.stop_recording(&http, temp_dir.path()).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("No recording"),
        "Expected 'No recording' error, got: {err}"
    );
}

#[tokio::test]
async fn list_traces_returns_empty_for_empty_dir() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let entries = Recorder::list_traces(temp_dir.path()).await.unwrap();
    assert!(entries.is_empty());
}

#[tokio::test]
async fn list_traces_reads_valid_trace_files() {
    let temp_dir = tempfile::TempDir::new().unwrap();

    let trace = TraceFile {
        session_id: "test-123".to_string(),
        session_name: "my test".to_string(),
        start_url: "https://example.com".to_string(),
        started_at: "2026-06-21T10:00:00Z".to_string(),
        stopped_at: "2026-06-21T10:05:00Z".to_string(),
        events: vec![RecordingEvent {
            event_type: "click".to_string(),
            timestamp: 1000,
            url: "https://example.com".to_string(),
            selector: "#btn".to_string(),
            selector_candidates: vec!["#btn".to_string()],
            tag_name: "button".to_string(),
            value: None,
            screenshot: None,
        }],
    };

    let path = temp_dir.path().join("test-123.trace.json");
    std::fs::write(&path, serde_json::to_string_pretty(&trace).unwrap()).unwrap();

    // Also write a non-trace JSON file that should be ignored.
    std::fs::write(temp_dir.path().join("other.json"), "{}").unwrap();
    // And a non-JSON file.
    std::fs::write(temp_dir.path().join("readme.txt"), "ignored").unwrap();

    let entries = Recorder::list_traces(temp_dir.path()).await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].session_id, "test-123");
    assert_eq!(entries[0].session_name, "my test");
    assert_eq!(entries[0].start_url, "https://example.com");
    assert_eq!(entries[0].event_count, 1);
}

#[tokio::test]
async fn list_traces_returns_error_for_nonexistent_dir() {
    let result = Recorder::list_traces(std::path::Path::new("/nonexistent/dir/xyz")).await;
    assert!(result.is_err());
}

#[test]
fn recorder_inject_js_is_well_formed() {
    assert!(!RECORDER_INJECT_JS.is_empty());
    assert!(RECORDER_INJECT_JS.contains("__rr_push"));
    assert!(RECORDER_INJECT_JS.contains("__rr_initialized"));
    assert!(RECORDER_INJECT_JS.contains("__rr_isActive"));
    assert!(RECORDER_INJECT_JS.contains("click"));
    assert!(RECORDER_INJECT_JS.contains("input"));
    assert!(RECORDER_INJECT_JS.contains("submit"));
    assert!(RECORDER_INJECT_JS.contains("popstate"));
    assert!(RECORDER_INJECT_JS.contains("hashchange"));
}
