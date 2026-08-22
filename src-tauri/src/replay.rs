//! Record & Replay: turn a recorded browser trace into a resilient Playwright
//! Python script, list saved scripts, and run them while capturing structured
//! output for the main pipeline to inspect and repair.

use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::recording::TraceFile;

/// How long a single replay script run is allowed to take before it is killed.
const RUN_TIMEOUT_SECS: u64 = 180;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayScriptMeta {
    pub id: String,
    pub name: String,
    pub path: String,
    pub trace_session_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub step_count: usize,
    pub start_url: String,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub last_run_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayRunResult {
    pub ok: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    /// Human-readable, structured failure summary extracted from the script output.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayReadResult {
    pub id: String,
    pub path: String,
    pub content: String,
}

pub fn scripts_dir(recordings_dir: &Path) -> PathBuf {
    recordings_dir.join("scripts")
}

/// A sidecar JSON file that persists the last run result for a script so the
/// list command can surface status/error after a reload.
fn last_run_path(scripts_dir: &Path, id: &str) -> PathBuf {
    scripts_dir.join(format!("{id}.last.json"))
}

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn is_http_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// Merge raw recording events into resilient replay steps.
fn build_steps(trace: &TraceFile) -> (String, Vec<serde_json::Value>) {
    // Prefer the first real page the user visited when the initial tab is blank.
    let start_url = if is_http_url(&trace.start_url) {
        trace.start_url.clone()
    } else {
        trace
            .events
            .iter()
            .find_map(|e| is_http_url(&e.url).then(|| e.url.clone()))
            .unwrap_or_default()
    };

    let mut steps: Vec<serde_json::Value> = Vec::new();
    for event in &trace.events {
        let kind = match event.event_type.as_str() {
            "click" => "click",
            "type" => "type",
            "select" => "select",
            "submit" => "submit",
            "navigate" => "navigate",
            _ => continue,
        };

        // Intermediate keystrokes are captured as empty "type" events; skip them.
        if kind == "type" {
            let value = event.value.as_deref().unwrap_or("").trim();
            if value.is_empty() {
                continue;
            }
        }

        let mut selectors: Vec<String> = if event.selector_candidates.is_empty() {
            vec![event.selector.clone()]
        } else {
            event.selector_candidates.clone()
        };
        selectors.retain(|s| !s.trim().is_empty());
        if selectors.is_empty() {
            continue;
        }

        // Merge consecutive "type" steps on the same element & page into a single fill
        // (the recorder emits one event per keystroke).
        if kind == "type" {
            if let Some(last) = steps.last_mut() {
                let same_kind = last.get("kind").and_then(|v| v.as_str()) == Some("type");
                let same_selectors = last
                    .get("selectors")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>())
                    == Some(selectors.iter().map(String::as_str).collect::<Vec<_>>());
                let same_url =
                    last.get("expected_url").and_then(|v| v.as_str()) == Some(event.url.as_str());
                if same_kind && same_selectors && same_url {
                    last["value"] = json!(event.value.clone().unwrap_or_default());
                    continue;
                }
            }
        }

        steps.push(json!({
            "kind": kind,
            "selectors": selectors,
            "value": event.value.clone().unwrap_or_default(),
            "expected_url": event.url.clone(),
        }));
    }

    (start_url, steps)
}

/// Generate a self-contained Playwright Python script for the given trace.
pub fn generate_script_content(trace: &TraceFile) -> Result<String, String> {
    let (start_url, steps) = build_steps(trace);

    let payload = json!({
        "start_url": start_url,
        "steps": steps,
        "timeout_ms": 15_000,
        "headless": false,
    });
    let payload_str = serde_json::to_string(&payload)
        .map_err(|e| format!("Failed to serialize replay payload: {e}"))?;
    let payload_b64 = general_purpose::STANDARD.encode(payload_str.as_bytes());

    let script = PYTHON_TEMPLATE.replace("__PAYLOAD_B64__", &payload_b64);
    Ok(script)
}

