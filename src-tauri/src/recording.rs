use std::path::Path;
use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::external_browser::ExternalBrowser;

/// JS script injected into the page to capture user actions.
/// Events are pushed immediately to Rust via the `__rr_push` CDP binding
/// registered through `Runtime.addBinding`. This avoids buffering in JS memory
/// which would be lost on page navigation.
const RECORDER_INJECT_JS: &str = r#"
(function initRecorder() {
  if (window.__rr_initialized) return;
  window.__rr_initialized = true;

  function locators(el) {
    var c = [];
    if (el.id) c.push('#' + el.id);
    var al = el.getAttribute && el.getAttribute('aria-label');
    if (al) c.push('[aria-label="' + al + '"]');
    var nm = el.getAttribute && el.getAttribute('name');
    if (nm) c.push('[name="' + nm + '"]');
    var ti = el.getAttribute && (el.getAttribute('data-testid') || el.getAttribute('data-test-id'));
    if (ti) c.push('[data-testid="' + ti + '"]');
    var ph = el.getAttribute && el.getAttribute('placeholder');
    if (ph) c.push('[placeholder="' + ph + '"]');
    if (el.tagName && el.className && typeof el.className === 'string') {
      var cls = el.className.trim().split(/\s+/).slice(0, 2).join('.');
      if (cls) c.push(el.tagName.toLowerCase() + '.' + cls);
    }
    var txt = (el.textContent || '').trim().slice(0, 50);
    if (txt && el.children && el.children.length === 0) c.push('text="' + txt + '"');
    return c;
  }

  function rec(type, el, extra) {
    var e = {
      type: type,
      timestamp: Date.now(),
      url: location.href,
      selector: locators(el)[0] || '',
      selectorCandidates: locators(el),
      tagName: el.tagName ? el.tagName.toLowerCase() : '',
    };
    if (extra) {
      for (var k in extra) { if (extra.hasOwnProperty(k)) e[k] = extra[k]; }
    }
    try {
      if (window.__rr_push) {
        window.__rr_push(JSON.stringify(e));
      }
    } catch(_) {}
  }

  document.addEventListener('click', function(e) {
    if (e.target) rec('click', e.target);
  }, true);

  var inputTimers = new WeakMap();
  document.addEventListener('input', function(e) {
    var el = e.target;
    if (!el) return;
    if (inputTimers.has(el)) clearTimeout(inputTimers.get(el));
    inputTimers.set(el, setTimeout(function() {
      rec('type', el, { value: el.value || '' });
      inputTimers.delete(el);
    }, 500));
  }, true);

  document.addEventListener('submit', function(e) {
    if (e.target) rec('submit', e.target);
  }, true);

  document.addEventListener('change', function(e) {
    var el = e.target;
    if (el && el.tagName && el.tagName.toLowerCase() === 'select') {
      rec('select', el, { value: el.value || '' });
    }
  }, true);

  var lastUrl = location.href;
  function checkNav() {
    if (location.href !== lastUrl) {
      rec('navigate', document.documentElement, { value: location.href });
      lastUrl = location.href;
    }
  }
  window.addEventListener('popstate', checkNav);
  window.addEventListener('hashchange', checkNav);

  window.__rr_isActive = function() { return true; };
})();
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordingStatus {
    Idle,
    Recording,
    Processing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub timestamp: u64,
    pub url: String,
    #[serde(default)]
    pub selector: String,
    #[serde(default)]
    pub selector_candidates: Vec<String>,
    #[serde(default)]
    pub tag_name: String,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub screenshot: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceFile {
    pub session_id: String,
    pub session_name: String,
    pub start_url: String,
    pub started_at: String,
    pub stopped_at: String,
    pub events: Vec<RecordingEvent>,
}

struct RecordingSession {
    session_id: String,
    session_name: String,
    start_url: String,
    started_at: chrono::DateTime<chrono::Utc>,
    /// Accumulated events pushed from the browser via Runtime.bindingCalled.
    events: Arc<Mutex<Vec<RecordingEvent>>>,
    /// Handle to the background CDP reader task. Aborted on stop.
    reader_handle: tokio::task::JoinHandle<()>,
    /// Writer half for sending CDP commands.
    writer: CdpWriter,
}

pub struct Recorder {
    inner: Arc<Mutex<RecorderState>>,
}

struct RecorderState {
    status: RecordingStatus,
    session: Option<RecordingSession>,
}

impl Recorder {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(RecorderState {
                status: RecordingStatus::Idle,
                session: None,
            })),
        }
    }

    /// Start recording user actions in the external browser.
    /// Registers a CDP binding so events are pushed to Rust in real-time,
    /// surviving cross-page navigations.
    pub async fn start_recording(
        &self,
        session_name: &str,
        external_browser: &ExternalBrowser,
        http: &reqwest::Client,
    ) -> Result<String, String> {
        let mut state = self.inner.lock().await;

        if state.status == RecordingStatus::Recording {
            return Err("Recording already in progress".to_string());
        }

        let cdp_endpoint = external_browser
            .get_cdp_endpoint()
            .await
            .ok_or_else(|| "External browser is not running. Launch it first.".to_string())?;

        let ws_url = get_active_tab_ws_url(http, &cdp_endpoint).await?;

        // Connect and split into writer + reader
        let (writer, reader) = CdpConnection::connect(&ws_url).await?;
        let mut writer = writer;

        let events: Arc<Mutex<Vec<RecordingEvent>>> = Arc::new(Mutex::new(Vec::new()));

        // Spawn the background reader FIRST — it routes CDP command responses
        // back to the writer. Without it, writer.command() would hang.
        let events_clone = Arc::clone(&events);
        let reader_handle = tokio::spawn(cdp_event_reader(reader, events_clone));

        writer.command("Page.enable", json!({})).await?;
        writer.command("Runtime.enable", json!({})).await?;

        // Get current URL
        let start_url = writer
            .evaluate("location.href", true)
            .await?
            .as_str()
            .unwrap_or("about:blank")
            .to_string();

        // Register the binding so injected JS can push events to Rust
        writer
            .command("Runtime.addBinding", json!({ "name": "__rr_push" }))
            .await?;

        // Inject recording script into the current page
        writer.evaluate(RECORDER_INJECT_JS, false).await?;

        // Auto-inject on all future navigations
        writer
            .command(
                "Page.addScriptToEvaluateOnNewDocument",
                json!({ "source": RECORDER_INJECT_JS }),
            )
            .await?;

        let session_id = uuid::Uuid::new_v4().to_string();
        let session_name = if session_name.is_empty() {
            format!("recording-{}", &session_id[..8])
        } else {
            session_name.to_string()
        };

        info!("Recording started: session={session_id}, name={session_name}, url={start_url}");

        state.session = Some(RecordingSession {
            session_id: session_id.clone(),
            session_name,
            start_url,
            started_at: chrono::Utc::now(),
            events,
            reader_handle,
            writer,
        });
        state.status = RecordingStatus::Recording;

        Ok(session_id)
    }

    /// Stop recording, collect accumulated events, save the trace file.
    pub async fn stop_recording(
        &self,
        _http: &reqwest::Client,
        recordings_dir: &Path,
    ) -> Result<TraceFile, String> {
        let mut state = self.inner.lock().await;

        if state.status != RecordingStatus::Recording {
            return Err("No recording in progress".to_string());
        }

        let mut session = state
            .session
            .take()
            .ok_or_else(|| "No recording session found".to_string())?;

        state.status = RecordingStatus::Processing;
        drop(state);

        // Send CDP cleanup commands BEFORE aborting the reader, because
        // the reader task routes command responses back to the writer.
        session
            .writer
            .command("Page.removeAllScriptsToEvaluateOnNewDocument", json!({}))
            .await
            .ok();

        // Capture final screenshot while connection is still live
        let screenshot_dir = recordings_dir.join("screenshots").join(&session.session_id);
        tokio::fs::create_dir_all(&screenshot_dir).await.ok();

        let _final_screenshot_path =
            capture_and_save_screenshot(&mut session.writer, &screenshot_dir, "final")
                .await
                .ok();

        // Now abort the background reader — no more CDP commands needed
        session.reader_handle.abort();
        let _ = session.reader_handle.await;

        let stopped_at = chrono::Utc::now();

        // Collect all accumulated events
        let events = {
            let guard = session.events.lock().await;
            guard.clone()
        };

        let trace = TraceFile {
            session_id: session.session_id.clone(),
            session_name: session.session_name,
            start_url: session.start_url,
            started_at: session.started_at.to_rfc3339(),
            stopped_at: stopped_at.to_rfc3339(),
            events,
        };

        // Save trace JSON
        tokio::fs::create_dir_all(recordings_dir).await.ok();
        let trace_path = recordings_dir.join(format!("{}.trace.json", session.session_id));
        let trace_json = serde_json::to_string_pretty(&trace)
            .map_err(|e| format!("Failed to serialize trace: {e}"))?;
        tokio::fs::write(&trace_path, &trace_json)
            .await
            .map_err(|e| format!("Failed to write trace file: {e}"))?;

        info!(
            "Recording stopped: session={}, events={}, trace={}",
            session.session_id,
            trace.events.len(),
            trace_path.display()
        );

        // Reset state
        let mut state = self.inner.lock().await;
        state.status = RecordingStatus::Idle;

        Ok(trace)
    }

    pub async fn get_status(&self) -> RecordingStatus {
        let state = self.inner.lock().await;
        state.status
    }

    /// List all saved trace files from the recordings directory.
    pub async fn list_traces(recordings_dir: &Path) -> Result<Vec<TraceListEntry>, String> {
        let mut entries = Vec::new();
        let mut dir = tokio::fs::read_dir(recordings_dir)
            .await
            .map_err(|e| format!("Failed to read recordings dir: {e}"))?;

        while let Ok(Some(entry)) = dir.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if !filename.ends_with(".trace.json") {
                continue;
            }
            if let Ok(content) = tokio::fs::read_to_string(&path).await {
                if let Ok(trace) = serde_json::from_str::<TraceFile>(&content) {
                    entries.push(TraceListEntry {
                        session_id: trace.session_id,
                        session_name: trace.session_name,
                        start_url: trace.start_url,
                        started_at: trace.started_at,
                        event_count: trace.events.len(),
                        path: path.to_string_lossy().to_string(),
                    });
                }
            }
        }

        entries.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        Ok(entries)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceListEntry {
    pub session_id: String,
    pub session_name: String,
    pub start_url: String,
    pub started_at: String,
    pub event_count: usize,
    pub path: String,
}

