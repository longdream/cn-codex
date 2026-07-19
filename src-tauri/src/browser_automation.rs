use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::{Engine as _, engine::general_purpose};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio::fs;
use tokio::net::TcpStream;
use tokio::time::sleep;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

use crate::commands::window::{browser_cdp_endpoint_candidates, open_browser_embedded};

const DEFAULT_ACTION_TIMEOUT_MS: u64 = 10_000;
const DEFAULT_RUN_TIMEOUT_MS: u64 = 60_000;
const DEFAULT_NAV_TIMEOUT_MS: u64 = 30_000;
const MAX_WAIT_TIMEOUT_MS: u64 = 120_000;

type WsClientStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// browser_run 的统一输出结构。
///
/// 这里尽量保持与历史 JS runner 输出字段一致，避免前端与解析器回归。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserRunOutput {
    pub ok: bool,
    pub browser_mode: String,
    pub cdp_endpoint: String,
    pub final_url: String,
    pub title: String,
    pub tabs: Vec<BrowserTabSnapshot>,
    pub duration_ms: u128,
    pub actions: Vec<serde_json::Value>,
    pub screenshots: Vec<String>,
    pub asset_bundles: Vec<String>,
    pub console: Vec<serde_json::Value>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserTabSnapshot {
    pub index: usize,
    pub url: String,
    pub title: String,
    pub active: bool,
    pub closed: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteTabInfo {
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default, rename = "webSocketDebuggerUrl")]
    web_socket_debugger_url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct PageAssets {
    #[serde(default)]
    images: Vec<serde_json::Value>,
    #[serde(default)]
    stylesheets: Vec<serde_json::Value>,
    #[serde(default)]
    scripts: Vec<serde_json::Value>,
    #[serde(default)]
    links: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AssetCandidate {
    kind: String,
    url: Option<String>,
    metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BundledAssetFile {
    kind: String,
    url: Option<String>,
    metadata: serde_json::Value,
    path: Option<String>,
    downloaded: bool,
    bytes: Option<usize>,
    mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BundledAssetSkipped {
    kind: String,
    url: Option<String>,
    metadata: serde_json::Value,
    reason: String,
}

/// 入口：执行基于 Tauri WebView + JS Injection 的 browser_run。
///
/// 注意：
/// - 所有交互动作统一走 CDP `Runtime.evaluate` 注入脚本；
/// - 保留与历史 runner 相同的输出字段；
/// - 通过 `cancel_flag` 支持用户“停止”。
pub async fn run_webview_js_injection(
    app_handle: &AppHandle,
    workspace_config_dir: &Path,
    cwd: &Path,
    http: reqwest::Client,
    payload: &serde_json::Value,
    cancel_flag: Arc<AtomicBool>,
) -> Result<BrowserRunOutput, String> {
    let started_at = Instant::now();
    let initial_url = browser_run_initial_url(payload);
    let browser_info = open_browser_embedded(
        app_handle,
        workspace_config_dir,
        initial_url.as_deref(),
        -9999.0,
        -9999.0,
        800.0,
        600.0,
    )
    .map_err(|e| format!("Failed to open browser: {e}"))?;
    let cdp_endpoint = ensure_cdp_ready(
        &http,
        &browser_info.cdp_endpoint,
        Duration::from_secs(12),
        &cancel_flag,
    )
    .await?;
    app_handle.emit("browser-webview-ready", ()).ok();
    let mut session =
        BrowserSession::connect(http.clone(), cdp_endpoint.clone(), cancel_flag.clone()).await?;

    // 兼容旧 runner：默认截图和资源目录都在 codey/browser 下。
    let browser_dir = workspace_config_dir.join("browser");
    let screenshot_dir = path_from_payload_or_default(
        payload,
        "defaultScreenshotDir",
        browser_dir.join("screenshots"),
    );
    let asset_dir =
        path_from_payload_or_default(payload, "defaultAssetDir", browser_dir.join("assets"));
    fs::create_dir_all(&screenshot_dir).await.ok();
    fs::create_dir_all(&asset_dir).await.ok();

    // 若顶层 url 指定，则先导航一次，后续 actions 继续在该页执行。
    if let Some(url) = payload
        .get("url")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        session.navigate(url, DEFAULT_NAV_TIMEOUT_MS).await?;
    }

    if let Some(viewport) = payload.get("viewport") {
        let width = viewport.get("width").and_then(serde_json::Value::as_u64);
        let height = viewport.get("height").and_then(serde_json::Value::as_u64);
        if let (Some(width), Some(height)) = (width, height) {
            session
                .set_viewport(width as i64, height as i64)
                .await
                .map_err(|e| format!("Failed to apply viewport: {e}"))?;
        }
    }

    let mut screenshots = Vec::new();
    let mut asset_bundles = Vec::new();
    let mut action_results = Vec::new();
    let actions = payload
        .get("actions")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();

    for (index, action) in actions.iter().enumerate() {
        ensure_not_cancelled(&cancel_flag)?;
        let action_result = run_action(
            &mut session,
            action,
            index,
            cwd,
            &screenshot_dir,
            &asset_dir,
            &mut screenshots,
            &mut asset_bundles,
            &http,
            &cancel_flag,
        )
        .await
        .map_err(|error| {
            let action_type = action
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("(missing type)");
            format!("Action {index} ({action_type}) failed: {error}")
        })?;
        action_results.push(action_result);
    }

    let final_url = session.current_url().await.unwrap_or_default();
    let title = session.current_title().await.unwrap_or_default();
    let tabs = session.describe_tabs().await?;

    Ok(BrowserRunOutput {
        ok: true,
        browser_mode: "tauri-webview-js-injection".to_string(),
        cdp_endpoint,
        final_url,
        title,
        tabs,
        duration_ms: started_at.elapsed().as_millis(),
        actions: action_results,
        screenshots,
        asset_bundles,
        console: Vec::new(),
        notes: vec!["js-injection".to_string()],
    })
}

/// Run browser actions against an external Chrome/Edge via CDP.
///
/// Unlike `run_webview_js_injection`, this connects to a pre-launched external
/// browser (managed by `ExternalBrowser`) rather than the embedded Tauri WebView.
pub async fn run_external_browser(
    cdp_endpoint: &str,
    workspace_config_dir: &Path,
    cwd: &Path,
    http: reqwest::Client,
    payload: &serde_json::Value,
    cancel_flag: Arc<AtomicBool>,
) -> Result<BrowserRunOutput, String> {
    let started_at = Instant::now();

    let cdp_endpoint =
        ensure_cdp_ready(&http, cdp_endpoint, Duration::from_secs(12), &cancel_flag).await?;
    let mut session =
        BrowserSession::connect(http.clone(), cdp_endpoint.clone(), cancel_flag.clone()).await?;

    let browser_dir = workspace_config_dir.join("browser");
    let screenshot_dir = path_from_payload_or_default(
        payload,
        "defaultScreenshotDir",
        browser_dir.join("screenshots"),
    );
    let asset_dir =
        path_from_payload_or_default(payload, "defaultAssetDir", browser_dir.join("assets"));
    fs::create_dir_all(&screenshot_dir).await.ok();
    fs::create_dir_all(&asset_dir).await.ok();

    if let Some(url) = payload
        .get("url")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        session.navigate(url, DEFAULT_NAV_TIMEOUT_MS).await?;
    }

    if let Some(viewport) = payload.get("viewport") {
        let width = viewport.get("width").and_then(serde_json::Value::as_u64);
        let height = viewport.get("height").and_then(serde_json::Value::as_u64);
        if let (Some(width), Some(height)) = (width, height) {
            session
                .set_viewport(width as i64, height as i64)
                .await
                .map_err(|e| format!("Failed to apply viewport: {e}"))?;
        }
    }

    let mut screenshots = Vec::new();
    let mut asset_bundles = Vec::new();
    let mut action_results = Vec::new();
    let actions = payload
        .get("actions")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();

    for (index, action) in actions.iter().enumerate() {
        ensure_not_cancelled(&cancel_flag)?;
        let action_result = run_action(
            &mut session,
            action,
            index,
            cwd,
            &screenshot_dir,
            &asset_dir,
            &mut screenshots,
            &mut asset_bundles,
            &http,
            &cancel_flag,
        )
        .await
        .map_err(|error| {
            let action_type = action
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("(missing type)");
            format!("Action {index} ({action_type}) failed: {error}")
        })?;
        action_results.push(action_result);
    }

    let final_url = session.current_url().await.unwrap_or_default();
    let title = session.current_title().await.unwrap_or_default();
    let tabs = session.describe_tabs().await?;

    Ok(BrowserRunOutput {
        ok: true,
        browser_mode: "external-chrome".to_string(),
        cdp_endpoint,
        final_url,
        title,
        tabs,
        duration_ms: started_at.elapsed().as_millis(),
        actions: action_results,
        screenshots,
        asset_bundles,
        console: Vec::new(),
        notes: vec!["external-chrome".to_string()],
    })
}

#[allow(clippy::too_many_arguments)]
async fn run_action(
    session: &mut BrowserSession,
    action: &serde_json::Value,
    index: usize,
    cwd: &Path,
    screenshot_dir: &Path,
    asset_dir: &Path,
    screenshots: &mut Vec<String>,
    asset_bundles: &mut Vec<String>,
    http: &reqwest::Client,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<serde_json::Value, String> {
    // 兼容历史参数：部分旧调用使用 action 字段而不是 type。
    // 新链路优先读取 type，缺失时回退 action，避免旧 prompt 直接报错。
    let action_type = action
        .get("type")
        .or_else(|| action.get("action"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "missing action.type".to_string())?;

    match action_type {
        "goto" => {
            let url = required_string(action, "url")?;
            let timeout = action_timeout_ms(action, DEFAULT_NAV_TIMEOUT_MS);
            session.navigate(&url, timeout).await?;
            let current_url = session.current_url().await.unwrap_or(url);
            Ok(action_result(
                index,
                action_type,
                json!({ "url": current_url }),
            ))
        }
        "click" => {
            if let Some(selector) = optional_string(action, "selector") {
                let value = session
                    .evaluate(
                        &format!(
                            r#"
                            (() => {{
                                const selector = {selector};
                                const el = document.querySelector(selector);
                                if (!el) {{
                                    throw new Error(`Selector not found: ${{selector}}`);
                                }}
                                el.scrollIntoView({{ block: "center", inline: "center", behavior: "instant" }});
                                const rect = el.getBoundingClientRect();
                                const cx = rect.left + Math.max(1, rect.width) / 2;
                                const cy = rect.top + Math.max(1, rect.height) / 2;
                                for (const type of ["pointerover","pointerenter","mouseover","mouseenter","mousemove","mousedown","mouseup","click"]) {{
                                    el.dispatchEvent(new MouseEvent(type, {{
                                        bubbles: true,
                                        cancelable: true,
                                        composed: true,
                                        clientX: cx,
                                        clientY: cy,
                                        button: 0
                                    }}));
                                }}
                                return {{ selector }};
                            }})()
                            "#,
                            selector = json_string(&selector)
                        ),
                        true,
                    )
                    .await?;
                Ok(action_result(index, action_type, value))
            } else {
                let x = required_number(action, "x")?;
                let y = required_number(action, "y")?;
                let value = session
                    .evaluate(
                        &format!(
                            r#"
                            (() => {{
                                const x = {x};
                                const y = {y};
                                const el = document.elementFromPoint(x, y);
                                if (!el) {{
                                    throw new Error(`No element at (${{x}}, ${{y}})`);
                                }}
                                for (const type of ["pointerover","pointerenter","mouseover","mouseenter","mousemove","mousedown","mouseup","click"]) {{
                                    el.dispatchEvent(new MouseEvent(type, {{
                                        bubbles: true,
                                        cancelable: true,
                                        composed: true,
                                        clientX: x,
                                        clientY: y,
                                        button: 0
                                    }}));
                                }}
                                return {{ x, y, tag: el.tagName.toLowerCase() }};
                            }})()
                            "#,
                            x = x,
                            y = y
                        ),
                        true,
                    )
                    .await?;
                Ok(action_result(index, action_type, value))
            }
        }
        "fill" => {
            let selector = required_string(action, "selector")?;
            let text = optional_string(action, "text").unwrap_or_default();
            let value = session
                .evaluate(
                    &format!(
                        r#"
                        (() => {{
                            const selector = {selector};
                            const text = {text};
                            const el = document.querySelector(selector);
                            if (!el) {{
                                throw new Error(`Selector not found: ${{selector}}`);
                            }}
                            el.scrollIntoView({{ block: "center", inline: "center", behavior: "instant" }});
                            el.focus?.();
                            if ("value" in el) {{
                                el.value = text;
                            }} else {{
                                el.textContent = text;
                            }}
                            el.dispatchEvent(new Event("input", {{ bubbles: true }}));
                            el.dispatchEvent(new Event("change", {{ bubbles: true }}));
                            return {{ selector, length: text.length }};
                        }})()
                        "#,
                        selector = json_string(&selector),
                        text = json_string(&text),
                    ),
                    true,
                )
                .await?;
            Ok(action_result(index, action_type, value))
        }
        "type" => {
            let text = optional_string(action, "text").unwrap_or_default();
            let selector_literal = optional_string(action, "selector")
                .map(|value| json_string(&value))
                .unwrap_or_else(|| "null".to_string());
            let value = session
                .evaluate(
                    &format!(
                        r#"
                        (() => {{
                            const selector = {selector_literal};
                            const text = {text};
                            const el = selector ? document.querySelector(selector) : document.activeElement;
                            if (!el) {{
                                throw new Error(selector ? `Selector not found: ${{selector}}` : "No active element");
                            }}
                            el.focus?.();
                            if ("value" in el) {{
                                const current = String(el.value ?? "");
                                el.value = current + text;
                            }} else {{
                                const current = String(el.textContent ?? "");
                                el.textContent = current + text;
                            }}
                            el.dispatchEvent(new Event("input", {{ bubbles: true }}));
                            el.dispatchEvent(new Event("change", {{ bubbles: true }}));
                            return {{ selector, length: text.length }};
                        }})()
                        "#,
                        selector_literal = selector_literal,
                        text = json_string(&text),
                    ),
                    true,
                )
                .await?;
            Ok(action_result(index, action_type, value))
        }
        "press" => {
            let key = required_string(action, "key")?;
            let selector_literal = optional_string(action, "selector")
                .map(|value| json_string(&value))
                .unwrap_or_else(|| "null".to_string());
            let value = session
                .evaluate(
                    &format!(
                        r#"
                        (() => {{
                            const selector = {selector_literal};
                            const key = {key};
                            const el = selector ? document.querySelector(selector) : document.activeElement;
                            if (!el) {{
                                throw new Error(selector ? `Selector not found: ${{selector}}` : "No active element");
                            }}
                            el.focus?.();
                            for (const type of ["keydown", "keypress", "keyup"]) {{
                                el.dispatchEvent(new KeyboardEvent(type, {{
                                    key,
                                    code: key,
                                    bubbles: true,
                                    cancelable: true,
                                    composed: true
                                }}));
                            }}
                            return {{ key, selector }};
                        }})()
                        "#,
                        selector_literal = selector_literal,
                        key = json_string(&key),
                    ),
                    true,
                )
                .await?;
            Ok(action_result(index, action_type, value))
        }
        "hover" => {
            let selector = required_string(action, "selector")?;
            let value = session
                .evaluate(
                    &format!(
                        r#"
                        (() => {{
                            const selector = {selector};
                            const el = document.querySelector(selector);
                            if (!el) {{
                                throw new Error(`Selector not found: ${{selector}}`);
                            }}
                            el.scrollIntoView({{ block: "center", inline: "center", behavior: "instant" }});
                            const rect = el.getBoundingClientRect();
                            const cx = rect.left + Math.max(1, rect.width) / 2;
                            const cy = rect.top + Math.max(1, rect.height) / 2;
                            for (const type of ["pointerover","pointerenter","mouseover","mouseenter","mousemove"]) {{
                                el.dispatchEvent(new MouseEvent(type, {{
                                    bubbles: true,
                                    cancelable: true,
                                    composed: true,
                                    clientX: cx,
                                    clientY: cy
                                }}));
                            }}
                            return {{ selector }};
                        }})()
                        "#,
                        selector = json_string(&selector),
                    ),
                    true,
                )
                .await?;
            Ok(action_result(index, action_type, value))
        }
        "check" | "uncheck" => {
            let selector = required_string(action, "selector")?;
            let target = action_type == "check";
            let value = session
                .evaluate(
                    &format!(
                        r#"
                        (() => {{
                            const selector = {selector};
                            const checked = {checked};
                            const el = document.querySelector(selector);
                            if (!(el instanceof HTMLInputElement)) {{
                                throw new Error(`Selector is not an input element: ${{selector}}`);
                            }}
                            if (el.type !== "checkbox" && el.type !== "radio") {{
                                throw new Error(`Unsupported input type for check/uncheck: ${{el.type}}`);
                            }}
                            el.checked = checked;
                            el.dispatchEvent(new Event("input", {{ bubbles: true }}));
                            el.dispatchEvent(new Event("change", {{ bubbles: true }}));
                            return {{ selector, checked: el.checked }};
                        }})()
                        "#,
                        selector = json_string(&selector),
                        checked = target,
                    ),
                    true,
                )
                .await?;
            Ok(action_result(index, action_type, value))
        }
        "select_option" => {
            let selector = required_string(action, "selector")?;
            let value = action.get("value");
            let values = action.get("values");
            let label = action.get("label");
            let index_value = action.get("index");
            if value.is_none() && values.is_none() && label.is_none() && index_value.is_none() {
                return Err(
                    "Action 'select_option' requires value, values, label, or index".to_string(),
                );
            }
            let payload = json!({
                "selector": selector,
                "value": value.cloned().unwrap_or(serde_json::Value::Null),
                "values": values.cloned().unwrap_or(serde_json::Value::Null),
                "label": label.cloned().unwrap_or(serde_json::Value::Null),
                "index": index_value.cloned().unwrap_or(serde_json::Value::Null),
            });
            let value = session
                .evaluate(
                    &format!(
                        r#"
                        (() => {{
                            const args = {args};
                            const el = document.querySelector(args.selector);
                            if (!(el instanceof HTMLSelectElement)) {{
                                throw new Error(`Selector is not a <select>: ${{args.selector}}`);
                            }}

                            const byValue = [];
                            if (typeof args.value === "string") {{
                                byValue.push(args.value);
                            }}
                            if (Array.isArray(args.values)) {{
                                for (const v of args.values) {{
                                    if (typeof v === "string") byValue.push(v);
                                }}
                            }}
                            if (byValue.length > 0) {{
                                for (const option of el.options) {{
                                    option.selected = byValue.includes(option.value);
                                }}
                            }} else if (typeof args.label === "string") {{
                                for (const option of el.options) {{
                                    option.selected = option.label === args.label;
                                }}
                            }} else if (typeof args.index === "number") {{
                                if (args.index < 0 || args.index >= el.options.length) {{
                                    throw new Error(`select index out of range: ${{args.index}}`);
                                }}
                                el.selectedIndex = args.index;
                            }} else {{
                                throw new Error("No valid select_option selector provided");
                            }}

                            el.dispatchEvent(new Event("input", {{ bubbles: true }}));
                            el.dispatchEvent(new Event("change", {{ bubbles: true }}));
                            return {{
                                selector: args.selector,
                                selectedValues: Array.from(el.selectedOptions).map((item) => item.value)
                            }};
                        }})()
                        "#,
                        args = payload.to_string()
                    ),
                    true,
                )
                .await?;
            Ok(action_result(index, action_type, value))
        }
        "wait_for_selector" => {
            let selector = required_string(action, "selector")?;
            let timeout_ms = action_timeout_ms(action, DEFAULT_ACTION_TIMEOUT_MS);
            let deadline = Instant::now() + Duration::from_millis(timeout_ms);
            loop {
                ensure_not_cancelled(cancel_flag)?;
                let exists = session
                    .evaluate(
                        &format!(
                            r#"
                            (() => {{
                                const selector = {selector};
                                return Boolean(document.querySelector(selector));
                            }})()
                            "#,
                            selector = json_string(&selector)
                        ),
                        true,
                    )
                    .await?
                    .as_bool()
                    .unwrap_or(false);
                if exists {
                    return Ok(action_result(
                        index,
                        action_type,
                        json!({ "selector": selector }),
                    ));
                }
                if Instant::now() >= deadline {
                    let current_url = session.current_url().await.unwrap_or_default();
                    let current_title = session.current_title().await.unwrap_or_default();
                    return Err(format!(
                        "Timeout waiting for selector: {selector} (url: {current_url}, title: {current_title})"
                    ));
                }
                sleep(Duration::from_millis(100)).await;
            }
        }
        "wait_for_timeout" => {
            let ms = optional_u64(action, &["ms", "timeout", "timeout_ms"])
                .unwrap_or(0)
                .min(60_000);
            sleep(Duration::from_millis(ms)).await;
            Ok(action_result(index, action_type, json!({ "ms": ms })))
        }
        "screenshot" => {
            let screenshot_path = resolve_output_path(
                cwd,
                optional_string(action, "path"),
                screenshot_dir,
                "browser",
                index,
                ".png",
            );
            if let Some(parent) = screenshot_path.parent() {
                fs::create_dir_all(parent).await.ok();
            }
            let bytes = session.capture_screenshot().await?;
            fs::write(&screenshot_path, bytes)
                .await
                .map_err(|e| format!("Failed to write screenshot: {e}"))?;
            // 输出路径统一去掉 Windows `\\?\` 前缀，保证前端预览 URL 可正常转换。
            let display_path = normalize_display_path(&screenshot_path);
            screenshots.push(display_path.clone());
            Ok(action_result(
                index,
                action_type,
                json!({ "path": display_path }),
            ))
        }
        "set_viewport" => {
            let width = required_i64(action, "width")?;
            let height = required_i64(action, "height")?;
            if width < 320 || height < 240 {
                return Err("set_viewport requires width >= 320 and height >= 240".to_string());
            }
            session.set_viewport(width, height).await?;
            Ok(action_result(
                index,
                action_type,
                json!({ "width": width, "height": height }),
            ))
        }
        "reload" => {
            let timeout = action_timeout_ms(action, DEFAULT_NAV_TIMEOUT_MS);
            session.reload(timeout).await?;
            Ok(action_result(
                index,
                action_type,
                json!({
                    "url": session.current_url().await.unwrap_or_default(),
                    "title": session.current_title().await.unwrap_or_default()
                }),
            ))
        }
        "back" => {
            let timeout = action_timeout_ms(action, DEFAULT_NAV_TIMEOUT_MS);
            session.back(timeout).await?;
            Ok(action_result(
                index,
                action_type,
                json!({
                    "url": session.current_url().await.unwrap_or_default(),
                    "title": session.current_title().await.unwrap_or_default()
                }),
            ))
        }
        "forward" => {
            let timeout = action_timeout_ms(action, DEFAULT_NAV_TIMEOUT_MS);
            session.forward(timeout).await?;
            Ok(action_result(
                index,
                action_type,
                json!({
                    "url": session.current_url().await.unwrap_or_default(),
                    "title": session.current_title().await.unwrap_or_default()
                }),
            ))
        }
        "title" => Ok(action_result(
            index,
            action_type,
            json!({ "title": session.current_title().await.unwrap_or_default() }),
        )),
        "url" => Ok(action_result(
            index,
            action_type,
            json!({ "url": session.current_url().await.unwrap_or_default() }),
        )),
        "html" => {
            let max_chars = optional_u64(action, &["maxChars", "max_chars"])
                .unwrap_or(8_000)
                .clamp(1_000, 20_000) as usize;
            let html = session
                .evaluate(
                    "(() => document.documentElement?.outerHTML ?? \"\")()",
                    true,
                )
                .await?
                .as_str()
                .unwrap_or_default()
                .to_string();
            Ok(action_result(
                index,
                action_type,
                json!({ "html": truncate_chars(&html, max_chars) }),
            ))
        }
        "snapshot" => {
            let max_items = optional_u64(action, &["maxItems", "max_items"]).unwrap_or(80);
            let snapshot = session
                .evaluate(
                    &format!(
                        r#"
                        (() => {{
                            const maxItems = {max_items};
                            const text = document.body?.innerText || "";
                            const serialize = (element) => {{
                                const rect = element.getBoundingClientRect();
                                return {{
                                    tag: element.tagName.toLowerCase(),
                                    text: (element.innerText || element.value || element.alt || element.title || "").trim().slice(0, 160),
                                    id: element.id || null,
                                    name: element.getAttribute("name"),
                                    type: element.getAttribute("type"),
                                    href: element.href || null,
                                    role: element.getAttribute("role"),
                                    ariaLabel: element.getAttribute("aria-label"),
                                    placeholder: element.getAttribute("placeholder"),
                                    visible: rect.width > 0 && rect.height > 0
                                }};
                            }};
                            const controls = Array.from(document.querySelectorAll("button,a,input,textarea,select,[role='button'],[role='link']"))
                                .slice(0, maxItems)
                                .map(serialize);
                            return {{
                                title: document.title,
                                url: location.href,
                                text: text.slice(0, 6000),
                                controls
                            }};
                        }})()
                        "#,
                        max_items = max_items
                    ),
                    true,
                )
                .await?;
            Ok(action_result(
                index,
                action_type,
                json!({ "snapshot": snapshot }),
            ))
        }
        "assets" => {
            let max_items = optional_u64(action, &["maxItems", "max_items"]).unwrap_or(100);
            let assets = collect_page_assets(session, max_items).await?;
            Ok(action_result(
                index,
                action_type,
                json!({ "assets": assets }),
            ))
        }
        "bundle_assets" => {
            let bundle_dir = resolve_output_path(
                cwd,
                optional_string(action, "path").or_else(|| optional_string(action, "output_path")),
                asset_dir,
                "bundle",
                index,
                "",
            );
            fs::create_dir_all(&bundle_dir).await.ok();
            let include = normalize_asset_include(action.get("include"));
            let max_downloads = optional_u64(action, &["maxDownloads", "max_downloads"])
                .unwrap_or(50)
                .min(200) as usize;
            let download = action
                .get("download")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true);

            let assets = collect_page_assets(
                session,
                optional_u64(action, &["maxItems", "max_items"]).unwrap_or(100),
            )
            .await?;
            let candidates = flatten_asset_candidates(&assets)
                .into_iter()
                .filter(|asset| include.iter().any(|kind| kind == &asset.kind))
                .collect::<Vec<_>>();
            let mut files = Vec::new();
            let mut skipped = Vec::new();
            for candidate in candidates.iter().take(max_downloads) {
                if candidate.url.as_deref().unwrap_or("").trim().is_empty() {
                    skipped.push(BundledAssetSkipped {
                        kind: candidate.kind.clone(),
                        url: candidate.url.clone(),
                        metadata: candidate.metadata.clone(),
                        reason: "missing URL".to_string(),
                    });
                    continue;
                }
                if !download {
                    files.push(BundledAssetFile {
                        kind: candidate.kind.clone(),
                        url: candidate.url.clone(),
                        metadata: candidate.metadata.clone(),
                        path: None,
                        downloaded: false,
                        bytes: None,
                        mime_type: None,
                    });
                    continue;
                }
                match save_asset(
                    http,
                    candidate.url.as_deref().unwrap_or_default(),
                    &bundle_dir,
                    &format!("{}-{}", files.len() + 1, candidate.kind),
                )
                .await
                {
                    Ok(saved) => files.push(BundledAssetFile {
                        kind: candidate.kind.clone(),
                        url: candidate.url.clone(),
                        metadata: candidate.metadata.clone(),
                        path: Some(saved.path),
                        downloaded: true,
                        bytes: Some(saved.bytes),
                        mime_type: saved.mime_type,
                    }),
                    Err(error) => skipped.push(BundledAssetSkipped {
                        kind: candidate.kind.clone(),
                        url: candidate.url.clone(),
                        metadata: candidate.metadata.clone(),
                        reason: error,
                    }),
                }
            }

            let manifest_path = bundle_dir.join("manifest.json");
            let manifest = json!({
                "url": session.current_url().await.unwrap_or_default(),
                "title": session.current_title().await.unwrap_or_default(),
                "createdAt": chrono::Utc::now().to_rfc3339(),
                "include": include,
                "download": download,
                "assets": assets,
                "files": files,
                "skipped": skipped,
            });
            let manifest_bytes = serde_json::to_vec_pretty(&manifest)
                .map_err(|e| format!("Failed to encode manifest: {e}"))?;
            fs::write(&manifest_path, manifest_bytes)
                .await
                .map_err(|e| format!("Failed to write manifest: {e}"))?;

            let manifest_display = normalize_display_path(&manifest_path);
            asset_bundles.push(manifest_display.clone());
            Ok(action_result(
                index,
                action_type,
                json!({
                    "bundleDir": normalize_display_path(&bundle_dir),
                    "manifestPath": manifest_display,
                    "assetCount": candidates.len(),
                    "downloadedCount": files.iter().filter(|item| item.downloaded).count(),
                    "skippedCount": skipped.len(),
                    "files": files,
                    "skipped": skipped
                }),
            ))
        }
        "eval" => {
            let script = required_string(action, "script")?;
            let value = session
                .evaluate(
                    &format!(
                        "(() => globalThis.eval({script}))()",
                        script = json_string(&script)
                    ),
                    true,
                )
                .await?;
            Ok(action_result(index, action_type, json!({ "value": value })))
        }
        "text" => {
            let selector =
                optional_string(action, "selector").unwrap_or_else(|| "body".to_string());
            let max_chars = optional_u64(action, &["maxChars", "max_chars"])
                .unwrap_or(4_000)
                .clamp(500, 20_000) as usize;
            let text = session
                .evaluate(
                    &format!(
                        r#"
                        (() => {{
                            const selector = {selector};
                            const el = document.querySelector(selector);
                            if (!el) {{
                                throw new Error(`Selector not found: ${{selector}}`);
                            }}
                            return (el.innerText || el.textContent || "").trim();
                        }})()
                        "#,
                        selector = json_string(&selector),
                    ),
                    true,
                )
                .await?
                .as_str()
                .unwrap_or_default()
                .to_string();
            Ok(action_result(
                index,
                action_type,
                json!({
                    "selector": selector,
                    "text": truncate_chars(&text, max_chars)
                }),
            ))
        }
        "list_tabs" => {
            let tabs = session.describe_tabs().await?;
            Ok(action_result(index, action_type, json!({ "tabs": tabs })))
        }
        "new_tab" => {
            let url = optional_string(action, "url");
            let tab_id = session.new_tab(url.as_deref()).await?;
            session.switch_to_tab(&tab_id).await?;
            let tabs = session.describe_tabs().await?;
            let active_index = tabs.iter().position(|tab| tab.active).unwrap_or(0);
            let current_url = session.current_url().await.unwrap_or_default();
            let title = session.current_title().await.unwrap_or_default();
            Ok(action_result(
                index,
                action_type,
                json!({
                    "tabIndex": active_index,
                    "url": current_url,
                    "title": title,
                    "tabs": tabs
                }),
            ))
        }
        "switch_tab" => {
            let tab_id = session.resolve_tab_from_action(action).await?;
            session.switch_to_tab(&tab_id).await?;
            let tabs = session.describe_tabs().await?;
            let active_index = tabs.iter().position(|tab| tab.active).unwrap_or(0);
            let current_url = session.current_url().await.unwrap_or_default();
            let title = session.current_title().await.unwrap_or_default();
            Ok(action_result(
                index,
                action_type,
                json!({
                    "tabIndex": active_index,
                    "url": current_url,
                    "title": title,
                    "tabs": tabs
                }),
            ))
        }
        "close_tab" => {
            let tabs_before = session.list_page_tabs().await?;
            let target = if action.get("index").is_some()
                || action.get("tab_index").is_some()
                || action.get("tabIndex").is_some()
                || action.get("url_contains").is_some()
                || action.get("title_contains").is_some()
            {
                session.resolve_tab_from_action(action).await?
            } else {
                session.current_tab_id.clone()
            };
            let closed_index = tabs_before
                .iter()
                .position(|tab| tab.id == target)
                .unwrap_or(0);
            session.close_tab(&target).await?;
            let tabs_after_close = session.list_page_tabs().await?;
            if tabs_after_close.is_empty() {
                let new_tab_id = session.new_tab(Some("about:blank")).await?;
                session.switch_to_tab(&new_tab_id).await?;
            } else {
                let next_index = closed_index.min(tabs_after_close.len().saturating_sub(1));
                let next_id = tabs_after_close[next_index].id.clone();
                session.switch_to_tab(&next_id).await?;
            }
            let tabs = session.describe_tabs().await?;
            let active_index = tabs.iter().position(|tab| tab.active).unwrap_or(0);
            Ok(action_result(
                index,
                action_type,
                json!({
                    "closedIndex": closed_index,
                    "activeIndex": active_index,
                    "tabs": tabs
                }),
            ))
        }
        other => Err(format!(
            "Unsupported browser action at index {index}: {other}"
        )),
    }
}

struct BrowserSession {
    http: reqwest::Client,
    cdp_endpoint: String,
    current_tab_id: String,
    client: CdpClient,
    cancel_flag: Arc<AtomicBool>,
}

impl BrowserSession {
    async fn connect(
        http: reqwest::Client,
        cdp_endpoint: String,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let tabs = list_page_tabs_http(&http, &cdp_endpoint).await?;
        let target = if let Some(tab) = tabs
            .iter()
            .find(|tab| !tab.web_socket_debugger_url.is_empty())
            .cloned()
        {
            tab
        } else {
            let created = create_tab_http(&http, &cdp_endpoint, Some("about:blank")).await?;
            if created.web_socket_debugger_url.is_empty() {
                return Err("CDP target does not expose webSocketDebuggerUrl".to_string());
            }
            created
        };
        let client = CdpClient::connect(&target.web_socket_debugger_url).await?;
        let mut session = Self {
            http,
            cdp_endpoint,
            current_tab_id: target.id,
            client,
            cancel_flag,
        };
        session.enable_domains().await?;
        Ok(session)
    }

    async fn enable_domains(&mut self) -> Result<(), String> {
        let _ = self.command("Page.enable", json!({})).await;
        let _ = self.command("Runtime.enable", json!({})).await;
        let _ = self.command("DOM.enable", json!({})).await;
        Ok(())
    }

    async fn command(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        ensure_not_cancelled(&self.cancel_flag)?;
        self.client.command(method, params).await
    }

    async fn evaluate(
        &mut self,
        script: &str,
        return_by_value: bool,
    ) -> Result<serde_json::Value, String> {
        let result = self
            .command(
                "Runtime.evaluate",
                json!({
                    "expression": script,
                    "returnByValue": return_by_value,
                    "awaitPromise": true,
                    "userGesture": true
                }),
            )
            .await?;

        if let Some(exception) = result.get("exceptionDetails") {
            let message = exception
                .get("text")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| exception.to_string());
            return Err(format!("JavaScript evaluation failed: {message}"));
        }

        let value = result
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        if return_by_value {
            if let Some(v) = value.get("value") {
                return Ok(v.clone());
            }
            if let Some(v) = value.get("unserializableValue") {
                return Ok(v.clone());
            }
            if let Some(v) = value.get("description") {
                return Ok(v.clone());
            }
        }
        Ok(value)
    }

    async fn navigate(&mut self, url: &str, timeout_ms: u64) -> Result<(), String> {
        self.command("Page.navigate", json!({ "url": url })).await?;
        self.wait_document_ready(timeout_ms).await
    }

    async fn reload(&mut self, timeout_ms: u64) -> Result<(), String> {
        self.command("Page.reload", json!({ "ignoreCache": false }))
            .await?;
        self.wait_document_ready(timeout_ms).await
    }

    async fn back(&mut self, timeout_ms: u64) -> Result<(), String> {
        let history = self.command("Page.getNavigationHistory", json!({})).await?;
        let current_index = history
            .get("currentIndex")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        let entries = history
            .get("entries")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        if current_index <= 0 || entries.is_empty() {
            return Ok(());
        }
        let target_entry = entries
            .get((current_index - 1) as usize)
            .and_then(|entry| entry.get("id"))
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| "Navigation history is unavailable for back".to_string())?;
        self.command(
            "Page.navigateToHistoryEntry",
            json!({ "entryId": target_entry }),
        )
        .await?;
        self.wait_document_ready(timeout_ms).await
    }

    async fn forward(&mut self, timeout_ms: u64) -> Result<(), String> {
        let history = self.command("Page.getNavigationHistory", json!({})).await?;
        let current_index = history
            .get("currentIndex")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        let entries = history
            .get("entries")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        if entries.is_empty() || current_index as usize >= entries.len().saturating_sub(1) {
            return Ok(());
        }
        let target_entry = entries
            .get((current_index + 1) as usize)
            .and_then(|entry| entry.get("id"))
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| "Navigation history is unavailable for forward".to_string())?;
        self.command(
            "Page.navigateToHistoryEntry",
            json!({ "entryId": target_entry }),
        )
        .await?;
        self.wait_document_ready(timeout_ms).await
    }

    async fn wait_document_ready(&mut self, timeout_ms: u64) -> Result<(), String> {
        let deadline =
            Instant::now() + Duration::from_millis(timeout_ms.clamp(1_000, MAX_WAIT_TIMEOUT_MS));
        loop {
            ensure_not_cancelled(&self.cancel_flag)?;
            let state = self
                .evaluate("(() => document.readyState || \"\")()", true)
                .await?
                .as_str()
                .unwrap_or_default()
                .to_string();
            if state == "interactive" || state == "complete" {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "Timed out waiting for document readiness after {timeout_ms} ms"
                ));
            }
            sleep(Duration::from_millis(80)).await;
        }
    }

    async fn capture_screenshot(&mut self) -> Result<Vec<u8>, String> {
        let result = self
            .command(
                "Page.captureScreenshot",
                json!({
                    "format": "png",
                    "fromSurface": true
                }),
            )
            .await?;
        let data = result
            .get("data")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "Page.captureScreenshot did not return image data".to_string())?;
        general_purpose::STANDARD
            .decode(data.as_bytes())
            .map_err(|e| format!("Failed to decode screenshot base64: {e}"))
    }

    async fn set_viewport(&mut self, width: i64, height: i64) -> Result<(), String> {
        self.command(
            "Emulation.setDeviceMetricsOverride",
            json!({
                "width": width,
                "height": height,
                "deviceScaleFactor": 1,
                "mobile": false
            }),
        )
        .await?;
        Ok(())
    }

    async fn current_url(&mut self) -> Result<String, String> {
        Ok(self
            .evaluate("(() => location.href || \"\")()", true)
            .await?
            .as_str()
            .unwrap_or_default()
            .to_string())
    }

    async fn current_title(&mut self) -> Result<String, String> {
        Ok(self
            .evaluate("(() => document.title || \"\")()", true)
            .await?
            .as_str()
            .unwrap_or_default()
            .to_string())
    }

    async fn list_page_tabs(&self) -> Result<Vec<RemoteTabInfo>, String> {
        list_page_tabs_http(&self.http, &self.cdp_endpoint).await
    }

    async fn describe_tabs(&self) -> Result<Vec<BrowserTabSnapshot>, String> {
        let tabs = self.list_page_tabs().await?;
        Ok(tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| BrowserTabSnapshot {
                index,
                url: tab.url.clone(),
                title: tab.title.clone(),
                active: tab.id == self.current_tab_id,
                closed: false,
            })
            .collect())
    }

    async fn resolve_tab_from_action(&self, action: &serde_json::Value) -> Result<String, String> {
        let tabs = self.list_page_tabs().await?;
        if tabs.is_empty() {
            return Err("No browser tabs available".to_string());
        }

        if let Some(index) = optional_u64(action, &["index", "tabIndex", "tab_index"]) {
            let index = index as usize;
            let tab = tabs
                .get(index)
                .ok_or_else(|| format!("Tab index out of range: {index}"))?;
            return Ok(tab.id.clone());
        }
        if let Some(url_contains) = optional_string(action, "url_contains") {
            if let Some(tab) = tabs.iter().find(|tab| tab.url.contains(&url_contains)) {
                return Ok(tab.id.clone());
            }
            return Err(format!("No tab matched url_contains: {url_contains}"));
        }
        if let Some(title_contains) = optional_string(action, "title_contains") {
            if let Some(tab) = tabs.iter().find(|tab| tab.title.contains(&title_contains)) {
                return Ok(tab.id.clone());
            }
            return Err(format!("No tab matched title_contains: {title_contains}"));
        }
        Ok(self.current_tab_id.clone())
    }

    async fn switch_to_tab(&mut self, tab_id: &str) -> Result<(), String> {
        activate_tab_http(&self.http, &self.cdp_endpoint, tab_id).await?;
        let tabs = self.list_page_tabs().await?;
        let target = tabs
            .into_iter()
            .find(|tab| tab.id == tab_id)
            .ok_or_else(|| format!("Tab not found after activate: {tab_id}"))?;
        if target.web_socket_debugger_url.is_empty() {
            return Err("Selected tab missing webSocketDebuggerUrl".to_string());
        }
        let client = CdpClient::connect(&target.web_socket_debugger_url).await?;
        self.client = client;
        self.current_tab_id = target.id;
        self.enable_domains().await?;
        Ok(())
    }

    async fn new_tab(&self, url: Option<&str>) -> Result<String, String> {
        let created = create_tab_http(&self.http, &self.cdp_endpoint, url).await?;
        Ok(created.id)
    }

    async fn close_tab(&self, tab_id: &str) -> Result<(), String> {
        close_tab_http(&self.http, &self.cdp_endpoint, tab_id).await
    }
}