/// Generate and persist a replay script for a completed recording trace.
pub async fn generate_and_save(
    recordings_dir: &Path,
    trace: &TraceFile,
) -> Result<ReplayScriptMeta, String> {
    let dir = scripts_dir(recordings_dir);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("Failed to create scripts dir: {e}"))?;

    let script = generate_script_content(trace)?;
    let id = trace.session_id.clone();
    let path = dir.join(format!("{id}.py"));
    tokio::fs::write(&path, &script)
        .await
        .map_err(|e| format!("Failed to write replay script: {e}"))?;

    let name = if trace.session_name.trim().is_empty() {
        format!("replay-{}", &id[..id.len().min(8)])
    } else {
        trace.session_name.clone()
    };

    // Preserve last run status across regeneration.
    let mut last_status = None;
    let mut last_error = None;
    let mut last_run_at = None;
    if let Ok(content) = tokio::fs::read_to_string(last_run_path(&dir, &id)).await {
        if let Ok(last) = serde_json::from_str::<serde_json::Value>(&content) {
            last_status = last
                .get("last_status")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            last_error = last
                .get("last_error")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            last_run_at = last
                .get("last_run_at")
                .and_then(serde_json::Value::as_i64);
        }
    }

    let now = now_millis();
    let (start_url, steps) = build_steps(trace);
    let step_count = steps.len();

    Ok(ReplayScriptMeta {
        id,
        name,
        path: path.to_string_lossy().to_string(),
        trace_session_id: trace.session_id.clone(),
        created_at: now,
        updated_at: now,
        step_count,
        start_url,
        last_status,
        last_error,
        last_run_at,
    })
}

/// List all saved replay scripts, newest first.
pub async fn list_scripts(recordings_dir: &Path) -> Result<Vec<ReplayScriptMeta>, String> {
    let dir = scripts_dir(recordings_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut metas = Vec::new();
    let mut entries = tokio::fs::read_dir(&dir)
        .await
        .map_err(|e| format!("Failed to read scripts dir: {e}"))?;

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("py") {
            continue;
        }
        let Some(file_stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let id = file_stem.to_string();
        let mut name = id.clone();
        let mut trace_session_id = id.clone();

        // Prefer the human-friendly session name from the original trace when available.
        if let Ok(trace_content) =
            tokio::fs::read_to_string(recordings_dir.join(format!("{id}.trace.json"))).await
        {
            if let Ok(trace) = serde_json::from_str::<TraceFile>(&trace_content) {
                if !trace.session_name.trim().is_empty() {
                    name = trace.session_name.clone();
                }
                trace_session_id = trace.session_id.clone();
            }
        }

        let (mut last_status, mut last_error, mut last_run_at) = (None, None, None);
        if let Ok(content) = tokio::fs::read_to_string(last_run_path(&dir, &id)).await {
            if let Ok(last) = serde_json::from_str::<serde_json::Value>(&content) {
                last_status = last
                    .get("last_status")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string);
                last_error = last
                    .get("last_error")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string);
                last_run_at = last
                    .get("last_run_at")
                    .and_then(serde_json::Value::as_i64);
            }
        }

        // Best-effort: extract metadata from the generated file header.
        let (mut step_count, mut start_url) = (0usize, String::new());
        if let Ok(content) = tokio::fs::read_to_string(&path).await {
            if let Some(b64) = extract_payload_b64(&content) {
                if let Ok(decoded) = general_purpose::STANDARD.decode(b64.as_bytes()) {
                    if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&String::from_utf8_lossy(&decoded)) {
                        step_count = payload
                            .get("steps")
                            .and_then(serde_json::Value::as_array)
                            .map(|a| a.len())
                            .unwrap_or(0);
                        start_url = payload
                            .get("start_url")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                            .to_string();
                    }
                }
            }
        }

        let modified = tokio::fs::metadata(&path)
            .await
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or_else(now_millis);

        metas.push(ReplayScriptMeta {
            id,
            name,
            path: path.to_string_lossy().to_string(),
            trace_session_id,
            created_at: modified,
            updated_at: modified,
            step_count,
            start_url,
            last_status,
            last_error,
            last_run_at,
        });
    }

    metas.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(metas)
}

