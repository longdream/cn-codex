//! 知识库共享：本机授权视图 + 对端按需检索/拉取。
//!
//! 原则：
//! - 默认不共享任何本地知识
//! - 共享的是授权视图，不是裸盘全量拷贝
//! - 检索在共享方执行，接收方按需拉取正文
//! - 无中心服务器；撤销后停止响应

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::smartbrain::bm25_index::{SearchFilter, SourceType};
use crate::smartbrain::knowledge::KnowledgeIndex;
use crate::smartbrain::search::{self, SmartBrainSearchResult};
use crate::smartbrain::{self, knowledge_dir};

use super::types::{
    RemoteKnowledgeDoc, RemoteKnowledgeHit, SharedKnowledgeDocMeta, SharedKnowledgeOffer,
};

const MAX_OFFER_DOCS: usize = 40;
const MAX_SEARCH_TOP_K: usize = 20;
const DEFAULT_SEARCH_TOP_K: usize = 8;
const MAX_FETCH_CHARS: usize = 200_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedKnowledgeConfig {
    pub share_id: String,
    pub title: String,
    pub group_id: Option<String>,
    /// 仅共享指定 source_group（可选）
    pub source_group: Option<String>,
    /// 仅共享指定 domain（可选）
    pub domain: Option<String>,
    /// 仅共享指定文档；空表示在其它过滤条件下共享全部 knowledge
    #[serde(default)]
    pub doc_ids: Vec<String>,
    /// search_and_read（首期固定）
    pub permission: String,
    pub enabled: bool,
    pub created_at: i64,
}

#[derive(Clone)]
pub struct KnowledgeShareService {
    workspace_config_dir: PathBuf,
    shares: Arc<RwLock<Vec<SharedKnowledgeConfig>>>,
}

