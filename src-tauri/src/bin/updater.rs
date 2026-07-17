#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::ZipArchive;

const DEFAULT_MANIFEST_URL: &str = "http://47.113.221.244:5005/latest.json";

#[derive(Debug, Parser)]
#[command(name = "updater", about = "CN-Codex portable updater")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Check remote latest.json against current version.
    Check {
        #[arg(long)]
        current_version: String,
        #[arg(long, default_value = DEFAULT_MANIFEST_URL)]
        manifest_url: String,
        #[arg(long, default_value_t = 12)]
        timeout_secs: u64,
    },
    /// Download package (zip preferred, exe fallback), replace files, then relaunch.
    Update {
        #[arg(long)]
        url: String,
        #[arg(long)]
        target: PathBuf,
        #[arg(long)]
        pid: u32,
        #[arg(long, default_value = "")]
        sha256: String,
        #[arg(long)]
        launch: Option<PathBuf>,
        #[arg(long, default_value_t = 120)]
        wait_secs: u64,
    },
}

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CheckResult {
    update_available: bool,
    current_version: String,
    latest_version: String,
    url: String,
    sha256: String,
    notes: String,
    force: bool,
    published_at: String,
    message: String,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let code = match cli.command {
        Commands::Check {
            current_version,
            manifest_url,
            timeout_secs,
        } => run_check(current_version, manifest_url, timeout_secs).await,
        Commands::Update {
            url,
            target,
            pid,
            sha256,
            launch,
            wait_secs,
        } => run_update(url, target, pid, sha256, launch, wait_secs).await,
    };

    std::process::exit(code);
}

async fn run_check(current_version: String, manifest_url: String, timeout_secs: u64) -> i32 {
    let result = match fetch_manifest(&manifest_url, timeout_secs).await {
        Ok(manifest) => {
            let latest = manifest.version.trim().to_string();
            let available = is_newer_version(&latest, &current_version);
            CheckResult {
                update_available: available,
                current_version: current_version.clone(),
                latest_version: latest.clone(),
                url: manifest.url,
                sha256: manifest.sha256,
                notes: manifest.notes,
                force: manifest.force,
                published_at: manifest.published_at,
                message: if available {
                    format!("发现新版本 {latest}")
                } else {
                    "当前已是最新版本".to_string()
                },
            }
        }
        Err(err) => CheckResult {
            update_available: false,
            current_version,
            latest_version: String::new(),
            url: String::new(),
            sha256: String::new(),
            notes: String::new(),
            force: false,
            published_at: String::new(),
            message: format!("检查更新失败: {err}"),
        },
    };

    match serde_json::to_string(&result) {
        Ok(json) => {
            println!("{json}");
            if result.message.starts_with("检查更新失败") {
                2
            } else {
                0
            }
        }
        Err(err) => {
            eprintln!("serialize check result failed: {err}");
            1
        }
    }
}