struct CdpClient {
    stream: WsClientStream,
    next_id: i64,
}

impl CdpClient {
    async fn connect(ws_url: &str) -> Result<Self, String> {
        let (stream, _) = connect_async(ws_url)
            .await
            .map_err(|e| format!("CDP connect failed: {e}"))?;
        Ok(Self { stream, next_id: 1 })
    }

    async fn command(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let payload = json!({
            "id": id,
            "method": method,
            "params": params
        });
        let text =
            serde_json::to_string(&payload).map_err(|e| format!("CDP encode failed: {e}"))?;
        self.stream
            .send(Message::Text(text))
            .await
            .map_err(|e| format!("CDP send failed: {e}"))?;

        while let Some(message) = self.stream.next().await {
            let message = message.map_err(|e| format!("CDP receive failed: {e}"))?;
            let parsed = parse_ws_message_json(message)?;
            let Some(response_id) = parsed.get("id").and_then(serde_json::Value::as_i64) else {
                // 这是 CDP event（无 id），继续读取目标响应。
                continue;
            };
            if response_id != id {
                continue;
            }
            if let Some(error) = parsed.get("error") {
                let message = error
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown CDP error");
                return Err(format!("{method} failed: {message}"));
            }
            return Ok(parsed
                .get("result")
                .cloned()
                .unwrap_or(serde_json::Value::Null));
        }
        Err("CDP connection closed unexpectedly".to_string())
    }
}

