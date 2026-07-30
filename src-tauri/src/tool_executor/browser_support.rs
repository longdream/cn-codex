use super::*;

pub(crate) fn apply_browser_defaults(_workspace_config_dir: &Path, payload: &mut serde_json::Value) {
    if let Some(object) = payload.as_object_mut() {
        // 兼容历史 prompt：保留传入 engine，但统一映射到新引擎。
        let requested_engine = object
            .get("engine")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("webview-js-injection")
            .to_string();
        if requested_engine != "webview-js-injection" {
            object.insert(
                "engineRequested".to_string(),
                serde_json::Value::String(requested_engine),
            );
        }
        object.insert(
            "engine".to_string(),
            serde_json::Value::String("webview-js-injection".to_string()),
        );
        object
            .entry("use_visible_browser".to_string())
            .or_insert_with(|| serde_json::Value::Bool(true));
    }
}


pub(crate) fn browser_run_display(payload: &serde_json::Value) -> String {
    let url = payload
        .get("url")
        .and_then(serde_json::Value::as_str)
        .filter(|url| !url.trim().is_empty())
        .unwrap_or("browser");
    let action_count = payload
        .get("actions")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if action_count == 0 {
        url.to_string()
    } else {
        format!("{url} ({action_count} actions)")
    }
}