// ---------------------------------------------------------------------------
// CDP connection with split reader/writer
// ---------------------------------------------------------------------------

type WsSink = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    tokio_tungstenite::tungstenite::Message,
>;

type WsStream = futures_util::stream::SplitStream<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
>;

/// Writer half: sends CDP commands and receives their responses via an mpsc channel
/// fed by the background reader task.
struct CdpWriter {
    sink: WsSink,
    next_id: i64,
    /// Channel to receive command responses routed by the reader task.
    response_rx: tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>,
}

/// Reader half: reads all incoming WebSocket messages, routes command responses
/// to the writer and processes CDP events (like Runtime.bindingCalled).
struct CdpReader {
    stream: WsStream,
    response_tx: tokio::sync::mpsc::UnboundedSender<serde_json::Value>,
}

struct CdpConnection;

impl CdpConnection {
    async fn connect(ws_url: &str) -> Result<(CdpWriter, CdpReader), String> {
        use futures_util::StreamExt;

        let (ws_stream, _) = tokio_tungstenite::connect_async(ws_url)
            .await
            .map_err(|e| format!("CDP connect failed: {e}"))?;

        let (sink, stream) = ws_stream.split();

        let (response_tx, response_rx) = tokio::sync::mpsc::unbounded_channel();

        let writer = CdpWriter {
            sink,
            next_id: 1,
            response_rx,
        };

        let reader = CdpReader {
            stream,
            response_tx,
        };

        Ok((writer, reader))
    }
}