fn parse_ws_message_json(message: Message) -> Result<serde_json::Value, String> {
    match message {
        Message::Text(text) => {
            serde_json::from_str(&text).map_err(|e| format!("CDP json parse failed: {e}"))
        }
        Message::Binary(bytes) => {
            let text =
                String::from_utf8(bytes).map_err(|e| format!("CDP utf8 parse failed: {e}"))?;
            serde_json::from_str(&text).map_err(|e| format!("CDP json parse failed: {e}"))
        }
        Message::Ping(_) | Message::Pong(_) => Ok(serde_json::Value::Null),
        Message::Close(_) => Err("CDP websocket closed".to_string()),
        _ => Ok(serde_json::Value::Null),
    }
}

fn local_cdp_http_client() -> reqwest::Client {
    // CDP is always local loopback; proxies can black-hole or 502 these requests.
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

fn cdp_http_client(_preferred: &reqwest::Client) -> reqwest::Client {
    // Prefer a dedicated local client so env/system proxies cannot intercept loopback CDP.
    local_cdp_http_client()
}

fn cdp_endpoint_candidates(preferred: &str) -> Vec<String> {
    let preferred = preferred.trim().trim_end_matches('/').to_string();
    let mut candidates = Vec::new();
    if !preferred.is_empty() {
        candidates.push(preferred.clone());
    }
    for candidate in browser_cdp_endpoint_candidates() {
        if !candidates.iter().any(|existing| existing == &candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}

async fn ensure_cdp_ready(
    http: &reqwest::Client,
    cdp_endpoint: &str,
    timeout: Duration,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;
    let client = cdp_http_client(http);
    let candidates = cdp_endpoint_candidates(cdp_endpoint);
    let mut last_error = format!("cannot connect to {cdp_endpoint}");

    loop {
        ensure_not_cancelled(cancel_flag)?;
        for endpoint in &candidates {
            let version_ok = client
                .get(format!("{endpoint}/json/version"))
                .send()
                .await
                .map(|resp| resp.status().is_success())
                .unwrap_or(false);
            if !version_ok {
                last_error = format!("version probe failed for {endpoint}");
                continue;
            }
            match list_page_tabs_http(&client, endpoint).await {
                Ok(_) => return Ok(endpoint.clone()),
                Err(error) => {
                    last_error = error;
                }
            }
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "WEBVIEW_CDP_UNAVAILABLE: cannot connect to {} (last error: {last_error})",
                candidates.join(" | ")
            ));
        }
        sleep(Duration::from_millis(150)).await;
    }
}

async fn list_page_tabs_http(
    http: &reqwest::Client,
    cdp_endpoint: &str,
) -> Result<Vec<RemoteTabInfo>, String> {
    let client = cdp_http_client(http);
    let response = client
        .get(format!("{cdp_endpoint}/json/list"))
        .send()
        .await
        .map_err(|e| format!("Failed to query CDP tabs: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Failed to query CDP tabs: HTTP {}",
            response.status().as_u16()
        ));
    }
    let tabs = response
        .json::<Vec<RemoteTabInfo>>()
        .await
        .map_err(|e| format!("Failed to parse CDP tabs: {e}"))?;
    Ok(tabs
        .into_iter()
        .filter(|tab| tab.kind.is_empty() || tab.kind == "page")
        .collect())
}

