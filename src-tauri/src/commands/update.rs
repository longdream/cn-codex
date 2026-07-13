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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LatestManifest {
    version: String,
    url: String,
    #[serde(default)]
    sha256: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    force: bool,
    #[serde(default, alias = "published_at")]
    published_at: String,
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
    match fetch_latest_manifest(DEFAULT_MANIFEST_URL, CHECK_TIMEOUT_SECS).await {
        Ok(manifest) => {
            let latest_version = manifest.version.trim().to_string();
            let update_available = is_newer_version(&latest_version, &current_version);
            let message = if update_available {
                format!("发现新版本 {latest_version}")
            } else {
                "当前已是最新版本".to_string()
            };
            info!(
                "update check via {}: current={}, latest={}, available={}",
                DEFAULT_MANIFEST_URL, current_version, latest_version, update_available
            );
            Ok(UpdateCheckResult {
                update_available,
                current_version,
                latest_version,
                url: manifest.url.trim().to_string(),
                sha256: manifest.sha256,
                notes: manifest.notes,
                force: manifest.force,
                published_at: manifest.published_at,
                message,
            })
        }
        Err(err) => {
            warn!("update check failed from {DEFAULT_MANIFEST_URL}: {err}");
            Ok(UpdateCheckResult {
                update_available: false,
                current_version,
                latest_version: String::new(),
                url: String::new(),
                sha256: String::new(),
                notes: String::new(),
                force: false,
                published_at: String::new(),
                message: format!("检查更新失败: {err}"),
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
        .map_err(|err| {
            AppError::Custom(format_updater_launch_error(
                "启动更新程序失败",
                &updater_path,
                &err,
            ))
        })?;

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

async fn fetch_latest_manifest(url: &str, timeout_secs: u64) -> Result<LatestManifest, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|err| format!("创建 HTTP 客户端失败: {err}"))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|err| format!("请求 {url} 失败: {err}"))?
        .error_for_status()
        .map_err(|err| format!("请求 {url} 返回错误: {err}"))?;

    response
        .json::<LatestManifest>()
        .await
        .map_err(|err| format!("解析 latest.json 失败: {err}"))
}

fn is_newer_version(latest: &str, current: &str) -> bool {
    parse_version(latest) > parse_version(current)
}

fn parse_version(raw: &str) -> (u64, u64, u64) {
    let cleaned = raw.trim().trim_start_matches(['v', 'V']);
    let mut parts = cleaned.split(|c| c == '.' || c == '-' || c == '+');
    let major = parts
        .next()
        .and_then(|part| part.parse::<u64>().ok())
        .unwrap_or(0);
    let minor = parts
        .next()
        .and_then(|part| part.parse::<u64>().ok())
        .unwrap_or(0);
    let patch = parts
        .next()
        .and_then(|part| part.parse::<u64>().ok())
        .unwrap_or(0);
    (major, minor, patch)
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

    // If previous zip update left a side-car updater, promote it now.
    promote_pending_updater(&candidate);

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

fn promote_pending_updater(candidate: &Path) {
    // Windows: updater.exe.new ; Unix: updater.new
    let mut pending_os = candidate.as_os_str().to_owned();
    pending_os.push(".new");
    let pending = PathBuf::from(pending_os);

    if !pending.is_file() {
        return;
    }

    // Best-effort: rename pending over current updater when not locked.
    if candidate.exists() {
        let backup = candidate.with_extension(format!(
            "{}.bak",
            candidate
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("bin")
        ));
        let _ = std::fs::remove_file(&backup);
        if std::fs::rename(candidate, &backup).is_ok() {
            if std::fs::rename(&pending, candidate).is_ok() {
                let _ = std::fs::remove_file(&backup);
                info!("promoted pending updater: {}", candidate.display());
                return;
            }
            let _ = std::fs::rename(&backup, candidate);
        }
    } else if std::fs::rename(&pending, candidate).is_ok() {
        info!("installed pending updater: {}", candidate.display());
    }
}

fn current_main_exe_path() -> AppResult<PathBuf> {
    std::env::current_exe().map_err(|err| AppError::Custom(format!("解析主程序路径失败: {err}")))
}

fn format_updater_launch_error(prefix: &str, updater_path: &Path, err: &std::io::Error) -> String {
    let base = format!("{prefix}: {err} ({})", updater_path.display());
    #[cfg(windows)]
    {
        // ERROR_ELEVATION_REQUIRED = 740
        if err.raw_os_error() == Some(740) {
            return format!(
                "{base}。Windows 把 updater 识别为需要管理员权限的安装程序。请使用已嵌入 asInvoker 清单的 updater.exe（重新 build/publish），或右键属性取消“以管理员身份运行”。"
            );
        }
    }
    base
}
