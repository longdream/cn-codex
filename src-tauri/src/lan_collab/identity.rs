use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::types::NodeIdentity;

const IDENTITY_FILE: &str = "identity.json";

/// 加载或首次生成节点身份。
pub fn load_or_create_identity(data_dir: &Path) -> Result<NodeIdentity, String> {
    fs::create_dir_all(data_dir).map_err(|e| format!("创建 lan_collab 目录失败: {e}"))?;
    let path = data_dir.join(IDENTITY_FILE);
    if path.is_file() {
        let raw = fs::read_to_string(&path).map_err(|e| format!("读取节点身份失败: {e}"))?;
        let identity: NodeIdentity =
            serde_json::from_str(&raw).map_err(|e| format!("解析节点身份失败: {e}"))?;
        return Ok(identity);
    }

    let node_id = format!("node_{}", Uuid::new_v4().simple());
    let display_name = default_display_name();
    let seed = format!("{node_id}:{display_name}");
    let digest = Sha256::digest(seed.as_bytes());
    let device_pubkey = format!("ed25519_placeholder_{}", hex::encode(&digest[..16]));
    let identity = NodeIdentity {
        node_id,
        display_name,
        device_pubkey,
        created_at: chrono::Utc::now().timestamp(),
    };
    save_identity(data_dir, &identity)?;
    Ok(identity)
}

pub fn save_identity(data_dir: &Path, identity: &NodeIdentity) -> Result<(), String> {
    fs::create_dir_all(data_dir).map_err(|e| format!("创建 lan_collab 目录失败: {e}"))?;
    let path = data_dir.join(IDENTITY_FILE);
    let raw = serde_json::to_string_pretty(identity)
        .map_err(|e| format!("序列化节点身份失败: {e}"))?;
    fs::write(&path, raw).map_err(|e| format!("写入节点身份失败: {e}"))
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