async fn create_tab_http(
    http: &reqwest::Client,
    cdp_endpoint: &str,
    url: Option<&str>,
) -> Result<RemoteTabInfo, String> {
    let client = cdp_http_client(http);
    let target = url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("about:blank");
    let endpoint = format!("{cdp_endpoint}/json/new?{}", encode_query_component(target));
    let response = client
        .put(endpoint)
        .send()
        .await
        .map_err(|e| format!("Failed to create tab: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Failed to create tab: HTTP {}",
            response.status().as_u16()
        ));
    }
    response
        .json::<RemoteTabInfo>()
        .await
        .map_err(|e| format!("Failed to parse created tab: {e}"))
}

async fn activate_tab_http(
    http: &reqwest::Client,
    cdp_endpoint: &str,
    tab_id: &str,
) -> Result<(), String> {
    let client = cdp_http_client(http);
    let response = client
        .get(format!("{cdp_endpoint}/json/activate/{tab_id}"))
        .send()
        .await
        .map_err(|e| format!("Failed to activate tab: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Failed to activate tab: HTTP {}",
            response.status().as_u16()
        ));
    }
    Ok(())
}

async fn close_tab_http(
    http: &reqwest::Client,
    cdp_endpoint: &str,
    tab_id: &str,
) -> Result<(), String> {
    let client = cdp_http_client(http);
    let response = client
        .get(format!("{cdp_endpoint}/json/close/{tab_id}"))
        .send()
        .await
        .map_err(|e| format!("Failed to close tab: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Failed to close tab: HTTP {}",
            response.status().as_u16()
        ));
    }
    Ok(())
}