impl CdpWriter {
    async fn command(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        use futures_util::SinkExt;
        use tokio_tungstenite::tungstenite::Message;

        let id = self.next_id;
        self.next_id += 1;
        let payload = json!({ "id": id, "method": method, "params": params });
        let text =
            serde_json::to_string(&payload).map_err(|e| format!("CDP encode failed: {e}"))?;
        self.sink
            .send(Message::Text(text))
            .await
            .map_err(|e| format!("CDP send failed: {e}"))?;

        // Wait for the response with the matching id, with a timeout
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            match tokio::time::timeout_at(deadline, self.response_rx.recv()).await {
                Ok(Some(msg)) => {
                    let Some(resp_id) = msg.get("id").and_then(serde_json::Value::as_i64) else {
                        continue;
                    };
                    if resp_id != id {
                        continue;
                    }
                    if let Some(error) = msg.get("error") {
                        let err_msg = error
                            .get("message")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("unknown CDP error");
                        return Err(format!("{method} failed: {err_msg}"));
                    }
                    return Ok(msg
                        .get("result")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null));
                }
                Ok(None) => {
                    return Err("CDP connection closed unexpectedly".to_string());
                }
                Err(_) => {
                    return Err(format!("{method} timed out after 15s"));
                }
            }
        }
    }

    async fn evaluate(
        &mut self,
        expression: &str,
        return_by_value: bool,
    ) -> Result<serde_json::Value, String> {
        let result = self
            .command(
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "returnByValue": return_by_value,
                    "awaitPromise": true,
                    "userGesture": true,
                }),
            )
            .await?;

        if let Some(exception) = result.get("exceptionDetails") {
            let msg = exception
                .get("text")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| exception.to_string());
            return Err(format!("JS evaluation failed: {msg}"));
        }

        let value = result
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        if return_by_value {
            if let Some(v) = value.get("value") {
                return Ok(v.clone());
            }
        }
        Ok(value)
    }
}

