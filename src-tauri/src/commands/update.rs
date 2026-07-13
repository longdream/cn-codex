use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tracing::{info, warn};

use crate::error::{AppError, AppResult};

const DEFAULT_MANIFEST_URL: &str = "http://47.113.221.244:5005/latest.json";
const CHECK_TIMEOUT_SECS: u64 = 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    pub update_available: bool,
    pub current_version: String,
    pub latest_version: String,
    pub url: String,
    pub sha256: String,
    pub notes: String,
    pub force: bool,
    #[serde(default)]
    pub published_at: String,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStartResult {
    pub started: bool,
    pub message: String,
}

#[tauri::command]
pub async fn update_check() -> AppResult<UpdateCheckResult> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let updater_path = resolve_updater_path()?;
    if !updater_path.is_file() {
        return Ok(UpdateCheckResult {
            update_available: false,
            current_version,
            latest_version: String::new(),
            url: String::new(),
            sha256: String::new(),
            notes: String::new(),
            force: false,
            published_at: String::new(),
            message: format!("未找到 updater: {}", updater_path.display()),
        });
    }

    let output = tokio::process::Command::new(&updater_path)
        .args([
            "check",
            "--current-version",
            &current_version,
            "--manifest-url",
            DEFAULT_MANIFEST_URL,
            "--timeout-secs",
            &CHECK_TIMEOUT_SECS.to_string(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|err| AppError::Custom(format!("启动 updater 失败: {err}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if stdout.is_empty() {
        let msg = if stderr.is_empty() {
            format!(
                "updater 未返回结果 (exit={})",
                output.status.code().unwrap_or(-1)
            )
        } else {
            stderr
        };
        return Ok(UpdateCheckResult {
            update_available: false,
            current_version,
            latest_version: String::new(),
            url: String::new(),
            sha256: String::new(),
            notes: String::new(),
            force: false,
            published_at: String::new(),
            message: msg,
        });
    }

    match serde_json::from_str::<UpdateCheckResult>(&stdout) {
        Ok(mut result) => {
            if result.current_version.trim().is_empty() {
                result.current_version = current_version;
            }
            Ok(result)
        }
        Err(err) => {
            warn!("failed to parse updater output: {err}; stdout={stdout}");
            Ok(UpdateCheckResult {
                update_available: false,
                current_version,
                latest_version: String::new(),
                url: String::new(),
                sha256: String::new(),
                notes: String::new(),
                force: false,
                published_at: String::new(),
                message: format!("解析更新结果失败: {err}"),
            })
        }
    }
}

#[tauri::command]
pub async fn update_start(
    app: AppHandle,
    url: String,
    sha256: Option<String>,
) -> AppResult<UpdateStartResult> {
    let url = url.trim().to_string();
    if url.is_empty() {
        return Err(AppError::Custom("更新下载地址为空".into()));
    }

    let updater_path = resolve_updater_path()?;
    if !updater_path.is_file() {
        return Err(AppError::Custom(format!(
            "未找到 updater: {}",
            updater_path.display()
        )));
    }

    let target = current_main_exe_path()?;
    let pid = std::process::id();
    let sha = sha256.unwrap_or_default();

    let mut command = Command::new(&updater_path);
    command
        .arg("update")
        .arg("--url")
        .arg(&url)
        .arg("--target")
        .arg(&target)
        .arg("--pid")
        .arg(pid.to_string())
        .arg("--launch")
        .arg(&target);
    if !sha.trim().is_empty() {
        command.arg("--sha256").arg(sha.trim());
    }

    if let Some(parent) = updater_path.parent() {
        command.current_dir(parent);
    }

    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| AppError::Custom(format!("启动更新程序失败: {err}")))?;

    info!(
        "updater started for target {} with pid {pid}",
        target.display()
    );

    // Give updater a short moment to attach, then exit main process so file can be replaced.
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        app_handle.exit(0);
    });

    Ok(UpdateStartResult {
        started: true,
        message: "更新程序已启动，主程序即将退出".to_string(),
    })
}

fn resolve_updater_path() -> AppResult<PathBuf> {
    let exe = current_main_exe_path()?;
    let dir = exe
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| AppError::Custom("无法解析主程序目录".into()))?;

    #[cfg(windows)]
    let candidate = dir.join("updater.exe");
    #[cfg(not(windows))]
    let candidate = dir.join("updater");

    if candidate.is_file() {
        return Ok(candidate);
    }

    // Dev/debug fallback: same cargo target dir.
    if let Some(parent) = dir.parent() {
        #[cfg(windows)]
        let alt = parent.join("updater.exe");
        #[cfg(not(windows))]
        let alt = parent.join("updater");
        if alt.is_file() {
            return Ok(alt);
        }
    }

    Ok(candidate)
}

fn current_main_exe_path() -> AppResult<PathBuf> {
    std::env::current_exe().map_err(|err| AppError::Custom(format!("解析主程序路径失败: {err}")))
}