fn extract_payload_b64(content: &str) -> Option<&str> {
    let marker = "_PAYLOAD_B64 = \"";
    let start = content.find(marker)? + marker.len();
    let rest = &content[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// Read a script by id, returning its path and content.
pub async fn read_script(
    recordings_dir: &Path,
    id: &str,
) -> Result<ReplayReadResult, String> {
    let dir = scripts_dir(recordings_dir);
    let path = dir.join(format!("{id}.py"));
    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("Failed to read script {id}: {e}"))?;
    Ok(ReplayReadResult {
        id: id.to_string(),
        path: path.to_string_lossy().to_string(),
        content,
    })
}

/// Run a saved replay script via a Python interpreter, capturing stdout/stderr.
pub async fn run_script(
    recordings_dir: &Path,
    id: &str,
) -> Result<ReplayRunResult, String> {
    let dir = scripts_dir(recordings_dir);
    let path = dir.join(format!("{id}.py"));
    if !path.exists() {
        return Err(format!("Replay script not found: {id}"));
    }

    let candidates: &[&str] = if cfg!(windows) {
        &["python", "py", "python3"]
    } else {
        &["python3", "python"]
    };

    let started = std::time::Instant::now();
    let mut last_spawn_error: Option<String> = None;

    for exe in candidates {
        let exe = *exe;
        let script_path = path.clone();
        let fut = async move {
            tokio::process::Command::new(exe)
                .env("PYTHONIOENCODING", "utf-8")
                .env("PYTHONUTF8", "1")
                .arg(&script_path)
                .output()
                .await
        };

        match tokio::time::timeout(Duration::from_secs(RUN_TIMEOUT_SECS), fut).await {
            Err(_) => {
                last_spawn_error = Some(format!(
                    "Replay script timed out after {RUN_TIMEOUT_SECS}s"
                ));
                continue;
            }
            Ok(Err(e)) => {
                last_spawn_error = Some(format!("Failed to launch {exe}: {e}"));
                continue;
            }
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = output.status.code();
                let duration_ms = started.elapsed().as_millis() as u64;

                let parsed = parse_structured_output(&stdout);
                let ok = parsed
                    .as_ref()
                    .and_then(|v| v.get("ok").and_then(serde_json::Value::as_bool))
                    .unwrap_or_else(|| output.status.success());
                let error = extract_error(parsed.as_ref(), &stdout, &stderr, !ok);

                let result = ReplayRunResult {
                    ok,
                    exit_code,
                    stdout,
                    stderr,
                    duration_ms,
                    error,
                };

                persist_last_run(&dir, id, &result).await;
                return Ok(result);
            }
        }
    }

    let error = last_spawn_error.unwrap_or_else(|| "No Python interpreter found".to_string());
    let duration_ms = started.elapsed().as_millis() as u64;
    let result = ReplayRunResult {
        ok: false,
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        duration_ms,
        error: Some(error),
    };
    persist_last_run(&dir, id, &result).await;
    Ok(result)
}

/// Parse the last JSON line emitted by the generated script.
fn parse_structured_output(stdout: &str) -> Option<serde_json::Value> {
    stdout
        .lines()
        .rev()
        .find_map(|line| {
            let trimmed = line.trim();
            if !trimmed.starts_with('{') {
                return None;
            }
            serde_json::from_str::<serde_json::Value>(trimmed).ok()
        })
}

fn extract_error(
    parsed: Option<&serde_json::Value>,
    stdout: &str,
    stderr: &str,
    failed: bool,
) -> Option<String> {
    if let Some(parsed) = parsed {
        if let Some(error) = parsed.get("error").and_then(serde_json::Value::as_str) {
            let mut parts = vec![error.to_string()];
            if let Some(step) = parsed.get("step").and_then(serde_json::Value::as_i64) {
                parts.push(format!("step={step}"));
            }
            if let Some(kind) = parsed.get("kind").and_then(serde_json::Value::as_str) {
                parts.push(format!("kind={kind}"));
            }
            if let Some(url) = parsed.get("url").and_then(serde_json::Value::as_str) {
                if !url.is_empty() {
                    parts.push(format!("url={url}"));
                }
            }
            if let Some(shot) = parsed.get("screenshot").and_then(serde_json::Value::as_str) {
                if !shot.is_empty() {
                    parts.push(format!("screenshot={shot}"));
                }
            }
            return Some(parts.join(" | "));
        }
    }
    if failed {
        let trimmed_err = stderr.trim();
        if !trimmed_err.is_empty() {
            return Some(trimmed_err.chars().take(2000).collect());
        }
        let tail: String = stdout.trim_end().lines().rev().take(3).collect::<Vec<_>>().join("\n");
        if !tail.is_empty() {
            return Some(tail.chars().take(2000).collect());
        }
    }
    None
}

