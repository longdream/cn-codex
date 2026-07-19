use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::types::NodeIdentity;

const IDENTITY_FILE: &str = "identity.json";
const INSTANCE_MARKER_FILE: &str = "instance_root.txt";

/// 加载或首次生成节点身份。
///
/// 若用户整目录复制了安装包（含 `codey/lan_collab/identity.json`），
/// 两个客户端会共用同一个 node_id，局域网发现会互相过滤成“自己”。
/// 因此这里用安装根路径做实例标记：检测到复制安装时自动换新 node_id。
pub fn load_or_create_identity(data_dir: &Path) -> Result<NodeIdentity, String> {
    fs::create_dir_all(data_dir).map_err(|e| format!("创建 lan_collab 目录失败: {e}"))?;
    let path = data_dir.join(IDENTITY_FILE);
    let instance_root = resolve_instance_root(data_dir);
    let instance_token = instance_root.to_string_lossy().to_string();

    if path.is_file() {
        let raw = fs::read_to_string(&path).map_err(|e| format!("读取节点身份失败: {e}"))?;
        let mut identity: NodeIdentity =
            serde_json::from_str(&raw).map_err(|e| format!("解析节点身份失败: {e}"))?;
        let marker_path = data_dir.join(INSTANCE_MARKER_FILE);
        let marker = fs::read_to_string(&marker_path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        if marker.as_deref() == Some(instance_token.as_str()) {
            return Ok(identity);
        }

        if marker.is_some() {
            // 整包复制导致 identity 冲突：保留显示名，换新节点 ID。
            let old_node_id = identity.node_id.clone();
            identity = regenerate_identity(identity.display_name.clone());
            save_identity(data_dir, &identity)?;
            write_instance_marker(data_dir, &instance_token)?;
            tracing::warn!(
                "[lan_collab] detected copied install identity; regenerated node_id {} -> {} (root={})",
                old_node_id,
                identity.node_id,
                instance_token
            );
            return Ok(identity);
        }

        // 旧版本没有实例标记：首次启动时写入，避免误伤升级路径。
        write_instance_marker(data_dir, &instance_token)?;
        return Ok(identity);
    }

    let identity = regenerate_identity(default_display_name());
    save_identity(data_dir, &identity)?;
    write_instance_marker(data_dir, &instance_token)?;
    Ok(identity)
}

pub fn save_identity(data_dir: &Path, identity: &NodeIdentity) -> Result<(), String> {
    fs::create_dir_all(data_dir).map_err(|e| format!("创建 lan_collab 目录失败: {e}"))?;
    let path = data_dir.join(IDENTITY_FILE);
    let raw =
        serde_json::to_string_pretty(identity).map_err(|e| format!("序列化节点身份失败: {e}"))?;
    fs::write(&path, raw).map_err(|e| format!("写入节点身份失败: {e}"))
}

fn write_instance_marker(data_dir: &Path, token: &str) -> Result<(), String> {
    let path = data_dir.join(INSTANCE_MARKER_FILE);
    fs::write(&path, format!("{token}\n")).map_err(|e| format!("写入实例标记失败: {e}"))
}

/// 生成新的节点身份（保留调用方传入的显示名）。
pub fn new_identity_with_display_name(display_name: String) -> NodeIdentity {
    regenerate_identity(display_name)
}

fn regenerate_identity(display_name: String) -> NodeIdentity {
    let node_id = format!("node_{}", Uuid::new_v4().simple());
    let seed = format!("{node_id}:{display_name}");
    let digest = Sha256::digest(seed.as_bytes());
    let device_pubkey = format!("ed25519_placeholder_{}", hex::encode(&digest[..16]));
    NodeIdentity {
        node_id,
        display_name,
        device_pubkey,
        created_at: chrono::Utc::now().timestamp(),
    }
}

/// 用安装根路径作为“实例身份”。
/// portable 场景下，复制整个目录后根路径会变化，从而触发 node_id 再生。
fn resolve_instance_root(data_dir: &Path) -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            return canonicalize_or_keep(parent.to_path_buf());
        }
    }
    if let Some(parent) = data_dir.parent() {
        return canonicalize_or_keep(parent.to_path_buf());
    }
    canonicalize_or_keep(data_dir.to_path_buf())
}

fn canonicalize_or_keep(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or(path)
}

fn default_display_name() -> String {
    if let Ok(computer) = std::env::var("COMPUTERNAME") {
        if !computer.trim().is_empty() {
            return computer;
        }
    }
    if let Ok(host) = std::env::var("HOSTNAME") {
        if !host.trim().is_empty() {
            return host;
        }
    }
    if let Ok(user) = std::env::var("USERNAME").or_else(|_| std::env::var("USER")) {
        if !user.trim().is_empty() {
            return format!("{user}-PC");
        }
    }
    "CN-Codex-Node".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn regenerates_identity_when_instance_root_changes() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("cn_codex_identity_{stamp}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let first = load_or_create_identity(&dir).unwrap();
        // 模拟“复制安装目录后根路径变化”
        write_instance_marker(&dir, "D:/old-install-root").unwrap();
        let second = load_or_create_identity(&dir).unwrap();

        assert_ne!(first.node_id, second.node_id);
        assert_eq!(first.display_name, second.display_name);

        let third = load_or_create_identity(&dir).unwrap();
        assert_eq!(second.node_id, third.node_id);

        let _ = fs::remove_dir_all(&dir);
    }
}
