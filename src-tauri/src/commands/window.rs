use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, Url, WebviewUrl,
    WebviewWindowBuilder, Window, webview::WebviewBuilder,
};
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::error::{AppError, AppResult};
use crate::state::{AppState, RunSummaryDiffPayload};

const BROWSER_WEBVIEW_LABEL: &str = "cn-browser";
const BROWSER_POPUP_WINDOW_LABEL: &str = "cn-browser-popup";
const BROWSER_DEBUG_PORT: u16 = 9242;
const BROWSER_ENDPOINT_FILE: &str = "visible-browser.json";
const BROWSER_DETACHED_CHANGED_EVENT: &str = "browser-detached-changed";
const BROWSER_POPUP_CLOSED_EVENT: &str = "browser-popup-closed";
const DOCUMENT_DETAIL_WINDOW_LABEL: &str = "document-detail";
const DOCUMENT_DETAIL_OPEN_EVENT: &str = "document-detail-open";
const DOCUMENT_DETAIL_INSERT_EVENT: &str = "document-detail-insert-snippet";
const RUNSUMMARY_DIFF_WINDOW_LABEL: &str = "runsummary-diff";
const RUNSUMMARY_DIFF_OPEN_EVENT: &str = "runsummary-diff-open";
const MAX_PICKED_ELEMENT_QUEUE: usize = 24;
const WEB_EDITABLE_EXTENSIONS: &[&str] = &[
    "html", "htm", "css", "js", "jsx", "mjs", "cjs", "ts", "tsx", "vue", "svelte",
];
const CDP_EVALUATE_MAX_ATTEMPTS: usize = 3;
const CDP_RETRY_BASE_DELAY_MS: u64 = 120;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserWindowInfo {
    pub label: String,
    pub url: String,
    pub created: bool,
    pub debug_port: u16,
    pub cdp_endpoint: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentDetailWindowInfo {
    pub label: String,
    pub path: String,
    pub created: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummaryDiffWindowInfo {
    pub label: String,
    pub path: String,
    pub created: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserEndpointMetadata {
    label: String,
    url: String,
    debug_port: u16,
    cdp_endpoint: String,
    updated_at_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserEditContext {
    pub editable: bool,
    pub current_url: String,
    pub source_path: Option<String>,
    pub reason: Option<String>,
    pub live_preview_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPickedElement {
    pub selector: String,
    pub selector_candidates: Vec<String>,
    pub tag_name: String,
    pub text: String,
    pub url: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub picked_at: u128,
    pub source_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserDomEditRequest {
    pub selector: String,
    pub text: Option<String>,
    pub color: Option<String>,
    pub background_color: Option<String>,
    pub font_size: Option<String>,
    pub font_weight: Option<String>,
    pub line_height: Option<String>,
    pub margin: Option<String>,
    pub padding: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserDomEditResult {
    pub selector: String,
    pub current_url: String,
    pub source_path: Option<String>,
    pub preview_html: String,
    pub live_preview: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserDetachedChangedEvent {
    detached: bool,
    url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteTabInfo {
    #[serde(default)]
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

#[tauri::command]
pub fn window_start_dragging(window: Window) -> AppResult<()> {
    window.start_dragging()?;
    Ok(())
}

#[tauri::command]
pub fn window_minimize(window: Window) -> AppResult<()> {
    window.minimize()?;
    Ok(())
}

#[tauri::command]
pub fn window_toggle_maximize(window: Window) -> AppResult<()> {
    if window.is_maximized()? {
        window.unmaximize()?;
    } else {
        window.maximize()?;
    }
    Ok(())
}

#[tauri::command]
pub fn window_close(window: Window) -> AppResult<()> {
    window.close()?;
    Ok(())
}

#[tauri::command]
pub fn window_show_main(app: AppHandle) -> AppResult<()> {
    if let Some(window) = app.get_webview_window("main") {
        // 仅在隐藏状态下执行 show，避免重复 show 导致额外窗口闪动。
        if !window.is_visible().unwrap_or(true) {
            window.show()?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn window_open_browser(
    app: AppHandle,
    state: State<'_, AppState>,
    url: Option<String>,
    x: Option<f64>,
    y: Option<f64>,
    width: Option<f64>,
    height: Option<f64>,
    workspace_root: Option<String>,
) -> AppResult<BrowserWindowInfo> {
    if let Some(window) = app.get_webview_window(BROWSER_POPUP_WINDOW_LABEL) {
        window.close()?;
        emit_browser_detached_state(&app, false, None);
    }
    let pos_x = x.unwrap_or(0.0);
    let pos_y = y.unwrap_or(0.0);
    let w = width.unwrap_or(400.0);
    let h = height.unwrap_or(600.0);
    let info = open_browser_embedded(
        &app,
        &state.workspace_config_dir,
        url.as_deref(),
        pos_x,
        pos_y,
        w,
        h,
    )?;
    {
        let mut guard = state.browser_last_url.write().await;
        *guard = Some(info.url.clone());
    }
    {
        let mut guard = state.browser_active_root.write().await;
        *guard = normalize_workspace_root_hint(workspace_root);
    }
    Ok(info)
}

#[tauri::command]
pub async fn window_resize_browser(
    app: AppHandle,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> AppResult<()> {
    if let Some(webview) = app.get_webview(BROWSER_WEBVIEW_LABEL) {
        webview.set_position(LogicalPosition::new(x, y))?;
        webview.set_size(LogicalSize::new(width, height))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn window_navigate_browser(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
    workspace_root: Option<String>,
) -> AppResult<()> {
    let browser_url = normalize_browser_url(Some(&url))?;
    let mut navigated = false;
    if let Some(webview) = app.get_webview(BROWSER_WEBVIEW_LABEL) {
        webview.navigate(browser_url.clone())?;
        navigated = true;
    } else if let Some(window) = app.get_webview_window(BROWSER_POPUP_WINDOW_LABEL) {
        window.navigate(browser_url.clone())?;
        navigated = true;
    }
    if navigated {
        let url = browser_url.as_str().to_string();
        let info = BrowserWindowInfo {
            label: if app.get_webview(BROWSER_WEBVIEW_LABEL).is_some() {
                BROWSER_WEBVIEW_LABEL.to_string()
            } else {
                BROWSER_POPUP_WINDOW_LABEL.to_string()
            },
            url: url.clone(),
            created: false,
            debug_port: BROWSER_DEBUG_PORT,
            cdp_endpoint: browser_cdp_endpoint(),
        };
        write_browser_endpoint_metadata(&state.workspace_config_dir, &info)?;
        let mut guard = state.browser_last_url.write().await;
        *guard = Some(url);
    }
    if workspace_root.is_some() {
        let mut guard = state.browser_active_root.write().await;
        *guard = normalize_workspace_root_hint(workspace_root);
    }
    Ok(())
}

#[tauri::command]
pub async fn window_close_browser(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    if let Some(webview) = app.get_webview(BROWSER_WEBVIEW_LABEL) {
        webview.close()?;
    }
    if let Some(window) = app.get_webview_window(BROWSER_POPUP_WINDOW_LABEL) {
        window.close()?;
    }
    {
        let mut guard = state.browser_last_url.write().await;
        *guard = None;
    }
    let mut guard = state.browser_active_root.write().await;
    *guard = None;
    emit_browser_detached_state(&app, false, None);
    Ok(())
}

#[tauri::command]
pub async fn window_detach_browser(
    app: AppHandle,
    state: State<'_, AppState>,
    width: Option<f64>,
    height: Option<f64>,
) -> AppResult<BrowserWindowInfo> {
    let browser_url = resolve_detach_browser_url(&state).await?;
    if let Some(webview) = app.get_webview(BROWSER_WEBVIEW_LABEL) {
        webview.close()?;
    }

    let popup_width = width.unwrap_or(1280.0).max(640.0);
    let popup_height = height.unwrap_or(860.0).max(480.0);
    let info = open_browser_popup(
        &app,
        &state.workspace_config_dir,
        &state.browser_last_url,
        &browser_url,
        popup_width,
        popup_height,
    )?;
    {
        let mut guard = state.browser_last_url.write().await;
        *guard = Some(info.url.clone());
    }
    emit_browser_detached_state(&app, true, Some(info.url.clone()));
    Ok(info)
}

#[tauri::command]
pub async fn window_attach_browser(
    app: AppHandle,
    state: State<'_, AppState>,
    url: Option<String>,
    x: Option<f64>,
    y: Option<f64>,
    width: Option<f64>,
    height: Option<f64>,
    workspace_root: Option<String>,
) -> AppResult<BrowserWindowInfo> {
    if let Some(window) = app.get_webview_window(BROWSER_POPUP_WINDOW_LABEL) {
        window.close()?;
    }

    let resolved_url = if let Some(value) = url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value.to_string())
    } else {
        state.browser_last_url.read().await.clone()
    };

    let info = open_browser_embedded(
        &app,
        &state.workspace_config_dir,
        resolved_url.as_deref(),
        x.unwrap_or(0.0),
        y.unwrap_or(0.0),
        width.unwrap_or(400.0),
        height.unwrap_or(600.0),
    )?;
    {
        let mut guard = state.browser_last_url.write().await;
        *guard = Some(info.url.clone());
    }
    if workspace_root.is_some() {
        let mut guard = state.browser_active_root.write().await;
        *guard = normalize_workspace_root_hint(workspace_root);
    }
    emit_browser_detached_state(&app, false, Some(info.url.clone()));
    Ok(info)
}

#[tauri::command]
pub async fn browser_get_edit_context(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<BrowserEditContext> {
    resolve_browser_edit_context(&app, &state).await
}

#[tauri::command]
pub async fn browser_start_pick_mode(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<BrowserEditContext> {
    let context = resolve_browser_edit_context(&app, &state).await?;
    if !context.editable {
        return Err(AppError::Custom(context.reason.unwrap_or_else(|| {
            "Current page is not editable. Open a workspace local web page first.".to_string()
        })));
    }

    evaluate_browser_script(
        &pick_mode_start_script(),
        true,
        Some(context.current_url.as_str()),
    )
    .await?;
    Ok(context)
}

#[tauri::command]
pub async fn browser_stop_pick_mode(app: AppHandle) -> AppResult<()> {
    ensure_browser_webview(&app)?;
    evaluate_browser_script(
        r#"
        (() => {
            const state = window.__cnPickState;
            if (state && typeof state.cleanup === "function") {
                state.cleanup();
            }
            if (state) {
                state.enabled = false;
            }
            return { enabled: false };
        })()
        "#,
        true,
        None,
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub async fn browser_poll_picked_element(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Option<BrowserPickedElement>> {
    ensure_browser_webview(&app)?;
    let context = resolve_browser_edit_context(&app, &state).await?;
    if !context.editable {
        return Ok(None);
    }

    let value = evaluate_browser_script(
        r#"
        (() => {
            const queue = window.__cnPickState && Array.isArray(window.__cnPickState.queue)
                ? window.__cnPickState.queue
                : null;
            if (!queue || queue.length === 0) {
                return null;
            }
            return queue.shift();
        })()
        "#,
        true,
        Some(context.current_url.as_str()),
    )
    .await?;

    if value.is_null() {
        return Ok(None);
    }
    let mut picked: BrowserPickedElement = serde_json::from_value(value)
        .map_err(|e| AppError::Custom(format!("Failed to parse picked element payload: {e}")))?;
    if picked.source_path.is_none() {
        picked.source_path = context.source_path;
    }
    Ok(Some(picked))
}

#[tauri::command]
pub async fn browser_apply_dom_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    request: BrowserDomEditRequest,
) -> AppResult<BrowserDomEditResult> {
    ensure_browser_webview(&app)?;
    let context = resolve_browser_edit_context(&app, &state).await?;
    if !context.editable {
        return Err(AppError::Custom(context.reason.unwrap_or_else(|| {
            "Current page is not editable. Open a workspace local web page first.".to_string()
        })));
    }

    let selector = request.selector.trim();
    if selector.is_empty() {
        return Err(AppError::Custom(
            "Selector is empty, cannot apply DOM edit.".to_string(),
        ));
    }

    let payload = serde_json::to_string(&request)
        .map_err(|e| AppError::Custom(format!("Failed to encode DOM edit request: {e}")))?;
    let value = evaluate_browser_script(
        &format!(
            r#"
            (() => {{
                const request = {payload};
                const selector = (request.selector || "").trim();
                if (!selector) {{
                    throw new Error("Selector is empty.");
                }}
                const element = document.querySelector(selector);
                if (!element) {{
                    throw new Error(`Selector not found: ${{selector}}`);
                }}
                if (typeof request.text === "string") {{
                    element.textContent = request.text;
                }}
                const style = element.style;
                const styleMap = [
                    ["color", request.color],
                    ["backgroundColor", request.backgroundColor],
                    ["fontSize", request.fontSize],
                    ["fontWeight", request.fontWeight],
                    ["lineHeight", request.lineHeight],
                    ["margin", request.margin],
                    ["padding", request.padding],
                ];
                for (const [key, value] of styleMap) {{
                    if (typeof value === "string" && value.trim().length > 0) {{
                        style[key] = value.trim();
                    }}
                }}
                return {{
                    selector,
                    currentUrl: location.href || "",
                    previewHtml: (element.outerHTML || "").slice(0, 4000),
                    livePreview: true,
                }};
            }})()
            "#,
            payload = payload
        ),
        true,
        Some(context.current_url.as_str()),
    )
    .await?;

    let selector = value
        .get("selector")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(selector)
        .to_string();
    let current_url = value
        .get("currentUrl")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(context.current_url.as_str())
        .to_string();
    let preview_html = value
        .get("previewHtml")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let live_preview = value
        .get("livePreview")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);

    Ok(BrowserDomEditResult {
        selector,
        current_url,
        source_path: context.source_path,
        preview_html,
        live_preview,
    })
}

#[tauri::command]
pub async fn browser_refresh_preview(app: AppHandle) -> AppResult<String> {
    ensure_browser_webview(&app)?;
    let mode = evaluate_browser_script(
        r#"
        (() => {
            const hasViteHmr = Boolean(window.__vite_hot) || Boolean(window.__vite_plugin_react_preamble_installed__);
            const hasWebpackHmr = Boolean(window.webpackHotUpdate) || Boolean(window.__webpack_hash__);
            if (hasViteHmr || hasWebpackHmr) {
                return "hmr";
            }
            location.reload();
            return "reload";
        })()
        "#,
        true,
        None,
    )
    .await;
    if let Ok(mode) = mode {
        return Ok(mode.as_str().unwrap_or("reload").to_string());
    }

    if let Some(window) = app.get_webview_window(BROWSER_POPUP_WINDOW_LABEL) {
        window.eval("window.location.reload();")?;
        return Ok("reload".to_string());
    }
    if let Some(webview) = app.get_webview(BROWSER_WEBVIEW_LABEL) {
        webview.eval("window.location.reload();")?;
        return Ok("reload".to_string());
    }

    Err(AppError::Custom(
        "No browser target available for preview refresh.".to_string(),
    ))
}

#[tauri::command]
pub async fn window_open_document_detail(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    workspace_root: Option<String>,
) -> AppResult<DocumentDetailWindowInfo> {
    let (display_path, _) = resolve_existing_file_path(&path)?;
    let active_root = normalize_workspace_root_hint(workspace_root);
    {
        // 先写入“当前目标路径”，保障详情窗首次启动时可通过 command 主动读取。
        let mut guard = state.document_detail_active_path.write().await;
        *guard = Some(display_path.clone());
    }
    {
        let mut guard = state.document_detail_active_root.write().await;
        *guard = active_root;
    }

    if let Some(window) = app.get_webview_window(DOCUMENT_DETAIL_WINDOW_LABEL) {
        if !window.is_visible().unwrap_or(true) {
            window.show()?;
        }
        window.set_focus()?;
        let _ = app.emit_to(
            DOCUMENT_DETAIL_WINDOW_LABEL,
            DOCUMENT_DETAIL_OPEN_EVENT,
            serde_json::json!({ "path": display_path.clone() }),
        );
        return Ok(DocumentDetailWindowInfo {
            label: DOCUMENT_DETAIL_WINDOW_LABEL.to_string(),
            path: display_path,
            created: false,
        });
    }

    let detail_window = WebviewWindowBuilder::new(
        &app,
        DOCUMENT_DETAIL_WINDOW_LABEL,
        document_detail_window_url()?,
    )
    .title("文档详情")
    .inner_size(1060.0, 760.0)
    .min_inner_size(760.0, 520.0)
    .resizable(true)
    .decorations(false)
    .build()?;

    // 允许用户最小化后再次打开时回到前台，保持“单实例复用”行为一致。
    detail_window.show()?;
    detail_window.set_focus()?;

    let active_path = state.document_detail_active_path.clone();
    let active_root = state.document_detail_active_root.clone();
    detail_window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            let active_path = active_path.clone();
            let active_root = active_root.clone();
            tauri::async_runtime::spawn(async move {
                {
                    let mut guard = active_path.write().await;
                    *guard = None;
                }
                let mut guard = active_root.write().await;
                *guard = None;
            });
        }
    });

    Ok(DocumentDetailWindowInfo {
        label: DOCUMENT_DETAIL_WINDOW_LABEL.to_string(),
        path: display_path,
        created: true,
    })
}

#[tauri::command]
pub async fn window_close_document_detail(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    if let Some(window) = app.get_webview_window(DOCUMENT_DETAIL_WINDOW_LABEL) {
        window.close()?;
    }
    {
        let mut guard = state.document_detail_active_path.write().await;
        *guard = None;
    }
    let mut guard = state.document_detail_active_root.write().await;
    *guard = None;
    Ok(())
}

#[tauri::command]
pub async fn window_get_document_detail_path(
    state: State<'_, AppState>,
) -> AppResult<Option<String>> {
    Ok(state.document_detail_active_path.read().await.clone())
}

#[tauri::command]
pub async fn window_open_runsummary_diff(
    app: AppHandle,
    state: State<'_, AppState>,
    payload: RunSummaryDiffPayload,
) -> AppResult<RunSummaryDiffWindowInfo> {
    // 统一在命令入口完成路径与枚举值规范化，避免前端多入口造成不一致。
    let normalized_payload = normalize_runsummary_diff_payload(payload);
    {
        // 先缓存 payload，确保新窗口首帧可通过 get command 拉到最新数据。
        let mut guard = state.runsummary_diff_payload.write().await;
        *guard = Some(normalized_payload.clone());
    }

    if let Some(window) = app.get_webview_window(RUNSUMMARY_DIFF_WINDOW_LABEL) {
        if !window.is_visible().unwrap_or(true) {
            window.show()?;
        }
        window.set_focus()?;
        let _ = app.emit_to(
            RUNSUMMARY_DIFF_WINDOW_LABEL,
            RUNSUMMARY_DIFF_OPEN_EVENT,
            normalized_payload.clone(),
        );
        return Ok(RunSummaryDiffWindowInfo {
            label: RUNSUMMARY_DIFF_WINDOW_LABEL.to_string(),
            path: normalized_payload.path,
            created: false,
        });
    }

    let diff_window = WebviewWindowBuilder::new(
        &app,
        RUNSUMMARY_DIFF_WINDOW_LABEL,
        runsummary_diff_window_url()?,
    )
    .title("RunSummary Diff")
    .inner_size(1060.0, 760.0)
    .min_inner_size(760.0, 520.0)
    .resizable(true)
    .decorations(false)
    .build()?;

    diff_window.show()?;
    diff_window.set_focus()?;

    let active_payload = state.runsummary_diff_payload.clone();
    diff_window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            let active_payload = active_payload.clone();
            tauri::async_runtime::spawn(async move {
                let mut guard = active_payload.write().await;
                *guard = None;
            });
        }
    });

    Ok(RunSummaryDiffWindowInfo {
        label: RUNSUMMARY_DIFF_WINDOW_LABEL.to_string(),
        path: normalized_payload.path,
        created: true,
    })
}

#[tauri::command]
pub async fn window_close_runsummary_diff(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    if let Some(window) = app.get_webview_window(RUNSUMMARY_DIFF_WINDOW_LABEL) {
        window.close()?;
    }
    let mut guard = state.runsummary_diff_payload.write().await;
    *guard = None;
    Ok(())
}

#[tauri::command]
pub async fn window_get_runsummary_diff_payload(
    state: State<'_, AppState>,
) -> AppResult<Option<RunSummaryDiffPayload>> {
    Ok(state.runsummary_diff_payload.read().await.clone())
}

#[tauri::command]
pub fn document_detail_insert_snippet(app: AppHandle, snippet: String) -> AppResult<()> {
    let payload_text = snippet.trim();
    if payload_text.is_empty() {
        return Err(AppError::Custom(
            "Snippet content is empty, cannot insert into chat.".to_string(),
        ));
    }
    app.emit_to(
        "main",
        DOCUMENT_DETAIL_INSERT_EVENT,
        DocumentDetailInsertEvent {
            snippet: payload_text.to_string(),
        },
    )?;
    Ok(())
}

pub fn open_browser_embedded(
    app: &AppHandle,
    workspace_config_dir: &Path,
    url: Option<&str>,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> AppResult<BrowserWindowInfo> {
    let should_navigate_existing = url.is_some_and(|value| !value.trim().is_empty());
    let browser_url = normalize_browser_url(url)?;
    let url_string = browser_url.as_str().to_string();
    let cdp_endpoint = browser_cdp_endpoint();

    if let Some(webview) = app.get_webview(BROWSER_WEBVIEW_LABEL) {
        if should_navigate_existing {
            webview.navigate(browser_url)?;
        }
        webview.set_position(LogicalPosition::new(x, y))?;
        webview.set_size(LogicalSize::new(width, height))?;
        let info = BrowserWindowInfo {
            label: BROWSER_WEBVIEW_LABEL.to_string(),
            url: url_string,
            created: false,
            debug_port: BROWSER_DEBUG_PORT,
            cdp_endpoint,
        };
        write_browser_endpoint_metadata(workspace_config_dir, &info)?;
        return Ok(info);
    }

    let browser_dir = workspace_config_dir.join("browser");
    fs::create_dir_all(&browser_dir)?;

    let main_window = app
        .get_window("main")
        .ok_or_else(|| AppError::Custom("Main window not found".to_string()))?;

    let webview_builder =
        WebviewBuilder::new(BROWSER_WEBVIEW_LABEL, WebviewUrl::External(browser_url))
            .enable_clipboard_access()
            .data_directory(browser_dir.join("webview-data"))
            .additional_browser_args(&browser_additional_args());

    main_window.add_child(
        webview_builder,
        LogicalPosition::new(x, y),
        LogicalSize::new(width, height),
    )?;

    let info = BrowserWindowInfo {
        label: BROWSER_WEBVIEW_LABEL.to_string(),
        url: url_string,
        created: true,
        debug_port: BROWSER_DEBUG_PORT,
        cdp_endpoint,
    };
    write_browser_endpoint_metadata(workspace_config_dir, &info)?;
    Ok(info)
}

pub fn browser_cdp_endpoint() -> String {
    format!("http://127.0.0.1:{BROWSER_DEBUG_PORT}")
}

fn browser_additional_args() -> String {
    format!("--remote-debugging-port={BROWSER_DEBUG_PORT} --remote-allow-origins=*")
}

fn open_browser_popup(
    app: &AppHandle,
    workspace_config_dir: &Path,
    browser_last_url: &std::sync::Arc<tokio::sync::RwLock<Option<String>>>,
    url: &str,
    width: f64,
    height: f64,
) -> AppResult<BrowserWindowInfo> {
    let browser_url = normalize_browser_url(Some(url))?;
    let url_string = browser_url.as_str().to_string();
    let cdp_endpoint = browser_cdp_endpoint();

    if let Some(window) = app.get_webview_window(BROWSER_POPUP_WINDOW_LABEL) {
        window.navigate(browser_url.clone())?;
        window.set_size(LogicalSize::new(width, height))?;
        if !window.is_visible().unwrap_or(true) {
            window.show()?;
        }
        window.set_focus()?;
        let info = BrowserWindowInfo {
            label: BROWSER_POPUP_WINDOW_LABEL.to_string(),
            url: url_string,
            created: false,
            debug_port: BROWSER_DEBUG_PORT,
            cdp_endpoint,
        };
        write_browser_endpoint_metadata(workspace_config_dir, &info)?;
        return Ok(info);
    }

    let popup = WebviewWindowBuilder::new(
        app,
        BROWSER_POPUP_WINDOW_LABEL,
        WebviewUrl::External(browser_url),
    )
    .title("Browser")
    .inner_size(width, height)
    .min_inner_size(640.0, 480.0)
    .resizable(true)
    .build()?;
    popup.show()?;
    popup.set_focus()?;

    let app_handle = app.clone();
    let last_url = browser_last_url.clone();
    popup.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            let app_handle = app_handle.clone();
            let last_url = last_url.clone();
            tauri::async_runtime::spawn(async move {
                let url = last_url.read().await.clone();
                let _ = app_handle.emit_to(
                    "main",
                    BROWSER_POPUP_CLOSED_EVENT,
                    serde_json::json!({ "url": url }),
                );
                let _ = app_handle.emit_to(
                    "main",
                    BROWSER_DETACHED_CHANGED_EVENT,
                    BrowserDetachedChangedEvent {
                        detached: false,
                        url,
                    },
                );
            });
        }
    });

    let info = BrowserWindowInfo {
        label: BROWSER_POPUP_WINDOW_LABEL.to_string(),
        url: url_string,
        created: true,
        debug_port: BROWSER_DEBUG_PORT,
        cdp_endpoint,
    };
    write_browser_endpoint_metadata(workspace_config_dir, &info)?;
    Ok(info)
}

async fn resolve_detach_browser_url(state: &State<'_, AppState>) -> AppResult<String> {
    if let Some(url) = state.browser_last_url.read().await.clone() {
        let trimmed = url.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    Ok("about:blank".to_string())
}

fn emit_browser_detached_state(app: &AppHandle, detached: bool, url: Option<String>) {
    let _ = app.emit_to(
        "main",
        BROWSER_DETACHED_CHANGED_EVENT,
        BrowserDetachedChangedEvent { detached, url },
    );
}

fn ensure_browser_webview(app: &AppHandle) -> AppResult<()> {
    if app.get_webview(BROWSER_WEBVIEW_LABEL).is_some()
        || app.get_webview_window(BROWSER_POPUP_WINDOW_LABEL).is_some()
    {
        return Ok(());
    }
    Err(AppError::Custom(
        "Browser panel is not active. Open the browser tab first.".to_string(),
    ))
}

async fn resolve_browser_edit_context(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> AppResult<BrowserEditContext> {
    if app.get_webview(BROWSER_WEBVIEW_LABEL).is_none() {
        return Ok(BrowserEditContext {
            editable: false,
            current_url: "about:blank".to_string(),
            source_path: None,
            reason: Some("Browser panel is not active.".to_string()),
            live_preview_mode: "readonly".to_string(),
        });
    }

    let current_url = current_browser_url().await?;
    let parsed_url = match Url::parse(&current_url) {
        Ok(parsed) => parsed,
        Err(_) => {
            return Ok(BrowserEditContext {
                editable: false,
                current_url,
                source_path: None,
                reason: Some("Current browser URL is invalid.".to_string()),
                live_preview_mode: "readonly".to_string(),
            });
        }
    };

    let workspace_roots = browser_workspace_roots_from_state(state).await?;
    let Some(source_path) = workspace_roots
        .iter()
        .find_map(|root| resolve_workspace_web_source_path(root, &parsed_url))
    else {
        let reason = if !matches!(parsed_url.scheme(), "http" | "https" | "file") {
            "Only http/https/file local pages can enter edit mode.".to_string()
        } else {
            "Current page cannot be mapped to a workspace web file.".to_string()
        };
        return Ok(BrowserEditContext {
            editable: false,
            current_url,
            source_path: None,
            reason: Some(reason),
            live_preview_mode: "readonly".to_string(),
        });
    };

    Ok(BrowserEditContext {
        editable: true,
        current_url,
        source_path: Some(normalize_windows_verbatim_prefix(
            &source_path.to_string_lossy(),
        )),
        reason: None,
        live_preview_mode: "hmr-or-reload".to_string(),
    })
}

async fn workspace_root_from_state(state: &State<'_, AppState>) -> AppResult<PathBuf> {
    let cwd = state.cwd.read().await.clone();
    let path = PathBuf::from(normalize_windows_verbatim_prefix(&cwd));
    if path.is_dir() {
        return Ok(path.canonicalize().unwrap_or(path));
    }
    Err(AppError::Custom(format!(
        "Workspace directory is invalid: {}",
        path.display()
    )))
}

async fn browser_workspace_roots_from_state(
    state: &State<'_, AppState>,
) -> AppResult<Vec<PathBuf>> {
    let mut roots = vec![workspace_root_from_state(state).await?];
    if let Some(active_root) = state.browser_active_root.read().await.clone() {
        if let Some(root) = resolve_workspace_root_candidate(&active_root) {
            if !roots.iter().any(|item| item == &root) {
                roots.push(root);
            }
        }
    }
    Ok(roots)
}

fn resolve_workspace_web_source_path(workspace_root: &Path, browser_url: &Url) -> Option<PathBuf> {
    let root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let scheme = browser_url.scheme();
    let mut candidates = Vec::new();
    if scheme == "file" {
        if let Ok(file_path) = browser_url.to_file_path() {
            candidates.push(file_path);
        }
    } else {
        if scheme != "http" && scheme != "https" {
            return None;
        }
        let host = browser_url.host_str()?.to_ascii_lowercase();
        if !is_local_browser_host(&host) {
            return None;
        }

        if let Some(file_query) = browser_url
            .query_pairs()
            .find_map(|(key, value)| {
                if key == "file" || key == "path" {
                    Some(value.to_string())
                } else {
                    None
                }
            })
            .map(|value| value.trim().to_string())
        {
            if !file_query.is_empty() {
                let query_path = PathBuf::from(file_query);
                if query_path.is_absolute() {
                    candidates.push(query_path);
                } else {
                    candidates.push(workspace_root.join(query_path));
                }
            }
        }

        let mut relative_path = browser_url
            .path()
            .trim()
            .trim_start_matches('/')
            .to_string();
        if relative_path.is_empty() {
            relative_path = "index.html".to_string();
        }
        if relative_path.ends_with('/') {
            relative_path.push_str("index.html");
        }

        candidates.push(workspace_root.join(&relative_path));
        candidates.push(workspace_root.join("public").join(&relative_path));

        let relative_no_suffix = relative_path.trim_end_matches('/');
        if Path::new(relative_no_suffix).extension().is_none() {
            candidates.push(workspace_root.join(format!("{relative_no_suffix}.html")));
            candidates.push(workspace_root.join(relative_no_suffix).join("index.html"));
            candidates.push(
                workspace_root
                    .join("public")
                    .join(relative_no_suffix)
                    .join("index.html"),
            );
        }
    }

    candidates.into_iter().find_map(|candidate| {
        if !candidate.is_file() || !has_editable_web_extension(&candidate) {
            return None;
        }
        let canonical = candidate.canonicalize().ok()?;
        if canonical.starts_with(&root) {
            Some(canonical)
        } else {
            None
        }
    })
}

fn has_editable_web_extension(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    WEB_EDITABLE_EXTENSIONS
        .iter()
        .any(|item| *item == extension)
}

fn is_local_browser_host(host: &str) -> bool {
    host == "localhost" || host == "::1" || host.starts_with("127.") || host.starts_with("0.0.0.0")
}

fn pick_mode_start_script() -> String {
    format!(
        r#"
        (() => {{
            const maxQueue = {max_queue};
            if (!window.__cnPickState) {{
                window.__cnPickState = {{
                    enabled: false,
                    queue: [],
                    hovered: null,
                    previousOutline: "",
                    previousOutlineOffset: "",
                    cleanup: null
                }};
            }}
            const state = window.__cnPickState;
            if (state.enabled) {{
                return {{ enabled: true }};
            }}

            const cssEscape = (value) => {{
                if (window.CSS && typeof window.CSS.escape === "function") {{
                    return window.CSS.escape(value);
                }}
                return String(value).replace(/([^\w-])/g, "\\\\$1");
            }};

            const buildCandidates = (element) => {{
                const candidates = [];
                if (!element || !(element instanceof Element)) {{
                    return candidates;
                }}
                if (element.id) {{
                    candidates.push(`#${{cssEscape(element.id)}}`);
                }}
                const testId = element.getAttribute("data-testid") || element.getAttribute("data-test-id");
                if (testId) {{
                    candidates.push(`[data-testid="${{cssEscape(testId)}}"]`);
                }}
                const ariaLabel = element.getAttribute("aria-label");
                if (ariaLabel) {{
                    candidates.push(`[aria-label="${{cssEscape(ariaLabel)}}"]`);
                }}
                const name = element.getAttribute("name");
                if (name) {{
                    candidates.push(`[name="${{cssEscape(name)}}"]`);
                }}
                const placeholder = element.getAttribute("placeholder");
                if (placeholder) {{
                    candidates.push(`[placeholder="${{cssEscape(placeholder)}}"]`);
                }}
                if (element.tagName) {{
                    const tagName = element.tagName.toLowerCase();
                    const classList = Array.from(element.classList || []).slice(0, 2).map(cssEscape);
                    if (classList.length) {{
                        candidates.push(`${{tagName}}.${{classList.join(".")}}`);
                    }} else {{
                        candidates.push(tagName);
                    }}
                }}
                const pathSegments = [];
                let cursor = element;
                while (cursor && cursor.nodeType === 1 && pathSegments.length < 6) {{
                    let segment = cursor.tagName.toLowerCase();
                    if (cursor.id) {{
                        segment += `#${{cssEscape(cursor.id)}}`;
                        pathSegments.unshift(segment);
                        break;
                    }}
                    let siblingIndex = 1;
                    let previous = cursor.previousElementSibling;
                    while (previous) {{
                        if (previous.tagName === cursor.tagName) {{
                            siblingIndex += 1;
                        }}
                        previous = previous.previousElementSibling;
                    }}
                    segment += `:nth-of-type(${{siblingIndex}})`;
                    pathSegments.unshift(segment);
                    cursor = cursor.parentElement;
                }}
                if (pathSegments.length) {{
                    candidates.push(pathSegments.join(" > "));
                }}
                return Array.from(new Set(candidates.filter((item) => typeof item === "string" && item.trim().length > 0)));
            }};

            const restoreHover = () => {{
                if (state.hovered instanceof Element) {{
                    state.hovered.style.outline = state.previousOutline || "";
                    state.hovered.style.outlineOffset = state.previousOutlineOffset || "";
                }}
                state.hovered = null;
                state.previousOutline = "";
                state.previousOutlineOffset = "";
            }};

            const onMove = (event) => {{
                const target = event.target instanceof Element ? event.target : null;
                if (!target || target === state.hovered) {{
                    return;
                }}
                restoreHover();
                state.hovered = target;
                state.previousOutline = target.style.outline || "";
                state.previousOutlineOffset = target.style.outlineOffset || "";
                target.style.outline = "2px solid #22c55e";
                target.style.outlineOffset = "2px";
            }};

            const onClick = (event) => {{
                const target = event.target instanceof Element ? event.target : null;
                if (!target) {{
                    return;
                }}
                event.preventDefault();
                event.stopPropagation();
                event.stopImmediatePropagation();
                const rect = target.getBoundingClientRect();
                const candidates = buildCandidates(target);
                const text = (target.innerText || target.textContent || target.getAttribute("value") || "").trim().slice(0, 240);
                const payload = {{
                    selector: candidates[0] || "",
                    selectorCandidates: candidates,
                    tagName: target.tagName ? target.tagName.toLowerCase() : "",
                    text,
                    url: location.href || "",
                    x: rect.x || rect.left || 0,
                    y: rect.y || rect.top || 0,
                    width: rect.width || 0,
                    height: rect.height || 0,
                    pickedAt: Date.now(),
                    sourcePath: null
                }};
                state.queue.push(payload);
                if (state.queue.length > maxQueue) {{
                    state.queue.splice(0, state.queue.length - maxQueue);
                }}
            }};

            document.addEventListener("mousemove", onMove, true);
            document.addEventListener("click", onClick, true);
            state.cleanup = () => {{
                document.removeEventListener("mousemove", onMove, true);
                document.removeEventListener("click", onClick, true);
                restoreHover();
            }};
            state.enabled = true;
            return {{ enabled: true }};
        }})()
        "#,
        max_queue = MAX_PICKED_ELEMENT_QUEUE
    )
}

async fn current_browser_url() -> AppResult<String> {
    let value = evaluate_browser_script("(() => location.href || \"\")()", true, None).await?;
    Ok(value.as_str().unwrap_or("about:blank").to_string())
}

async fn evaluate_browser_script(
    script: &str,
    return_by_value: bool,
    preferred_url: Option<&str>,
) -> AppResult<serde_json::Value> {
    let mut attempt = 0usize;
    loop {
        let result = evaluate_browser_script_once(script, return_by_value, preferred_url).await;
        match result {
            Ok(value) => return Ok(value),
            Err(error) => {
                if attempt + 1 >= CDP_EVALUATE_MAX_ATTEMPTS || !is_transient_cdp_error(&error) {
                    return Err(error);
                }
                attempt += 1;
                let delay_ms = CDP_RETRY_BASE_DELAY_MS.saturating_mul(attempt as u64);
                sleep(Duration::from_millis(delay_ms)).await;
            }
        }
    }
}

async fn evaluate_browser_script_once(
    script: &str,
    return_by_value: bool,
    preferred_url: Option<&str>,
) -> AppResult<serde_json::Value> {
    let cdp_endpoint = browser_cdp_endpoint();
    let ws_url = browser_tab_websocket_url(&cdp_endpoint, preferred_url).await?;
    let (mut stream, _) = connect_async(&ws_url)
        .await
        .map_err(|e| AppError::Custom(format!("CDP connect failed: {e}")))?;

    cdp_send_command(&mut stream, 1, "Runtime.enable", json!({})).await?;
    let response = cdp_send_command(
        &mut stream,
        2,
        "Runtime.evaluate",
        json!({
            "expression": script,
            "returnByValue": return_by_value,
            "awaitPromise": true,
            "userGesture": true
        }),
    )
    .await?;

    if let Some(exception) = response
        .get("result")
        .and_then(|value| value.get("exceptionDetails"))
    {
        let message = exception
            .get("text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown browser script error");
        return Err(AppError::Custom(format!(
            "Browser script execution failed: {message}"
        )));
    }

    let result = response
        .get("result")
        .and_then(|value| value.get("result"))
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    if return_by_value {
        if let Some(value) = result.get("value") {
            return Ok(value.clone());
        }
        if let Some(value) = result.get("unserializableValue") {
            return Ok(value.clone());
        }
        if let Some(value) = result.get("description") {
            return Ok(value.clone());
        }
    }

    Ok(result)
}

fn is_transient_cdp_error(error: &AppError) -> bool {
    let AppError::Custom(message) = error else {
        return false;
    };
    let lowered = message.to_ascii_lowercase();
    lowered.contains("cdp connect failed")
        || lowered.contains("cdp receive failed")
        || lowered.contains("cdp websocket closed")
        || lowered.contains("connection closed before receiving")
        || lowered.contains("no browser tab exposes a cdp websocket endpoint")
        || lowered.contains("failed to query browser tabs")
        || lowered.contains("websocket protocol error")
        || lowered.contains("connection reset")
        || lowered.contains("broken pipe")
}

async fn browser_tab_websocket_url(
    cdp_endpoint: &str,
    preferred_url: Option<&str>,
) -> AppResult<String> {
    let tabs = list_browser_tabs(cdp_endpoint).await?;
    let page_tabs: Vec<RemoteTabInfo> = tabs
        .into_iter()
        .filter(|tab| {
            (tab.kind.is_empty() || tab.kind == "page")
                && !tab.web_socket_debugger_url.trim().is_empty()
        })
        .collect();
    if page_tabs.is_empty() {
        return Err(AppError::Custom(
            "No browser tab exposes a CDP websocket endpoint.".to_string(),
        ));
    }

    let normalized_preferred = preferred_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(normalize_tab_url_for_match);
    if let Some(preferred) = normalized_preferred.as_deref() {
        if let Some(tab) = page_tabs
            .iter()
            .find(|tab| tab_matches_preferred_url(tab, preferred))
        {
            return Ok(tab.web_socket_debugger_url.clone());
        }
    }

    if let Some(tab) = page_tabs.iter().find(|tab| is_non_blank_browser_tab(tab)) {
        return Ok(tab.web_socket_debugger_url.clone());
    }

    if let Some(tab) = page_tabs
        .iter()
        .find(|tab| !tab.id.trim().is_empty() && !tab.title.trim().is_empty())
    {
        return Ok(tab.web_socket_debugger_url.clone());
    }

    Ok(page_tabs
        .first()
        .map(|tab| tab.web_socket_debugger_url.clone())
        .unwrap_or_default())
}

fn normalize_tab_url_for_match(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_ascii_lowercase()
}

fn tab_matches_preferred_url(tab: &RemoteTabInfo, preferred_url: &str) -> bool {
    let tab_url = normalize_tab_url_for_match(&tab.url);
    if tab_url.is_empty() {
        return false;
    }
    tab_url == preferred_url
        || tab_url.starts_with(preferred_url)
        || preferred_url.starts_with(&tab_url)
}

fn is_non_blank_browser_tab(tab: &RemoteTabInfo) -> bool {
    let url = tab.url.trim();
    if url.is_empty() || url.eq_ignore_ascii_case("about:blank") {
        return false;
    }
    !url.to_ascii_lowercase().starts_with("devtools://")
}

async fn list_browser_tabs(cdp_endpoint: &str) -> AppResult<Vec<RemoteTabInfo>> {
    let response = reqwest::Client::new()
        .get(format!("{cdp_endpoint}/json/list"))
        .send()
        .await
        .map_err(|e| AppError::Custom(format!("Failed to query browser tabs: {e}")))?;
    if !response.status().is_success() {
        return Err(AppError::Custom(format!(
            "Failed to query browser tabs: HTTP {}",
            response.status().as_u16()
        )));
    }
    let tabs = response
        .json::<Vec<RemoteTabInfo>>()
        .await
        .map_err(|e| AppError::Custom(format!("Failed to parse browser tabs: {e}")))?;
    Ok(tabs)
}

async fn cdp_send_command(
    stream: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    id: i64,
    method: &str,
    params: serde_json::Value,
) -> AppResult<serde_json::Value> {
    let payload = serde_json::to_string(&json!({
        "id": id,
        "method": method,
        "params": params
    }))
    .map_err(|e| AppError::Custom(format!("Failed to encode CDP payload: {e}")))?;
    stream
        .send(Message::Text(payload))
        .await
        .map_err(|e| AppError::Custom(format!("CDP send failed: {e}")))?;

    while let Some(message) = stream.next().await {
        let message = message.map_err(|e| AppError::Custom(format!("CDP receive failed: {e}")))?;
        let Some(parsed) = parse_cdp_message(message)? else {
            continue;
        };
        let Some(response_id) = parsed.get("id").and_then(serde_json::Value::as_i64) else {
            continue;
        };
        if response_id != id {
            continue;
        }
        if let Some(error) = parsed.get("error") {
            let reason = error
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown CDP error");
            return Err(AppError::Custom(format!("{method} failed: {reason}")));
        }
        return Ok(parsed);
    }

    Err(AppError::Custom(
        "CDP connection closed before receiving a response.".to_string(),
    ))
}

fn parse_cdp_message(message: Message) -> AppResult<Option<serde_json::Value>> {
    match message {
        Message::Text(text) => {
            let parsed = serde_json::from_str(&text)
                .map_err(|e| AppError::Custom(format!("CDP json parse failed: {e}")))?;
            Ok(Some(parsed))
        }
        Message::Binary(bytes) => {
            let text = String::from_utf8(bytes)
                .map_err(|e| AppError::Custom(format!("CDP binary decode failed: {e}")))?;
            let parsed = serde_json::from_str(&text)
                .map_err(|e| AppError::Custom(format!("CDP json parse failed: {e}")))?;
            Ok(Some(parsed))
        }
        Message::Ping(_) | Message::Pong(_) => Ok(None),
        Message::Close(_) => Err(AppError::Custom("CDP websocket closed.".to_string())),
        _ => Ok(None),
    }
}

fn write_browser_endpoint_metadata(
    workspace_config_dir: &Path,
    info: &BrowserWindowInfo,
) -> AppResult<()> {
    let browser_dir = workspace_config_dir.join("browser");
    fs::create_dir_all(&browser_dir)?;
    let updated_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let metadata = BrowserEndpointMetadata {
        label: info.label.clone(),
        url: info.url.clone(),
        debug_port: info.debug_port,
        cdp_endpoint: info.cdp_endpoint.clone(),
        updated_at_ms,
    };
    fs::write(
        browser_dir.join(BROWSER_ENDPOINT_FILE),
        serde_json::to_vec_pretty(&metadata)?,
    )?;
    Ok(())
}

fn normalize_browser_url(raw: Option<&str>) -> AppResult<Url> {
    let trimmed = raw.unwrap_or("").trim();
    let candidate = if trimmed.is_empty() {
        "about:blank".to_string()
    } else if let Some(local_file_url) = local_file_url_from_input(trimmed) {
        local_file_url.to_string()
    } else if looks_like_local_dev_host(trimmed) {
        format!("http://{trimmed}")
    } else if has_explicit_scheme(trimmed) {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };

    let parsed = Url::parse(&candidate)
        .map_err(|e| AppError::Custom(format!("Invalid browser URL '{candidate}': {e}")))?;
    match parsed.scheme() {
        "http" | "https" | "file" | "about" => Ok(parsed),
        scheme => Err(AppError::Custom(format!(
            "Unsupported browser URL scheme '{scheme}'. Use http, https, file, or about."
        ))),
    }
}

fn local_file_url_from_input(raw: &str) -> Option<Url> {
    if has_explicit_scheme(raw) {
        return None;
    }
    let normalized = normalize_windows_verbatim_prefix(raw);
    let path = PathBuf::from(normalized);
    if !path.is_absolute() {
        return None;
    }
    Url::from_file_path(path).ok()
}

fn has_explicit_scheme(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("about:")
        || lower.contains("://")
        || lower.starts_with("file:")
        || lower.starts_with("data:")
        || lower.starts_with("javascript:")
        || lower.starts_with("mailto:")
        || lower.starts_with("ftp:")
        || lower.starts_with("chrome:")
        || lower.starts_with("edge:")
}

fn looks_like_local_dev_host(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower == "localhost"
        || lower.starts_with("localhost:")
        || lower.starts_with("localhost/")
        || lower == "127.0.0.1"
        || lower.starts_with("127.")
        || lower.starts_with("[::1]")
        || lower.starts_with("::1")
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

fn normalize_runsummary_diff_payload(mut payload: RunSummaryDiffPayload) -> RunSummaryDiffPayload {
    // 路径统一去除 Windows `\\?\` 前缀，前后端展示和写盘命令都使用同一形态。
    payload.path = normalize_windows_verbatim_prefix(&payload.path);
    payload.file_action = payload.file_action.trim().to_lowercase();
    payload.diff_source = payload.diff_source.trim().to_lowercase();
    payload
}

fn resolve_existing_file_path(path: &str) -> AppResult<(String, std::path::PathBuf)> {
    // 所有“文档详情窗相关读写”统一使用同一套路径标准化逻辑，
    // 避免 Windows `\\?\` 前缀在不同命令中的判定不一致。
    let display_path = normalize_windows_verbatim_prefix(path);
    let file_path = std::path::PathBuf::from(&display_path);
    if !file_path.is_file() {
        return Err(AppError::Custom(format!(
            "Path is not a file: {display_path}"
        )));
    }
    Ok((display_path, file_path))
}

fn normalize_workspace_root_hint(workspace_root: Option<String>) -> Option<String> {
    workspace_root
        .as_deref()
        .and_then(resolve_workspace_root_candidate)
        .map(|path| normalize_windows_verbatim_prefix(&path.to_string_lossy()))
}

fn resolve_workspace_root_candidate(raw: &str) -> Option<PathBuf> {
    let normalized = normalize_windows_verbatim_prefix(raw);
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = PathBuf::from(trimmed);
    if !path.is_dir() {
        return None;
    }
    Some(path.canonicalize().unwrap_or(path))
}

async fn ensure_workspace_file_access(
    state: &State<'_, AppState>,
    file_path: &Path,
) -> AppResult<()> {
    let workspace_root = workspace_root_from_state(state).await?;
    let canonical_target = file_path
        .canonicalize()
        .unwrap_or_else(|_| file_path.to_path_buf());
    if canonical_target.starts_with(&workspace_root) {
        return Ok(());
    }

    Err(AppError::Custom(format!(
        "File path is outside workspace and cannot be edited: {}",
        normalize_windows_verbatim_prefix(&canonical_target.to_string_lossy())
    )))
}

fn document_detail_window_url() -> AppResult<WebviewUrl> {
    #[cfg(debug_assertions)]
    {
        // 开发环境下详情窗走 Vite dev server 的独立入口，
        // 这样可以和主窗分离打包逻辑并保持热更新体验。
        let dev_url =
            std::env::var("TAURI_DEV_URL").unwrap_or_else(|_| "http://localhost:1420".to_string());
        let base = dev_url.trim().trim_end_matches('/');
        let final_url = format!("{base}/detail.html");
        let parsed = Url::parse(&final_url).map_err(|e| {
            AppError::Custom(format!(
                "Invalid document detail dev url '{final_url}': {e}"
            ))
        })?;
        return Ok(WebviewUrl::External(parsed));
    }

    #[cfg(not(debug_assertions))]
    {
        // 生产构建使用打包后的 detail.html 入口，避免与主窗 UI 互相污染。
        Ok(WebviewUrl::App("detail.html".into()))
    }
}

fn runsummary_diff_window_url() -> AppResult<WebviewUrl> {
    #[cfg(debug_assertions)]
    {
        // 开发环境下独立 Diff 窗走独立入口，确保主窗与 Diff 窗构建隔离且支持热更新。
        let dev_url =
            std::env::var("TAURI_DEV_URL").unwrap_or_else(|_| "http://localhost:1420".to_string());
        let base = dev_url.trim().trim_end_matches('/');
        let final_url = format!("{base}/diff.html");
        let parsed = Url::parse(&final_url).map_err(|e| {
            AppError::Custom(format!(
                "Invalid runsummary diff dev url '{final_url}': {e}"
            ))
        })?;
        return Ok(WebviewUrl::External(parsed));
    }

    #[cfg(not(debug_assertions))]
    {
        // 生产构建使用打包后的 diff.html，避免主窗内弹层造成交互耦合。
        Ok(WebviewUrl::App("diff.html".into()))
    }
}

#[tauri::command]
pub async fn reveal_in_explorer(path: String) -> AppResult<()> {
    // 统一移除 Windows 扩展前缀，确保 exists 校验与 explorer 打开行为一致。
    let display_path = normalize_windows_verbatim_prefix(&path);
    let p = std::path::Path::new(&display_path);
    if !p.exists() {
        return Err(AppError::Custom(format!(
            "Path does not exist: {display_path}"
        )));
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let win_path = display_path.replace('/', "\\");
        if p.is_dir() {
            std::process::Command::new("explorer")
                .arg(&win_path)
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .map_err(|e| AppError::Custom(format!("Failed to open explorer: {e}")))?;
        } else {
            std::process::Command::new("explorer")
                .arg(format!("/select,{win_path}"))
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .map_err(|e| AppError::Custom(format!("Failed to open explorer: {e}")))?;
        }
    }

    #[cfg(target_os = "macos")]
    {
        if p.is_dir() {
            std::process::Command::new("open")
                .arg(&display_path)
                .spawn()
                .map_err(|e| AppError::Custom(format!("Failed to open finder: {e}")))?;
        } else {
            std::process::Command::new("open")
                .arg("-R")
                .arg(&display_path)
                .spawn()
                .map_err(|e| AppError::Custom(format!("Failed to open finder: {e}")))?;
        }
    }

    #[cfg(target_os = "linux")]
    {
        let dir = if p.is_dir() {
            p.to_path_buf()
        } else {
            p.parent().unwrap_or(p).to_path_buf()
        };
        std::process::Command::new("xdg-open")
            .arg(dir)
            .spawn()
            .map_err(|e| AppError::Custom(format!("Failed to open file manager: {e}")))?;
    }

    Ok(())
}

#[tauri::command]
pub async fn window_toggle_devtools(app: AppHandle) -> AppResult<()> {
    if let Some(webview) = app.get_webview_window("main") {
        if webview.is_devtools_open() {
            webview.close_devtools();
        } else {
            webview.open_devtools();
        }
    }
    Ok(())
}

#[tauri::command]
pub fn get_user_home_dir() -> AppResult<String> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
        .ok_or_else(|| AppError::Custom("Could not determine user home directory".into()))?;
    Ok(super::normalize_windows_verbatim_prefix(
        &home.to_string_lossy(),
    ))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

const HIDDEN_DIRS: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    "node_modules",
    "target",
    ".next",
    ".nuxt",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    "dist",
    ".vscode",
    ".idea",
    ".DS_Store",
];

#[tauri::command]
pub async fn read_directory(path: String) -> AppResult<Vec<FileEntry>> {
    let display_path = normalize_windows_verbatim_prefix(&path);
    let dir = std::path::Path::new(&display_path);
    if !dir.is_dir() {
        return Err(AppError::Custom(format!(
            "Path is not a directory: {display_path}"
        )));
    }

    let mut dirs = Vec::new();
    let mut files = Vec::new();

    let entries = fs::read_dir(dir)
        .map_err(|e| AppError::Custom(format!("Failed to read directory: {e}")))?;

    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().to_string();
        if file_name.starts_with('.') && HIDDEN_DIRS.contains(&file_name.as_str()) {
            continue;
        }
        if HIDDEN_DIRS.contains(&file_name.as_str()) {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        let entry_path = entry.path();
        let abs_path = super::normalize_windows_verbatim_prefix(&entry_path.to_string_lossy());

        let fe = FileEntry {
            name: file_name,
            path: abs_path,
            is_dir: metadata.is_dir(),
            size: metadata.len(),
        };

        if metadata.is_dir() {
            dirs.push(fe);
        } else {
            files.push(fe);
        }
    }

    dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    dirs.append(&mut files);
    Ok(dirs)
}

const MAX_ATTACH_SIZE: u64 = 2 * 1024 * 1024;
const MAX_TEXT_PREVIEW_SIZE: u64 = 512 * 1024;
const MAX_TEXT_PREVIEW_WRITE_SIZE: u64 = 512 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAttachResult {
    pub name: String,
    pub mime_type: String,
    pub data_url: String,
    pub size: u64,
    pub source_path: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextFilePreviewResult {
    pub name: String,
    pub path: String,
    pub mime_type: String,
    pub content: String,
    pub size: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextFileWriteResult {
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentDetailInsertEvent {
    pub snippet: String,
}

#[tauri::command]
pub async fn read_file_for_attach(
    state: State<'_, AppState>,
    path: String,
) -> AppResult<FileAttachResult> {
    use base64::Engine;

    let (display_path, file_path) = resolve_existing_file_path(&path)?;
    ensure_workspace_file_access(&state, &file_path).await?;

    let metadata = fs::metadata(&file_path)
        .map_err(|e| AppError::Custom(format!("Failed to read file metadata: {e}")))?;
    let file_size = metadata.len();
    let file_name = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let ext = file_path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "json" => "application/json",
        "xml" => "application/xml",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" | "cjs" => "text/javascript",
        "ts" | "tsx" | "jsx" => "text/typescript",
        "rs" => "text/x-rust",
        "py" => "text/x-python",
        "md" => "text/markdown",
        "txt" | "log" => "text/plain",
        "toml" => "application/toml",
        "yaml" | "yml" => "text/yaml",
        "csv" => "text/csv",
        "sh" | "bash" | "zsh" => "text/x-shellscript",
        "sql" => "text/x-sql",
        "go" => "text/x-go",
        "java" => "text/x-java",
        "c" | "h" => "text/x-c",
        "cpp" | "cc" | "cxx" | "hpp" => "text/x-c++",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        _ => "application/octet-stream",
    };

    let truncated = file_size > MAX_ATTACH_SIZE;
    let read_size = if truncated {
        MAX_ATTACH_SIZE as usize
    } else {
        file_size as usize
    };

    let bytes =
        fs::read(&file_path).map_err(|e| AppError::Custom(format!("Failed to read file: {e}")))?;
    let actual_bytes = &bytes[..read_size.min(bytes.len())];

    let b64 = base64::engine::general_purpose::STANDARD.encode(actual_bytes);
    let data_url = format!("data:{mime};base64,{b64}");

    Ok(FileAttachResult {
        name: file_name,
        mime_type: mime.to_string(),
        data_url,
        size: file_size,
        source_path: display_path,
        truncated,
    })
}

#[tauri::command]
pub async fn read_text_file_preview(path: String) -> AppResult<TextFilePreviewResult> {
    let (display_path, file_path) = resolve_existing_file_path(&path)?;

    let metadata = fs::metadata(&file_path)
        .map_err(|e| AppError::Custom(format!("Failed to read file metadata: {e}")))?;
    let file_size = metadata.len();
    let file_name = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = file_path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    let mime = match ext.as_str() {
        "md" | "mdx" => "text/markdown",
        "txt" | "log" => "text/plain",
        "json" | "jsonc" => "application/json",
        "xml" => "application/xml",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" | "cjs" => "text/javascript",
        "ts" | "tsx" | "jsx" => "text/typescript",
        "rs" => "text/x-rust",
        "py" => "text/x-python",
        "toml" => "application/toml",
        "yaml" | "yml" => "text/yaml",
        "csv" => "text/csv",
        "sh" | "bash" | "zsh" => "text/x-shellscript",
        "sql" => "text/x-sql",
        "go" => "text/x-go",
        "java" => "text/x-java",
        "c" | "h" => "text/x-c",
        "cpp" | "cc" | "cxx" | "hpp" => "text/x-c++",
        _ => "text/plain",
    };

    let truncated = file_size > MAX_TEXT_PREVIEW_SIZE;
    let read_size = if truncated {
        MAX_TEXT_PREVIEW_SIZE as usize
    } else {
        file_size as usize
    };

    let bytes =
        fs::read(&file_path).map_err(|e| AppError::Custom(format!("Failed to read file: {e}")))?;
    let actual_bytes = &bytes[..read_size.min(bytes.len())];
    let content = String::from_utf8_lossy(actual_bytes).to_string();

    Ok(TextFilePreviewResult {
        name: file_name,
        path: display_path,
        mime_type: mime.to_string(),
        content,
        size: file_size,
        truncated,
    })
}

#[tauri::command]
pub async fn write_text_file_preview(
    path: String,
    content: String,
) -> AppResult<TextFileWriteResult> {
    let (display_path, file_path) = resolve_existing_file_path(&path)?;
    let metadata = fs::metadata(&file_path)
        .map_err(|e| AppError::Custom(format!("Failed to read file metadata: {e}")))?;
    // 为防止“读取时已截断但保存时覆盖整文件”的误写，这里限制仅允许处理预览上限内的文本文件。
    if metadata.len() > MAX_TEXT_PREVIEW_SIZE {
        return Err(AppError::Custom(format!(
            "File is larger than preview limit ({} bytes), save blocked for safety.",
            MAX_TEXT_PREVIEW_SIZE
        )));
    }

    let bytes = content.as_bytes();
    if bytes.len() as u64 > MAX_TEXT_PREVIEW_WRITE_SIZE {
        return Err(AppError::Custom(format!(
            "Edited content exceeds safety limit ({} bytes).",
            MAX_TEXT_PREVIEW_WRITE_SIZE
        )));
    }

    fs::write(&file_path, bytes)
        .map_err(|e| AppError::Custom(format!("Failed to write file: {e}")))?;

    Ok(TextFileWriteResult {
        path: display_path,
        size: bytes.len() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn normalize_browser_url_defaults_to_about_blank() {
        assert_eq!(normalize_browser_url(None).unwrap().as_str(), "about:blank");
        assert_eq!(
            normalize_browser_url(Some("   ")).unwrap().as_str(),
            "about:blank"
        );
    }

    #[test]
    fn normalize_browser_url_adds_expected_scheme() {
        assert_eq!(
            normalize_browser_url(Some("localhost:1420"))
                .unwrap()
                .as_str(),
            "http://localhost:1420/"
        );
        assert_eq!(
            normalize_browser_url(Some("example.com")).unwrap().as_str(),
            "https://example.com/"
        );
    }

    #[test]
    fn normalize_browser_url_accepts_file_scheme() {
        let parsed = normalize_browser_url(Some("file:///C:/secret/index.html")).unwrap();
        assert_eq!(parsed.scheme(), "file");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn normalize_browser_url_converts_windows_absolute_path_to_file_url() {
        let parsed = normalize_browser_url(Some(r"D:\cn-codex\index.html")).unwrap();
        assert_eq!(parsed.scheme(), "file");
        assert_eq!(
            parsed.to_file_path().unwrap(),
            std::path::PathBuf::from(r"D:\cn-codex\index.html")
        );
    }

    #[test]
    fn normalize_browser_url_rejects_unsupported_schemes() {
        let err = normalize_browser_url(Some("ftp://example.com/resource")).unwrap_err();
        assert!(err.to_string().contains("Unsupported browser URL scheme"));
    }

    #[test]
    fn browser_cdp_endpoint_uses_loopback_debug_port() {
        assert_eq!(browser_cdp_endpoint(), "http://127.0.0.1:9242");
        assert!(browser_additional_args().contains("--remote-debugging-port=9242"));
    }

    #[test]
    fn normalize_windows_verbatim_prefix_strips_verbatim_prefix() {
        assert_eq!(
            normalize_windows_verbatim_prefix(r"\\?\E:\work\cn-codex\codey"),
            r"E:\work\cn-codex\codey"
        );
        assert_eq!(
            normalize_windows_verbatim_prefix(r"\\?\UNC\server\share\folder"),
            r"\\server\share\folder"
        );
    }

    #[test]
    fn resolve_existing_file_path_accepts_file() {
        let temp_dir = std::env::temp_dir().join(format!(
            "cn_codex_window_test_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        fs::create_dir_all(&temp_dir).unwrap();
        let file_path = temp_dir.join("sample.txt");
        fs::write(&file_path, b"hello").unwrap();

        let (_, resolved) = resolve_existing_file_path(&file_path.to_string_lossy()).unwrap();
        assert!(resolved.is_file());

        fs::remove_file(&file_path).unwrap();
        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn resolve_existing_file_path_rejects_directory() {
        let temp_dir = std::env::temp_dir().join(format!(
            "cn_codex_window_test_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        fs::create_dir_all(&temp_dir).unwrap();

        let err = resolve_existing_file_path(&temp_dir.to_string_lossy()).unwrap_err();
        assert!(err.to_string().contains("Path is not a file"));

        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn resolve_workspace_web_source_path_accepts_local_workspace_file() {
        let temp_dir = std::env::temp_dir().join(format!(
            "cn_codex_window_test_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        fs::create_dir_all(&temp_dir).unwrap();
        let html_path = temp_dir.join("index.html");
        fs::write(&html_path, "<html><body>ok</body></html>").unwrap();

        let parsed = Url::parse("http://localhost:5173/").unwrap();
        let resolved = resolve_workspace_web_source_path(&temp_dir, &parsed).unwrap();
        assert!(resolved.ends_with("index.html"));

        fs::remove_file(&html_path).unwrap();
        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn resolve_workspace_web_source_path_accepts_file_scheme_url() {
        let temp_dir = std::env::temp_dir().join(format!(
            "cn_codex_window_test_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        fs::create_dir_all(&temp_dir).unwrap();
        let html_path = temp_dir.join("index.html");
        fs::write(&html_path, "<html><body>ok</body></html>").unwrap();

        let parsed = Url::from_file_path(&html_path).unwrap();
        let resolved = resolve_workspace_web_source_path(&temp_dir, &parsed).unwrap();
        assert!(resolved.ends_with("index.html"));

        fs::remove_file(&html_path).unwrap();
        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn resolve_workspace_web_source_path_rejects_remote_host() {
        let temp_dir = std::env::temp_dir().join(format!(
            "cn_codex_window_test_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        fs::create_dir_all(&temp_dir).unwrap();
        fs::write(temp_dir.join("index.html"), "<html></html>").unwrap();

        let parsed = Url::parse("https://example.com/").unwrap();
        assert!(resolve_workspace_web_source_path(&temp_dir, &parsed).is_none());

        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn resolve_workspace_web_source_path_rejects_outside_workspace_file_url() {
        let temp_dir = std::env::temp_dir().join(format!(
            "cn_codex_window_test_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        fs::create_dir_all(&temp_dir).unwrap();

        let outside_file = std::env::temp_dir().join(format!(
            "cn_codex_window_outside_file_{}.html",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        fs::write(&outside_file, "<html></html>").unwrap();

        let parsed = Url::from_file_path(&outside_file).unwrap();
        assert!(resolve_workspace_web_source_path(&temp_dir, &parsed).is_none());

        fs::remove_file(outside_file).unwrap();
        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn resolve_workspace_web_source_path_rejects_outside_workspace_query_file() {
        let temp_dir = std::env::temp_dir().join(format!(
            "cn_codex_window_test_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        fs::create_dir_all(&temp_dir).unwrap();

        let outside_file = std::env::temp_dir().join(format!(
            "cn_codex_window_outside_{}.html",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        fs::write(&outside_file, "<html></html>").unwrap();

        let query_value = outside_file.to_string_lossy().replace('\\', "/");
        let parsed = Url::parse(&format!("http://localhost:1420/?file={query_value}")).unwrap();
        assert!(resolve_workspace_web_source_path(&temp_dir, &parsed).is_none());

        fs::remove_file(outside_file).unwrap();
        fs::remove_dir_all(&temp_dir).unwrap();
    }
}