async fn persist_last_run(dir: &Path, id: &str, result: &ReplayRunResult) {
    let payload = json!({
        "last_status": if result.ok { "success" } else { "failed" },
        "last_error": result.error.clone(),
        "last_run_at": now_millis(),
    });
    if let Ok(json) = serde_json::to_string(&payload) {
        let _ = tokio::fs::write(last_run_path(dir, id), json).await;
    }
}

/// The generated Python program. `__PAYLOAD_B64__` is replaced with a base64
/// JSON blob containing start_url / steps / timeouts.
const PYTHON_TEMPLATE: &str = r#"# -*- coding: utf-8 -*-
"""Auto-generated Playwright replay script (CN-Codex Record & Replay).

Resilience built in:
  * every action waits for its selector with multiple candidates and retries;
  * navigation is settled before the next step;
  * on failure a structured JSON error (step / url / title / screenshot) is
    printed and the process exits non-zero so the main pipeline can inspect
    the page and repair the script.
"""
import base64
import json
import os
import sys
import time
import traceback

_PAYLOAD_B64 = "__PAYLOAD_B64__"

try:
    from playwright.sync_api import sync_playwright, TimeoutError as PWTimeoutError
except Exception as _imp_err:  # pragma: no cover
    print(json.dumps({
        "ok": False,
        "stage": "import",
        "error": (
            "playwright import failed: %s. "
            "Install with: pip install playwright && playwright install chromium" % _imp_err
        ),
    }, ensure_ascii=False))
    sys.exit(2)


def _load_payload():
    return json.loads(base64.b64decode(_PAYLOAD_B64).decode("utf-8"))


def _log(level, msg, **extra):
    record = {"level": level, "msg": msg}
    record.update(extra)
    print(json.dumps(record, ensure_ascii=False), flush=True)


def _find(page, selectors, timeout_ms):
    last_error = None
    for sel in selectors:
        if not sel:
            continue
        try:
            loc = page.locator(sel).first
            loc.wait_for(state="attached", timeout=timeout_ms)
            return loc
        except PWTimeoutError as exc:
            last_error = exc
    raise (last_error or RuntimeError("no selector matched: %r" % (selectors,)))


def _settle(page, timeout_ms):
    try:
        page.wait_for_load_state("networkidle", timeout=min(timeout_ms, 5000))
    except Exception:
        pass


def _safe_title(page):
    try:
        return page.title()
    except Exception:
        return ""


def _screenshot(page, screenshot_dir):
    if not screenshot_dir:
        return ""
    try:
        os.makedirs(screenshot_dir, exist_ok=True)
        shot = os.path.join(screenshot_dir, "replay_fail_%d.png" % int(time.time() * 1000))
        page.screenshot(path=shot, full_page=False)
        return shot
    except Exception:
        return ""


def _step_navigate(page, step, timeout_ms):
    target = step.get("url") or step.get("value")
    if not target or target in ("about:blank",):
        return
    page.goto(target, wait_until="domcontentloaded", timeout=timeout_ms)
    _settle(page, timeout_ms)


def _step_click(page, step, timeout_ms):
    loc = _find(page, step.get("selectors") or [], timeout_ms)
    try:
        loc.wait_for(state="visible", timeout=timeout_ms)
    except PWTimeoutError:
        pass
    loc.click(timeout=timeout_ms)


def _step_type(page, step, timeout_ms):
    loc = _find(page, step.get("selectors") or [], timeout_ms)
    try:
        loc.wait_for(state="visible", timeout=timeout_ms)
    except PWTimeoutError:
        pass
    value = step.get("value") or ""
    loc.fill("", timeout=timeout_ms)
    if value:
        loc.fill(value, timeout=timeout_ms)


def _step_select(page, step, timeout_ms):
    loc = _find(page, step.get("selectors") or [], timeout_ms)
    loc.select_option(step.get("value") or "", timeout=timeout_ms)


def _step_submit(page, step, timeout_ms):
    loc = _find(page, step.get("selectors") or [], timeout_ms)
    loc.press("Enter", timeout=timeout_ms)