async fn run_update(
    url: String,
    target: PathBuf,
    pid: u32,
    sha256: String,
    launch: Option<PathBuf>,
    wait_secs: u64,
) -> i32 {
    let progress = ProgressUi::create("CN-Codex 更新");
    progress.set_status("正在等待主程序安全退出...");
    progress.set_progress(0);

    if let Err(err) = wait_for_process_exit(pid, wait_secs) {
        progress.fail(&format!("等待主程序退出失败: {err}"));
        return 1;
    }

    // Give Windows a short moment to release file handles.
    thread::sleep(Duration::from_millis(500));

    let install_dir = match target.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => {
            progress.fail("无法解析安装目录");
            return 1;
        }
    };

    let work_dir = install_dir.join(".cn-codex-update-work");
    let _ = fs::remove_dir_all(&work_dir);
    if let Err(err) = fs::create_dir_all(&work_dir) {
        progress.fail(&format!("创建临时目录失败: {err}"));
        return 1;
    }

    let package_name = package_filename_from_url(&url);
    let package_path = work_dir.join(&package_name);

    progress.set_status("正在下载更新包...");
    if let Err(err) = download_with_progress(&url, &package_path, &progress).await {
        progress.fail(&format!("下载失败: {err}"));
        let _ = fs::remove_dir_all(&work_dir);
        return 1;
    }

    if !sha256.trim().is_empty() {
        progress.set_status("正在校验文件...");
        match file_sha256(&package_path) {
            Ok(actual) if actual.eq_ignore_ascii_case(sha256.trim()) => {}
            Ok(actual) => {
                progress.fail(&format!(
                    "SHA256 校验失败\n期望: {}\n实际: {actual}",
                    sha256.trim()
                ));
                let _ = fs::remove_dir_all(&work_dir);
                return 1;
            }
            Err(err) => {
                progress.fail(&format!("计算 SHA256 失败: {err}"));
                let _ = fs::remove_dir_all(&work_dir);
                return 1;
            }
        }
    }

    let apply_result = if is_zip_package(&url, &package_path) {
        progress.set_status("正在解压更新包...");
        progress.set_progress(92);
        let extract_dir = work_dir.join("extract");
        if let Err(err) = fs::create_dir_all(&extract_dir) {
            progress.fail(&format!("创建解压目录失败: {err}"));
            let _ = fs::remove_dir_all(&work_dir);
            return 1;
        }
        if let Err(err) = extract_zip(&package_path, &extract_dir) {
            progress.fail(&format!("解压失败: {err}"));
            let _ = fs::remove_dir_all(&work_dir);
            return 1;
        }

        let package_root = match resolve_package_root(&extract_dir) {
            Ok(root) => root,
            Err(err) => {
                progress.fail(&err);
                let _ = fs::remove_dir_all(&work_dir);
                return 1;
            }
        };

        progress.set_status("正在替换程序文件...");
        progress.set_progress(96);
        apply_extracted_package(&package_root, &install_dir)
    } else {
        progress.set_status("正在替换主程序...");
        progress.set_progress(96);
        replace_executable(&package_path, &target)
    };

    if let Err(err) = apply_result {
        progress.fail(&format!("覆盖失败: {err}"));
        let _ = fs::remove_dir_all(&work_dir);
        return 1;
    }

    let _ = fs::remove_dir_all(&work_dir);

    progress.set_status("更新完成，正在启动...");
    progress.set_progress(100);

    let launch_path = launch.unwrap_or(target);
    if let Err(err) = launch_process(&launch_path) {
        progress.fail(&format!("启动新程序失败: {err}"));
        return 1;
    }

    progress.close();
    0
}

async fn fetch_manifest(url: &str, timeout_secs: u64) -> Result<LatestManifest, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|err| err.to_string())?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|err| err.to_string())?
        .error_for_status()
        .map_err(|err| err.to_string())?;
    response
        .json::<LatestManifest>()
        .await
        .map_err(|err| err.to_string())
}

async fn download_with_progress(
    url: &str,
    dest: &Path,
    progress: &ProgressUi,
) -> Result<(), String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|err| err.to_string())?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|err| err.to_string())?
        .error_for_status()
        .map_err(|err| err.to_string())?;

    let total = response.content_length().unwrap_or(0);
    let mut stream = response.bytes_stream();
    let mut file = File::create(dest).map_err(|err| err.to_string())?;
    let mut downloaded: u64 = 0;
    let mut last_ui = Instant::now();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|err| err.to_string())?;
        file.write_all(&chunk).map_err(|err| err.to_string())?;
        downloaded = downloaded.saturating_add(chunk.len() as u64);

        if last_ui.elapsed() >= Duration::from_millis(80) || downloaded == total {
            let percent = if total > 0 {
                ((downloaded as f64 / total as f64) * 100.0).round() as i32
            } else {
                0
            };
            progress.set_progress(percent.clamp(0, 99));
            if total > 0 {
                progress.set_status(&format!(
                    "正在下载... {} / {} ({percent}%)",
                    format_bytes(downloaded),
                    format_bytes(total)
                ));
            } else {
                progress.set_status(&format!("正在下载... {}", format_bytes(downloaded)));
            }
            last_ui = Instant::now();
        }
    }

    file.flush().map_err(|err| err.to_string())?;
    Ok(())
}

fn replace_executable(temp_path: &Path, target: &Path) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    // Prefer rename-over, fall back to copy.
    if target.exists() {
        let backup = target.with_extension("exe.bak");
        let _ = fs::remove_file(&backup);
        fs::rename(target, &backup).map_err(|err| format!("backup old exe failed: {err}"))?;
        match fs::rename(temp_path, target) {
            Ok(()) => {
                let _ = fs::remove_file(&backup);
                Ok(())
            }
            Err(rename_err) => {
                // Try restore old binary if replace failed.
                let _ = fs::rename(&backup, target);
                // Final fallback: copy bytes.
                fs::copy(temp_path, target)
                    .map(|_| ())
                    .map_err(|copy_err| {
                        format!("replace failed: rename={rename_err}; copy={copy_err}")
                    })
            }
        }
    } else {
        fs::rename(temp_path, target).map_err(|err| err.to_string())
    }
}