#[allow(dead_code)]
pub(crate) fn browser_run_use_visible_browser(payload: &serde_json::Value) -> bool {
    payload
        .get("use_visible_browser")
        .or_else(|| payload.get("useVisibleBrowser"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true)
}


#[allow(dead_code)]
pub(crate) fn browser_run_initial_url(payload: &serde_json::Value) -> Option<String> {
    if let Some(url) = payload
        .get("url")
        .and_then(serde_json::Value::as_str)
        .filter(|url| !url.trim().is_empty())
    {
        return Some(url.trim().to_string());
    }

    payload
        .get("actions")
        .and_then(serde_json::Value::as_array)
        .and_then(|actions| {
            actions.iter().find_map(|action| {
                let action_type = action.get("type")?.as_str()?;
                if action_type != "goto" {
                    return None;
                }
                action
                    .get("url")
                    .and_then(serde_json::Value::as_str)
                    .filter(|url| !url.trim().is_empty())
                    .map(|url| url.trim().to_string())
            })
        })
}


pub(crate) fn browser_run_failure_json(error_code: &str, message: &str, hint: Option<&str>) -> String {
    let mapped = serde_json::json!({
        "ok": false,
        "errorCode": error_code,
        "message": message,
        "hint": hint.unwrap_or("请检查页面是否可访问、CDP 端口是否可用，并重试。"),
        "browserMode": "unavailable",
        "actions": [],
        "screenshots": [],
        "assetBundles": [],
        "tabs": []
    });
    serde_json::to_string_pretty(&mapped).unwrap_or_else(|_| mapped.to_string())
}


/// 将 browser_run 运行期错误归类为更精确的错误码，避免把所有失败都显示为 CDP 失效。
pub(crate) fn classify_browser_run_error(error: &str) -> (&'static str, &'static str) {
    let lower = error.to_ascii_lowercase();
    if lower.contains("webview_cdp_unavailable")
        || lower.contains("cdp connect failed")
        || lower.contains("failed to query cdp tabs")
        || lower.contains("cdp websocket closed")
    {
        return (
            "WEBVIEW_CDP_UNAVAILABLE",
            "未建立到内置浏览器的 CDP 通道。请先打开内置浏览器面板；若仍失败，关闭浏览器面板后重开，或重启应用后再试。",
        );
    }
    if lower.contains("timeout waiting for selector:") {
        return (
            "SELECTOR_TIMEOUT",
            "页面已打开，但在超时时间内未找到目标选择器。请核对 selector 是否与当前 DOM 匹配。",
        );
    }
    if lower.contains("browser run interrupted by user") {
        return (
            "BROWSER_RUN_INTERRUPTED",
            "本次浏览器任务已被中断，未继续执行剩余动作。",
        );
    }
    (
        "JS_INJECTION_FAILED",
        "请检查页面是否可访问、动作参数是否正确，并重试。",
    )
}


impl ToolExecutor {
    pub(crate) async fn exec_browser_run(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let mut payload: serde_json::Value = match serde_json::from_str(arguments) {
            Ok(value) => value,
            Err(e) => {
                let msg = format!("Invalid browser_run args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "browser_run", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "browser_run", -1, &msg);
                return Ok(msg);
            }
        };

        if !payload.is_object() {
            let msg = "Invalid browser_run args: expected a JSON object".to_string();
            self.emit_tool_start(app_handle, thread_id, call_id, "browser_run", "invalid");
            self.emit_tool_end(app_handle, thread_id, call_id, "browser_run", -1, &msg);
            return Ok(msg);
        }

        apply_browser_defaults(&self.workspace_config_dir, &mut payload);

        let display = browser_run_display(&payload);
        self.emit_tool_start(app_handle, thread_id, call_id, "browser_run", &display);

        let timeout_ms = crate::browser_automation::browser_run_timeout_ms(&payload);
        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.register_active_browser_cancellation(thread_id, call_id, cancel_flag.clone())
            .await;

        // Try external browser first if available, fall back to embedded WebView.
        let external_endpoint = {
            let state = app_handle.state::<crate::state::AppState>();
            state.external_browser.get_cdp_endpoint().await
        };

        let outcome = if let Some(ref ext_endpoint) = external_endpoint {
            tokio::time::timeout(
                Duration::from_millis(timeout_ms),
                crate::browser_automation::run_external_browser(
                    ext_endpoint,
                    &self.workspace_config_dir,
                    &self.cwd,
                    self.http.clone(),
                    &payload,
                    cancel_flag.clone(),
                ),
            )
            .await
        } else {
            tokio::time::timeout(
                Duration::from_millis(timeout_ms),
                crate::browser_automation::run_webview_js_injection(
                    app_handle,
                    &self.workspace_config_dir,
                    &self.cwd,
                    self.http.clone(),
                    &payload,
                    cancel_flag.clone(),
                ),
            )
            .await
        };

        self.unregister_active_browser_cancellation(thread_id, call_id)
            .await;

        let (exit_code, output) = match outcome {
            Ok(Ok(result)) => {
                let output = serde_json::to_string_pretty(&result).unwrap_or_else(|_| {
                    serde_json::json!({
                        "ok": true,
                        "browserMode": "tauri-webview-js-injection",
                        "actions": [],
                        "screenshots": [],
                        "assetBundles": [],
                        "tabs": []
                    })
                    .to_string()
                });
                (0, output)
            }
            Ok(Err(error)) => {
                let (error_code, hint) = classify_browser_run_error(&error);
                (-1, browser_run_failure_json(error_code, &error, Some(hint)))
            }
            Err(_) => (
                124,
                browser_run_failure_json(
                    "WEBVIEW_RUN_TIMEOUT",
                    &format!("WebView browser run timed out after {timeout_ms} ms"),
                    Some("请缩小动作批次、减少 wait_for_timeout，或检查页面是否卡死。"),
                ),
            ),
        };

        let truncated = truncate_output(&output, TOOL_OUTPUT_BROWSER_MAX_CHARS);
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "browser_run",
            exit_code,
            &truncated,
        );
        Ok(truncated)
    }


    pub(crate) async fn exec_recording_control(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(serde::Deserialize)]
        struct Args {
            action: String,
            #[serde(default)]
            session_id: Option<String>,
        }

        let args: Args = match serde_json::from_str(arguments) {
            Ok(a) => a,
            Err(e) => {
                let msg = format!("Invalid recording_control args: {e}");
                self.emit_tool_start(
                    app_handle,
                    thread_id,
                    call_id,
                    "recording_control",
                    "invalid",
                );
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "recording_control",
                    -1,
                    &msg,
                );
                return Ok(msg);
            }
        };

        self.emit_tool_start(
            app_handle,
            thread_id,
            call_id,
            "recording_control",
            &args.action,
        );

        let state = app_handle.state::<crate::state::AppState>();
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default();

        let result = match args.action.as_str() {
            "launch_browser" => {
                match state.external_browser.launch(&http, None, None).await {
                    Ok(endpoint) => {
                        // Also show the recording toggle
                        app_handle.emit("recording-toggle-visibility", true).ok();
                        serde_json::json!({
                            "ok": true,
                            "cdpEndpoint": endpoint,
                            "message": "External Chrome launched. The recording toggle is now visible. Tell the user to click 'Start Recording', perform their actions in Chrome, then click 'Stop Recording'."
                        })
                        .to_string()
                    }
                    Err(e) => serde_json::json!({ "ok": false, "error": e }).to_string(),
                }
            }
            "show_toggle" => {
                app_handle.emit("recording-toggle-visibility", true).ok();
                serde_json::json!({
                    "ok": true,
                    "message": "Recording toggle is now visible in the UI."
                })
                .to_string()
            }
            "hide_toggle" => {
                app_handle.emit("recording-toggle-visibility", false).ok();
                serde_json::json!({
                    "ok": true,
                    "message": "Recording toggle hidden."
                })
                .to_string()
            }
            "status" => {
                let recording_status = state.recorder.get_status().await;
                let browser_running = state.external_browser.is_running(&http).await;
                serde_json::json!({
                    "ok": true,
                    "recordingStatus": recording_status,
                    "browserRunning": browser_running,
                })
                .to_string()
            }
            "read_trace" => {
                let session_id = match &args.session_id {
                    Some(id) => id.clone(),
                    None => {
                        let msg = "session_id is required for read_trace action";
                        self.emit_tool_end(
                            app_handle,
                            thread_id,
                            call_id,
                            "recording_control",
                            -1,
                            msg,
                        );
                        return Ok(msg.to_string());
                    }
                };
                let trace_path = self
                    .workspace_config_dir
                    .join("recordings")
                    .join(format!("{session_id}.trace.json"));
                match tokio::fs::read_to_string(&trace_path).await {
                    Ok(content) => content,
                    Err(e) => serde_json::json!({
                        "ok": false,
                        "error": format!("Failed to read trace: {e}")
                    })
                    .to_string(),
                }
            }
            "list_traces" => {
                let recordings_dir = self.workspace_config_dir.join("recordings");
                match crate::recording::Recorder::list_traces(&recordings_dir).await {
                    Ok(traces) => serde_json::json!({ "ok": true, "traces": traces }).to_string(),
                    Err(e) => serde_json::json!({ "ok": false, "error": e }).to_string(),
                }
            }
            other => serde_json::json!({
                "ok": false,
                "error": format!("Unknown recording_control action: {other}")
            })
            .to_string(),
        };

        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "recording_control",
            0,
            &result,
        );
        Ok(result)
    }

}