async fn collect_page_assets(
    session: &mut BrowserSession,
    max_items: u64,
) -> Result<PageAssets, String> {
    let assets = session
        .evaluate(
            &format!(
                r#"
                (() => {{
                    const limit = {max_items};
                    const absolute = (value) => {{
                        if (!value) return null;
                        try {{
                            return new URL(value, location.href).href;
                        }} catch {{
                            return value;
                        }}
                    }};
                    const take = (items) => Array.from(items).slice(0, limit);
                    return {{
                        images: take(document.images).map((image) => ({{
                            src: absolute(image.currentSrc || image.src),
                            alt: image.alt || "",
                            width: image.naturalWidth || image.width || null,
                            height: image.naturalHeight || image.height || null
                        }})),
                        stylesheets: take(document.querySelectorAll("link[rel~='stylesheet']")).map((link) => ({{
                            href: absolute(link.getAttribute("href")),
                            media: link.getAttribute("media")
                        }})),
                        scripts: take(document.scripts).map((script) => ({{
                            src: absolute(script.getAttribute("src")),
                            type: script.getAttribute("type")
                        }})),
                        links: take(document.querySelectorAll("a[href]")).map((link) => ({{
                            href: absolute(link.getAttribute("href")),
                            text: (link.innerText || link.textContent || "").trim().slice(0, 160)
                        }}))
                    }};
                }})()
                "#,
                max_items = max_items
            ),
            true,
        )
        .await?;
    serde_json::from_value(assets).map_err(|e| format!("Failed to decode assets: {e}"))
}

