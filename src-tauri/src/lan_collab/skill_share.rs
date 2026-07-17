//! Skill 共享：广播本机技能目录，对端按需拉取 SKILL.md 并安装。
//!
//! 原则：
//! - 默认不共享任何本地 skill
//! - 首期仅共享 SKILL.md 正文（不含 scripts 等附属文件）
//! - 接收方需主动确认安装；安装到 workspace skills 目录

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

use super::types::SharedSkillOffer;

const MAX_SKILL_CONTENT_CHARS: usize = 400_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedSkillConfig {
    pub share_id: String,
    pub skill_id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub group_id: Option<String>,
    pub enabled: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedSkillPayload {
    pub share_id: String,
    pub skill_id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub content: String,
}

#[derive(Clone)]
pub struct SkillShareService {
    workspace_config_dir: PathBuf,
    shares: Arc<RwLock<Vec<SharedSkillConfig>>>,
}

impl SkillShareService {
    pub fn new(workspace_config_dir: PathBuf) -> Self {
        Self {
            workspace_config_dir,
            shares: Arc::new(RwLock::new(Vec::new())),
        }
    }

    fn skills_dir(&self) -> PathBuf {
        self.workspace_config_dir.join("skills")
    }

    pub async fn list_local_shares(&self) -> Vec<SharedSkillConfig> {
        self.shares.read().await.clone()
    }

    pub async fn local_offers(
        &self,
        host_node_id: &str,
        host_display_name: &str,
        allowed_group_ids: Option<&HashSet<String>>,
    ) -> Vec<SharedSkillOffer> {
        let guard = self.shares.read().await;
        guard
            .iter()
            .filter(|s| s.enabled)
            .filter(|s| match (&s.group_id, allowed_group_ids) {
                (None, _) => true,
                (Some(_), None) => true,
                (Some(gid), Some(allowed)) => allowed.contains(gid),
            })
            .map(|s| SharedSkillOffer {
                share_id: s.share_id.clone(),
                host_node_id: host_node_id.to_string(),
                host_display_name: host_display_name.to_string(),
                skill_id: s.skill_id.clone(),
                name: s.name.clone(),
                description: s.description.clone(),
                tags: s.tags.clone(),
                group_id: s.group_id.clone(),
                online: true,
            })
            .collect()
    }

    pub async fn share_skill(
        &self,
        skill_id: String,
        group_id: Option<String>,
    ) -> Result<SharedSkillConfig, String> {
        let skill_id = skill_id.trim().to_string();
        if skill_id.is_empty() {
            return Err("skillId 不能为空".to_string());
        }
        sanitize_skill_id(&skill_id)?;

        let detail = self.read_local_skill(&skill_id)?;
        {
            let guard = self.shares.read().await;
            if guard
                .iter()
                .any(|s| s.enabled && s.skill_id == skill_id)
            {
                return Err("该 Skill 已在共享中".to_string());
            }
        }

        let cfg = SharedSkillConfig {
            share_id: format!("sshare_{}", Uuid::new_v4().simple()),
            skill_id: detail.skill_id,
            name: detail.name,
            description: detail.description,
            tags: detail.tags,
            group_id: group_id
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            enabled: true,
            created_at: chrono::Utc::now().timestamp(),
        };
        self.shares.write().await.push(cfg.clone());
        Ok(cfg)
    }

    pub async fn unshare_skill(&self, share_id: String) -> Result<(), String> {
        let mut guard = self.shares.write().await;
        let before = guard.len();
        guard.retain(|s| s.share_id != share_id);
        if guard.len() == before {
            return Err("未找到该 Skill 共享项".to_string());
        }
        Ok(())
    }

    pub async fn fetch_share(&self, share_id: &str) -> Result<SharedSkillPayload, String> {
        let cfg = {
            let guard = self.shares.read().await;
            guard
                .iter()
                .find(|s| s.enabled && s.share_id == share_id)
                .cloned()
                .ok_or_else(|| "Skill 共享项不存在或已撤销".to_string())?
        };
        let detail = self.read_local_skill(&cfg.skill_id)?;
        Ok(SharedSkillPayload {
            share_id: cfg.share_id,
            skill_id: detail.skill_id,
            name: detail.name,
            description: detail.description,
            tags: detail.tags,
            content: detail.content,
        })
    }

    pub fn install_skill_payload(
        &self,
        payload: &SharedSkillPayload,
        overwrite: bool,
    ) -> Result<String, String> {
        let skill_id = payload.skill_id.trim().to_string();
        sanitize_skill_id(&skill_id)?;
        if payload.content.trim().is_empty() {
            return Err("Skill 内容为空".to_string());
        }
        if payload.content.chars().count() > MAX_SKILL_CONTENT_CHARS {
            return Err(format!(
                "Skill 内容过大（最多 {MAX_SKILL_CONTENT_CHARS} 字符）"
            ));
        }

        let dir = self.skills_dir().join(&skill_id);
        if dir.exists() && !overwrite {
            return Err(format!("本机已存在同名 Skill：{skill_id}"));
        }
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建 Skill 目录失败: {e}"))?;
        let skill_md = dir.join("SKILL.md");
        std::fs::write(&skill_md, &payload.content)
            .map_err(|e| format!("写入 SKILL.md 失败: {e}"))?;
        Ok(skill_id)
    }

    fn read_local_skill(&self, skill_id: &str) -> Result<LocalSkillDetail, String> {
        sanitize_skill_id(skill_id)?;
        let skill_md = self.skills_dir().join(skill_id).join("SKILL.md");
        if !skill_md.exists() {
            return Err(format!("Skill 不存在: {skill_id}"));
        }
        let content = std::fs::read_to_string(&skill_md)
            .map_err(|e| format!("读取 Skill 失败: {e}"))?;
        if content.chars().count() > MAX_SKILL_CONTENT_CHARS {
            return Err(format!(
                "Skill 内容过大（最多 {MAX_SKILL_CONTENT_CHARS} 字符）"
            ));
        }
        let (name, description, tags) = parse_skill_frontmatter(&content);
        Ok(LocalSkillDetail {
            skill_id: skill_id.to_string(),
            name: if name.is_empty() {
                skill_id.to_string()
            } else {
                name
            },
            description,
            tags,
            content,
        })
    }
}

#[derive(Debug, Clone)]
struct LocalSkillDetail {
    skill_id: String,
    name: String,
    description: String,
    tags: Vec<String>,
    content: String,
}

fn sanitize_skill_id(skill_id: &str) -> Result<(), String> {
    if skill_id.is_empty()
        || skill_id.contains('/')
        || skill_id.contains('\\')
        || skill_id.contains("..")
        || Path::new(skill_id).components().count() != 1
    {
        return Err("非法 skillId".to_string());
    }
    Ok(())
}

fn parse_skill_frontmatter(content: &str) -> (String, String, Vec<String>) {
    let mut name = String::new();
    let mut description = String::new();
    let mut tags = Vec::new();

    if content.starts_with("---") {
        if let Some(end) = content[3..].find("---") {
            let front = &content[3..3 + end];
            for line in front.lines() {
                let line = line.trim();
                if let Some(val) = line.strip_prefix("name:") {
                    name = val.trim().trim_matches('"').to_string();
                } else if let Some(val) = line.strip_prefix("description:") {
                    description = val.trim().trim_matches('"').to_string();
                } else if let Some(val) = line.strip_prefix("tags:") {
                    let val = val.trim();
                    if val.starts_with('[') {
                        tags = val
                            .trim_matches(|c| c == '[' || c == ']')
                            .split(',')
                            .map(|s| s.trim().trim_matches('"').to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                }
            }
        }
    }

    (name, description, tags)
}
