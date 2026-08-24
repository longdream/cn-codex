use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::sleep;
use tracing::{info, warn};

const DEFAULT_CDP_PORT: u16 = 9222;
const CDP_READY_TIMEOUT: Duration = Duration::from_secs(15);
const CDP_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Manages an external Chrome/Edge browser instance launched with `--remote-debugging-port`.
pub struct ExternalBrowser {
    inner: Arc<Mutex<BrowserState>>,
}

struct BrowserState {
    child: Option<Child>,
    cdp_port: u16,
    user_data_dir: Option<tempfile::TempDir>,
    /// 已就绪的 CDP endpoint，无论浏览器是本进程启动的（child）还是复用的外部实例。
    endpoint: Option<String>,
}

impl ExternalBrowser {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(BrowserState {
                child: None,
                cdp_port: DEFAULT_CDP_PORT,
                user_data_dir: None,
                endpoint: None,
            })),
        }
    }

    /// Launch an external Chrome/Edge instance. Returns the CDP endpoint URL.
    /// If already running, returns the existing endpoint.
    pub async fn launch(
        &self,
        http: &reqwest::Client,
        custom_browser_path: Option<&str>,
        cdp_port: Option<u16>,
    ) -> Result<String, String> {
        let mut state = self.inner.lock().await;

        let port = cdp_port.unwrap_or(DEFAULT_CDP_PORT);
        let endpoint = format!("http://127.0.0.1:{port}");

        if state.child.is_some() {
            if is_cdp_ready(http, &endpoint).await {
                state.endpoint = Some(endpoint.clone());
                return Ok(endpoint);
            }
            warn!("External browser process exists but CDP is not responsive; relaunching");
            shutdown_child(&mut state).await;
        }

        // Also check if there's already a Chrome listening on this port (user-started)
        if is_cdp_ready(http, &endpoint).await {
            info!("Found existing CDP endpoint at {endpoint}, reusing");
            state.cdp_port = port;
            state.endpoint = Some(endpoint.clone());
            return Ok(endpoint);
        }

        let browser_path = match custom_browser_path {
            Some(p) if !p.is_empty() => PathBuf::from(p),
            _ => find_browser_path()
                .ok_or_else(|| "Cannot find Chrome or Edge. Please install Chrome/Edge or configure browser_path in config.toml".to_string())?,
        };

        if !browser_path.exists() {
            return Err(format!(
                "Browser executable not found: {}",
                browser_path.display()
            ));
        }

        let temp_dir = tempfile::TempDir::new()
            .map_err(|e| format!("Failed to create temp user-data-dir: {e}"))?;
        let user_data_path = temp_dir.path().to_path_buf();

        info!(
            "Launching external browser: {} (port={port}, user-data-dir={})",
            browser_path.display(),
            user_data_path.display()
        );

        let mut cmd = Command::new(&browser_path);
        cmd.arg(format!("--remote-debugging-port={port}"))
            .arg(format!("--user-data-dir={}", user_data_path.display()))
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--start-maximized");

        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed to launch browser: {e}"))?;

        state.child = Some(child);
        state.cdp_port = port;
        state.user_data_dir = Some(temp_dir);

        wait_for_cdp_ready(http, &endpoint).await?;
        state.endpoint = Some(endpoint.clone());

        info!("External browser CDP ready at {endpoint}");
        Ok(endpoint)
    }

    /// Shut down the external browser process.
    pub async fn shutdown(&self) {
        let mut state = self.inner.lock().await;
        shutdown_child(&mut state).await;
    }

    /// Check if the browser is currently running and CDP is accessible.
    pub async fn is_running(&self, http: &reqwest::Client) -> bool {
        let state = self.inner.lock().await;
        // 本进程启动的（child）或复用的外部浏览器（endpoint）都算“在运行”。
        if state.child.is_none() && state.endpoint.is_none() {
            return false;
        }
        let endpoint = format!("http://127.0.0.1:{}", state.cdp_port);
        is_cdp_ready(http, &endpoint).await
    }

    /// Get the CDP endpoint URL if browser is available.
    pub async fn get_cdp_endpoint(&self) -> Option<String> {
        let state = self.inner.lock().await;
        if let Some(endpoint) = &state.endpoint {
            return Some(endpoint.clone());
        }
        if state.child.is_some() {
            Some(format!("http://127.0.0.1:{}", state.cdp_port))
        } else {
            None
        }
    }

    pub async fn get_cdp_port(&self) -> u16 {
        let state = self.inner.lock().await;
        state.cdp_port
    }
}

impl Drop for ExternalBrowser {
    fn drop(&mut self) {
        // Best-effort sync cleanup; the async shutdown should be called before drop.
        if let Ok(mut state) = self.inner.try_lock() {
            if let Some(ref mut child) = state.child {
                let _ = child.start_kill();
            }
        }
    }
}

async fn shutdown_child(state: &mut BrowserState) {
    if let Some(ref mut child) = state.child {
        info!("Shutting down external browser");
        let _ = child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
    }
    state.child = None;
    state.user_data_dir = None;
    state.endpoint = None;
}

async fn is_cdp_ready(http: &reqwest::Client, endpoint: &str) -> bool {
    http.get(format!("{endpoint}/json/version"))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

async fn wait_for_cdp_ready(http: &reqwest::Client, endpoint: &str) -> Result<(), String> {
    let deadline = Instant::now() + CDP_READY_TIMEOUT;
    loop {
        if is_cdp_ready(http, endpoint).await {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "Timed out waiting for CDP to become ready at {endpoint}"
            ));
        }
        sleep(CDP_POLL_INTERVAL).await;
    }
}

/// Auto-detect Chrome or Edge installation path on the current platform.
pub fn find_browser_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        find_browser_path_windows()
    }

    #[cfg(target_os = "macos")]
    {
        find_browser_path_macos()
    }

    #[cfg(target_os = "linux")]
    {
        find_browser_path_linux()
    }
}

#[cfg(target_os = "windows")]
fn find_browser_path_windows() -> Option<PathBuf> {
    // Check all Chrome paths first (system-wide and per-user) before falling
    // back to Edge, so Chrome is always preferred when installed.
    let mut candidates: Vec<PathBuf> = vec![
        PathBuf::from(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
        PathBuf::from(r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe"),
    ];
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local).join(r"Google\Chrome\Application\chrome.exe"));
    }
    candidates.push(PathBuf::from(
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    ));
    candidates.push(PathBuf::from(
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    ));
    candidates.into_iter().find(|p| p.exists())
}

#[cfg(target_os = "macos")]
fn find_browser_path_macos() -> Option<PathBuf> {
    let candidates = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    ];
    candidates.iter().map(PathBuf::from).find(|p| p.exists())
}

#[cfg(target_os = "linux")]
fn find_browser_path_linux() -> Option<PathBuf> {
    let candidates = [
        "google-chrome",
        "google-chrome-stable",
        "chromium-browser",
        "chromium",
        "microsoft-edge",
        "microsoft-edge-stable",
    ];
    for name in &candidates {
        if let Ok(output) = std::process::Command::new("which").arg(name).output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return Some(PathBuf::from(path));
                }
            }
        }
    }
    None
}

#[cfg(test)]
#[path = "external_browser_tests.rs"]
mod tests;
