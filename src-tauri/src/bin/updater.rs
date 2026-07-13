#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    fs::{self, File},
    io::{Read, Write},
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
    /// Download latest exe, replace target, then relaunch.
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
    progress.set_status("正在等待主程序退出...");
    progress.set_progress(0);

    if let Err(err) = wait_for_process_exit(pid, wait_secs) {
        progress.fail(&format!("等待主程序退出失败: {err}"));
        return 1;
    }

    // Give Windows a short moment to release file handles.
    thread::sleep(Duration::from_millis(500));

    progress.set_status("正在下载新版本...");
    let temp_path = target.with_extension("exe.new");
    if let Err(err) = download_with_progress(&url, &temp_path, &progress).await {
        progress.fail(&format!("下载失败: {err}"));
        let _ = fs::remove_file(&temp_path);
        return 1;
    }

    if !sha256.trim().is_empty() {
        progress.set_status("正在校验文件...");
        match file_sha256(&temp_path) {
            Ok(actual) if actual.eq_ignore_ascii_case(sha256.trim()) => {}
            Ok(actual) => {
                progress.fail(&format!(
                    "SHA256 校验失败\n期望: {}\n实际: {actual}",
                    sha256.trim()
                ));
                let _ = fs::remove_file(&temp_path);
                return 1;
            }
            Err(err) => {
                progress.fail(&format!("计算 SHA256 失败: {err}"));
                let _ = fs::remove_file(&temp_path);
                return 1;
            }
        }
    }

    progress.set_status("正在替换主程序...");
    progress.set_progress(98);
    if let Err(err) = replace_executable(&temp_path, &target) {
        progress.fail(&format!("覆盖失败: {err}"));
        let _ = fs::remove_file(&temp_path);
        return 1;
    }
    let _ = fs::remove_file(&temp_path);

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
            if let Some(bar) = self.bar_hwnd {
                unsafe {
                    use windows::Win32::UI::Controls::PBM_SETPOS;
                    use windows::Win32::UI::WindowsAndMessaging::SendMessageW;
                    let _ = SendMessageW(
                        windows::Win32::Foundation::HWND(bar as *mut _),
                        PBM_SETPOS,
                        Some(windows::Win32::Foundation::WPARAM(percent.clamp(0, 100) as usize)),
                        None,
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
        use windows::Win32::UI::Controls::{InitCommonControlsEx, INITCOMMONCONTROLSEX, ICC_PROGRESS_CLASS, PBM_SETRANGE, PBM_SETPOS};
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, LoadCursorW, RegisterClassW, SendMessageW,
            ShowWindow, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, IDC_ARROW, SW_SHOW,
            WINDOW_EX_STYLE, WNDCLASSW, WS_CAPTION, WS_CHILD,
            WS_OVERLAPPED, WS_SYSMENU, WS_VISIBLE,
        };

        unsafe {
            let _ = InitCommonControlsEx(&INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_PROGRESS_CLASS,
            });

            let hinstance = GetModuleHandleW(None).unwrap_or_default();
            let class_name = to_wide("CNCodexUpdaterWindow");
            let brush = CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x00201A12));
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

            let title_wide = to_wide(title);
            let static_class = to_wide("STATIC");
            let progress_class = to_wide("msctls_progress32");
            let empty_text = to_wide("");
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(class_name.as_ptr()),
                PCWSTR(title_wide.as_ptr()),
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                460,
                180,
                None,
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
                24,
                28,
                400,
                24,
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
                24,
                70,
                400,
                24,
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
            let font = GetStockObject(DEFAULT_GUI_FONT);
            if !font.0.is_null() {
                let _ = SendMessageW(
                    status_hwnd,
                    0x0030, // WM_SETFONT
                    Some(WPARAM(font.0 as usize)),
                    Some(LPARAM(1)),
                );
            }

            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = UpdateWindow(hwnd);
            pump_messages();

            Self {
                hwnd: Some(hwnd.0 as isize),
                status_hwnd: Some(status_hwnd.0 as isize),
                bar_hwnd: Some(bar_hwnd.0 as isize),
            }
        }
    }
}

#[cfg(windows)]
unsafe extern "system" fn wnd_proc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, PostQuitMessage, WM_DESTROY};
    if msg == WM_DESTROY {
        unsafe {
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
