use serde::Deserialize;
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

/// 系统提示音播放通道错误（Windows MessageBeep）。
#[derive(Debug, thiserror::Error)]
pub enum TaskDoneNotifyError {
    /// 播放系统提示音失败。
    #[error("failed to play task-done sound: {0}")]
    Sound(String),
}

impl serde::Serialize for TaskDoneNotifyError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[derive(Debug, Deserialize)]
pub struct TaskDoneNotifyRequest {
    /// 通知标题；为空时使用默认文案。
    #[serde(default)]
    pub title: Option<String>,
    /// 通知正文；为空时使用默认文案。
    #[serde(default)]
    pub body: Option<String>,
    /// 是否播放系统提示音，默认 true。
    #[serde(default = "default_true")]
    pub sound: bool,
    /// 失败音效：true 使用感叹号音，false 使用完成/确认音。
    #[serde(default)]
    pub failed: bool,
    /// 发出的前端事件名，用于测试与联动。
    #[serde(default)]
    pub event: Option<String>,
}

fn default_true() -> bool {
    true
}

const DEFAULT_TASK_DONE_TITLE: &str = "CN-Codex";
const DEFAULT_TASK_DONE_BODY_ZH: &str = "任务已完成";

/// 任务完成后弹出 Windows 右下角系统通知，并播放系统提示音。
///
/// - 通知复用 `tauri-plugin-notification` 的原生 Toast 能力；
/// - 声音通过 Win32 `MessageBeep` 播放系统事件音，无需自带音频资源；
/// - 非 Windows 平台跳过声音步骤。
#[tauri::command]
pub async fn notify_task_done(
    app: AppHandle,
    request: Option<TaskDoneNotifyRequest>,
) -> Result<(), TaskDoneNotifyError> {
    let request = request.unwrap_or(TaskDoneNotifyRequest {
        title: None,
        body: None,
        sound: default_true(),
        failed: false,
        event: None,
    });

    let title = request
        .title
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_TASK_DONE_TITLE.to_string());
    let body = request
        .body
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_TASK_DONE_BODY_ZH.to_string());

    // 先发声音再弹 Toast，确保“听到即看到”的体验一致。
    if request.sound {
        play_task_done_sound(request.failed)?;
    }

    // notification 插件已在应用启动时注册，这里通过 NotificationExt 直接构建 Toast。
    let mut builder = app
        .notification()
        .builder()
        .title(title)
        .body(body);
    // Windows 上允许传入 wav 路径自定义音效；未传则使用系统默认。
    if let Some(sound_path) = std::env::var("CN_CODEX_NOTIFY_SOUND")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        builder = builder.sound(&sound_path);
    }
    let _ = builder.show();

    if let Some(event) = request.event.as_deref().filter(|v| !v.trim().is_empty()) {
        use tauri::Emitter as _;
        let _ = app.emit(event, serde_json::json!({ "notifiedAt": chrono::Utc::now().timestamp_millis() }));
    }

    Ok(())
}

fn play_task_done_sound(failed: bool) -> Result<(), TaskDoneNotifyError> {
    #[cfg(target_os = "windows")]
    {
        const MB_ICONASTERISK: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE =
            windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE(0x00000040);
        const MB_ICONEXCLAMATION: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE =
            windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE(0x00000030);
        let style = if failed { MB_ICONEXCLAMATION } else { MB_ICONASTERISK };
        unsafe {
            windows::Win32::System::Diagnostics::Debug::MessageBeep(style)
                .map_err(|err| TaskDoneNotifyError::Sound(err.to_string()))?;
        }
        return Ok(());
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = failed;
        Ok(())
    }
}
