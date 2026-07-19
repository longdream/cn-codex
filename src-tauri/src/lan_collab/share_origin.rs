//! 统一 Share Origin：区分本机原创 vs 共享下载副本，并支持版本对照与更新冲突检测。
//!
//! 元数据文件：
//! - skill/workflow: `<resource_dir>/origin.share.json`
//! - knowledge: `memories/knowledge/origins/<doc_id>.origin.share.json`

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const ORIGIN_FILE_NAME: &str = "origin.share.json";
pub const ORIGIN_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShareOriginKind {
    Workflow,
    Skill,
    Knowledge,
}

impl ShareOriginKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Workflow => "workflow",
            Self::Skill => "skill",
            Self::Knowledge => "knowledge",
        }
    }
}

/// 安装来源 sidecar。仅共享下载副本存在；本机原创不写此文件。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ShareOrigin {
    pub schema_version: u32,
    pub kind: ShareOriginKind,
    pub resource_id: String,
    pub title: String,
    pub source_share_id: String,
    pub source_host_node_id: String,
    pub source_host_display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_group_id: Option<String>,
    /// 首次安装时的远端版本哈希。
    pub origin_content_hash: String,
    /// 最近一次成功安装/拉取的内容哈希。
    pub installed_content_hash: String,
    pub installed_at: i64,
    pub last_pulled_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_checked_at: Option<i64>,
    /// 当前本地内容是否相对 installed 哈希发生修改。
    #[serde(default)]
    pub local_modified: bool,
}

/// 与远端 offer 对照后的更新状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShareUpdateStatus {
    /// 本机原创或无 origin。
    LocalOriginal,
    /// 已安装且与远端一致、本地未改。
    UpToDate,
    /// 远端有新版本，本地未改。
    HasUpdate,
    /// 本地已改，远端无新版本。
    LocalModified,
    /// 本地已改且远端有新版本。
    Conflict,
    /// 远端未提供可用 contentHash，无法可靠检测。
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareOriginInspection {
    pub kind: ShareOriginKind,
    pub resource_id: String,
    pub exists: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<ShareOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_content_hash: Option<String>,
    pub local_modified: bool,
    pub update_status: ShareUpdateStatus,
}

impl ShareOrigin {
    pub fn new_installed(
        kind: ShareOriginKind,
        resource_id: impl Into<String>,
        title: impl Into<String>,
        source_share_id: impl Into<String>,
        source_host_node_id: impl Into<String>,
        source_host_display_name: impl Into<String>,
        source_group_id: Option<String>,
        content_hash: impl Into<String>,
    ) -> Self {
        let now = chrono::Utc::now().timestamp();
        let content_hash = content_hash.into();
        Self {
            schema_version: ORIGIN_SCHEMA_VERSION,
            kind,
            resource_id: resource_id.into(),
            title: title.into(),
            source_share_id: source_share_id.into(),
            source_host_node_id: source_host_node_id.into(),
            source_host_display_name: source_host_display_name.into(),
            source_group_id,
            origin_content_hash: content_hash.clone(),
            installed_content_hash: content_hash,
            installed_at: now,
            last_pulled_at: now,
            last_checked_at: Some(now),
            local_modified: false,
        }
    }

    pub fn mark_pulled(&mut self, content_hash: impl Into<String>) {
        let now = chrono::Utc::now().timestamp();
        let content_hash = content_hash.into();
        self.installed_content_hash = content_hash;
        self.last_pulled_at = now;
        self.last_checked_at = Some(now);
        self.local_modified = false;
    }

    pub fn refresh_local_modified(&mut self, current_content_hash: &str) {
        self.local_modified = current_content_hash != self.installed_content_hash;
    }
}

/// 计算带 `sha256:` 前缀的内容哈希。
pub fn content_hash_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("sha256:{}", hex::encode(digest))
}

pub fn content_hash_str(text: &str) -> String {
    content_hash_hex(text.as_bytes())
}

/// skill/workflow 目录旁的 origin 路径。
pub fn origin_path_in_dir(resource_dir: &Path) -> PathBuf {
    resource_dir.join(ORIGIN_FILE_NAME)
}

/// knowledge 导出副本 origin 路径。
pub fn knowledge_origin_path(knowledge_dir: &Path, doc_id: &str) -> PathBuf {
    knowledge_dir
        .join("origins")
        .join(format!("{doc_id}.origin.share.json"))
}

pub fn read_origin_file(path: &Path) -> Result<Option<ShareOrigin>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path).map_err(|e| format!("读取 origin 失败: {e}"))?;
    let origin: ShareOrigin =
        serde_json::from_str(&raw).map_err(|e| format!("解析 origin 失败: {e}"))?;
    Ok(Some(origin))
}