fn package_filename_from_url(url: &str) -> String {
    url.split(['?', '#'])
        .next()
        .and_then(|path| path.rsplit('/').next())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string())
        .unwrap_or_else(|| "CN-Codex-update.bin".to_string())
}

fn is_zip_package(url: &str, package_path: &Path) -> bool {
    let name = package_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name.ends_with(".zip") {
        return true;
    }
    let url_name = package_filename_from_url(url).to_ascii_lowercase();
    url_name.ends_with(".zip")
}

fn extract_zip(zip_path: &Path, dest_dir: &Path) -> Result<(), String> {
    let file = File::open(zip_path).map_err(|err| format!("open zip failed: {err}"))?;
    let mut archive =
        ZipArchive::new(file).map_err(|err| format!("read zip failed: {err}"))?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| format!("read zip entry failed: {err}"))?;
        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
        if rel.as_os_str().is_empty() {
            continue;
        }

        let out_path = dest_dir.join(&rel);
        if entry.is_dir() || rel.to_string_lossy().ends_with('/') {
            fs::create_dir_all(&out_path).map_err(|err| {
                format!("create dir {} failed: {err}", out_path.display())
            })?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                format!("create parent {} failed: {err}", parent.display())
            })?;
        }

        let mut outfile =
            File::create(&out_path).map_err(|err| format!("create {} failed: {err}", out_path.display()))?;
        io::copy(&mut entry, &mut outfile)
            .map_err(|err| format!("extract {} failed: {err}", out_path.display()))?;
    }

    Ok(())
}

fn resolve_package_root(extract_dir: &Path) -> Result<PathBuf, String> {
    let mut entries = fs::read_dir(extract_dir)
        .map_err(|err| format!("read extract dir failed: {err}"))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            let name = entry.file_name();
            let text = name.to_string_lossy();
            !text.eq_ignore_ascii_case(".ds_store") && text != "__MACOSX"
        })
        .collect::<Vec<_>>();

    if entries.is_empty() {
        return Err("更新包为空".to_string());
    }

    if entries.len() == 1 {
        let only = &entries[0];
        let path = only.path();
        if path.is_dir() {
            // Common layout: CN-Codex-x.y.z/...portable files
            return Ok(path);
        }
    }

    // Flat layout: files directly under extract dir.
    let _ = entries.sort_by_key(|entry| entry.file_name());
    Ok(extract_dir.to_path_buf())
}

fn apply_extracted_package(package_root: &Path, install_dir: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    collect_files(package_root, package_root, &mut files)?;
    if files.is_empty() {
        return Err("更新包内没有可替换文件".to_string());
    }

    for rel in files {
        if is_protected_user_path(&rel) {
            let existing = install_dir.join(&rel);
            if existing.exists() {
                // Keep local user data / runtime state.
                continue;
            }
        }

        let src = package_root.join(&rel);
        let dest = install_dir.join(&rel);
        copy_file_replace(&src, &dest)?;
    }

    Ok(())
}

fn collect_files(root: &Path, current: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries =
        fs::read_dir(current).map_err(|err| format!("read {} failed: {err}", current.display()))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("read dir entry failed: {err}"))?;
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .map_err(|err| format!("strip prefix failed: {err}"))?
            .to_path_buf();
        if path.is_dir() {
            collect_files(root, &path, out)?;
        } else if path.is_file() {
            out.push(rel);
        }
    }
    Ok(())
}

fn is_protected_user_path(rel: &Path) -> bool {
    let normalized = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/");
    let lower = normalized.to_ascii_lowercase();

    matches!(
        lower.as_str(),
        "codey/config.toml"
            | "codey/usage.db"
            | "codey/usage.db-wal"
            | "codey/usage.db-shm"
            | "codey/hooks.json"
            | "codey/browser/visible-browser.json"
            | "latest.json"
    ) || lower.starts_with("codey/sessions/")
        || lower.starts_with("codey/memories/")
        || lower.starts_with("codey/browser/webview-data/")
        || lower.starts_with("codey/browser/screenshots/")
        || lower.starts_with(".cn-codex-update-work/")
}

