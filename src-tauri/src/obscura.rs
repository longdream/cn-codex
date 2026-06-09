use std::path::PathBuf;
use std::sync::Arc;

use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{info, warn};

/// Manages an Obscura headless browser process for AI agent automation.
///
/// Obscura exposes a CDP WebSocket interface compatible with Playwright
/// `connectOverCDP`, enabling browser automation without requiring
/// Chrome or Node.
pub struct ObscuraManager {
    binary_path: PathBuf,
    port: u16,
    process: Arc<Mutex<Option<Child>>>,
}

impl ObscuraManager {
    pub fn new(binary_path: PathBuf, port: u16) -> Self {
        Self {
            binary_path,
            port,
            process: Arc::new(Mutex::new(None)),
        }
    }

    pub fn cdp_endpoint(&self) -> String {
        format!("ws://127.0.0.1:{}", self.port)
    }

    pub async fn start(&self) -> Result<String, String> {
        let mut guard = self.process.lock().await;
        if let Some(ref mut child) = *guard {
            match child.try_wait() {
                Ok(Some(_)) => {
                    info!("Obscura process exited, restarting");
                }
                Ok(None) => {
                    info!("Obscura already running on port {}", self.port);
                    return Ok(self.cdp_endpoint());
                }
                Err(e) => {
                    warn!("Failed to check Obscura status: {e}");
                }
            }
        }

        if !self.binary_path.is_file() {
            return Err(format!(
                "Obscura binary not found: {}",
                self.binary_path.display()
            ));
        }

        info!(
            "Starting Obscura: {} --port {}",
            self.binary_path.display(),
            self.port
        );

        let child = Command::new(&self.binary_path)
            .arg("--port")
            .arg(self.port.to_string())
            .arg("--headless")
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("Failed to start Obscura: {e}"))?;

        *guard = Some(child);

        tokio::time::sleep(std::time::Duration::from_millis(800)).await;

        if self.health_check().await {
            info!("Obscura started successfully on port {}", self.port);
            Ok(self.cdp_endpoint())
        } else {
            warn!("Obscura started but health check failed; proceeding anyway");
            Ok(self.cdp_endpoint())
        }
    }

    pub async fn stop(&self) {
        let mut guard = self.process.lock().await;
        if let Some(ref mut child) = guard.take() {
            info!("Stopping Obscura process");
            let _ = child.kill().await;
        }
    }

    pub async fn health_check(&self) -> bool {
        let url = format!("http://127.0.0.1:{}/json/version", self.port);
        match reqwest::get(&url).await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    pub fn is_configured(binary_path: &Option<String>) -> bool {
        match binary_path {
            Some(p) => !p.is_empty() && PathBuf::from(p).is_file(),
            None => false,
        }
    }
}

impl Drop for ObscuraManager {
    fn drop(&mut self) {
        let process = self.process.clone();
        tokio::spawn(async move {
            let mut guard = process.lock().await;
            if let Some(ref mut child) = guard.take() {
                let _ = child.kill().await;
            }
        });
    }
}