pub fn write_origin_file(path: &Path, origin: &ShareOrigin) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建 origin 目录失败: {e}"))?;
    }
    let raw =
        serde_json::to_string_pretty(origin).map_err(|e| format!("序列化 origin 失败: {e}"))?;
    std::fs::write(path, raw).map_err(|e| format!("写入 origin 失败: {e}"))
}

pub fn read_origin_in_dir(resource_dir: &Path) -> Result<Option<ShareOrigin>, String> {
    read_origin_file(&origin_path_in_dir(resource_dir))
}

pub fn write_origin_in_dir(resource_dir: &Path, origin: &ShareOrigin) -> Result<(), String> {
    write_origin_file(&origin_path_in_dir(resource_dir), origin)
}

/// 对照远端 contentHash，计算更新状态。
pub fn evaluate_update_status(
    origin: Option<&ShareOrigin>,
    current_content_hash: Option<&str>,
    remote_content_hash: Option<&str>,
) -> ShareUpdateStatus {
    let Some(origin) = origin else {
        return ShareUpdateStatus::LocalOriginal;
    };

    let local_modified = match current_content_hash {
        Some(current) => current != origin.installed_content_hash,
        None => origin.local_modified,
    };

    let remote = remote_content_hash.map(str::trim).filter(|s| !s.is_empty());
    let Some(remote_hash) = remote else {
        return if local_modified {
            ShareUpdateStatus::LocalModified
        } else {
            ShareUpdateStatus::Unknown
        };
    };

    let remote_differs = remote_hash != origin.installed_content_hash;
    match (remote_differs, local_modified) {
        (false, false) => ShareUpdateStatus::UpToDate,
        (true, false) => ShareUpdateStatus::HasUpdate,
        (false, true) => ShareUpdateStatus::LocalModified,
        (true, true) => ShareUpdateStatus::Conflict,
    }
}

pub fn inspect_resource(
    kind: ShareOriginKind,
    resource_id: &str,
    resource_dir_or_file: &Path,
    current_content_hash: Option<String>,
    remote_content_hash: Option<&str>,
) -> Result<ShareOriginInspection, String> {
    let origin_path = match kind {
        ShareOriginKind::Knowledge => {
            // resource_dir_or_file 期望为 knowledge 根目录
            knowledge_origin_path(resource_dir_or_file, resource_id)
        }
        ShareOriginKind::Skill | ShareOriginKind::Workflow => {
            origin_path_in_dir(resource_dir_or_file)
        }
    };

    let mut origin = read_origin_file(&origin_path)?;
    let mut local_modified = false;
    if let (Some(origin_mut), Some(current)) = (origin.as_mut(), current_content_hash.as_deref()) {
        origin_mut.refresh_local_modified(current);
        local_modified = origin_mut.local_modified;
    } else if let Some(origin_ref) = origin.as_ref() {
        local_modified = origin_ref.local_modified;
    }

    let update_status = evaluate_update_status(
        origin.as_ref(),
        current_content_hash.as_deref(),
        remote_content_hash,
    );

    Ok(ShareOriginInspection {
        kind,
        resource_id: resource_id.to_string(),
        exists: origin.is_some(),
        origin,
        current_content_hash,
        local_modified,
        update_status,
    })
}

/// 规范化 workflow 内容哈希：workflow.json 文本 + 脚本快照（路径字典序）。
pub fn hash_workflow_bundle(workflow_json: &str, scripts: &[(String, String)]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"workflow_json\0");
    hasher.update(workflow_json.as_bytes());
    hasher.update(b"\0scripts\0");

    let mut ordered = scripts.to_vec();
    ordered.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, content) in ordered {
        hasher.update(path.as_bytes());
        hasher.update(b"\0");
        hasher.update(content.as_bytes());
        hasher.update(b"\0");
    }
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

pub fn hash_skill_content(content: &str) -> String {
    content_hash_str(content)
}

pub fn hash_knowledge_doc(content: &str) -> String {
    content_hash_str(content)
}

/// 安装前冲突检查。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallConflict {
    /// 目标不存在，可直接安装。
    None,
    /// 存在本机原创（无 origin），需 overwrite。
    LocalOriginalExists,
    /// 已是共享副本且本地未改，overwrite 即可。
    SharedExists,
    /// 共享副本本地已改，需 forceOverwrite。
    SharedLocalModified,
}

pub fn classify_install_conflict(
    target_exists: bool,
    origin: Option<&ShareOrigin>,
    current_content_hash: Option<&str>,
) -> InstallConflict {
    if !target_exists {
        return InstallConflict::None;
    }
    let Some(origin) = origin else {
        return InstallConflict::LocalOriginalExists;
    };
    let local_modified = match current_content_hash {
        Some(h) => h != origin.installed_content_hash,
        None => origin.local_modified,
    };
    if local_modified {
        InstallConflict::SharedLocalModified
    } else {
        InstallConflict::SharedExists
    }
}

