//! Workflow 共享：广播本机 workflow 目录，对端按需拉取并安装到 codey/workflows。
//!
//! 安装时写入 origin.share.json，区分本机原创与共享下载副本，支持再次拉取最新版本。

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::workflow::{self, WorkflowDef};

use super::share_origin::{
    self, InstallConflict, ShareOrigin, ShareOriginKind, ensure_install_allowed,
    hash_workflow_bundle, read_origin_in_dir, write_origin_in_dir,
};
use super::types::SharedWorkflowOffer;

const MAX_WORKFLOW_TOTAL_CHARS: usize = 800_000;
const MAX_SINGLE_SCRIPT_CHARS: usize = 200_000;
const MAX_SCRIPTS: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedWorkflowConfig {
    pub share_id: String,
    pub workflow_name: String,
    pub title: String,
    pub description: String,
    pub node_count: usize,
    pub content_hash: String,
    pub group_id: Option<String>,
    pub enabled: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedWorkflowScript {
    /// 相对 workflow 根目录路径，如 `scripts/tools/build.py`
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedWorkflowPayload {
    pub share_id: String,
    pub workflow_name: String,
    pub title: String,
    pub description: String,
    pub content_hash: String,
    pub workflow_json: String,
    #[serde(default)]
    pub skill_md: String,
    #[serde(default)]
    pub scripts: Vec<SharedWorkflowScript>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scripts_manifest_json: Option<String>,
}

#[derive(Clone)]
pub struct WorkflowShareService {
    workspace_config_dir: PathBuf,
    shares: Arc<RwLock<Vec<SharedWorkflowConfig>>>,
}

impl WorkflowShareService {
    pub fn new(workspace_config_dir: PathBuf) -> Self {
        Self {
            workspace_config_dir,
            shares: Arc::new(RwLock::new(Vec::new())),
        }
    }

    fn workflows_dir(&self) -> PathBuf {
        workflow::workflows_dir(&self.workspace_config_dir)
    }

    pub async fn list_local_shares(&self) -> Vec<SharedWorkflowConfig> {
        self.shares.read().await.clone()
    }

    pub async fn local_offers(
        &self,
        host_node_id: &str,
        host_display_name: &str,
        allowed_group_ids: Option<&HashSet<String>>,
    ) -> Vec<SharedWorkflowOffer> {
        let guard = self.shares.read().await;
        let mut offers = Vec::new();
        for s in guard.iter().filter(|s| s.enabled) {
            if !group_allowed(&s.group_id, allowed_group_ids) {
                continue;
            }
            // 共享目录广播时尽量刷新最新 hash/元数据
            let detail = self.read_local_workflow(&s.workflow_name).ok();
            let (title, description, node_count, content_hash) = if let Some(d) = detail {
                (d.title, d.description, d.node_count, d.content_hash)
            } else {
                (
                    s.title.clone(),
                    s.description.clone(),
                    s.node_count,
                    s.content_hash.clone(),
                )
            };
            offers.push(SharedWorkflowOffer {
                share_id: s.share_id.clone(),
                host_node_id: host_node_id.to_string(),
                host_display_name: host_display_name.to_string(),
                workflow_name: s.workflow_name.clone(),
                title,
                description,
                node_count,
                content_hash,
                group_id: s.group_id.clone(),
                online: true,
            });
        }
        offers
    }

    pub async fn share_workflow(
        &self,
        workflow_name: String,
        group_id: Option<String>,
    ) -> Result<SharedWorkflowConfig, String> {
        let workflow_name = workflow_name.trim().to_string();
        if workflow_name.is_empty() {
            return Err("workflowName 不能为空".to_string());
        }
        sanitize_workflow_name(&workflow_name)?;
        let group_id = require_group_id(group_id)?;

        let detail = self.read_local_workflow(&workflow_name)?;
        {
            let guard = self.shares.read().await;
            if guard.iter().any(|s| {
                s.enabled
                    && s.workflow_name == workflow_name
                    && s.group_id.as_deref() == Some(group_id.as_str())
            }) {
                return Err("该 Workflow 已共享到此协作组".to_string());
            }
        }

        let cfg = SharedWorkflowConfig {
            share_id: format!("wshare_{}", Uuid::new_v4().simple()),
            workflow_name: detail.workflow_name,
            title: detail.title,
            description: detail.description,
            node_count: detail.node_count,
            content_hash: detail.content_hash,
            group_id: Some(group_id),
            enabled: true,
            created_at: chrono::Utc::now().timestamp(),
        };
        self.shares.write().await.push(cfg.clone());
        Ok(cfg)
    }

    pub async fn unshare_workflow(&self, share_id: String) -> Result<(), String> {
        let mut guard = self.shares.write().await;
        let before = guard.len();
        guard.retain(|s| s.share_id != share_id);
        if guard.len() == before {
            return Err("未找到该 Workflow 共享项".to_string());
        }
        Ok(())
    }

    pub async fn share_group_id(&self, share_id: &str) -> Result<Option<String>, String> {
        let guard = self.shares.read().await;
        guard
            .iter()
            .find(|s| s.enabled && s.share_id == share_id)
            .map(|s| s.group_id.clone())
            .ok_or_else(|| "Workflow 共享项不存在或已撤销".to_string())
    }

    pub async fn fetch_share(&self, share_id: &str) -> Result<SharedWorkflowPayload, String> {
        let cfg = {
            let guard = self.shares.read().await;
            guard
                .iter()
                .find(|s| s.enabled && s.share_id == share_id)
                .cloned()
                .ok_or_else(|| "Workflow 共享项不存在或已撤销".to_string())?
        };
        let detail = self.read_local_workflow(&cfg.workflow_name)?;
        let payload = SharedWorkflowPayload {
            share_id: cfg.share_id,
            workflow_name: detail.workflow_name,
            title: detail.title,
            description: detail.description,
            content_hash: detail.content_hash,
            workflow_json: detail.workflow_json,
            skill_md: detail.skill_md,
            scripts: detail.scripts,
            scripts_manifest_json: detail.scripts_manifest_json,
        };
        validate_payload_size(&payload)?;
        Ok(payload)
    }

    pub fn install_workflow_payload(
        &self,
        payload: &SharedWorkflowPayload,
        host_node_id: &str,
        host_display_name: &str,
        group_id: Option<String>,
        overwrite: bool,
        force_overwrite: bool,
        install_as: Option<String>,
    ) -> Result<String, String> {
        validate_payload_size(payload)?;

        let target_name = install_as
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| payload.workflow_name.trim().to_string());
        sanitize_workflow_name(&target_name)?;

        // 校验 workflow.json 可解析，并统一 name 为安装名
        let mut def: WorkflowDef = serde_json::from_str(&payload.workflow_json)
            .map_err(|e| format!("远端 workflow.json 无效: {e}"))?;
        def.name = target_name.clone();
        if def.title.trim().is_empty() {
            def.title = payload.title.clone();
        }
        if def.description.trim().is_empty() {
            def.description = payload.description.clone();
        }
        let normalized_json = serde_json::to_string_pretty(&def)
            .map_err(|e| format!("规范化 workflow.json 失败: {e}"))?;

        let script_pairs: Vec<(String, String)> = payload
            .scripts
            .iter()
            .map(|s| (s.path.clone(), s.content.clone()))
            .collect();
        let content_hash = if payload.content_hash.trim().is_empty() {
            hash_workflow_bundle(&normalized_json, &script_pairs)
        } else {
            // 仍以本地规范化结果校验一致性；不一致时以实际内容为准，避免脏 hash
            let computed = hash_workflow_bundle(&normalized_json, &script_pairs);
            if payload.content_hash != computed {
                tracing::warn!(
                    "[workflow_share] contentHash mismatch remote={} computed={}",
                    payload.content_hash,
                    computed
                );
            }
            computed
        };

        let dir = self.workflows_dir().join(&target_name);
        let target_exists = dir.exists();
        let existing_origin = if target_exists {
            read_origin_in_dir(&dir)?
        } else {
            None
        };
        let current_hash = if target_exists {
            self.hash_existing_workflow_dir(&dir).ok()
        } else {
            None
        };
        let conflict = share_origin::classify_install_conflict(
            target_exists,
            existing_origin.as_ref(),
            current_hash.as_deref(),
        );
        ensure_install_allowed(conflict, overwrite, force_overwrite, &target_name)?;

        // 覆盖安装：清空旧目录内容（保留目录创建）
        if target_exists {
            std::fs::remove_dir_all(&dir).map_err(|e| format!("清理旧 Workflow 目录失败: {e}"))?;
        }
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建 Workflow 目录失败: {e}"))?;

        std::fs::write(dir.join("workflow.json"), &normalized_json)
            .map_err(|e| format!("写入 workflow.json 失败: {e}"))?;

        if !payload.skill_md.trim().is_empty() {
            std::fs::write(dir.join("SKILL.md"), &payload.skill_md)
                .map_err(|e| format!("写入 SKILL.md 失败: {e}"))?;
        }

        if let Some(manifest) = &payload.scripts_manifest_json {
            if !manifest.trim().is_empty() {
                std::fs::write(dir.join("scripts-manifest.json"), manifest)
                    .map_err(|e| format!("写入 scripts-manifest.json 失败: {e}"))?;
            }
        }

        for script in &payload.scripts {
            let rel = sanitize_script_rel_path(&script.path)?;
            let abs = dir.join(&rel);
            if let Some(parent) = abs.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("创建脚本目录失败: {e}"))?;
            }
            std::fs::write(&abs, &script.content)
                .map_err(|e| format!("写入脚本 {} 失败: {e}", script.path))?;
        }

        let mut origin = if let Some(mut existing) = existing_origin {
            // 若仍是同一来源的更新，保留 originContentHash 语义为“首次来源版本”
            // 但 source 元数据以本次拉取为准
            existing.kind = ShareOriginKind::Workflow;
            existing.resource_id = target_name.clone();
            existing.title = def.title.clone();
            existing.source_share_id = payload.share_id.clone();
            existing.source_host_node_id = host_node_id.to_string();
            existing.source_host_display_name = host_display_name.to_string();
            existing.source_group_id = group_id;
            if existing.origin_content_hash.trim().is_empty() {
                existing.origin_content_hash = content_hash.clone();
            }
            existing.mark_pulled(&content_hash);
            existing
        } else {
            ShareOrigin::new_installed(
                ShareOriginKind::Workflow,
                &target_name,
                &def.title,
                &payload.share_id,
                host_node_id,
                host_display_name,
                group_id,
                &content_hash,
            )
        };
        origin.local_modified = false;
        write_origin_in_dir(&dir, &origin)?;

        Ok(target_name)
    }

    fn read_local_workflow(&self, workflow_name: &str) -> Result<LocalWorkflowDetail, String> {
        sanitize_workflow_name(workflow_name)?;
        let dir = self.workflows_dir().join(workflow_name);
        if !dir.is_dir() {
            return Err(format!("Workflow 不存在: {workflow_name}"));
        }
        let def = workflow::load_workflow(&dir)
            .ok_or_else(|| format!("无法读取 Workflow: {workflow_name}"))?;
        let workflow_json = serde_json::to_string_pretty(&def)
            .map_err(|e| format!("序列化 workflow.json 失败: {e}"))?;

        let skill_md = std::fs::read_to_string(dir.join("SKILL.md")).unwrap_or_default();
        let scripts_manifest_json = std::fs::read_to_string(dir.join("scripts-manifest.json")).ok();
        let scripts = collect_workflow_scripts(&dir)?;
        let script_pairs: Vec<(String, String)> = scripts
            .iter()
            .map(|s| (s.path.clone(), s.content.clone()))
            .collect();
        let content_hash = hash_workflow_bundle(&workflow_json, &script_pairs);

        let total_chars = workflow_json.chars().count()
            + skill_md.chars().count()
            + scripts
                .iter()
                .map(|s| s.content.chars().count())
                .sum::<usize>();
        if total_chars > MAX_WORKFLOW_TOTAL_CHARS {
            return Err(format!(
                "Workflow 内容过大（{total_chars} 字符，上限 {MAX_WORKFLOW_TOTAL_CHARS}）"
            ));
        }

        Ok(LocalWorkflowDetail {
            workflow_name: def.name,
            title: def.title,
            description: def.description,
            node_count: def.nodes.len(),
            content_hash,
            workflow_json,
            skill_md,
            scripts,
            scripts_manifest_json,
        })
    }

    fn hash_existing_workflow_dir(&self, dir: &Path) -> Result<String, String> {
        let def =
            workflow::load_workflow(dir).ok_or_else(|| "无法读取本地 workflow".to_string())?;
        let workflow_json = serde_json::to_string_pretty(&def)
            .map_err(|e| format!("序列化 workflow.json 失败: {e}"))?;
        let scripts = collect_workflow_scripts(dir)?;
        let pairs: Vec<(String, String)> =
            scripts.into_iter().map(|s| (s.path, s.content)).collect();
        Ok(hash_workflow_bundle(&workflow_json, &pairs))
    }

    /// 列出本机 workflows 的 origin 检查信息（供前端徽章）。
    pub fn list_local_origin_summaries(&self) -> Vec<WorkflowOriginSummary> {
        let dir = self.workflows_dir();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let origin = match read_origin_in_dir(&path) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let Some(mut origin) = origin else {
                continue;
            };
            let current = self.hash_existing_workflow_dir(&path).ok();
            if let Some(ref h) = current {
                origin.refresh_local_modified(h);
            }
            out.push(WorkflowOriginSummary {
                workflow_name: name,
                title: origin.title.clone(),
                source_host_display_name: origin.source_host_display_name.clone(),
                source_share_id: origin.source_share_id.clone(),
                source_host_node_id: origin.source_host_node_id.clone(),
                installed_content_hash: origin.installed_content_hash.clone(),
                current_content_hash: current,
                local_modified: origin.local_modified,
            });
        }
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowOriginSummary {
    pub workflow_name: String,
    pub title: String,
    pub source_host_display_name: String,
    pub source_share_id: String,
    pub source_host_node_id: String,
    pub installed_content_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_content_hash: Option<String>,
    pub local_modified: bool,
}