fn flatten_asset_candidates(assets: &PageAssets) -> Vec<AssetCandidate> {
    let mut output = Vec::new();
    for image in &assets.images {
        output.push(AssetCandidate {
            kind: "images".to_string(),
            url: image
                .get("src")
                .and_then(serde_json::Value::as_str)
                .map(|value| value.to_string()),
            metadata: image.clone(),
        });
    }
    for stylesheet in &assets.stylesheets {
        output.push(AssetCandidate {
            kind: "stylesheets".to_string(),
            url: stylesheet
                .get("href")
                .and_then(serde_json::Value::as_str)
                .map(|value| value.to_string()),
            metadata: stylesheet.clone(),
        });
    }
    for script in &assets.scripts {
        output.push(AssetCandidate {
            kind: "scripts".to_string(),
            url: script
                .get("src")
                .and_then(serde_json::Value::as_str)
                .map(|value| value.to_string()),
            metadata: script.clone(),
        });
    }
    for link in &assets.links {
        output.push(AssetCandidate {
            kind: "links".to_string(),
            url: link
                .get("href")
                .and_then(serde_json::Value::as_str)
                .map(|value| value.to_string()),
            metadata: link.clone(),
        });
    }
    output
}

fn normalize_asset_include(include: Option<&serde_json::Value>) -> Vec<String> {
    let valid = ["images", "stylesheets", "scripts", "links"];
    if let Some(serde_json::Value::String(all)) = include {
        if all == "all" {
            return valid.iter().map(|item| item.to_string()).collect();
        }
    }
    if let Some(serde_json::Value::Array(items)) = include {
        let mut selected = Vec::new();
        for item in items {
            if let Some(value) = item.as_str() {
                if valid.iter().any(|candidate| candidate == &value) {
                    selected.push(value.to_string());
                }
            }
        }
        if !selected.is_empty() {
            return selected;
        }
    }
    vec![
        "images".to_string(),
        "stylesheets".to_string(),
        "scripts".to_string(),
    ]
}

