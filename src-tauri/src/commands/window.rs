use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize,
    State, Url, WebviewUrl, WebviewWindowBuilder, Window, webview::WebviewBuilder,
};
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::error::{AppError, AppResult};
use crate::state::{AppState, DocumentDetailSession, RunSummaryDiffPayload};

const BROWSER_WEBVIEW_LABEL: &str = "cn-browser";
const BROWSER_POPUP_WINDOW_LABEL: &str = "cn-browser-popup";
const BROWSER_DEBUG_PORT: u16 = 9242;
const BROWSER_ENDPOINT_FILE: &str = "visible-browser.json";
const BROWSER_DETACHED_CHANGED_EVENT: &str = "browser-detached-changed";
const BROWSER_POPUP_CLOSED_EVENT: &str = "browser-popup-closed";
const BROWSER_NAVIGATION_CHANGED_EVENT: &str = "browser-navigation-changed";
const BROWSER_POPUP_WEBVIEW_TOP_GAP: f64 = 38.0;
const DOCUMENT_DETAIL_WINDOW_LABEL_PREFIX: &str = "document-detail";
const DOCUMENT_DETAIL_OPEN_EVENT: &str = "document-detail-open";
const DOCUMENT_DETAIL_INSERT_EVENT: &str = "document-detail-insert-snippet";
const DOCUMENT_DETAIL_DEFAULT_WIDTH: f64 = 1060.0;
const DOCUMENT_DETAIL_DEFAULT_HEIGHT: f64 = 760.0;
const RUNSUMMARY_DIFF_WINDOW_LABEL: &str = "runsummary-diff";
const RUNSUMMARY_DIFF_OPEN_EVENT: &str = "runsummary-diff-open";
const COMPUTER_USE_OVERLAY_WINDOW_LABEL: &str = "computer-use-overlay";
const COMPUTER_USE_OVERLAY_STATE_EVENT: &str = "computer-use-overlay-state";
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerUseOverlayWindowInfo {
    pub label: String,
    pub active: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserNavigationState {
    pub url: String,
    pub title: String,
    pub can_go_back: bool,
    pub can_go_forward: bool,
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
pub async fn window_set_computer_use_overlay(
    app: AppHandle,
    active: bool,
) -> AppResult<ComputerUseOverlayWindowInfo> {
    if !active {
        if let Some(window) = app.get_webview_window(COMPUTER_USE_OVERLAY_WINDOW_LABEL) {
            // 直接关闭窗口，避免 hide 后仍残留 always-on-top 透明层。
            let _ = window.set_always_on_top(false);
            let _ = window.hide();
            let _ = window.close();
        }
        let _ = app.emit(
            COMPUTER_USE_OVERLAY_STATE_EVENT,
            serde_json::json!({ "active": false }),
        );
        return Ok(ComputerUseOverlayWindowInfo {
            label: COMPUTER_USE_OVERLAY_WINDOW_LABEL.to_string(),
            active: false,
            created: false,
        });
    }

    if let Some(window) = app.get_webview_window(COMPUTER_USE_OVERLAY_WINDOW_LABEL) {
        apply_computer_use_overlay_geometry(&app, &window)?;
        if !window.is_visible().unwrap_or(false) {
            window.show()?;
        }
        let _ = window.set_always_on_top(true);
        // 整屏层必须 click-through，否则会挡住桌面操控；关闭请用主窗 Esc 或自动结束。
        let _ = window.set_ignore_cursor_events(true);
        let _ = app.emit(
            COMPUTER_USE_OVERLAY_STATE_EVENT,
            serde_json::json!({ "active": true }),
        );
        return Ok(ComputerUseOverlayWindowInfo {
            label: COMPUTER_USE_OVERLAY_WINDOW_LABEL.to_string(),
            active: true,
            created: false,
        });
    }

    let overlay = WebviewWindowBuilder::new(
        &app,
        COMPUTER_USE_OVERLAY_WINDOW_LABEL,
        computer_use_overlay_window_url()?,
    )
    .title("Computer Use Overlay")
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .focused(false)
    .visible(false)
    .shadow(false)
    .build()
    .map_err(|error| {
        AppError::Custom(format!(
            "Failed to create computer use overlay window: {error}"
        ))
    })?;

    apply_computer_use_overlay_geometry(&app, &overlay)?;
    let _ = overlay.set_ignore_cursor_events(true);
    overlay.show()?;
    let _ = overlay.set_always_on_top(true);
    let _ = app.emit(
        COMPUTER_USE_OVERLAY_STATE_EVENT,
        serde_json::json!({ "active": true }),
    );

    // 新建 webview 时前端监听可能尚未就绪，短延迟后再广播一次，避免首帧丢状态。
    let app_for_retry = app.clone();
    tauri::async_runtime::spawn(async move {
        sleep(Duration::from_millis(120)).await;
        // 仅当覆盖窗仍存在时才补发 active=true，避免关闭后被延迟事件重新点亮。
        if app_for_retry
            .get_webview_window(COMPUTER_USE_OVERLAY_WINDOW_LABEL)
            .is_some()
        {
            let _ = app_for_retry.emit(
                COMPUTER_USE_OVERLAY_STATE_EVENT,
                serde_json::json!({ "active": true }),
            );
        }
    });

    Ok(ComputerUseOverlayWindowInfo {
        label: COMPUTER_USE_OVERLAY_WINDOW_LABEL.to_string(),
        active: true,
        created: true,
    })
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
        if let Some(webview) = app.get_webview(BROWSER_WEBVIEW_LABEL) {
            webview.close()?;
        }
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
    schedule_browser_navigation_state_sync(&app, Some(info.url.clone()));
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
        if should_preserve_popup_browser_geometry(&app, x, y, width, height) {
            return Ok(());
        }
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
    if let Some(webview) = app.get_webview(BROWSER_WEBVIEW_LABEL) {
        webview.navigate(browser_url.clone())?;
    } else {
        return Err(AppError::Custom(
            "Browser panel is not active. Open the browser tab first.".to_string(),
        ));
    }
    let url = browser_url.as_str().to_string();
    let info = BrowserWindowInfo {
        label: BROWSER_WEBVIEW_LABEL.to_string(),
        url: url.clone(),
        created: false,
        debug_port: BROWSER_DEBUG_PORT,
        cdp_endpoint: browser_cdp_endpoint(),
    };
    write_browser_endpoint_metadata(&state.workspace_config_dir, &info)?;
    let mut guard = state.browser_last_url.write().await;
    *guard = Some(url.clone());
    if workspace_root.is_some() {
        let mut guard = state.browser_active_root.write().await;
        *guard = normalize_workspace_root_hint(workspace_root);
    }
    schedule_browser_navigation_state_sync(&app, Some(url.clone()));
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
    schedule_browser_navigation_state_sync(&app, Some(info.url.clone()));
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
    if let Some(webview) = app.get_webview(BROWSER_WEBVIEW_LABEL) {
        webview.close()?;
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
    schedule_browser_navigation_state_sync(&app, Some(info.url.clone()));
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
pub async fn browser_refresh_preview(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<String> {
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
        let _ = emit_browser_navigation_state(&app, &state, None).await;
        return Ok(mode.as_str().unwrap_or("reload").to_string());
    }

    if let Some(webview) = app.get_webview(BROWSER_WEBVIEW_LABEL) {
        webview.eval("window.location.reload();")?;
        let _ = emit_browser_navigation_state(&app, &state, None).await;
        return Ok("reload".to_string());
    }

    Err(AppError::Custom(
        "No browser target available for preview refresh.".to_string(),
    ))
}

#[tauri::command]
pub async fn browser_get_navigation_state(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<BrowserNavigationState> {
    ensure_browser_webview(&app)?;
    emit_browser_navigation_state(&app, &state, None).await
}

#[tauri::command]
pub async fn browser_go_back(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<BrowserNavigationState> {
    ensure_browser_webview(&app)?;
    evaluate_browser_script(
        r#"
        (() => {
            history.back();
            return true;
        })()
        "#,
        true,
        None,
    )
    .await?;
    sleep(Duration::from_millis(120)).await;
    emit_browser_navigation_state(&app, &state, None).await
}

#[tauri::command]
pub async fn browser_go_forward(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<BrowserNavigationState> {
    ensure_browser_webview(&app)?;
    evaluate_browser_script(
        r#"
        (() => {
            history.forward();
            return true;
        })()
        "#,
        true,
        None,
    )
    .await?;
    sleep(Duration::from_millis(120)).await;
    emit_browser_navigation_state(&app, &state, None).await
}

#[tauri::command]
pub async fn browser_navigate_home(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<BrowserNavigationState> {
    ensure_browser_webview(&app)?;
    let browser_url = normalize_browser_url(Some("about:blank"))?;
    if let Some(webview) = app.get_webview(BROWSER_WEBVIEW_LABEL) {
        webview.navigate(browser_url.clone())?;
    }
    let url = browser_url.as_str().to_string();
    let info = BrowserWindowInfo {
        label: BROWSER_WEBVIEW_LABEL.to_string(),
        url: url.clone(),
        created: false,
        debug_port: BROWSER_DEBUG_PORT,
        cdp_endpoint: browser_cdp_endpoint(),
    };
    write_browser_endpoint_metadata(&state.workspace_config_dir, &info)?;
    let mut guard = state.browser_last_url.write().await;
    *guard = Some(url);
    sleep(Duration::from_millis(120)).await;
    emit_browser_navigation_state(&app, &state, Some(info.url.as_str())).await
}

fn is_document_detail_window_label(label: &str) -> bool {
    label == DOCUMENT_DETAIL_WINDOW_LABEL_PREFIX
        || label.starts_with(&format!("{DOCUMENT_DETAIL_WINDOW_LABEL_PREFIX}-"))
}

fn next_document_detail_window_label(app: &AppHandle) -> String {
    // 递增寻找空闲 label，允许多个详情窗并存。
    for index in 1u32..10_000 {
        let label = format!("{DOCUMENT_DETAIL_WINDOW_LABEL_PREFIX}-{index}");
        if app.get_webview_window(&label).is_none() {
            return label;
        }
    }
    format!(
        "{DOCUMENT_DETAIL_WINDOW_LABEL_PREFIX}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    )
}

fn paths_equal_for_detail(left: &str, right: &str) -> bool {
    let normalize = |value: &str| {
        let trimmed = value.trim().trim_end_matches(['/', '\\']);
        if cfg!(windows) {
            trimmed.replace('/', "\\").to_ascii_lowercase()
        } else {
            trimmed.replace('\\', "/").to_string()
        }
    };
    normalize(left) == normalize(right)
}

async fn find_document_detail_label_by_path(
    app: &AppHandle,
    state: &AppState,
    path: &str,
) -> Option<String> {
    let sessions = state.document_detail_sessions.read().await;
    for (label, session) in sessions.iter() {
        if !paths_equal_for_detail(&session.path, path) {
            continue;
        }
        if app.get_webview_window(label).is_some() {
            return Some(label.clone());
        }
    }
    None
}

#[tauri::command]
pub async fn window_open_document_detail(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    workspace_root: Option<String>,
    line: Option<u32>,
) -> AppResult<DocumentDetailWindowInfo> {
    let (display_path, _) = resolve_existing_file_path(&path)?;
    let active_root = normalize_workspace_root_hint(workspace_root);
    let active_line = line.filter(|value| *value > 0);
    let session = DocumentDetailSession {
        path: display_path.clone(),
        workspace_root: active_root.clone(),
        line: active_line,
    };

    // 同路径已打开：复用并聚焦，避免重复堆叠同一文件。
    if let Some(existing_label) =
        find_document_detail_label_by_path(&app, &state, &display_path).await
    {
        if let Some(window) = app.get_webview_window(&existing_label) {
            {
                let mut sessions = state.document_detail_sessions.write().await;
                sessions.insert(existing_label.clone(), session);
            }
            if !window.is_visible().unwrap_or(true) {
                window.show()?;
            }
            window.set_focus()?;
            let _ = app.emit_to(
                &existing_label,
                DOCUMENT_DETAIL_OPEN_EVENT,
                serde_json::json!({
                    "path": display_path.clone(),
                    "line": active_line,
                }),
            );
            return Ok(DocumentDetailWindowInfo {
                label: existing_label,
                path: display_path,
                created: false,
            });
        }
        // 会话表有残留但窗口已不在，清掉后继续新建。
        let mut sessions = state.document_detail_sessions.write().await;
        sessions.remove(&existing_label);
    }

    let label = next_document_detail_window_label(&app);
    {
        let mut sessions = state.document_detail_sessions.write().await;
        sessions.insert(label.clone(), session);
    }

    let open_count = {
        let sessions = state.document_detail_sessions.read().await;
        sessions.len().saturating_sub(1) as f64
    };
    let offset = (open_count % 8.0) * 28.0;

    let detail_window = WebviewWindowBuilder::new(&app, &label, document_detail_window_url()?)
        .title("文档详情")
        .inner_size(DOCUMENT_DETAIL_DEFAULT_WIDTH, DOCUMENT_DETAIL_DEFAULT_HEIGHT)
        .min_inner_size(760.0, 520.0)
        .position(72.0 + offset, 56.0 + offset)
        .resizable(true)
        .maximizable(true)
        .decorations(false)
        .build()?;

    detail_window.show()?;
    detail_window.set_focus()?;

    let sessions = state.document_detail_sessions.clone();
    let closed_label = label.clone();
    detail_window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            let sessions = sessions.clone();
            let closed_label = closed_label.clone();
            tauri::async_runtime::spawn(async move {
                let mut guard = sessions.write().await;
                guard.remove(&closed_label);
            });
        }
    });

    Ok(DocumentDetailWindowInfo {
        label,
        path: display_path,
        created: true,
    })
}

#[tauri::command]
pub async fn window_close_document_detail(
    window: Window,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let label = window.label().to_string();
    if is_document_detail_window_label(&label) {
        window.close()?;
        let mut sessions = state.document_detail_sessions.write().await;
        sessions.remove(&label);
    }
    Ok(())
}

#[tauri::command]
pub async fn window_get_document_detail_path(
    window: Window,
    state: State<'_, AppState>,
) -> AppResult<Option<String>> {
    let label = window.label().to_string();
    Ok(state
        .document_detail_sessions
        .read()
        .await
        .get(&label)
        .map(|session| session.path.clone()))
}

#[tauri::command]
pub async fn window_get_document_detail_line(
    window: Window,
    state: State<'_, AppState>,
) -> AppResult<Option<u32>> {
    let label = window.label().to_string();
    Ok(state
        .document_detail_sessions
        .read()
        .await
        .get(&label)
        .and_then(|session| session.line))
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
    // 独立浏览器窗口打开时，后续 automation/browser_run 不能把 WebView 抢回主窗口。
    let host_label = if app.get_webview_window(BROWSER_POPUP_WINDOW_LABEL).is_some() {
        BROWSER_POPUP_WINDOW_LABEL
    } else {
        "main"
    };

    let (next_x, next_y, next_width, next_height) =
        resolve_browser_geometry_for_host(app, host_label, x, y, width, height);

    open_browser_in_window(
        app,
        workspace_config_dir,
        host_label,
        url,
        next_x,
        next_y,
        next_width,
        next_height,
    )
}

fn open_browser_in_window(
    app: &AppHandle,
    workspace_config_dir: &Path,
    parent_window_label: &str,
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
        // 分离窗口场景下，忽略主窗口/automation 的 offscreen 隐藏坐标，避免布局被挤坏。
        if !should_preserve_popup_browser_geometry(app, x, y, width, height) {
            webview.set_position(LogicalPosition::new(x, y))?;
            webview.set_size(LogicalSize::new(width, height))?;
        }
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

    let host_window = app.get_window(parent_window_label).ok_or_else(|| {
        AppError::Custom(format!(
            "Browser host window '{parent_window_label}' not found"
        ))
    })?;

    let webview_builder =
        WebviewBuilder::new(BROWSER_WEBVIEW_LABEL, WebviewUrl::External(browser_url))
            .enable_clipboard_access()
            .data_directory(browser_dir.join("webview-data"))
            .additional_browser_args(&browser_additional_args());

    host_window.add_child(
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

/// Prefer IPv4 loopback; keep IPv6 as fallback for WebView2 builds that only
/// bind CDP to `::1` when `--remote-debugging-address` is not honored.
pub fn browser_cdp_endpoint_candidates() -> Vec<String> {
    vec![
        format!("http://127.0.0.1:{BROWSER_DEBUG_PORT}"),
        format!("http://[::1]:{BROWSER_DEBUG_PORT}"),
    ]
}

fn browser_additional_args() -> String {
    // Force IPv4 binding. Without this, some WebView2/Edge builds only listen on ::1,
    // while CN-Codex clients connect to 127.0.0.1 and fail with WEBVIEW_CDP_UNAVAILABLE.
    format!(
        "--remote-debugging-port={BROWSER_DEBUG_PORT} --remote-debugging-address=127.0.0.1 --remote-allow-origins=*"
    )
}

fn open_browser_popup(
    app: &AppHandle,
    workspace_config_dir: &Path,
    browser_last_url: &std::sync::Arc<tokio::sync::RwLock<Option<String>>>,
    url: &str,
    width: f64,
    height: f64,
) -> AppResult<BrowserWindowInfo> {
    let mut popup_created = false;
    if let Some(window) = app.get_webview_window(BROWSER_POPUP_WINDOW_LABEL) {
        window.set_size(LogicalSize::new(width, height))?;
        if !window.is_visible().unwrap_or(true) {
            window.show()?;
        }
        window.set_focus()?;
    } else {
        let popup =
            WebviewWindowBuilder::new(app, BROWSER_POPUP_WINDOW_LABEL, browser_popup_window_url()?)
                .title("Browser")
                .inner_size(width, height)
                .min_inner_size(640.0, 480.0)
                .resizable(true)
                .decorations(false)
                .build()?;
        popup.show()?;
        popup.set_focus()?;
        popup_created = true;

        let app_handle = app.clone();
        let last_url = browser_last_url.clone();
        popup.on_window_event(move |event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                let app_handle = app_handle.clone();
                let last_url = last_url.clone();
                tauri::async_runtime::spawn(async move {
                    if let Some(webview) = app_handle.get_webview(BROWSER_WEBVIEW_LABEL) {
                        let _ = webview.close();
                    }
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
    }

    let webview_info = open_browser_in_window(
        app,
        workspace_config_dir,
        BROWSER_POPUP_WINDOW_LABEL,
        Some(url),
        0.0,
        BROWSER_POPUP_WEBVIEW_TOP_GAP,
        width.max(320.0),
        (height - BROWSER_POPUP_WEBVIEW_TOP_GAP).max(180.0),
    )?;
    let info = BrowserWindowInfo {
        label: BROWSER_POPUP_WINDOW_LABEL.to_string(),
        url: webview_info.url.clone(),
        created: popup_created || webview_info.created,
        debug_port: webview_info.debug_port,
        cdp_endpoint: webview_info.cdp_endpoint.clone(),
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

fn is_browser_popup_open(app: &AppHandle) -> bool {
    app.get_webview_window(BROWSER_POPUP_WINDOW_LABEL).is_some()
}

fn is_offscreen_browser_geometry(x: f64, y: f64, width: f64, height: f64) -> bool {
    x <= -1000.0 || y <= -1000.0 || width <= 1.0 || height <= 1.0
}

fn is_popup_local_browser_geometry(x: f64, y: f64, width: f64, height: f64) -> bool {
    // popup 工具栏高度固定 38px，内容区从左上角附近开始。
    // 主窗口右侧面板坐标通常 x 很大，不能套到独立窗口上。
    x >= -1.0 && x <= 48.0 && y >= 20.0 && y <= 96.0 && width >= 200.0 && height >= 120.0
}

fn should_preserve_popup_browser_geometry(
    app: &AppHandle,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> bool {
    if !is_browser_popup_open(app) {
        return false;
    }
    is_offscreen_browser_geometry(x, y, width, height)
        || !is_popup_local_browser_geometry(x, y, width, height)
}

fn resolve_browser_geometry_for_host(
    app: &AppHandle,
    host_label: &str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> (f64, f64, f64, f64) {
    if host_label != BROWSER_POPUP_WINDOW_LABEL {
        return (x, y, width, height);
    }

    // 独立窗口已打开时，只接受 popup 本地坐标；其他坐标都回落到 popup 可用区域。
    if is_popup_local_browser_geometry(x, y, width, height) {
        return (x, y, width, height);
    }

    if let Some(window) = app.get_webview_window(BROWSER_POPUP_WINDOW_LABEL) {
        if let Ok(size) = window.inner_size() {
            let scale = window.scale_factor().unwrap_or(1.0).max(0.1);
            let logical_width = (size.width as f64 / scale).max(320.0);
            let logical_height = (size.height as f64 / scale).max(220.0);
            return (
                0.0,
                BROWSER_POPUP_WEBVIEW_TOP_GAP,
                logical_width.max(320.0),
                (logical_height - BROWSER_POPUP_WEBVIEW_TOP_GAP).max(180.0),
            );
        }
    }

    (
        0.0,
        BROWSER_POPUP_WEBVIEW_TOP_GAP,
        width.max(320.0),
        (height - BROWSER_POPUP_WEBVIEW_TOP_GAP).max(180.0),
    )
}

fn ensure_browser_webview(app: &AppHandle) -> AppResult<()> {
    if app.get_webview(BROWSER_WEBVIEW_LABEL).is_some() {
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

async fn read_browser_navigation_state(
    preferred_url: Option<&str>,
) -> AppResult<BrowserNavigationState> {
    match read_browser_navigation_state_via_history(preferred_url).await {
        Ok(state) => Ok(state),
        Err(_) => read_browser_navigation_state_via_eval(preferred_url).await,
    }
}

async fn read_browser_navigation_state_via_history(
    preferred_url: Option<&str>,
) -> AppResult<BrowserNavigationState> {
    let cdp_endpoint = browser_cdp_endpoint();
    let ws_url = browser_tab_websocket_url(&cdp_endpoint, preferred_url).await?;
    let (mut stream, _) = connect_async(&ws_url)
        .await
        .map_err(|e| AppError::Custom(format!("CDP connect failed: {e}")))?;

    cdp_send_command(&mut stream, 1, "Page.enable", json!({})).await?;
    let response = cdp_send_command(&mut stream, 2, "Page.getNavigationHistory", json!({})).await?;
    let history = response
        .get("result")
        .and_then(|value| value.get("result"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let entries = history
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if entries.is_empty() {
        return Err(AppError::Custom(
            "Browser navigation history is empty.".to_string(),
        ));
    }

    let current_index_raw = history
        .get("currentIndex")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    let current_index = current_index_raw.max(0) as usize;
    let selected_index = current_index.min(entries.len().saturating_sub(1));
    let current_entry = entries.get(selected_index).cloned().unwrap_or_default();
    let url = current_entry
        .get("url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("about:blank")
        .to_string();
    let title = current_entry
        .get("title")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();

    Ok(BrowserNavigationState {
        url,
        title,
        can_go_back: selected_index > 0,
        can_go_forward: selected_index + 1 < entries.len(),
    })
}

async fn read_browser_navigation_state_via_eval(
    preferred_url: Option<&str>,
) -> AppResult<BrowserNavigationState> {
    let value = evaluate_browser_script(
        r#"
        (() => {
            return {
                url: location.href || "about:blank",
                title: document.title || "",
                canGoBack: Boolean(history.length > 1),
                canGoForward: false,
            };
        })()
        "#,
        true,
        preferred_url,
    )
    .await?;
    let url = value
        .get("url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("about:blank")
        .trim()
        .to_string();
    let title = value
        .get("title")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    let can_go_back = value
        .get("canGoBack")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let can_go_forward = value
        .get("canGoForward")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    Ok(BrowserNavigationState {
        url: if url.is_empty() {
            "about:blank".to_string()
        } else {
            url
        },
        title,
        can_go_back,
        can_go_forward,
    })
}

async fn emit_browser_navigation_state(
    app: &AppHandle,
    state: &State<'_, AppState>,
    preferred_url: Option<&str>,
) -> AppResult<BrowserNavigationState> {
    let navigation = read_browser_navigation_state(preferred_url).await?;
    {
        let mut guard = state.browser_last_url.write().await;
        *guard = Some(navigation.url.clone());
    }
    let _ = app.emit(BROWSER_NAVIGATION_CHANGED_EVENT, navigation.clone());
    Ok(navigation)
}

fn schedule_browser_navigation_state_sync(app: &AppHandle, preferred_url: Option<String>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // A newly-created WebView2 may need a short amount of time before its CDP
        // endpoint exposes a page target. Do not make the open/navigation command
        // wait for that startup race; update the UI as soon as the target is ready.
        for attempt in 0..8u64 {
            if app.get_webview(BROWSER_WEBVIEW_LABEL).is_none() {
                return;
            }

            let state = app.state::<AppState>();
            if emit_browser_navigation_state(&app, &state, preferred_url.as_deref())
                .await
                .is_ok()
            {
                return;
            }

            sleep(Duration::from_millis(80 + attempt * 80)).await;
        }
    });
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
    let http = local_cdp_http_client();
    let mut last_error =
        AppError::Custom("Failed to query browser tabs: no CDP endpoint candidates".to_string());

    for endpoint in cdp_endpoint_candidates_for(cdp_endpoint) {
        match list_browser_tabs_at(&http, &endpoint).await {
            Ok(tabs) => return Ok(tabs),
            Err(error) => last_error = error,
        }
    }

    Err(last_error)
}

fn local_cdp_http_client() -> reqwest::Client {
    // Local CDP must never go through system/env proxies; proxies often return 502
    // for loopback debugging ports and surface as "Failed to query browser tabs: HTTP 502".
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

fn cdp_endpoint_candidates_for(preferred: &str) -> Vec<String> {
    let preferred = preferred.trim().trim_end_matches('/').to_string();
    let mut candidates = Vec::new();
    if !preferred.is_empty() {
        candidates.push(preferred);
    }
    for candidate in browser_cdp_endpoint_candidates() {
        if !candidates.iter().any(|existing| existing == &candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}

async fn list_browser_tabs_at(
    http: &reqwest::Client,
    cdp_endpoint: &str,
) -> AppResult<Vec<RemoteTabInfo>> {
    let response = http
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

fn resolve_existing_path(path: &str) -> AppResult<(String, std::path::PathBuf)> {
    let display_path = normalize_windows_verbatim_prefix(path);
    let target_path = std::path::PathBuf::from(&display_path);
    if !target_path.exists() {
        return Err(AppError::Custom(format!(
            "Path does not exist: {display_path}"
        )));
    }
    Ok((display_path, target_path))
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
    ensure_workspace_path_access(state, file_path).await
}

async fn ensure_workspace_path_access(
    state: &State<'_, AppState>,
    target_path: &Path,
) -> AppResult<()> {
    let workspace_root = workspace_root_from_state(state).await?;
    let canonical_target = target_path
        .canonicalize()
        .unwrap_or_else(|_| target_path.to_path_buf());
    if canonical_target.starts_with(&workspace_root) {
        return Ok(());
    }

    Err(AppError::Custom(format!(
        "Path is outside workspace and cannot be edited: {}",
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

fn browser_popup_window_url() -> AppResult<WebviewUrl> {
    #[cfg(debug_assertions)]
    {
        let dev_url =
            std::env::var("TAURI_DEV_URL").unwrap_or_else(|_| "http://localhost:1420".to_string());
        let base = dev_url.trim().trim_end_matches('/');
        let final_url = format!("{base}/browser.html");
        let parsed = Url::parse(&final_url).map_err(|e| {
            AppError::Custom(format!("Invalid browser popup dev url '{final_url}': {e}"))
        })?;
        return Ok(WebviewUrl::External(parsed));
    }

    #[cfg(not(debug_assertions))]
    {
        Ok(WebviewUrl::App("browser.html".into()))
    }
}

fn computer_use_overlay_window_url() -> AppResult<WebviewUrl> {
    #[cfg(debug_assertions)]
    {
        let dev_url =
            std::env::var("TAURI_DEV_URL").unwrap_or_else(|_| "http://localhost:1420".to_string());
        let base = dev_url.trim().trim_end_matches('/');
        let final_url = format!("{base}/computer-use-overlay.html");
        let parsed = Url::parse(&final_url).map_err(|e| {
            AppError::Custom(format!(
                "Invalid computer use overlay dev url '{final_url}': {e}"
            ))
        })?;
        return Ok(WebviewUrl::External(parsed));
    }

    #[cfg(not(debug_assertions))]
    {
        Ok(WebviewUrl::App("computer-use-overlay.html".into()))
    }
}

fn apply_computer_use_overlay_geometry(
    app: &AppHandle,
    window: &tauri::WebviewWindow,
) -> AppResult<()> {
    // 优先贴合主窗口所在显示器，保证 Computer Use 外框覆盖“整块屏幕”而不是应用客户区。
    let monitor = app
        .get_webview_window("main")
        .and_then(|main| main.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());

    if let Some(monitor) = monitor {
        let position = monitor.position();
        let size = monitor.size();
        window.set_position(PhysicalPosition::new(position.x, position.y))?;
        window.set_size(PhysicalSize::new(size.width, size.height))?;
    } else {
        // 无法读取显示器信息时退回最大化，仍尽量覆盖整个工作区。
        let _ = window.maximize();
    }

    let _ = window.set_always_on_top(true);
    let _ = window.set_ignore_cursor_events(true);
    Ok(())
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

const DEFAULT_SEARCH_MAX_RESULTS: usize = 100;
const DEFAULT_SEARCH_MAX_FILES: usize = 8_000;
const DEFAULT_SEARCH_MAX_MATCHES_PER_FILE: usize = 5;
const DEFAULT_SEARCH_MAX_FILE_BYTES: u64 = 1024 * 1024;
const SEARCH_TEXT_EXTENSIONS: &[&str] = &[
    "ts",
    "tsx",
    "js",
    "jsx",
    "mjs",
    "cjs",
    "json",
    "jsonc",
    "md",
    "mdx",
    "txt",
    "log",
    "rs",
    "py",
    "go",
    "java",
    "c",
    "h",
    "cpp",
    "cc",
    "cxx",
    "hpp",
    "cs",
    "kt",
    "swift",
    "html",
    "htm",
    "css",
    "scss",
    "less",
    "sass",
    "vue",
    "svelte",
    "xml",
    "yml",
    "yaml",
    "toml",
    "ini",
    "cfg",
    "conf",
    "env",
    "sql",
    "sh",
    "bash",
    "zsh",
    "ps1",
    "bat",
    "cmd",
    "csv",
    "tsv",
    "graphql",
    "gql",
    "proto",
    "rb",
    "php",
    "lua",
    "r",
    "dart",
    "scala",
    "dockerfile",
    "makefile",
    "cmake",
    "gradle",
    "properties",
    "gitignore",
    "editorconfig",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSearchMatch {
    pub path: String,
    pub name: String,
    pub relative_path: String,
    /// "name" = 文件名匹配；"content" = 文件内容匹配。
    pub kind: String,
    pub line: Option<u32>,
    pub preview: Option<String>,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSearchResult {
    pub query: String,
    pub matches: Vec<WorkspaceSearchMatch>,
    pub truncated: bool,
    pub searched_files: u32,
}

fn is_hidden_or_ignored_name(name: &str) -> bool {
    if name == "." || name == ".." {
        return true;
    }
    if HIDDEN_DIRS.contains(&name) {
        return true;
    }
    // 常见构建/缓存目录，避免内容搜索扫到巨型依赖树。
    matches!(
        name,
        "build"
            | "out"
            | "coverage"
            | ".cache"
            | ".turbo"
            | ".parcel-cache"
            | ".vite"
            | "vendor"
            | "Pods"
            | "DerivedData"
            | ".cn-codex"
            | "logs"
            | "release"
            | "publish"
            | "mobile-dist"
            | "node_modules"
            | "target"
            | "dist"
    )
}

fn is_probably_text_file(path: &Path) -> bool {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let lower_name = file_name.to_ascii_lowercase();
    if matches!(
        lower_name.as_str(),
        "dockerfile"
            | "makefile"
            | "cmakelists.txt"
            | "license"
            | "readme"
            | "cargo.lock"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
    ) {
        return true;
    }
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if ext.is_empty() {
        // 无扩展名时允许尝试读取小文件，后续仍会做二进制检测。
        return true;
    }
    SEARCH_TEXT_EXTENSIONS.iter().any(|item| *item == ext)
}

fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(4096).any(|b| *b == 0)
}

fn relative_path_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"))
}

fn clip_preview(line: &str, query: &str, case_sensitive: bool) -> String {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let max_len = 160usize;
    if trimmed.chars().count() <= max_len {
        return trimmed.to_string();
    }
    let match_idx = if case_sensitive {
        trimmed.find(query).unwrap_or(0)
    } else {
        trimmed
            .to_lowercase()
            .find(&query.to_lowercase())
            .unwrap_or(0)
    };
    // 以匹配位置为中心按字符截取，避免按字节切分中文导致 panic。
    let char_match = trimmed[..match_idx].chars().count();
    let chars: Vec<char> = trimmed.chars().collect();
    let start = char_match.saturating_sub(40);
    let end = (start + max_len).min(chars.len());
    let mut preview: String = chars[start..end].iter().collect();
    if start > 0 {
        preview = format!("…{preview}");
    }
    if end < chars.len() {
        preview = format!("{preview}…");
    }
    preview
}

fn text_contains(haystack: &str, needle: &str, case_sensitive: bool) -> bool {
    if needle.is_empty() {
        return false;
    }
    if case_sensitive {
        haystack.contains(needle)
    } else {
        haystack.to_lowercase().contains(needle)
    }
}

fn matches_include_filter(relative_path: &str, include: Option<&str>) -> bool {
    let Some(filter) = include else {
        return true;
    };
    let normalized_path = relative_path.replace('\\', "/").to_lowercase();
    let normalized_filter = filter.replace('\\', "/").to_lowercase();
    if normalized_filter.starts_with("*.") {
        return normalized_path.ends_with(&normalized_filter[1..]);
    }
    if normalized_filter.starts_with('.') && !normalized_filter.contains('/') {
        return normalized_path.ends_with(&normalized_filter);
    }
    normalized_path.contains(&normalized_filter)
}

fn collect_workspace_search(
    root: &Path,
    query: &str,
    max_results: usize,
    max_files: usize,
    max_matches_per_file: usize,
    max_file_bytes: u64,
    case_sensitive: bool,
    include: Option<&str>,
) -> WorkspaceSearchResult {
    let query_trimmed = query.trim();
    let query_for_match = if case_sensitive {
        query_trimmed.to_string()
    } else {
        query_trimmed.to_lowercase()
    };
    let include_filter = include
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.replace('\\', "/").to_lowercase());
    let mut matches = Vec::new();
    let mut truncated = false;
    let mut searched_files = 0u32;

    if query_for_match.is_empty() {
        return WorkspaceSearchResult {
            query: query_trimmed.to_string(),
            matches,
            truncated: false,
            searched_files: 0,
        };
    }

    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if matches.len() >= max_results {
            truncated = true;
            break;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        let mut dirs = Vec::new();
        let mut files = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if is_hidden_or_ignored_name(&name) {
                continue;
            }
            let path = entry.path();
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                dirs.push(path);
            } else if metadata.is_file() {
                files.push((path, metadata.len()));
            }
        }

        // 目录先入栈，保证深度优先且相对稳定。
        dirs.sort_by(|a, b| {
            a.file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default()
                .cmp(
                    &b.file_name()
                        .map(|n| n.to_string_lossy().to_lowercase())
                        .unwrap_or_default(),
                )
        });
        for path in dirs.into_iter().rev() {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let relative = relative_path_display(root, &path);
            if matches_include_filter(&relative, include_filter.as_deref())
                && text_contains(&name, &query_for_match, case_sensitive)
                && matches.len() < max_results
            {
                matches.push(WorkspaceSearchMatch {
                    path: super::normalize_windows_verbatim_prefix(&path.to_string_lossy()),
                    name: name.clone(),
                    relative_path: relative,
                    kind: "name".to_string(),
                    line: None,
                    preview: None,
                    is_dir: true,
                });
            }
            stack.push(path);
        }

        files.sort_by(|a, b| {
            a.0.file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default()
                .cmp(
                    &b.0.file_name()
                        .map(|n| n.to_string_lossy().to_lowercase())
                        .unwrap_or_default(),
                )
        });

        for (path, size) in files {
            if matches.len() >= max_results {
                truncated = true;
                break;
            }
            if searched_files as usize >= max_files {
                truncated = true;
                break;
            }

            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let abs_path = super::normalize_windows_verbatim_prefix(&path.to_string_lossy());
            let relative = relative_path_display(root, &path);

            if !matches_include_filter(&relative, include_filter.as_deref()) {
                continue;
            }

            if text_contains(&name, &query_for_match, case_sensitive) {
                matches.push(WorkspaceSearchMatch {
                    path: abs_path.clone(),
                    name: name.clone(),
                    relative_path: relative.clone(),
                    kind: "name".to_string(),
                    line: None,
                    preview: None,
                    is_dir: false,
                });
                if matches.len() >= max_results {
                    truncated = true;
                    break;
                }
            }

            if size == 0 || size > max_file_bytes || !is_probably_text_file(&path) {
                continue;
            }

            searched_files = searched_files.saturating_add(1);
            let Ok(bytes) = fs::read(&path) else {
                continue;
            };
            if looks_binary(&bytes) {
                continue;
            }
            let content = String::from_utf8_lossy(&bytes);
            let mut file_match_count = 0usize;
            for (idx, line) in content.lines().enumerate() {
                if text_contains(line, &query_for_match, case_sensitive) {
                    matches.push(WorkspaceSearchMatch {
                        path: abs_path.clone(),
                        name: name.clone(),
                        relative_path: relative.clone(),
                        kind: "content".to_string(),
                        line: Some((idx + 1) as u32),
                        preview: Some(clip_preview(line, &query_for_match, case_sensitive)),
                        is_dir: false,
                    });
                    file_match_count += 1;
                    if matches.len() >= max_results {
                        truncated = true;
                        break;
                    }
                    if file_match_count >= max_matches_per_file {
                        break;
                    }
                }
            }
        }
    }

    WorkspaceSearchResult {
        query: query_trimmed.to_string(),
        matches,
        truncated,
        searched_files,
    }
}

/// 在工作区中按文件名和文件内容搜索，类似 Cursor 的内容检索。
#[tauri::command]
pub async fn search_workspace_files(
    root: String,
    query: String,
    max_results: Option<u32>,
    case_sensitive: Option<bool>,
    include: Option<String>,
) -> AppResult<WorkspaceSearchResult> {
    let display_root = normalize_windows_verbatim_prefix(&root);
    let root_path = PathBuf::from(&display_root);
    if !root_path.is_dir() {
        return Err(AppError::Custom(format!(
            "Path is not a directory: {display_root}"
        )));
    }

    let max_results = max_results
        .map(|v| v as usize)
        .unwrap_or(DEFAULT_SEARCH_MAX_RESULTS)
        .clamp(1, 500);
    let case_sensitive = case_sensitive.unwrap_or(false);
    let include = include
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    // 同步扫盘放到阻塞线程，避免卡住 async runtime。
    let result = tokio::task::spawn_blocking(move || {
        collect_workspace_search(
            &root_path,
            &query,
            max_results,
            DEFAULT_SEARCH_MAX_FILES,
            DEFAULT_SEARCH_MAX_MATCHES_PER_FILE,
            DEFAULT_SEARCH_MAX_FILE_BYTES,
            case_sensitive,
            include.as_deref(),
        )
    })
    .await
    .map_err(|e| AppError::Custom(format!("Search task failed: {e}")))?;

    Ok(result)
}

#[tauri::command]
pub async fn delete_path(path: String, recursive: Option<bool>) -> AppResult<()> {
    let (display_path, target_path) = resolve_existing_path(&path)?;

    let metadata = fs::symlink_metadata(&target_path)
        .map_err(|e| AppError::Custom(format!("Failed to read path metadata: {e}")))?;

    if metadata.is_dir() {
        if !recursive.unwrap_or(false) {
            return Err(AppError::Custom(format!(
                "Refusing to delete directory without recursive confirmation: {display_path}"
            )));
        }
        fs::remove_dir_all(&target_path)
            .map_err(|e| AppError::Custom(format!("Failed to delete directory: {e}")))?;
    } else {
        fs::remove_file(&target_path)
            .map_err(|e| AppError::Custom(format!("Failed to delete file: {e}")))?;
    }

    Ok(())
}

fn sanitize_created_entry_name(name: &str) -> AppResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AppError::Custom("Name cannot be empty".into()));
    }
    if trimmed == "." || trimmed == ".." {
        return Err(AppError::Custom("Invalid name".into()));
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err(AppError::Custom(
            "Name cannot contain path separators".into(),
        ));
    }
    #[cfg(target_os = "windows")]
    {
        const INVALID: &[char] = &['<', '>', ':', '"', '|', '?', '*'];
        if trimmed.chars().any(|ch| INVALID.contains(&ch) || ch.is_control()) {
            return Err(AppError::Custom(
                "Name contains invalid characters".into(),
            ));
        }
    }
    Ok(trimmed.to_string())
}

fn file_entry_from_path(path: &std::path::Path) -> AppResult<FileEntry> {
    let metadata = fs::metadata(path)
        .map_err(|e| AppError::Custom(format!("Failed to read path metadata: {e}")))?;
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| path.to_string_lossy().to_string());
    Ok(FileEntry {
        name,
        path: super::normalize_windows_verbatim_prefix(&path.to_string_lossy()),
        is_dir: metadata.is_dir(),
        size: metadata.len(),
    })
}

fn copy_path_recursive(src: &std::path::Path, dest: &std::path::Path) -> AppResult<()> {
    let metadata = fs::symlink_metadata(src)
        .map_err(|e| AppError::Custom(format!("Failed to read source metadata: {e}")))?;
    if metadata.file_type().is_symlink() {
        return Err(AppError::Custom(
            "Copying symbolic links is not supported".into(),
        ));
    }
    if metadata.is_dir() {
        fs::create_dir_all(dest)
            .map_err(|e| AppError::Custom(format!("Failed to create directory: {e}")))?;
        for entry in fs::read_dir(src)
            .map_err(|e| AppError::Custom(format!("Failed to read directory: {e}")))?
        {
            let entry =
                entry.map_err(|e| AppError::Custom(format!("Failed to read directory entry: {e}")))?;
            let child_dest = dest.join(entry.file_name());
            copy_path_recursive(&entry.path(), &child_dest)?;
        }
        return Ok(());
    }

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| AppError::Custom(format!("Failed to create parent directory: {e}")))?;
    }
    fs::copy(src, dest).map_err(|e| AppError::Custom(format!("Failed to copy file: {e}")))?;
    Ok(())
}

fn ensure_destination_available(dest: &std::path::Path) -> AppResult<()> {
    if dest.exists() {
        return Err(AppError::Custom(format!(
            "Path already exists: {}",
            super::normalize_windows_verbatim_prefix(&dest.to_string_lossy())
        )));
    }
    Ok(())
}

fn ensure_not_moving_into_self(
    source: &std::path::Path,
    dest: &std::path::Path,
) -> AppResult<()> {
    let canonical_source = source.canonicalize().unwrap_or_else(|_| source.to_path_buf());
    let dest_parent = dest
        .parent()
        .map(|parent| parent.canonicalize().unwrap_or_else(|_| parent.to_path_buf()));
    if let Some(parent) = dest_parent {
        if parent.starts_with(&canonical_source) {
            return Err(AppError::Custom(
                "Cannot move or copy a folder into itself".into(),
            ));
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn create_path_entry(
    parent_dir: String,
    name: String,
    is_dir: bool,
) -> AppResult<FileEntry> {
    let safe_name = sanitize_created_entry_name(&name)?;
    let (display_parent, parent_path) = resolve_existing_path(&parent_dir)?;
    if !parent_path.is_dir() {
        return Err(AppError::Custom(format!(
            "Parent path is not a directory: {display_parent}"
        )));
    }

    let target_path = parent_path.join(&safe_name);
    ensure_destination_available(&target_path)?;

    if is_dir {
        fs::create_dir(&target_path)
            .map_err(|e| AppError::Custom(format!("Failed to create directory: {e}")))?;
    } else {
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| AppError::Custom(format!("Failed to create parent directory: {e}")))?;
        }
        fs::File::create(&target_path)
            .map_err(|e| AppError::Custom(format!("Failed to create file: {e}")))?;
    }

    file_entry_from_path(&target_path)
}

#[tauri::command]
pub async fn rename_path_entry(from: String, to: String) -> AppResult<FileEntry> {
    let (display_from, source_path) = resolve_existing_path(&from)?;
    let display_to = normalize_windows_verbatim_prefix(&to);
    let dest_path = std::path::PathBuf::from(&display_to);

    if let Some(name) = dest_path.file_name().and_then(|value| value.to_str()) {
        sanitize_created_entry_name(name)?;
    } else {
        return Err(AppError::Custom("Invalid destination path".into()));
    }

    if let Some(parent) = dest_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent)
                .map_err(|e| AppError::Custom(format!("Failed to create parent directory: {e}")))?;
        }
    }

    ensure_destination_available(&dest_path)?;
    ensure_not_moving_into_self(&source_path, &dest_path)?;

    fs::rename(&source_path, &dest_path).map_err(|e| {
        AppError::Custom(format!(
            "Failed to rename '{display_from}' -> '{display_to}': {e}"
        ))
    })?;

    file_entry_from_path(&dest_path)
}

#[tauri::command]
pub async fn copy_path_entry(from: String, to: String) -> AppResult<FileEntry> {
    let (display_from, source_path) = resolve_existing_path(&from)?;
    let display_to = normalize_windows_verbatim_prefix(&to);
    let dest_path = std::path::PathBuf::from(&display_to);

    if let Some(name) = dest_path.file_name().and_then(|value| value.to_str()) {
        sanitize_created_entry_name(name)?;
    } else {
        return Err(AppError::Custom("Invalid destination path".into()));
    }

    ensure_destination_available(&dest_path)?;
    ensure_not_moving_into_self(&source_path, &dest_path)?;

    copy_path_recursive(&source_path, &dest_path).map_err(|e| {
        AppError::Custom(format!(
            "Failed to copy '{display_from}' -> '{display_to}': {e}"
        ))
    })?;

    file_entry_from_path(&dest_path)
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
        assert_eq!(
            browser_cdp_endpoint_candidates(),
            vec![
                "http://127.0.0.1:9242".to_string(),
                "http://[::1]:9242".to_string(),
            ]
        );
        let args = browser_additional_args();
        assert!(args.contains("--remote-debugging-port=9242"));
        assert!(args.contains("--remote-debugging-address=127.0.0.1"));
        assert!(args.contains("--remote-allow-origins=*"));
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
    fn resolve_existing_path_accepts_directory() {
        let temp_dir = std::env::temp_dir().join(format!(
            "cn_codex_window_test_dir_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        fs::create_dir_all(&temp_dir).unwrap();

        let (_, resolved) = resolve_existing_path(&temp_dir.to_string_lossy()).unwrap();
        assert!(resolved.is_dir());

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

    #[test]
    fn collect_workspace_search_matches_name_and_content() {
        let temp_dir = std::env::temp_dir().join(format!(
            "cn_codex_search_test_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        fs::create_dir_all(temp_dir.join("src")).unwrap();
        fs::write(
            temp_dir.join("src").join("unique_search_target.rs"),
            "fn unique_search_marker() {}\n",
        )
        .unwrap();
        fs::create_dir_all(temp_dir.join("node_modules")).unwrap();
        fs::write(
            temp_dir.join("node_modules").join("ignored.rs"),
            "fn unique_search_marker() {}\n",
        )
        .unwrap();

        let result = collect_workspace_search(
            &temp_dir,
            "unique_search",
            50,
            1000,
            5,
            1024 * 1024,
            false,
            None,
        );
        assert!(
            result
                .matches
                .iter()
                .any(|item| { item.kind == "name" && item.name.contains("unique_search_target") })
        );
        assert!(result.matches.iter().any(|item| {
            item.kind == "content"
                && item.line == Some(1)
                && item
                    .preview
                    .as_deref()
                    .unwrap_or_default()
                    .contains("unique_search_marker")
        }));
        assert!(
            !result
                .matches
                .iter()
                .any(|item| { item.relative_path.contains("node_modules") })
        );

        fs::remove_dir_all(&temp_dir).unwrap();
    }
}
