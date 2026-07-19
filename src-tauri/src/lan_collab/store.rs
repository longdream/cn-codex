use std::fs;
use std::path::{Path, PathBuf};

use super::types::{ChatMessage, CollabGroup, NearbyPeer};

const GROUPS_FILE: &str = "groups.json";
const PEERS_FILE: &str = "peers.json";
const MESSAGES_DIR: &str = "messages";

#[derive(Debug, Clone)]
pub struct LanCollabStore {
    data_dir: PathBuf,
}

impl LanCollabStore {
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn ensure_dirs(&self) -> Result<(), String> {
        fs::create_dir_all(&self.data_dir).map_err(|e| format!("创建 lan_collab 目录失败: {e}"))?;
        fs::create_dir_all(self.data_dir.join(MESSAGES_DIR))
            .map_err(|e| format!("创建消息目录失败: {e}"))?;
        Ok(())
    }

    pub fn load_groups(&self) -> Result<Vec<CollabGroup>, String> {
        self.read_json_vec(GROUPS_FILE)
    }

    pub fn save_groups(&self, groups: &[CollabGroup]) -> Result<(), String> {
        self.write_json(GROUPS_FILE, &groups)
    }

    pub fn load_peers(&self) -> Result<Vec<NearbyPeer>, String> {
        self.read_json_vec(PEERS_FILE)
    }

    pub fn save_peers(&self, peers: &[NearbyPeer]) -> Result<(), String> {
        self.write_json(PEERS_FILE, &peers)
    }

    pub fn load_messages(&self, group_id: &str) -> Result<Vec<ChatMessage>, String> {
        let path = self
            .data_dir
            .join(MESSAGES_DIR)
            .join(format!("{group_id}.json"));
        if !path.is_file() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(&path).map_err(|e| format!("读取消息失败: {e}"))?;
        serde_json::from_str(&raw).map_err(|e| format!("解析消息失败: {e}"))
    }

    pub fn append_message(&self, message: &ChatMessage) -> Result<(), String> {
        self.ensure_dirs()?;
        let mut messages = self.load_messages(&message.group_id)?;
        if messages
            .iter()
            .any(|item| item.message_id == message.message_id)
        {
            return Ok(());
        }
        messages.push(message.clone());
        messages.sort_by_key(|item| item.created_at);
        let path = self
            .data_dir
            .join(MESSAGES_DIR)
            .join(format!("{}.json", message.group_id));
        let raw =
            serde_json::to_string_pretty(&messages).map_err(|e| format!("序列化消息失败: {e}"))?;
        fs::write(path, raw).map_err(|e| format!("写入消息失败: {e}"))
    }

    fn read_json_vec<T: serde::de::DeserializeOwned>(
        &self,
        file_name: &str,
    ) -> Result<Vec<T>, String> {
        let path = self.data_dir.join(file_name);
        if !path.is_file() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(&path).map_err(|e| format!("读取 {file_name} 失败: {e}"))?;
        serde_json::from_str(&raw).map_err(|e| format!("解析 {file_name} 失败: {e}"))
    }

    fn write_json<T: serde::Serialize>(&self, file_name: &str, value: &T) -> Result<(), String> {
        self.ensure_dirs()?;
        let path = self.data_dir.join(file_name);
        let raw = serde_json::to_string_pretty(value)
            .map_err(|e| format!("序列化 {file_name} 失败: {e}"))?;
        fs::write(path, raw).map_err(|e| format!("写入 {file_name} 失败: {e}"))
    }
}
