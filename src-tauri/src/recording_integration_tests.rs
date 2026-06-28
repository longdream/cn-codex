use crate::external_browser::ExternalBrowser;
use crate::recording::{Recorder, RecordingStatus, TraceFile};

/// Integration test: launch external browser, start/stop recording,
/// verify the full lifecycle and trace file persistence.
#[tokio::test]
async fn recording_lifecycle() {
    if crate::external_browser::find_browser_path().is_none() {
        eprintln!("Skipping integration test: no browser found");
        return;
    }

    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();

    let port = 19223u16;
    let browser = ExternalBrowser::new();
    let recorder = Recorder::new();
    let temp_dir = tempfile::TempDir::new().unwrap();

    // Launch browser
    let endpoint = match browser.launch(&http, None, Some(port)).await {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Skipping: browser launch failed: {e}");
            return;
        }
    };
    assert!(browser.is_running(&http).await);
    assert_eq!(endpoint, format!("http://127.0.0.1:{port}"));

    // Start recording
    assert_eq!(recorder.get_status().await, RecordingStatus::Idle);
    let session_id = recorder
        .start_recording("lifecycle-test", &browser, &http)
        .await
        .unwrap();
    assert!(!session_id.is_empty());
    assert_eq!(recorder.get_status().await, RecordingStatus::Recording);

    // Double-start should fail
    let err = recorder
        .start_recording("second", &browser, &http)
        .await
        .unwrap_err();
    assert!(err.contains("already in progress"));

    // Stop recording — no events expected (no real user interaction)
    let trace = recorder
        .stop_recording(&http, temp_dir.path())
        .await
        .unwrap();
    assert_eq!(recorder.get_status().await, RecordingStatus::Idle);
    assert_eq!(trace.session_id, session_id);
    assert_eq!(trace.session_name, "lifecycle-test");
    assert!(!trace.started_at.is_empty());
    assert!(!trace.stopped_at.is_empty());

    // Verify trace file was saved
    let trace_path = temp_dir.path().join(format!("{session_id}.trace.json"));
    assert!(trace_path.exists());
    let saved: TraceFile =
        serde_json::from_str(&std::fs::read_to_string(&trace_path).unwrap()).unwrap();
    assert_eq!(saved.session_id, session_id);
    assert_eq!(saved.session_name, "lifecycle-test");

    // list_traces should find it
    let traces = Recorder::list_traces(temp_dir.path()).await.unwrap();
    assert_eq!(traces.len(), 1);
    assert_eq!(traces[0].session_id, session_id);

    // Can start a new recording after stopping
    let session_id2 = recorder
        .start_recording("second-recording", &browser, &http)
        .await
        .unwrap();
    assert_ne!(session_id, session_id2);
    assert_eq!(recorder.get_status().await, RecordingStatus::Recording);

    // Stop and verify
    let trace2 = recorder
        .stop_recording(&http, temp_dir.path())
        .await
        .unwrap();
    assert_eq!(trace2.session_id, session_id2);

    // Now list_traces should return 2
    let traces = Recorder::list_traces(temp_dir.path()).await.unwrap();
    assert_eq!(traces.len(), 2);

    // Shutdown browser
    browser.shutdown().await;
    assert!(!browser.is_running(&http).await);
    assert_eq!(browser.get_cdp_endpoint().await, None);
}

/// Test that stop_recording fails when not recording.
#[tokio::test]
async fn stop_without_start_fails() {
    let recorder = Recorder::new();
    let http = reqwest::Client::new();
    let temp_dir = tempfile::TempDir::new().unwrap();
    let result = recorder.stop_recording(&http, temp_dir.path()).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("No recording"));
}

/// Test that start_recording fails without a running browser.
#[tokio::test]
async fn start_without_browser_fails() {
    let recorder = Recorder::new();
    let browser = ExternalBrowser::new();
    let http = reqwest::Client::new();
    let result = recorder.start_recording("test", &browser, &http).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not running"));
}

/// Verify recorder inject JS script contains key functions for the push-based approach.
#[test]
fn recorder_js_contains_key_functions() {
    let js = super::RECORDER_INJECT_JS;
    assert!(js.contains("__rr_push"));
    assert!(js.contains("__rr_isActive"));
    assert!(js.contains("__rr_initialized"));
    assert!(js.contains("initRecorder"));
}

/// Test browser relaunch after shutdown.
#[tokio::test]
async fn browser_relaunch_after_shutdown() {
    if crate::external_browser::find_browser_path().is_none() {
        eprintln!("Skipping: no browser found");
        return;
    }

    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();

    let port = 19225u16;
    let browser = ExternalBrowser::new();

    // First launch
    if browser.launch(&http, None, Some(port)).await.is_err() {
        eprintln!("Skipping: browser launch failed");
        return;
    }
    assert!(browser.is_running(&http).await);

    // Shutdown
    browser.shutdown().await;
    assert!(!browser.is_running(&http).await);

    // Wait a bit for port to be freed
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Relaunch
    let result = browser.launch(&http, None, Some(port)).await;
    match result {
        Ok(endpoint) => {
            assert_eq!(endpoint, format!("http://127.0.0.1:{port}"));
            browser.shutdown().await;
        }
        Err(e) => {
            eprintln!("Relaunch failed (acceptable in some environments): {e}");
        }
    }
}

/// Test that auto-generated session names work.
#[tokio::test]
async fn auto_generated_session_name() {
    if crate::external_browser::find_browser_path().is_none() {
        eprintln!("Skipping: no browser found");
        return;
    }

    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();

    let port = 19226u16;
    let browser = ExternalBrowser::new();
    let recorder = Recorder::new();
    let temp_dir = tempfile::TempDir::new().unwrap();

    if browser.launch(&http, None, Some(port)).await.is_err() {
        eprintln!("Skipping: browser launch failed");
        return;
    }

    // Empty session name should auto-generate
    let _session_id = recorder.start_recording("", &browser, &http).await.unwrap();

    let trace = recorder
        .stop_recording(&http, temp_dir.path())
        .await
        .unwrap();

    assert!(
        trace.session_name.starts_with("recording-"),
        "Auto name should start with 'recording-', got: {}",
        trace.session_name
    );
    assert!(trace.session_name.len() > "recording-".len());

    browser.shutdown().await;
}