_RUNNERS = {
    "navigate": _step_navigate,
    "click": _step_click,
    "type": _step_type,
    "select": _step_select,
    "submit": _step_submit,
}


def run(payload):
    headless = bool(payload.get("headless", False))
    timeout_ms = int(payload.get("timeout_ms", 15000))
    steps = payload.get("steps") or []
    start_url = payload.get("start_url") or ""
    screenshot_dir = payload.get("screenshot_dir") or ""

    with sync_playwright() as p:
        browser = p.chromium.launch(headless=headless)
        try:
            context = browser.new_context(viewport={"width": 1440, "height": 900})
            page = context.new_page()
            if start_url and start_url not in ("about:blank",):
                _step_navigate(page, {"url": start_url}, timeout_ms)

            for idx, step in enumerate(steps, start=1):
                kind = step.get("kind")
                runner = _RUNNERS.get(kind)
                if runner is None:
                    _log("warn", "unknown_step_kind", step=idx, kind=kind)
                    continue

                ok = False
                last_err = None
                for attempt in range(1, 4):
                    try:
                        runner(page, step, timeout_ms)
                        ok = True
                        break
                    except PWTimeoutError as exc:
                        last_err = exc
                        time.sleep(0.6)
                    except Exception as exc:
                        last_err = exc
                        break

                if not ok:
                    shot = _screenshot(page, screenshot_dir)
                    return {
                        "ok": False,
                        "step": idx,
                        "kind": kind,
                        "error": str(last_err),
                        "url": page.url,
                        "title": _safe_title(page),
                        "screenshot": shot,
                        "selectors": step.get("selectors"),
                    }

                expected_url = step.get("expected_url")
                if expected_url and expected_url.startswith(("http://", "https://")):
                    try:
                        page.wait_for_url(expected_url, timeout=min(timeout_ms, 8000))
                    except PWTimeoutError:
                        _log("warn", "expected_url_not_reached", step=idx,
                             expected=expected_url, actual=page.url)
                _settle(page, timeout_ms)
                _log("info", "step_ok", step=idx, kind=kind, url=page.url)

            return {"ok": True, "steps": len(steps), "url": page.url}
        finally:
            browser.close()


if __name__ == "__main__":
    payload = _load_payload()
    started = time.time()
    try:
        result = run(payload)
    except Exception as exc:
        result = {"ok": False, "error": str(exc), "traceback": traceback.format_exc()}
    result["ok"] = bool(result.get("ok"))
    result["duration_ms"] = int((time.time() - started) * 1000)
    _log("info", "replay_finished", **result)
    sys.exit(0 if result["ok"] else 1)
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::RecordingEvent;

    fn event(event_type: &str, url: &str, selector: &str, value: Option<&str>) -> RecordingEvent {
        RecordingEvent {
            event_type: event_type.to_string(),
            timestamp: 0,
            url: url.to_string(),
            selector: selector.to_string(),
            selector_candidates: if selector.is_empty() {
                vec![]
            } else {
                vec![selector.to_string()]
            },
            tag_name: String::new(),
            value: value.map(str::to_string),
            screenshot: None,
        }
    }

    #[test]
    fn generates_valid_script_and_merges_type_events() {
        let trace = TraceFile {
            session_id: "sess-1".to_string(),
            session_name: "demo".to_string(),
            start_url: "about:blank".to_string(),
            started_at: String::new(),
            stopped_at: String::new(),
            events: vec![
                event("type", "https://example.com/", "#q", Some("hello")),
                event("type", "https://example.com/", "#q", Some("hello world")),
                event("click", "https://example.com/", "#submit", None),
            ],
        };

        let script = generate_script_content(&trace).expect("generate script");
        assert!(script.contains("sync_playwright"));
        assert!(!script.contains("__PAYLOAD_B64__"));

        let b64 = extract_payload_b64(&script).expect("payload marker");
        let decoded = general_purpose::STANDARD.decode(b64).expect("decode");
        let payload: serde_json::Value = serde_json::from_slice(&decoded).expect("parse payload");

        let steps = payload["steps"].as_array().expect("steps");
        // Two consecutive type events on the same selector collapse into one.
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0]["kind"], "type");
        assert_eq!(steps[0]["value"], "hello world");
        assert_eq!(steps[1]["kind"], "click");
        assert_eq!(payload["start_url"], "https://example.com/");
    }
}