impl KnowledgeShareService {
    pub fn new(workspace_config_dir: PathBuf) -> Self {
        Self {
            workspace_config_dir,
            shares: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn list_local_shares(&self) -> Vec<SharedKnowledgeConfig> {
        self.shares.read().await.clone()
    }

    pub async fn local_offers(
        &self,
        host_node_id: &str,
        host_display_name: &str,
        allowed_group_ids: Option<&HashSet<String>>,
    ) -> Vec<SharedKnowledgeOffer> {
        let guard = self.shares.read().await;
        guard
            .iter()
            .filter(|s| s.enabled)
            .filter(|s| match (&s.group_id, allowed_group_ids) {
                (None, _) => true,
                (Some(_), None) => true,
                (Some(gid), Some(allowed)) => allowed.contains(gid),
            })
            .map(|s| self.config_to_offer(s, host_node_id, host_display_name))
            .collect()
    }

    pub async fn share_knowledge(
        &self,
        title: String,
        group_id: Option<String>,
        source_group: Option<String>,
        domain: Option<String>,
        doc_ids: Vec<String>,
    ) -> Result<SharedKnowledgeConfig, String> {
        let title = title.trim().to_string();
        let title = if title.is_empty() {
            "共享知识库".to_string()
        } else {
            title
        };
        let source_group = source_group
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let domain = domain
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let doc_ids: Vec<String> = doc_ids
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        // 至少要有可共享文档
        let docs = self.list_shareable_docs(&source_group, &domain, &doc_ids)?;
        if docs.is_empty() {
            return Err("没有可共享的知识文档，请先在本地知识库导入内容".to_string());
        }

        let cfg = SharedKnowledgeConfig {
            share_id: format!("kshare_{}", Uuid::new_v4().simple()),
            title,
            group_id: group_id
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            source_group,
            domain,
            doc_ids,
            permission: "search_and_read".to_string(),
            enabled: true,
            created_at: chrono::Utc::now().timestamp(),
        };
        self.shares.write().await.push(cfg.clone());
        Ok(cfg)
    }

    pub async fn unshare_knowledge(&self, share_id: String) -> Result<(), String> {
        let mut guard = self.shares.write().await;
        let before = guard.len();
        guard.retain(|s| s.share_id != share_id);
        if guard.len() == before {
            return Err("未找到该知识共享项".to_string());
        }
        Ok(())
    }

    pub fn list_shareable_docs(
        &self,
        source_group: &Option<String>,
        domain: &Option<String>,
        doc_ids: &[String],
    ) -> Result<Vec<SharedKnowledgeDocMeta>, String> {
        let kdir = knowledge_dir(&self.workspace_config_dir);
        let index = KnowledgeIndex::load(&kdir);
        let id_filter: Option<HashSet<&str>> = if doc_ids.is_empty() {
            None
        } else {
            Some(doc_ids.iter().map(|s| s.as_str()).collect())
        };

        let mut docs: Vec<SharedKnowledgeDocMeta> = index
            .entries
            .into_iter()
            .filter(|e| {
                if let Some(ids) = &id_filter {
                    if !ids.contains(e.doc_id.as_str()) {
                        return false;
                    }
                }
                if let Some(sg) = source_group {
                    if e.source_group.as_deref() != Some(sg.as_str()) {
                        return false;
                    }
                }
                if let Some(dom) = domain {
                    if e.domain.as_deref() != Some(dom.as_str()) {
                        return false;
                    }
                }
                true
            })
            .map(|e| SharedKnowledgeDocMeta {
                doc_id: e.doc_id,
                title: e.title,
                domain: e.domain,
                source_group: e.source_group,
                added_at: e.added_at,
                chunk_count: e.chunk_count,
            })
            .collect();
        docs.sort_by(|a, b| b.added_at.cmp(&a.added_at));
        Ok(docs)
    }

    pub async fn search_share(
        &self,
        share_id: &str,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<RemoteKnowledgeHit>, String> {
        let query = query.trim();
        if query.is_empty() {
            return Err("检索词不能为空".to_string());
        }
        let cfg = {
            let guard = self.shares.read().await;
            guard
                .iter()
                .find(|s| s.share_id == share_id && s.enabled)
                .cloned()
                .ok_or_else(|| "知识共享不存在或已撤销".to_string())?
        };

        let top_k = top_k.clamp(1, MAX_SEARCH_TOP_K);
        let bm25_path = smartbrain::bm25_index_path(&self.workspace_config_dir);

        // 先按 knowledge + 可选 domain/source_group 过滤检索
        let filter = SearchFilter {
            concept_type: None,
            tags: Vec::new(),
            domain: cfg.domain.clone(),
            source_group: cfg.source_group.clone(),
            relative_path_prefix: None,
            source_file: None,
            source_type: Some(SourceType::Knowledge),
            timestamp_after: None,
            timestamp_before: None,
        };
        let mut results = search::unified_search_with_filter(&bm25_path, query, top_k.max(DEFAULT_SEARCH_TOP_K) * 2, filter);

        // 若指定了 doc_ids，进一步收敛到这些文档（含 chunk parent）
        if !cfg.doc_ids.is_empty() {
            let allowed: HashSet<&str> = cfg.doc_ids.iter().map(|s| s.as_str()).collect();
            results.retain(|r| {
                let parent = r.parent_doc_id.as_deref().unwrap_or(r.doc_id.as_str());
                // chunk doc_id 形如 parent::chunk:n 时也尝试剥离
                let base = strip_chunk_suffix(&r.doc_id);
                allowed.contains(parent) || allowed.contains(base) || allowed.contains(r.doc_id.as_str())
            });
        }

        results.truncate(top_k);
        Ok(results
            .into_iter()
            .map(|r| hit_from_search(share_id, "", "", r))
            .collect())
    }

    pub async fn fetch_doc(
        &self,
        share_id: &str,
        doc_id: &str,
    ) -> Result<RemoteKnowledgeDoc, String> {
        let doc_id = doc_id.trim();
        if doc_id.is_empty() {
            return Err("doc_id 不能为空".to_string());
        }
        let cfg = {
            let guard = self.shares.read().await;
            guard
                .iter()
                .find(|s| s.share_id == share_id && s.enabled)
                .cloned()
                .ok_or_else(|| "知识共享不存在或已撤销".to_string())?
        };

        let base_doc_id = strip_chunk_suffix(doc_id).to_string();
        if !cfg.doc_ids.is_empty()
            && !cfg.doc_ids.iter().any(|id| id == &base_doc_id || id == doc_id)
        {
            return Err("该文档不在共享范围内".to_string());
        }

        let kdir = knowledge_dir(&self.workspace_config_dir);
        let index = KnowledgeIndex::load(&kdir);
        let entry = index
            .entries
            .iter()
            .find(|e| e.doc_id == base_doc_id)
            .ok_or_else(|| "文档不存在".to_string())?;

        if let Some(sg) = &cfg.source_group {
            if entry.source_group.as_deref() != Some(sg.as_str()) {
                return Err("该文档不在共享来源组内".to_string());
            }
        }
        if let Some(dom) = &cfg.domain {
            if entry.domain.as_deref() != Some(dom.as_str()) {
                return Err("该文档不在共享 domain 内".to_string());
            }
        }

        let (title, content, tags, domain) = read_knowledge_doc(&kdir, &base_doc_id)?;
        Ok(RemoteKnowledgeDoc {
            share_id: share_id.to_string(),
            host_node_id: String::new(),
            host_display_name: String::new(),
            doc_id: base_doc_id,
            title: if title.is_empty() {
                entry.title.clone()
            } else {
                title
            },
            content,
            domain: domain.or_else(|| entry.domain.clone()),
            tags,
            source_group: entry.source_group.clone(),
        })
    }

    fn config_to_offer(
        &self,
        cfg: &SharedKnowledgeConfig,
        host_node_id: &str,
        host_display_name: &str,
    ) -> SharedKnowledgeOffer {
        let docs = self
            .list_shareable_docs(&cfg.source_group, &cfg.domain, &cfg.doc_ids)
            .unwrap_or_default();
        let doc_count = docs.len();
        let preview_docs = docs.into_iter().take(MAX_OFFER_DOCS).collect();
        SharedKnowledgeOffer {
            share_id: cfg.share_id.clone(),
            host_node_id: host_node_id.to_string(),
            host_display_name: host_display_name.to_string(),
            title: cfg.title.clone(),
            group_id: cfg.group_id.clone(),
            permission: cfg.permission.clone(),
            source_group: cfg.source_group.clone(),
            domain: cfg.domain.clone(),
            doc_count,
            docs: preview_docs,
            online: true,
        }
    }
}

fn strip_chunk_suffix(doc_id: &str) -> &str {
    if let Some(idx) = doc_id.find("::chunk:") {
        &doc_id[..idx]
    } else if let Some(idx) = doc_id.find("__chunk_") {
        &doc_id[..idx]
    } else {
        doc_id
    }
}

fn hit_from_search(
    share_id: &str,
    host_node_id: &str,
    host_display_name: &str,
    r: SmartBrainSearchResult,
) -> RemoteKnowledgeHit {
    RemoteKnowledgeHit {
        share_id: share_id.to_string(),
        host_node_id: host_node_id.to_string(),
        host_display_name: host_display_name.to_string(),
        doc_id: r.parent_doc_id.clone().unwrap_or_else(|| strip_chunk_suffix(&r.doc_id).to_string()),
        title: r.title,
        score: r.score,
        domain: r.domain,
        source_group: r.source_group,
        tags: r.tags,
        is_chunk: r.is_chunk,
        chunk_index: r.chunk_index,
    }
}

fn read_knowledge_doc(
    knowledge_dir: &Path,
    doc_id: &str,
) -> Result<(String, String, Vec<String>, Option<String>), String> {
    let doc_path = knowledge_dir.join("docs").join(format!("{doc_id}.md"));
    if !doc_path.is_file() {
        return Err(format!("文档文件不存在: {doc_id}"));
    }
    let raw = std::fs::read_to_string(&doc_path).map_err(|e| format!("读取文档失败: {e}"))?;
    if let Some(doc) = smartbrain::okf::parse_document(&raw) {
        let mut content = doc.body;
        if content.chars().count() > MAX_FETCH_CHARS {
            content = content.chars().take(MAX_FETCH_CHARS).collect::<String>()
                + "\n\n…(内容过长，已截断)";
        }
        let domain = doc
            .frontmatter
            .extensions
            .get("domain")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        Ok((
            doc.frontmatter.title.unwrap_or_default(),
            content,
            doc.frontmatter.tags,
            domain,
        ))
    } else {
        let mut content = raw;
        if content.chars().count() > MAX_FETCH_CHARS {
            content = content.chars().take(MAX_FETCH_CHARS).collect::<String>()
                + "\n\n…(内容过长，已截断)";
        }
        Ok((String::new(), content, Vec::new(), None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_doc(kdir: &Path, doc_id: &str, title: &str, body: &str) {
        let docs = kdir.join("docs");
        fs::create_dir_all(&docs).unwrap();
        let content = format!(
            "---\ntype: Knowledge\ntitle: {title}\ndomain: test\n---\n\n{body}\n"
        );
        fs::write(docs.join(format!("{doc_id}.md")), content).unwrap();
    }

    fn write_index(kdir: &Path, entries: &[(&str, &str)]) {
        let index = serde_json::json!({
            "version": 1,
            "entries": entries.iter().map(|(id, title)| serde_json::json!({
                "doc_id": id,
                "source_file": format!("{id}.md"),
                "source_type": "markdown",
                "title": title,
                "added_at": 1,
                "chunk_count": 1,
                "categories": [],
                "domain": "test",
            })).collect::<Vec<_>>()
        });
        fs::create_dir_all(kdir).unwrap();
        fs::write(kdir.join("index.json"), serde_json::to_string_pretty(&index).unwrap()).unwrap();
    }

    #[tokio::test]
    async fn share_list_and_fetch_local_knowledge() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().to_path_buf();
        let kdir = workspace.join("memories").join("knowledge");
        write_index(&kdir, &[("doc_a", "Alpha Doc"), ("doc_b", "Beta Doc")]);
        write_doc(&kdir, "doc_a", "Alpha Doc", "hello alpha knowledge");
        write_doc(&kdir, "doc_b", "Beta Doc", "hello beta knowledge");

        let service = KnowledgeShareService::new(workspace);
        let docs = service.list_shareable_docs(&None, &None, &[]).unwrap();
        assert_eq!(docs.len(), 2);

        let cfg = service
            .share_knowledge(
                "Team KB".to_string(),
                None,
                None,
                None,
                vec!["doc_a".to_string()],
            )
            .await
            .unwrap();
        assert!(cfg.share_id.starts_with("kshare_"));

        let offers = service
            .local_offers("node_a", "Alice", None)
            .await;
        assert_eq!(offers.len(), 1);
        assert_eq!(offers[0].title, "Team KB");
        assert_eq!(offers[0].doc_count, 1);

        let fetched = service.fetch_doc(&cfg.share_id, "doc_a").await.unwrap();
        assert!(fetched.content.contains("hello alpha knowledge"));

        // 不在共享范围
        let denied = service.fetch_doc(&cfg.share_id, "doc_b").await;
        assert!(denied.is_err());

        service.unshare_knowledge(cfg.share_id.clone()).await.unwrap();
        let after = service.fetch_doc(&cfg.share_id, "doc_a").await;
        assert!(after.is_err());
    }

    #[test]
    fn strip_chunk_suffix_works() {
        assert_eq!(strip_chunk_suffix("doc_a::chunk:2"), "doc_a");
        assert_eq!(strip_chunk_suffix("doc_a__chunk_2"), "doc_a");
        assert_eq!(strip_chunk_suffix("doc_a"), "doc_a");
    }
}