#[derive(Debug)]
struct SavedAsset {
    path: String,
    bytes: usize,
    mime_type: Option<String>,
}

async fn save_asset(
    http: &reqwest::Client,
    url: &str,
    bundle_dir: &Path,
    basename: &str,
) -> Result<SavedAsset, String> {
    if url.starts_with("data:") {
        let parsed = parse_data_url(url)?;
        let extension = extension_for_mime(&parsed.mime_type).unwrap_or(".bin");
        let file_path = bundle_dir.join(format!("{basename}{extension}"));
        fs::write(&file_path, &parsed.bytes)
            .await
            .map_err(|e| format!("Failed to write data URL asset: {e}"))?;
        return Ok(SavedAsset {
            path: normalize_display_path(&file_path),
            bytes: parsed.bytes.len(),
            mime_type: Some(parsed.mime_type),
        });
    }

    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("only http, https, and data URLs can be bundled".to_string());
    }

    let response = http
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Asset download failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Asset download failed with HTTP {}",
            response.status().as_u16()
        ));
    }
    let mime_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read asset bytes: {e}"))?;
    let extension = extension_for_url(url)
        .or_else(|| mime_type.as_deref().and_then(extension_for_mime))
        .unwrap_or(".bin");
    let file_path = bundle_dir.join(format!("{basename}{extension}"));
    fs::write(&file_path, &bytes)
        .await
        .map_err(|e| format!("Failed to write asset file: {e}"))?;
    Ok(SavedAsset {
        path: normalize_display_path(&file_path),
        bytes: bytes.len(),
        mime_type,
    })
}

struct ParsedDataUrl {
    mime_type: String,
    bytes: Vec<u8>,
}