fn copy_file_replace(src: &Path, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create {} failed: {err}", parent.display()))?;
    }

    // Running updater.exe cannot overwrite itself; write side-car and keep going.
    if is_self_updater_path(dest) {
        let side = dest.with_extension("exe.new");
        let _ = fs::remove_file(&side);
        match fs::copy(src, dest) {
            Ok(_) => {
                let _ = fs::remove_file(&side);
                return Ok(());
            }
            Err(_) => {
                fs::copy(src, &side).map_err(|err| {
                    format!(
                        "copy updater to {} failed (self locked): {err}",
                        side.display()
                    )
                })?;
                return Ok(());
            }
        }
    }

    match fs::copy(src, dest) {
        Ok(_) => Ok(()),
        Err(copy_err) => {
            // Windows may lock the previous binary briefly; try rename swap.
            let temp = dest.with_extension(format!(
                "{}.new",
                dest.extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or("bin")
            ));
            let _ = fs::remove_file(&temp);
            fs::copy(src, &temp).map_err(|err| {
                format!(
                    "copy {} -> {} failed: {copy_err}; stage failed: {err}",
                    src.display(),
                    dest.display()
                )
            })?;

            if dest.exists() {
                let backup = dest.with_extension(format!(
                    "{}.bak",
                    dest.extension()
                        .and_then(|ext| ext.to_str())
                        .unwrap_or("bin")
                ));
                let _ = fs::remove_file(&backup);
                if let Err(rename_err) = fs::rename(dest, &backup) {
                    let _ = fs::remove_file(&temp);
                    return Err(format!(
                        "backup {} failed: {rename_err}",
                        dest.display()
                    ));
                }
                match fs::rename(&temp, dest) {
                    Ok(()) => {
                        let _ = fs::remove_file(&backup);
                        Ok(())
                    }
                    Err(final_err) => {
                        let _ = fs::rename(&backup, dest);
                        let _ = fs::remove_file(&temp);
                        Err(format!(
                            "replace {} failed: {final_err}",
                            dest.display()
                        ))
                    }
                }
            } else {
                fs::rename(&temp, dest).map_err(|err| {
                    format!("move {} -> {} failed: {err}", temp.display(), dest.display())
                })
            }
        }
    }
}

fn is_self_updater_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|name| name.eq_ignore_ascii_case("updater.exe") || name.eq_ignore_ascii_case("updater"))
        .unwrap_or(false)
}

fn launch_process(path: &Path) -> Result<(), String> {
    let mut command = Command::new(path);
    if let Some(parent) = path.parent() {
        command.current_dir(parent);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|err| err.to_string())
}

fn wait_for_process_exit(pid: u32, wait_secs: u64) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(wait_secs);
    let force_after = Instant::now() + Duration::from_secs(3);
    let mut forced = false;
    while Instant::now() < deadline {
        if !process_exists(pid) {
            return Ok(());
        }
        // Best effort kill after a short grace period so the main app can exit cleanly.
        if !forced && Instant::now() >= force_after {
            let _ = force_kill(pid);
            forced = true;
        }
        thread::sleep(Duration::from_millis(200));
    }
    if process_exists(pid) {
        Err(format!("process {pid} did not exit within {wait_secs}s"))
    } else {
        Ok(())
    }
}

fn process_exists(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        };

        unsafe {
            let Ok(handle) =
                OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, false, pid)
            else {
                return false;
            };
            let result = WaitForSingleObject(handle, 0);
            let _ = CloseHandle(handle);
            // WAIT_TIMEOUT (0x00000102) means still running.
            result.0 == 0x00000102
        }
    }

    #[cfg(not(windows))]
    {
        Path::new(&format!("/proc/{pid}")).exists()
    }
}

fn force_kill(pid: u32) -> Result<(), String> {
    #[cfg(windows)]
    {
        let status = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|err| err.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("taskkill exited with {status}"))
        }
    }

    #[cfg(not(windows))]
    {
        let status = Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status()
            .map_err(|err| err.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("kill exited with {status}"))
        }
    }
}