#[derive(Debug, Clone)]
struct LocalWorkflowDetail {
    workflow_name: String,
    title: String,
    description: String,
    node_count: usize,
    content_hash: String,
    workflow_json: String,
    skill_md: String,
    scripts: Vec<SharedWorkflowScript>,
    scripts_manifest_json: Option<String>,
}

fn group_allowed(group_id: &Option<String>, allowed: Option<&HashSet<String>>) -> bool {
    match (group_id, allowed) {
        // 本机清单：不过滤
        (_, None) => true,
        // 对端目录：未绑定协作组的条目不再视为全员公开
        (None, Some(_)) => false,
        (Some(gid), Some(set)) => set.contains(gid),
    }
}

fn sanitize_workflow_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || Path::new(name).components().count() != 1
    {
        return Err("非法 workflowName".to_string());
    }
    Ok(())
}

fn require_group_id(group_id: Option<String>) -> Result<String, String> {
    group_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "请选择要共享到的协作组（未选组的内容保持私有）".to_string())
}

fn sanitize_script_rel_path(path: &str) -> Result<PathBuf, String> {
    let path = path.replace('\\', "/");
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        return Err("脚本路径为空".to_string());
    }
    let mut out = PathBuf::new();
    for comp in Path::new(path).components() {
        match comp {
            Component::Normal(s) => out.push(s),
            Component::CurDir => {}
            _ => return Err(format!("非法脚本路径: {path}")),
        }
    }
    if out.as_os_str().is_empty() {
        return Err(format!("非法脚本路径: {path}"));
    }
    // 仅允许 scripts/ 下
    let first = out
        .components()
        .next()
        .and_then(|c| c.as_os_str().to_str())
        .unwrap_or("");
    if first != "scripts" {
        return Err(format!("脚本必须位于 scripts/ 下: {path}"));
    }
    Ok(out)
}

