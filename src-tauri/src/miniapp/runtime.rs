//! MiniApp process lifecycle using bundled codey/node.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use super::{resolve_bundled_node, write_manifest, MiniAppRecord, MiniAppStatus};

#[derive(Debug)]
struct RunningProcess {
    child: Child,
    port: u16,
    /// Keep stdin open so Node MCP servers that exit on stdin EOF stay alive.
    #[allow(dead_code)]
    stdin: Option<ChildStdin>,
}

fn processes() -> &'static Mutex<HashMap<String, RunningProcess>> {
    static CELL: OnceLock<Mutex<HashMap<String, RunningProcess>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn is_running(slug: &str) -> bool {
    let mut guard = processes().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(proc) = guard.get_mut(slug) {
        match proc.child.try_wait() {
            Ok(Some(_)) => {
                guard.remove(slug);
                false
            }
            Ok(None) => true,
            Err(_) => {
                guard.remove(slug);
                false
            }
        }
    } else {
        false
    }
}

pub fn running_port(slug: &str) -> Option<u16> {
    if !is_running(slug) {
        return None;
    }
    processes()
        .lock()
        .ok()
        .and_then(|g| g.get(slug).map(|p| p.port))
}

pub(crate) fn is_port_available(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

fn allocate_port() -> Result<u16, String> {
    let listener =
        TcpListener::bind("127.0.0.1:0").map_err(|e| format!("分配端口失败: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("读取分配端口失败: {e}"))?
        .port();
    drop(listener);
    Ok(port)
}

/// Prefer the previously assigned port when free; otherwise allocate a new one.
fn resolve_start_port(preferred: Option<u16>) -> Result<u16, String> {
    if let Some(port) = preferred.filter(|p| *p > 0) {
        if is_port_available(port) {
            return Ok(port);
        }
        tracing::warn!(
            target: "miniapp",
            "preferred miniapp port {port} is busy; allocating a new free port"
        );
    }
    allocate_port()
}

/// Resolve DB credentials for a MiniApp from Local Knowledge Base sources.
/// Returns (host, port, user, password, database_name).
pub(crate) fn resolve_miniapp_db_env(
    workspace_config_dir: &std::path::Path,
    database_id: &str,
) -> Option<(String, String, String, String, String)> {
    if database_id.trim().is_empty() {
        return None;
    }
    let sources = crate::smartbrain::db_query::load_db_sources(workspace_config_dir);
    let settings = crate::smartbrain::db_query::load_db_settings(workspace_config_dir);
    let source = crate::smartbrain::db_query::resolve_db_source(
        &sources,
        &settings,
        Some(database_id),
    )
    .ok()?;
    let source = crate::smartbrain::db_query::enrich_source_from_connection_uri(source.clone());
    let host = source.host.trim();
    let user = source.username.trim();
    let name = source.database_name.trim();
    if host.is_empty() || user.is_empty() || name.is_empty() {
        return None;
    }
    let port = source
        .port
        .map(|p| p.to_string())
        .unwrap_or_else(|| "3306".to_string());
    Some((
        host.to_string(),
        port,
        user.to_string(),
        source.password,
        name.to_string(),
    ))
}

pub fn start_app(
    workspace_config_dir: &std::path::Path,
    app: &mut MiniAppRecord,
) -> Result<u16, String> {
    if is_running(&app.slug) {
        if let Some(port) = running_port(&app.slug) {
            app.status = MiniAppStatus::Running;
            app.port = Some(port);
            app.last_error.clear();
            return Ok(port);
        }
    }

    let root = PathBuf::from(&app.root_path);
    if !root.exists() {
        return Err(format!("小程序目录不存在: {}", app.root_path));
    }
    let server_entry = root.join("server").join("index.mjs");
    if !server_entry.exists() {
        return Err("缺少 server/index.mjs，请先生成脚手架".into());
    }

    let node = resolve_bundled_node(workspace_config_dir)?;
    // 优先复用上次端口；若被占用再分配新端口。
    let port = resolve_start_port(app.port)?;
    let db_env = resolve_miniapp_db_env(workspace_config_dir, &app.database_id);

    let mut command = Command::new(&node);
    command
        .arg(&server_entry)
        .current_dir(&root)
        .env("MINIAPP_PORT", port.to_string())
        .env("PORT", port.to_string())
        .env("MINIAPP_DATABASE_ID", &app.database_id)
        .env("MINIAPP_SLUG", &app.slug)
        .env("MINIAPP_NAME", &app.name)
        // Keep stdin open. MiniApp MCP servers treat stdin EOF as shutdown.
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // 隐藏 Windows 控制台窗口
    #[cfg(target_os = "windows")]
    {
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    if let Some((host, db_port, user, password, name)) = db_env {
        command
            .env("MINIAPP_DB_HOST", host)
            .env("MINIAPP_DB_PORT", db_port)
            .env("MINIAPP_DB_USER", user)
            .env("MINIAPP_DB_PASSWORD", password)
            .env("MINIAPP_DB_NAME", name);
    }

    let mut child = command
        .spawn()
        .map_err(|e| format!("启动小程序失败 ({}): {e}", node.display()))?;

    // Retain stdin handle so the pipe is not closed immediately after spawn.
    let child_stdin = child.stdin.take();

    if let Some(stderr) = child.stderr.take() {
        let slug = app.slug.clone();
        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().flatten() {
                tracing::info!(target: "miniapp", "[{slug}] {line}");
            }
        });
    }
    if let Some(stdout) = child.stdout.take() {
        let slug = app.slug.clone();
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().flatten() {
                tracing::debug!(target: "miniapp", "[{slug}] {line}");
            }
        });
    }

    thread::sleep(Duration::from_millis(250));
    if let Ok(Some(status)) = child.try_wait() {
        app.status = MiniAppStatus::Error;
        // 保留端口，方便下次继续尝试同一端口。
        app.last_error = format!("小程序进程立即退出，code={status}");
        let _ = write_manifest(app);
        return Err(app.last_error.clone());
    }

    {
        let mut guard = processes().lock().unwrap_or_else(|e| e.into_inner());
        guard.insert(
            app.slug.clone(),
            RunningProcess {
                child,
                port,
                stdin: child_stdin,
            },
        );
    }

    app.status = MiniAppStatus::Running;
    app.port = Some(port);
    app.last_error.clear();
    app.updated_at = super::now_secs();
    app.mcp.command = node.to_string_lossy().to_string();
    app.mcp.args = vec![server_entry.to_string_lossy().to_string()];
    app.mcp.cwd = Some(root.to_string_lossy().to_string());
    write_manifest(app)?;
    Ok(port)
}

pub fn stop_app(app: &mut MiniAppRecord) -> Result<(), String> {
    let mut guard = processes().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(mut proc) = guard.remove(&app.slug) {
        // Dropping stdin first can help the child exit cleanly on next wait.
        drop(proc.stdin.take());
        let _ = proc.child.kill();
        let _ = proc.child.wait();
    }
    app.status = MiniAppStatus::Stopped;
    app.last_error.clear();
    app.updated_at = super::now_secs();
    write_manifest(app)?;
    Ok(())
}

pub fn stop_all() {
    let mut guard = processes().lock().unwrap_or_else(|e| e.into_inner());
    for (_, mut proc) in guard.drain() {
        drop(proc.stdin.take());
        let _ = proc.child.kill();
        let _ = proc.child.wait();
    }
}
