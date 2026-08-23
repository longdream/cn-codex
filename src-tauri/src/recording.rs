use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
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

  function quote(value) {
    return String(value).replace(/\\/g, '\\\\').replace(/"/g, '\\"');
  }

  function addUnique(list, value) {
    if (value && list.indexOf(value) < 0) list.push(value);
  }

  function nodeLocators(node) {
    var c = [];
    if (!node || !node.getAttribute) return c;
    if (node.id) addUnique(c, '#' + quote(node.id));
    var al = node.getAttribute('aria-label');
    if (al) addUnique(c, '[aria-label="' + quote(al) + '"]');
    var nm = node.getAttribute('name');
    if (nm) addUnique(c, '[name="' + quote(nm) + '"]');
    var ti = node.getAttribute('data-testid') || node.getAttribute('data-test-id');
    if (ti) addUnique(c, '[data-testid="' + quote(ti) + '"]');
    var ph = node.getAttribute('placeholder');
    if (ph) addUnique(c, '[placeholder="' + quote(ph) + '"]');
    var role = node.getAttribute('role');
    if (role) addUnique(c, '[role="' + quote(role) + '"]');
    if (node.tagName && node.className && typeof node.className === 'string') {
      var classes = node.className.trim().split(/\s+/).slice(0, 2).filter(Boolean);
      if (classes.length) {
        var safeClasses = classes.map(function(cls) {
          return cls.replace(/([^a-zA-Z0-9_-])/g, '\\$1');
        });
        addUnique(c, node.tagName.toLowerCase() + '.' + safeClasses.join('.'));
      }
    }
    var txt = (node.textContent || '').trim().replace(/\s+/g, ' ').slice(0, 50);
    if (txt && node.children && node.children.length === 0) {
      addUnique(c, 'text="' + quote(txt) + '"');
    }
    return c;
  }

  function locators(el) {
    var c = [];
    var originalTag = el && el.tagName ? el.tagName.toLowerCase() : '';
    var node = el;
    var depth = 0;
    // Click targets are often an img/svg/input proxy with no useful
    // attributes. Include actionable ancestors and a descendant fallback.
    while (node && node !== document && depth < 4) {
      var own = nodeLocators(node);
      for (var i = 0; i < own.length; i++) {
        addUnique(c, own[i]);
        if (depth > 0 && originalTag) {
          addUnique(c, own[i] + ' ' + originalTag);
        }
      }
      node = node.parentElement;
      depth += 1;
    }
    return c;
  }

  function rec(type, el, extra) {
    var selectors = locators(el);
    var e = {
      type: type,
      timestamp: Date.now(),
      url: location.href,
      selector: selectors[0] || '',
      selectorCandidates: selectors,
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

  // 键盘：记录回车键（搜索 / 登录提交）
  document.addEventListener('keydown', function(e) {
    var el = e.target;
    if (!el) return;
    if (e.key === 'Enter' || e.key === 'NumpadEnter') {
      rec('key', el, { key: 'Enter', value: el.value || '' });
    }
  }, true);

  // 鼠标悬停：停留 300ms 才记录 hover（过滤快速划过）
  var hoverTimer = null;
  var hoveredSelectors = {};
  document.addEventListener('mouseover', function(e) {
    var el = e.target;
    if (!el) return;
    var sel = locators(el)[0] || '';
    if (!sel || hoveredSelectors[sel]) return;
    if (hoverTimer) clearTimeout(hoverTimer);
    hoverTimer = setTimeout(function() {
      hoveredSelectors[sel] = true;
      rec('hover', el, {});
      hoverTimer = null;
    }, 300);
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
    /// Writer half for sending CDP commands (browser-level connection).
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

    /// Start recording user actions across ALL browser tabs.
    ///
    /// Connects to the browser-level CDP endpoint and enables target
    /// auto-attach (flatten mode), so every page tab — including tabs opened
    /// after recording starts — gets the recorder script injected.
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

        // Browser-level WebSocket URL (from /json/version), not a per-tab URL.
        let ws_url = get_browser_ws_url(http, &cdp_endpoint).await?;
        let start_url = get_active_tab_url(http, &cdp_endpoint)
            .await
            .unwrap_or_default();

        let (mut writer, reader) = CdpConnection::connect(&ws_url).await?;

        let events = Arc::clone(&reader.events);

        // Spawn the background reader FIRST — it routes CDP command responses
        // back to the writer and handles target-attach / binding events.
        let reader_handle = tokio::spawn(cdp_event_reader(reader));

        // Discover + auto-attach to every page target (existing and future),
        // so actions on the second/third tab are recorded too.
        writer
            .command("Target.setDiscoverTargets", json!({ "discover": true }))
            .await?;
        writer
            .command(
                "Target.setAutoAttach",
                json!({
                    "autoAttach": true,
                    "waitForDebuggerOnStart": false,
                    "flatten": true
                }),
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

        // Capture final screenshot while the browser-level connection is live.
        let screenshot_dir = recordings_dir.join("screenshots").join(&session.session_id);
        tokio::fs::create_dir_all(&screenshot_dir).await.ok();

        let last_page = session.writer.last_page_session.lock().await.clone();
        if let Some(page_session) = last_page {
            let _ = capture_and_save_screenshot(
                &mut session.writer,
                &page_session,
                &screenshot_dir,
                "final",
            )
            .await;
        }

        // Abort the background reader — no more CDP commands needed.
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
// CDP connection (browser-level, multi-tab via Target auto-attach flatten mode)
// ---------------------------------------------------------------------------

type WsSink = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    tokio_tungstenite::tungstenite::Message,
>;

type WsStream = futures_util::stream::SplitStream<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
>;

/// Writer half: sends CDP commands and receives their responses via an mpsc
/// channel fed by the background reader task. Commands may be browser-level or
/// session-scoped (flatten mode).
struct CdpWriter {
    sink: Arc<Mutex<WsSink>>,
    next_id: Arc<AtomicI64>,
    /// Channel to receive command responses routed by the reader task.
    response_rx: tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>,
    /// The most recently navigated page session, used for the final screenshot.
    last_page_session: Arc<Mutex<Option<String>>>,
}

/// Reader half: reads all incoming WebSocket messages, routes command responses
/// to the writer and processes CDP events (target attach, binding, navigation).
struct CdpReader {
    sink: Arc<Mutex<WsSink>>,
    next_id: Arc<AtomicI64>,
    stream: WsStream,
    response_tx: tokio::sync::mpsc::UnboundedSender<serde_json::Value>,
    events: Arc<Mutex<Vec<RecordingEvent>>>,
    /// Page sessions already injected with the recorder script.
    injected_sessions: Arc<Mutex<HashSet<String>>>,
    last_page_session: Arc<Mutex<Option<String>>>,
}

struct CdpConnection;

impl CdpConnection {
    async fn connect(ws_url: &str) -> Result<(CdpWriter, CdpReader), String> {
        use futures_util::StreamExt;

        let (ws_stream, _) = tokio_tungstenite::connect_async(ws_url)
            .await
            .map_err(|e| format!("CDP connect failed: {e}"))?;

        let (sink, stream) = ws_stream.split();

        let sink = Arc::new(Mutex::new(sink));
        let next_id = Arc::new(AtomicI64::new(1));
        let (response_tx, response_rx) = tokio::sync::mpsc::unbounded_channel();
        let last_page_session = Arc::new(Mutex::new(None::<String>));

        let writer = CdpWriter {
            sink: sink.clone(),
            next_id: next_id.clone(),
            response_rx,
            last_page_session: last_page_session.clone(),
        };

        let reader = CdpReader {
            sink,
            next_id,
            stream,
            response_tx,
            events: Arc::new(Mutex::new(Vec::new())),
            injected_sessions: Arc::new(Mutex::new(HashSet::new())),
            last_page_session,
        };

        Ok((writer, reader))
    }
}

impl CdpWriter {
    async fn send_command(
        &mut self,
        method: &str,
        params: serde_json::Value,
        session_id: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        use futures_util::SinkExt;
        use tokio_tungstenite::tungstenite::Message;

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let mut payload = json!({ "id": id, "method": method, "params": params });
        if let Some(sid) = session_id {
            payload["sessionId"] = json!(sid);
        }
        let text =
            serde_json::to_string(&payload).map_err(|e| format!("CDP encode failed: {e}"))?;
        self.sink
            .lock()
            .await
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

    async fn command(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.send_command(method, params, None).await
    }

    async fn command_for_session(
        &mut self,
        method: &str,
        params: serde_json::Value,
        session_id: &str,
    ) -> Result<serde_json::Value, String> {
        self.send_command(method, params, Some(session_id)).await
    }
}

/// Send a CDP command without waiting for its response (used by the reader task
/// when injecting the recorder into newly attached page sessions).
async fn fire_and_forget(
    sink: &Arc<Mutex<WsSink>>,
    next_id: &Arc<AtomicI64>,
    method: &str,
    params: serde_json::Value,
    session_id: Option<&str>,
) {
    use futures_util::SinkExt;
    use tokio_tungstenite::tungstenite::Message;

    let id = next_id.fetch_add(1, Ordering::SeqCst);
    let mut payload = json!({ "id": id, "method": method, "params": params });
    if let Some(sid) = session_id {
        payload["sessionId"] = json!(sid);
    }
    let Ok(text) = serde_json::to_string(&payload) else {
        return;
    };
    let mut sink = sink.lock().await;
    let _ = sink.send(Message::Text(text)).await;
}

/// Inject the recorder script into a freshly attached page session.
async fn inject_recorder(reader: &CdpReader, session_id: &str) {
    let _ = fire_and_forget(
        &reader.sink,
        &reader.next_id,
        "Page.enable",
        json!({}),
        Some(session_id),
    )
    .await;
    let _ = fire_and_forget(
        &reader.sink,
        &reader.next_id,
        "Runtime.enable",
        json!({}),
        Some(session_id),
    )
    .await;
    let _ = fire_and_forget(
        &reader.sink,
        &reader.next_id,
        "Runtime.addBinding",
        json!({ "name": "__rr_push" }),
        Some(session_id),
    )
    .await;
    let _ = fire_and_forget(
        &reader.sink,
        &reader.next_id,
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": RECORDER_INJECT_JS }),
        Some(session_id),
    )
    .await;
    let _ = fire_and_forget(
        &reader.sink,
        &reader.next_id,
        "Runtime.evaluate",
        json!({ "expression": RECORDER_INJECT_JS, "returnByValue": false }),
        Some(session_id),
    )
    .await;
}

/// Background task: reads all CDP WebSocket messages, routes command responses
/// to the writer channel, injects the recorder into every page target, and
/// accumulates recording events from `Runtime.bindingCalled` and full-page
/// navigations (`Page.frameNavigated`).
async fn cdp_event_reader(mut reader: CdpReader) {
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

        // If this is a command response (has "id"), route it to the writer.
        if parsed.get("id").is_some() {
            let _ = reader.response_tx.send(parsed);
            continue;
        }

        let method = parsed
            .get("method")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");

        match method {
            "Target.attachedToTarget" => {
                let session_id = parsed
                    .pointer("/params/sessionId")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let target_type = parsed
                    .pointer("/params/targetInfo/type")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");

                if target_type == "page" && !session_id.is_empty() {
                    let mut injected = reader.injected_sessions.lock().await;
                    let is_new = injected.insert(session_id.clone());
                    drop(injected);
                    if is_new {
                        inject_recorder(&reader, &session_id).await;
                    }
                }
            }
            "Runtime.bindingCalled" => {
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
                        let mut guard = reader.events.lock().await;
                        guard.push(event);
                    }
                }
            }
            "Page.frameNavigated" => {
                let url = parsed
                    .pointer("/params/frame/url")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let parent_id = parsed
                    .pointer("/params/frame/parentId")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");

                // Only record top-level (main-frame) navigations to real pages.
                if parent_id.is_empty() && is_http_url(url) {
                    if let Some(sid) = parsed.get("sessionId").and_then(serde_json::Value::as_str) {
                        *reader.last_page_session.lock().await = Some(sid.to_string());
                    }
                    let event = RecordingEvent {
                        event_type: "navigate".to_string(),
                        timestamp: now_millis(),
                        url: url.to_string(),
                        selector: String::new(),
                        selector_candidates: Vec::new(),
                        tag_name: String::new(),
                        value: Some(url.to_string()),
                        screenshot: None,
                    };
                    let mut guard = reader.events.lock().await;
                    guard.push(event);
                }
            }
            _ => {}
        }
    }
}

async fn get_browser_ws_url(http: &reqwest::Client, cdp_endpoint: &str) -> Result<String, String> {
    let response = http
        .get(format!("{cdp_endpoint}/json/version"))
        .send()
        .await
        .map_err(|e| format!("Failed to query CDP version: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "CDP version query failed: HTTP {}",
            response.status().as_u16()
        ));
    }

    #[derive(Deserialize)]
    struct VersionInfo {
        #[serde(default, rename = "webSocketDebuggerUrl")]
        ws_url: String,
    }

    let info: VersionInfo = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse CDP version: {e}"))?;

    if info.ws_url.is_empty() {
        return Err("Browser CDP WebSocket URL not found".to_string());
    }
    Ok(info.ws_url)
}

async fn get_active_tab_url(http: &reqwest::Client, cdp_endpoint: &str) -> Result<String, String> {
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
        #[serde(default)]
        url: String,
    }

    let tabs: Vec<TabInfo> = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse CDP tabs: {e}"))?;

    tabs.iter()
        .find(|t| {
            (t.kind.is_empty() || t.kind == "page")
                && !t.url.is_empty()
                && t.url != "about:blank"
        })
        .or_else(|| tabs.iter().find(|t| t.kind.is_empty() || t.kind == "page"))
        .map(|t| t.url.clone())
        .ok_or_else(|| "No browser tab found".to_string())
}

fn is_http_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

fn now_millis() -> u64 {
    chrono::Utc::now().timestamp_millis() as u64
}

async fn capture_and_save_screenshot(
    cdp: &mut CdpWriter,
    page_session: &str,
    dir: &Path,
    name: &str,
) -> Result<String, String> {
    let result = cdp
        .command_for_session(
            "Page.captureScreenshot",
            json!({ "format": "png", "fromSurface": true }),
            page_session,
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
#[path = "recording_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "recording_integration_tests.rs"]
mod integration_tests;