fn file_sha256(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|err| err.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 64];
    loop {
        let read = file.read(&mut buffer).map_err(|err| err.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn is_newer_version(latest: &str, current: &str) -> bool {
    parse_version(latest) > parse_version(current)
}

fn parse_version(raw: &str) -> (u64, u64, u64) {
    let cleaned = raw.trim().trim_start_matches('v').trim_start_matches('V');
    let mut parts = cleaned.split(|c| c == '.' || c == '-' || c == '+');
    let major = parts
        .next()
        .and_then(|p| p.parse::<u64>().ok())
        .unwrap_or(0);
    let minor = parts
        .next()
        .and_then(|p| p.parse::<u64>().ok())
        .unwrap_or(0);
    let patch = parts
        .next()
        .and_then(|p| p.parse::<u64>().ok())
        .unwrap_or(0);
    (major, minor, patch)
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let value = bytes as f64;
    if value >= GB {
        format!("{:.2} GB", value / GB)
    } else if value >= MB {
        format!("{:.2} MB", value / MB)
    } else if value >= KB {
        format!("{:.1} KB", value / KB)
    } else {
        format!("{bytes} B")
    }
}

struct ProgressUi {
    #[cfg(windows)]
    hwnd: Option<isize>,
    #[cfg(windows)]
    status_hwnd: Option<isize>,
    #[cfg(windows)]
    detail_hwnd: Option<isize>,
    #[cfg(windows)]
    percent_hwnd: Option<isize>,
    #[cfg(windows)]
    bar_hwnd: Option<isize>,
}

impl ProgressUi {
    fn create(title: &str) -> Self {
        #[cfg(windows)]
        {
            return Self::create_windows(title);
        }
        #[cfg(not(windows))]
        {
            let _ = title;
            eprintln!("[updater] starting...");
            Self {}
        }
    }

    fn set_status(&self, text: &str) {
        #[cfg(windows)]
        {
            if let Some(status) = self.status_hwnd {
                unsafe {
                    use windows::core::PCWSTR;
                    use windows::Win32::UI::WindowsAndMessaging::SetWindowTextW;
                    let wide = to_wide(text);
                    let _ = SetWindowTextW(
                        windows::Win32::Foundation::HWND(status as *mut _),
                        PCWSTR(wide.as_ptr()),
                    );
                }
            }
            pump_messages();
            return;
        }
        #[cfg(not(windows))]
        {
            eprintln!("[updater] {text}");
        }
    }

    fn set_progress(&self, percent: i32) {
        #[cfg(windows)]
        {
            let clamped = percent.clamp(0, 100);
            if let Some(bar) = self.bar_hwnd {
                unsafe {
                    use windows::Win32::UI::Controls::PBM_SETPOS;
                    use windows::Win32::UI::WindowsAndMessaging::SendMessageW;
                    let _ = SendMessageW(
                        windows::Win32::Foundation::HWND(bar as *mut _),
                        PBM_SETPOS,
                        Some(windows::Win32::Foundation::WPARAM(clamped as usize)),
                        None,
                    );
                }
            }
            if let Some(percent_hwnd) = self.percent_hwnd {
                unsafe {
                    use windows::core::PCWSTR;
                    use windows::Win32::UI::WindowsAndMessaging::SetWindowTextW;
                    let wide = to_wide(&format!("{clamped}%"));
                    let _ = SetWindowTextW(
                        windows::Win32::Foundation::HWND(percent_hwnd as *mut _),
                        PCWSTR(wide.as_ptr()),
                    );
                }
            }
            pump_messages();
            return;
        }
        #[cfg(not(windows))]
        {
            eprintln!("[updater] progress {percent}%");
        }
    }

    fn fail(&self, message: &str) {
        self.set_status(message);
        #[cfg(windows)]
        {
            if let Some(detail) = self.detail_hwnd {
                unsafe {
                    use windows::core::PCWSTR;
                    use windows::Win32::UI::WindowsAndMessaging::SetWindowTextW;
                    let wide = to_wide("更新失败，请查看提示后重试");
                    let _ = SetWindowTextW(
                        windows::Win32::Foundation::HWND(detail as *mut _),
                        PCWSTR(wide.as_ptr()),
                    );
                }
            }
            unsafe {
                use windows::core::PCWSTR;
                use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
                let text = to_wide(message);
                let caption = to_wide("CN-Codex 更新失败");
                let _ = MessageBoxW(
                    None,
                    PCWSTR(text.as_ptr()),
                    PCWSTR(caption.as_ptr()),
                    MB_OK | MB_ICONERROR,
                );
            }
            return;
        }
        #[cfg(not(windows))]
        {
            eprintln!("[updater][error] {message}");
            thread::sleep(Duration::from_secs(2));
        }
    }

    fn close(&self) {
        #[cfg(windows)]
        {
            if let Some(hwnd) = self.hwnd {
                unsafe {
                    use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;
                    let _ = DestroyWindow(windows::Win32::Foundation::HWND(hwnd as *mut _));
                }
            }
            pump_messages();
        }
    }

    #[cfg(windows)]
    fn create_windows(title: &str) -> Self {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::Graphics::Gdi::{
            CreateSolidBrush, GetStockObject, UpdateWindow, DEFAULT_GUI_FONT, HBRUSH, WHITE_BRUSH,
        };
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::Controls::{
            InitCommonControlsEx, ICC_PROGRESS_CLASS, INITCOMMONCONTROLSEX, PBM_SETRANGE, PBM_SETPOS,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, GetSystemMetrics, LoadCursorW, RegisterClassW, SendMessageW,
            SetWindowLongPtrW, ShowWindow, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, IDC_ARROW,
            SM_CXSCREEN, SM_CYSCREEN, SW_SHOW, WINDOW_EX_STYLE, WNDCLASSW, WS_CAPTION, WS_CHILD,
            WS_OVERLAPPED, WS_SYSMENU, WS_VISIBLE,
        };

        unsafe {
            let _ = InitCommonControlsEx(&INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_PROGRESS_CLASS,
            });

            let hinstance = GetModuleHandleW(None).unwrap_or_default();
            let class_name = to_wide("CNCodexUpdaterWindow");
            // COLORREF is 0x00BBGGRR. Match DESIGN.md dark canvas (#1a1a1a).
            let bg_color = windows::Win32::Foundation::COLORREF(0x001A1A1A);
            let brush = CreateSolidBrush(bg_color);
            let wnd_class = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wnd_proc),
                hInstance: hinstance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: if brush.0.is_null() {
                    HBRUSH(GetStockObject(WHITE_BRUSH).0)
                } else {
                    brush
                },
                lpszClassName: PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };
            let _ = RegisterClassW(&wnd_class);

            let window_width = 500i32;
            let window_height = 236i32;
            let screen_w = GetSystemMetrics(SM_CXSCREEN);
            let screen_h = GetSystemMetrics(SM_CYSCREEN);
            let pos_x = ((screen_w - window_width) / 2).max(0);
            let pos_y = ((screen_h - window_height) / 2).max(0);

            let title_wide = to_wide(title);
            let static_class = to_wide("STATIC");
            let progress_class = to_wide("msctls_progress32");
            let empty_text = to_wide("");
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(class_name.as_ptr()),
                PCWSTR(title_wide.as_ptr()),
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
                pos_x,
                pos_y,
                window_width,
                window_height,
                None,
                None,
                Some(hinstance.into()),
                None,
            )
            .unwrap_or_default();

            let colors = Box::new(UpdaterUiColors {
                bg: bg_color,
                muted: windows::Win32::Foundation::COLORREF(0x00C8C8C8),
                bg_brush: brush,
            });
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(colors) as isize);

            let heading_text = to_wide("正在安装更新");
            let heading_hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(static_class.as_ptr()),
                PCWSTR(heading_text.as_ptr()),
                WS_CHILD | WS_VISIBLE,
                28,
                20,
                360,
                24,
                Some(hwnd),
                None,
                Some(hinstance.into()),
                None,
            )
            .unwrap_or_default();

            let status_text = to_wide("准备更新...");
            let status_hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(static_class.as_ptr()),
                PCWSTR(status_text.as_ptr()),
                WS_CHILD | WS_VISIBLE,
                28,
                52,
                444,
                22,
                Some(hwnd),
                None,
                Some(hinstance.into()),
                None,
            )
            .unwrap_or_default();

            let detail_text = to_wide("请保持网络连接，更新期间请勿关闭此窗口");
            let detail_hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(static_class.as_ptr()),
                PCWSTR(detail_text.as_ptr()),
                WS_CHILD | WS_VISIBLE,
                28,
                80,
                360,
                20,
                Some(hwnd),
                None,
                Some(hinstance.into()),
                None,
            )
            .unwrap_or_default();

            let percent_text = to_wide("0%");
            let percent_hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(static_class.as_ptr()),
                PCWSTR(percent_text.as_ptr()),
                WS_CHILD | WS_VISIBLE,
                400,
                80,
                56,
                20,
                Some(hwnd),
                None,
                Some(hinstance.into()),
                None,
            )
            .unwrap_or_default();

            let bar_hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(progress_class.as_ptr()),
                PCWSTR(empty_text.as_ptr()),
                WS_CHILD | WS_VISIBLE,
                28,
                116,
                428,
                18,
                Some(hwnd),
                None,
                Some(hinstance.into()),
                None,
            )
            .unwrap_or_default();

            let footer_text = to_wide("CN-Codex Portable Updater");
            let footer_hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(static_class.as_ptr()),
                PCWSTR(footer_text.as_ptr()),
                WS_CHILD | WS_VISIBLE,
                28,
                152,
                428,
                18,
                Some(hwnd),
                None,
                Some(hinstance.into()),
                None,
            )
            .unwrap_or_default();

            let _ = SendMessageW(
                bar_hwnd,
                PBM_SETRANGE,
                None,
                Some(LPARAM(((100i32) << 16) as isize)),
            );
            let _ = SendMessageW(bar_hwnd, PBM_SETPOS, Some(WPARAM(0)), None);
            // PBM_SETBARCOLOR / PBM_SETBKCOLOR raw messages keep dependency surface small.
            let _ = SendMessageW(
                bar_hwnd,
                0x0409, // PBM_SETBARCOLOR
                None,
                Some(LPARAM(0x005EC522)),
            );
            let _ = SendMessageW(
                bar_hwnd,
                0x2001, // PBM_SETBKCOLOR
                None,
                Some(LPARAM(0x002E2E2E)),
            );

            let font = GetStockObject(DEFAULT_GUI_FONT);
            if !font.0.is_null() {
                for target in [
                    heading_hwnd,
                    status_hwnd,
                    detail_hwnd,
                    percent_hwnd,
                    footer_hwnd,
                ] {
                    let _ = SendMessageW(
                        target,
                        0x0030, // WM_SETFONT
                        Some(WPARAM(font.0 as usize)),
                        Some(LPARAM(1)),
                    );
                }
            }

            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = UpdateWindow(hwnd);
            pump_messages();

            Self {
                hwnd: Some(hwnd.0 as isize),
                status_hwnd: Some(status_hwnd.0 as isize),
                detail_hwnd: Some(detail_hwnd.0 as isize),
                percent_hwnd: Some(percent_hwnd.0 as isize),
                bar_hwnd: Some(bar_hwnd.0 as isize),
            }
        }
    }
}