pub fn ensure_install_allowed(
    conflict: InstallConflict,
    overwrite: bool,
    force_overwrite: bool,
    resource_label: &str,
) -> Result<(), String> {
    match conflict {
        InstallConflict::None => Ok(()),
        InstallConflict::LocalOriginalExists | InstallConflict::SharedExists => {
            if overwrite || force_overwrite {
                Ok(())
            } else {
                Err(format!("本机已存在同名资源：{resource_label}"))
            }
        }
        InstallConflict::SharedLocalModified => {
            if force_overwrite {
                Ok(())
            } else {
                Err(format!(
                    "本机共享副本「{resource_label}」已本地修改，覆盖需 forceOverwrite=true"
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn content_hash_stable() {
        let a = content_hash_str("hello");
        let b = content_hash_str("hello");
        let c = content_hash_str("world");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.starts_with("sha256:"));
    }

    #[test]
    fn workflow_hash_orders_scripts() {
        let json = r#"{"name":"w"}"#;
        let h1 = hash_workflow_bundle(
            json,
            &[
                ("scripts/b.py".into(), "b".into()),
                ("scripts/a.py".into(), "a".into()),
            ],
        );
        let h2 = hash_workflow_bundle(
            json,
            &[
                ("scripts/a.py".into(), "a".into()),
                ("scripts/b.py".into(), "b".into()),
            ],
        );
        assert_eq!(h1, h2);
        let h3 = hash_workflow_bundle(
            json,
            &[
                ("scripts/a.py".into(), "A".into()),
                ("scripts/b.py".into(), "b".into()),
            ],
        );
        assert_ne!(h1, h3);
    }

    #[test]
    fn origin_roundtrip_and_local_modified() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join("wf");
        std::fs::create_dir_all(&dir).unwrap();

        let mut origin = ShareOrigin::new_installed(
            ShareOriginKind::Workflow,
            "demo",
            "Demo",
            "wshare_1",
            "node_a",
            "Alice",
            None,
            "sha256:aaa",
        );
        write_origin_in_dir(&dir, &origin).unwrap();

        let loaded = read_origin_in_dir(&dir).unwrap().unwrap();
        assert_eq!(loaded.source_share_id, "wshare_1");
        assert!(!loaded.local_modified);

        origin.refresh_local_modified("sha256:bbb");
        assert!(origin.local_modified);
        origin.mark_pulled("sha256:bbb");
        assert!(!origin.local_modified);
        assert_eq!(origin.installed_content_hash, "sha256:bbb");
    }

    #[test]
    fn evaluate_update_status_matrix() {
        let origin = ShareOrigin::new_installed(
            ShareOriginKind::Skill,
            "s",
            "S",
            "sshare_1",
            "node_a",
            "Alice",
            None,
            "sha256:v1",
        );
        assert_eq!(
            evaluate_update_status(Some(&origin), Some("sha256:v1"), Some("sha256:v1")),
            ShareUpdateStatus::UpToDate
        );
        assert_eq!(
            evaluate_update_status(Some(&origin), Some("sha256:v1"), Some("sha256:v2")),
            ShareUpdateStatus::HasUpdate
        );
        assert_eq!(
            evaluate_update_status(Some(&origin), Some("sha256:local"), Some("sha256:v1")),
            ShareUpdateStatus::LocalModified
        );
        assert_eq!(
            evaluate_update_status(Some(&origin), Some("sha256:local"), Some("sha256:v2")),
            ShareUpdateStatus::Conflict
        );
        assert_eq!(
            evaluate_update_status(None, Some("sha256:x"), Some("sha256:v1")),
            ShareUpdateStatus::LocalOriginal
        );
    }

    #[test]
    fn install_conflict_rules() {
        let origin = ShareOrigin::new_installed(
            ShareOriginKind::Workflow,
            "w",
            "W",
            "wshare",
            "n",
            "A",
            None,
            "sha256:1",
        );
        assert_eq!(
            classify_install_conflict(false, None, None),
            InstallConflict::None
        );
        assert_eq!(
            classify_install_conflict(true, None, None),
            InstallConflict::LocalOriginalExists
        );
        assert_eq!(
            classify_install_conflict(true, Some(&origin), Some("sha256:1")),
            InstallConflict::SharedExists
        );
        assert_eq!(
            classify_install_conflict(true, Some(&origin), Some("sha256:2")),
            InstallConflict::SharedLocalModified
        );

        ensure_install_allowed(InstallConflict::SharedLocalModified, true, false, "w")
            .expect_err("should require force");
        ensure_install_allowed(InstallConflict::SharedLocalModified, true, true, "w").unwrap();
    }
}