/// Background task: reads all CDP WebSocket messages, routes command responses
/// to the writer channel, and accumulates recording events from
/// `Runtime.bindingCalled` notifications.
async fn cdp_event_reader(mut reader: CdpReader, events: Arc<Mutex<Vec<RecordingEvent>>>) {
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::Message;

    while let Some(msg_result) = reader.stream.next().await {
        let msg = match msg_result {
            Ok(m) => m,
            Err(e) => {
                warn!("CDP reader error: {e}");
                break;
            }
        };

        let text = match msg {
            Message::Text(t) => t,
            Message::Binary(b) => match String::from_utf8(b) {
                Ok(s) => s,
                Err(_) => continue,
            },
            Message::Close(_) => break,
            _ => continue,
        };

        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };

        // If this is a command response (has "id"), route it to the writer
        if parsed.get("id").is_some() {
            let _ = reader.response_tx.send(parsed);
            continue;
        }

        // If this is a Runtime.bindingCalled event with our binding name, accumulate
        let method = parsed
            .get("method")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if method == "Runtime.bindingCalled" {
            let binding_name = parsed
                .pointer("/params/name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            if binding_name == "__rr_push" {
                let payload_str = parsed
                    .pointer("/params/payload")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                if let Ok(event) = serde_json::from_str::<RecordingEvent>(payload_str) {
                    let mut guard = events.lock().await;
                    guard.push(event);
                }
            }
        }
    }
}

async fn get_active_tab_ws_url(
    http: &reqwest::Client,
    cdp_endpoint: &str,
) -> Result<String, String> {
    let response = http
        .get(format!("{cdp_endpoint}/json/list"))
        .send()
        .await
        .map_err(|e| format!("Failed to query CDP tabs: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "CDP tabs query failed: HTTP {}",
            response.status().as_u16()
        ));
    }

    #[derive(Deserialize)]
    struct TabInfo {
        #[serde(default, rename = "type")]
        kind: String,
        #[serde(default, rename = "webSocketDebuggerUrl")]
        ws_url: String,
        #[serde(default)]
        url: String,
    }

    let tabs: Vec<TabInfo> = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse CDP tabs: {e}"))?;

    // Prefer non-about:blank page tabs to avoid picking the initial empty tab
    tabs.iter()
        .find(|t| {
            (t.kind.is_empty() || t.kind == "page")
                && !t.ws_url.is_empty()
                && t.url != "about:blank"
        })
        .or_else(|| {
            tabs.iter()
                .find(|t| (t.kind.is_empty() || t.kind == "page") && !t.ws_url.is_empty())
        })
        .map(|t| t.ws_url.clone())
        .ok_or_else(|| "No active browser tab with CDP WebSocket URL found".to_string())
}

#[cfg(test)]
#[path = "recording_tests.rs"]
mod tests;

async fn capture_and_save_screenshot(
    cdp: &mut CdpWriter,
    dir: &Path,
    name: &str,
) -> Result<String, String> {
    let result = cdp
        .command(
            "Page.captureScreenshot",
            json!({ "format": "png", "fromSurface": true }),
        )
        .await?;
    let data = result
        .get("data")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "Screenshot did not return data".to_string())?;
    let bytes = general_purpose::STANDARD
        .decode(data.as_bytes())
        .map_err(|e| format!("Screenshot decode failed: {e}"))?;
    let path = dir.join(format!("{name}.png"));
    tokio::fs::write(&path, bytes)
        .await
        .map_err(|e| format!("Failed to write screenshot: {e}"))?;
    Ok(path.to_string_lossy().to_string())
}

#[cfg(test)]
#[path = "recording_integration_tests.rs"]
mod integration_tests;