fn parse_data_url(url: &str) -> Result<ParsedDataUrl, String> {
    let rest = url
        .strip_prefix("data:")
        .ok_or_else(|| "invalid data URL".to_string())?;
    let (meta, payload) = rest
        .split_once(',')
        .ok_or_else(|| "invalid data URL payload".to_string())?;
    let mut parts = meta.split(';');
    let mime_type = parts.next().unwrap_or("text/plain").to_string();
    let is_base64 = parts.any(|part| part.eq_ignore_ascii_case("base64"));
    let bytes = if is_base64 {
        general_purpose::STANDARD
            .decode(payload.as_bytes())
            .map_err(|e| format!("invalid data URL base64 payload: {e}"))?
    } else {
        percent_decode(payload)
    };
    Ok(ParsedDataUrl { mime_type, bytes })
}

fn percent_decode(input: &str) -> Vec<u8> {
    let mut output = Vec::new();
    let mut chars = input.as_bytes().iter().copied().peekable();
    while let Some(ch) = chars.next() {
        if ch == b'%' {
            let hi = chars.next().unwrap_or(b'0');
            let lo = chars.next().unwrap_or(b'0');
            let hex = [hi, lo];
            if let Ok(text) = std::str::from_utf8(&hex) {
                if let Ok(value) = u8::from_str_radix(text, 16) {
                    output.push(value);
                    continue;
                }
            }
        }
        output.push(ch);
    }
    output
}

fn extension_for_url(url: &str) -> Option<&'static str> {
    let path = url.split('?').next().unwrap_or(url);
    if path.ends_with(".png") {
        Some(".png")
    } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        Some(".jpg")
    } else if path.ends_with(".gif") {
        Some(".gif")
    } else if path.ends_with(".webp") {
        Some(".webp")
    } else if path.ends_with(".svg") {
        Some(".svg")
    } else if path.ends_with(".css") {
        Some(".css")
    } else if path.ends_with(".js") {
        Some(".js")
    } else if path.ends_with(".json") {
        Some(".json")
    } else if path.ends_with(".ico") {
        Some(".ico")
    } else {
        None
    }
}

fn extension_for_mime(mime: &str) -> Option<&'static str> {
    let lower = mime.to_ascii_lowercase();
    if lower.contains("image/png") {
        Some(".png")
    } else if lower.contains("image/jpeg") {
        Some(".jpg")
    } else if lower.contains("image/gif") {
        Some(".gif")
    } else if lower.contains("image/webp") {
        Some(".webp")
    } else if lower.contains("image/svg") {
        Some(".svg")
    } else if lower.contains("text/css") {
        Some(".css")
    } else if lower.contains("javascript") || lower.contains("application/x-javascript") {
        Some(".js")
    } else if lower.contains("application/json") {
        Some(".json")
    } else if lower.contains("text/html") {
        Some(".html")
    } else {
        None
    }
}

fn action_result(index: usize, action_type: &str, details: serde_json::Value) -> serde_json::Value {
    let mut result = json!({
        "index": index,
        "type": action_type
    });
    if let Some(map) = details.as_object() {
        for (key, value) in map {
            result
                .as_object_mut()
                .expect("object")
                .insert(key.clone(), value.clone());
        }
    }
    result
}

fn required_string(action: &serde_json::Value, key: &str) -> Result<String, String> {
    optional_string(action, key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("Action field '{key}' must be a non-empty string"))
}

fn optional_string(action: &serde_json::Value, key: &str) -> Option<String> {
    action
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(|value| value.to_string())
}

fn required_number(action: &serde_json::Value, key: &str) -> Result<f64, String> {
    action
        .get(key)
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| format!("Action field '{key}' must be a number"))
}

fn required_i64(action: &serde_json::Value, key: &str) -> Result<i64, String> {
    action
        .get(key)
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| format!("Action field '{key}' must be an integer"))
}

fn optional_u64(action: &serde_json::Value, keys: &[&str]) -> Option<u64> {
    for key in keys {
        if let Some(value) = action.get(*key).and_then(serde_json::Value::as_u64) {
            return Some(value);
        }
    }
    None
}

fn action_timeout_ms(action: &serde_json::Value, default_value: u64) -> u64 {
    optional_u64(action, &["timeoutMs", "timeout_ms", "timeout"])
        .unwrap_or(default_value)
        .clamp(1_000, MAX_WAIT_TIMEOUT_MS)
}

fn path_from_payload_or_default(
    payload: &serde_json::Value,
    key: &str,
    default_path: PathBuf,
) -> PathBuf {
    payload
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or(default_path)
}

fn resolve_output_path(
    cwd: &Path,
    raw: Option<String>,
    default_dir: &Path,
    prefix: &str,
    index: usize,
    extension: &str,
) -> PathBuf {
    if let Some(path) = raw
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        let candidate = PathBuf::from(path);
        if candidate.is_absolute() {
            return candidate;
        }
        return cwd.join(candidate);
    }
    let timestamp = now_millis();
    if extension.is_empty() {
        default_dir.join(format!("{prefix}-{timestamp}-{index}"))
    } else {
        default_dir.join(format!("{prefix}-{timestamp}-{index}{extension}"))
    }
}

/// 将内部文件路径转为前端展示路径：
/// - 保留绝对路径语义；
/// - 去除 Windows 扩展路径前缀（`\\?\` / `\\?\UNC\`），
///   避免 `convertFileSrc` 生成不可访问 URL。
fn normalize_display_path(path: &Path) -> String {
    normalize_windows_verbatim_prefix(&path.to_string_lossy())
}

fn normalize_windows_verbatim_prefix(raw: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        let trimmed = raw.trim();
        if let Some(rest) = trimmed.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{rest}");
        }
        if let Some(rest) = trimmed.strip_prefix(r"\\?\") {
            return rest.to_string();
        }
        return trimmed.to_string();
    }

    #[cfg(not(target_os = "windows"))]
    {
        raw.trim().to_string()
    }
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or_default()
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value.chars().take(max_chars).collect()
}

fn encode_query_component(input: &str) -> String {
    let mut output = String::new();
    for byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                output.push(*byte as char)
            }
            _ => output.push_str(&format!("%{byte:02X}")),
        }
    }
    output
}

fn ensure_not_cancelled(cancel_flag: &Arc<AtomicBool>) -> Result<(), String> {
    if cancel_flag.load(Ordering::SeqCst) {
        return Err("Browser run interrupted by user.".to_string());
    }
    Ok(())
}

fn browser_run_initial_url(payload: &serde_json::Value) -> Option<String> {
    if let Some(url) = payload
        .get("url")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        return Some(url.to_string());
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
                    .map(str::trim)
                    .filter(|url| !url.is_empty())
                    .map(|url| url.to_string())
            })
        })
}

pub fn browser_run_timeout_ms(payload: &serde_json::Value) -> u64 {
    payload
        .get("timeout_ms")
        .or_else(|| payload.get("timeoutMs"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(DEFAULT_RUN_TIMEOUT_MS)
        .clamp(1_000, MAX_WAIT_TIMEOUT_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_run_initial_url_prefers_top_level_or_goto() {
        assert_eq!(
            browser_run_initial_url(&json!({"url":" http://localhost:3000 "})),
            Some("http://localhost:3000".to_string())
        );
        assert_eq!(
            browser_run_initial_url(&json!({
                "actions": [
                    {"type":"wait_for_timeout","ms":100},
                    {"type":"goto","url":"https://example.com"}
                ]
            })),
            Some("https://example.com".to_string())
        );
    }

    #[test]
    fn normalize_windows_verbatim_prefix_strips_prefix() {
        assert_eq!(
            normalize_windows_verbatim_prefix(r"\\?\E:\tmp\shot.png"),
            r"E:\tmp\shot.png".to_string()
        );
        assert_eq!(
            normalize_windows_verbatim_prefix(r"\\?\UNC\server\share\a.png"),
            r"\\server\share\a.png".to_string()
        );
    }
}