#[cfg(windows)]
struct UpdaterUiColors {
    bg: windows::Win32::Foundation::COLORREF,
    muted: windows::Win32::Foundation::COLORREF,
    bg_brush: windows::Win32::Graphics::Gdi::HBRUSH,
}

#[cfg(windows)]
unsafe extern "system" fn wnd_proc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::Foundation::LRESULT;
    use windows::Win32::Graphics::Gdi::{SetBkColor, SetTextColor};
    use windows::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, GetWindowLongPtrW, PostQuitMessage, SetWindowLongPtrW, GWLP_USERDATA,
        WM_CTLCOLORSTATIC, WM_DESTROY,
    };

    if msg == WM_CTLCOLORSTATIC {
        unsafe {
            let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut _);
            let colors_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const UpdaterUiColors;
            if !colors_ptr.is_null() {
                let colors = &*colors_ptr;
                let _ = lparam;
                // Keep static labels readable on dark canvas.
                let _ = SetTextColor(hdc, colors.muted);
                let _ = SetBkColor(hdc, colors.bg);
                return LRESULT(colors.bg_brush.0 as isize);
            }
        }
    }

    if msg == WM_DESTROY {
        unsafe {
            let colors_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut UpdaterUiColors;
            if !colors_ptr.is_null() {
                drop(Box::from_raw(colors_ptr));
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            PostQuitMessage(0);
        }
        return windows::Win32::Foundation::LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(windows)]
fn pump_messages() {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, WM_QUIT,
    };
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            if msg.message == WM_QUIT {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(windows)]
fn to_wide(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