fn collect_workflow_scripts(workflow_dir: &Path) -> Result<Vec<SharedWorkflowScript>, String> {
    let scripts_root = workflow_dir.join("scripts");
    if !scripts_root.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect_files_recursively(&scripts_root, &scripts_root, &mut files)?;
    files.sort();
    if files.len() > MAX_SCRIPTS {
        return Err(format!(
            "脚本数量过多（{}，上限 {MAX_SCRIPTS}）",
            files.len()
        ));
    }
    let mut out = Vec::new();
    for abs in files {
        let rel = abs
            .strip_prefix(workflow_dir)
            .map_err(|_| "脚本路径解析失败".to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        let content =
            std::fs::read_to_string(&abs).map_err(|e| format!("读取脚本 {rel} 失败: {e}"))?;
        if content.chars().count() > MAX_SINGLE_SCRIPT_CHARS {
            return Err(format!(
                "脚本 {rel} 过大（上限 {MAX_SINGLE_SCRIPT_CHARS} 字符）"
            ));
        }
        out.push(SharedWorkflowScript { path: rel, content });
    }
    Ok(out)
}

fn collect_files_recursively(
    root: &Path,
    current: &Path,
    out: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(current).map_err(|e| format!("读取脚本目录失败: {e}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursively(root, &path, out)?;
        } else if path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

fn validate_payload_size(payload: &SharedWorkflowPayload) -> Result<(), String> {
    if payload.scripts.len() > MAX_SCRIPTS {
        return Err(format!(
            "脚本数量过多（{}，上限 {MAX_SCRIPTS}）",
            payload.scripts.len()
        ));
    }
    let mut total = payload.workflow_json.chars().count() + payload.skill_md.chars().count();
    for s in &payload.scripts {
        let n = s.content.chars().count();
        if n > MAX_SINGLE_SCRIPT_CHARS {
            return Err(format!(
                "脚本 {} 过大（上限 {MAX_SINGLE_SCRIPT_CHARS} 字符）",
                s.path
            ));
        }
        total += n;
        sanitize_script_rel_path(&s.path)?;
    }
    if total > MAX_WORKFLOW_TOTAL_CHARS {
        return Err(format!(
            "Workflow 内容过大（{total} 字符，上限 {MAX_WORKFLOW_TOTAL_CHARS}）"
        ));
    }
    if payload.workflow_json.trim().is_empty() {
        return Err("workflowJson 为空".to_string());
    }
    Ok(())
}

#[allow(dead_code)]
pub fn install_conflict_label(conflict: InstallConflict) -> &'static str {
    match conflict {
        InstallConflict::None => "none",
        InstallConflict::LocalOriginalExists => "local_original_exists",
        InstallConflict::SharedExists => "shared_exists",
        InstallConflict::SharedLocalModified => "shared_local_modified",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_sample_workflow(ws: &Path, name: &str, title: &str, body_marker: &str) {
        let dir = ws.join("workflows").join(name);
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        let def = serde_json::json!({
            "name": name,
            "title": title,
            "description": format!("desc {body_marker}"),
            "triggerPhrases": [],
            "createdAt": "2026-01-01T00:00:00Z",
            "variables": {},
            "nodes": [{
                "nodeId": "n1",
                "objective": body_marker,
                "tools": ["shell"],
                "dependsOn": []
            }]
        });
        std::fs::write(
            dir.join("workflow.json"),
            serde_json::to_string_pretty(&def).unwrap(),
        )
        .unwrap();
        std::fs::write(dir.join("SKILL.md"), format!("# {title}\n{body_marker}\n")).unwrap();
        std::fs::write(
            dir.join("scripts").join("run.py"),
            format!("print('{body_marker}')\n"),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn share_fetch_install_writes_origin() {
        let tmp = tempdir().unwrap();
        let host_ws = tmp.path().join("host");
        let client_ws = tmp.path().join("client");
        std::fs::create_dir_all(&host_ws).unwrap();
        std::fs::create_dir_all(&client_ws).unwrap();
        write_sample_workflow(&host_ws, "deploy", "Deploy", "v1");

        let host = WorkflowShareService::new(host_ws.clone());
        let cfg = host
            .share_workflow("deploy".into(), Some("grp_test".into()))
            .await
            .unwrap();
        let payload = host.fetch_share(&cfg.share_id).await.unwrap();
        assert!(!payload.content_hash.is_empty());
        assert_eq!(payload.scripts.len(), 1);

        let client = WorkflowShareService::new(client_ws.clone());
        let installed = client
            .install_workflow_payload(&payload, "node_host", "Alice", None, false, false, None)
            .unwrap();
        assert_eq!(installed, "deploy");
        let origin = read_origin_in_dir(&client_ws.join("workflows").join("deploy"))
            .unwrap()
            .unwrap();
        assert_eq!(origin.source_host_display_name, "Alice");
        assert!(!origin.local_modified);
        assert_eq!(origin.installed_content_hash, payload.content_hash);

        // 本地修改后应要求 forceOverwrite
        std::fs::write(
            client_ws
                .join("workflows")
                .join("deploy")
                .join("scripts")
                .join("run.py"),
            "print('local-edit')\n",
        )
        .unwrap();
        let err = client
            .install_workflow_payload(&payload, "node_host", "Alice", None, true, false, None)
            .unwrap_err();
        assert!(err.contains("forceOverwrite"), "{err}");

        let ok = client
            .install_workflow_payload(&payload, "node_host", "Alice", None, true, true, None)
            .unwrap();
        assert_eq!(ok, "deploy");
    }

    #[test]
    fn sanitize_script_path_blocks_escape() {
        assert!(sanitize_script_rel_path("../etc/passwd").is_err());
        assert!(sanitize_script_rel_path("workflow.json").is_err());
        assert_eq!(
            sanitize_script_rel_path("scripts/a/b.py").unwrap(),
            PathBuf::from("scripts/a/b.py")
        );
    }
}
