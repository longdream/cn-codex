use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::Path;

use serde::Serialize;

use super::bm25_index::{BM25Index, IndexedDocument, SearchFilter, SearchResult, SourceType};

const RECALL_MULTIPLIER: usize = 3;
const MIN_CANDIDATE_K: usize = 10;
const STAGE1_MIN_RECALL_K: usize = 20;
const STAGE2_MAX_CANDIDATES: usize = 50;
const STAGE2_MAX_ANCHORS: usize = 5;
const DOMAIN_EXPANSION_LIMIT_PER_ANCHOR: usize = 8;
const TAG_EXPANSION_LIMIT_PER_ANCHOR: usize = 6;
const SOURCE_TYPE_EXPANSION_LIMIT_PER_ANCHOR: usize = 4;
const DEFAULT_OKF_PREFILTER_ORDER: [&str; 6] = [
    "domain",
    "tags",
    "source_type",
    "source_group",
    "relative_path_prefix",
    "source_file",
];

#[derive(Debug, Clone, Serialize)]
pub struct SmartBrainSearchResult {
    pub doc_id: String,
    pub source_type: String,
    pub file_path: String,
    pub title: String,
    pub score: f64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concept_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relative_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_doc_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_total: Option<usize>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub is_chunk: bool,
}

impl SmartBrainSearchResult {
    fn from_result(r: SearchResult, index: &BM25Index) -> Self {
        let (
            tags,
            concept_type,
            domain,
            source_group,
            relative_path,
            source_file,
            parent_doc_id,
            chunk_index,
            chunk_total,
            is_chunk,
        ) = index
            .documents
            .iter()
            .find(|d| d.doc_id == r.doc_id)
            .map(|d| {
                (
                    d.tags.clone(),
                    d.concept_type.clone(),
                    d.domain.clone(),
                    d.source_group.clone(),
                    d.relative_path.clone(),
                    d.source_file.clone(),
                    d.parent_doc_id.clone(),
                    d.chunk_index,
                    d.chunk_total,
                    d.is_chunk,
                )
            })
            .unwrap_or_default();

        Self {
            doc_id: r.doc_id,
            source_type: match r.source_type {
                SourceType::Experience => "experience".to_string(),
                SourceType::Knowledge => "knowledge".to_string(),
            },
            file_path: r.file_path,
            title: r.title,
            score: r.score,
            tags,
            concept_type,
            domain,
            source_group,
            relative_path,
            source_file,
            parent_doc_id,
            chunk_index,
            chunk_total,
            is_chunk,
        }
    }
}

fn expanded_candidate_k(top_k: usize) -> usize {
    if top_k == 0 {
        0
    } else {
        top_k.saturating_mul(RECALL_MULTIPLIER).max(MIN_CANDIDATE_K)
    }
}

fn stage1_recall_k(top_k: usize) -> usize {
    expanded_candidate_k(top_k).max(STAGE1_MIN_RECALL_K)
}

fn find_document_by_id<'a>(index: &'a BM25Index, doc_id: &str) -> Option<&'a IndexedDocument> {
    index.documents.iter().find(|doc| doc.doc_id == doc_id)
}

fn extend_candidates_by_predicate<F>(
    index: &BM25Index,
    candidate_doc_ids: &mut HashSet<String>,
    candidate_scope: Option<&HashSet<String>>,
    limit: usize,
    mut predicate: F,
) -> usize
where
    F: FnMut(&IndexedDocument) -> bool,
{
    if limit == 0 || candidate_doc_ids.len() >= STAGE2_MAX_CANDIDATES {
        return 0;
    }

    let mut added = 0usize;
    for doc in &index.documents {
        if candidate_doc_ids.len() >= STAGE2_MAX_CANDIDATES || added >= limit {
            break;
        }
        if candidate_scope.is_some_and(|scope| !scope.contains(&doc.doc_id)) {
            continue;
        }
        if !predicate(doc) {
            continue;
        }
        if candidate_doc_ids.insert(doc.doc_id.clone()) {
            added += 1;
        }
    }
    added
}

fn build_second_stage_candidate_pool(
    index: &BM25Index,
    stage1_results: &[SearchResult],
    candidate_scope: Option<&HashSet<String>>,
) -> (HashSet<String>, bool) {
    let mut candidate_doc_ids: HashSet<String> = stage1_results
        .iter()
        .take(STAGE2_MAX_CANDIDATES)
        .map(|result| result.doc_id.clone())
        .collect();
    if let Some(scope) = candidate_scope {
        candidate_doc_ids.retain(|doc_id| scope.contains(doc_id));
    }
    let base_count = candidate_doc_ids.len();

    // 两阶段策略核心：
    // 1) 先用第一阶段高分结果作为“锚点”；
    // 2) 再从锚点的结构化元信息扩展候选；
    // 3) 只扩展局部邻域，避免逐层全扫导致噪音上升。
    for anchor in stage1_results.iter().take(STAGE2_MAX_ANCHORS) {
        if candidate_doc_ids.len() >= STAGE2_MAX_CANDIDATES {
            break;
        }

        let Some(anchor_doc) = find_document_by_id(index, &anchor.doc_id) else {
            continue;
        };

        // 优先级 1：同 domain（最强语义邻域）。
        if let Some(anchor_domain) = anchor_doc
            .domain
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            extend_candidates_by_predicate(
                index,
                &mut candidate_doc_ids,
                candidate_scope,
                DOMAIN_EXPANSION_LIMIT_PER_ANCHOR,
                |doc| {
                    doc.doc_id != anchor_doc.doc_id && doc.domain.as_deref() == Some(anchor_domain)
                },
            );
        }

        // 优先级 2：共享 tags（补齐同主题横向文档）。
        if !anchor_doc.tags.is_empty() {
            let anchor_tags: HashSet<&str> = anchor_doc.tags.iter().map(String::as_str).collect();
            extend_candidates_by_predicate(
                index,
                &mut candidate_doc_ids,
                candidate_scope,
                TAG_EXPANSION_LIMIT_PER_ANCHOR,
                |doc| {
                    doc.doc_id != anchor_doc.doc_id
                        && doc
                            .tags
                            .iter()
                            .any(|tag| anchor_tags.contains(tag.as_str()))
                },
            );
        }

        // 优先级 3：同 source_type（弱扩展，主要用于稀疏元数据兜底）。
        extend_candidates_by_predicate(
            index,
            &mut candidate_doc_ids,
            candidate_scope,
            SOURCE_TYPE_EXPANSION_LIMIT_PER_ANCHOR,
            |doc| doc.doc_id != anchor_doc.doc_id && doc.source_type == anchor_doc.source_type,
        );
    }

    let expanded = candidate_doc_ids.len() > base_count;
    (candidate_doc_ids, expanded)
}

fn two_stage_recall(
    index: &BM25Index,
    query: &str,
    top_k: usize,
    stage1_results: Vec<SearchResult>,
    filter: Option<&SearchFilter>,
    candidate_scope: Option<&HashSet<String>>,
) -> Vec<SearchResult> {
    if stage1_results.len() <= 1 {
        return stage1_results;
    }

    let (candidate_doc_ids, expanded) =
        build_second_stage_candidate_pool(index, &stage1_results, candidate_scope);
    if !expanded {
        // 元信息不足导致无法扩展时，回退到第一阶段结果，保证行为稳定。
        return stage1_results;
    }

    let stage2_k = stage1_results.len().min(STAGE2_MAX_CANDIDATES).max(top_k);
    let stage2_results = match filter {
        Some(active_filter) => {
            index.search_subset_with_filter(query, stage2_k, &candidate_doc_ids, active_filter)
        }
        None => index.search_subset(query, stage2_k, &candidate_doc_ids),
    };

    if stage2_results.is_empty() {
        // 二阶段异常退化（例如 query 极短）时，继续使用第一阶段结果兜底。
        stage1_results
    } else {
        stage2_results
    }
}

fn all_doc_ids(index: &BM25Index) -> HashSet<String> {
    index.documents.iter().map(|doc| doc.doc_id.clone()).collect()
}

fn normalized_okf_prefilter_order(order: Option<&[String]>) -> Vec<String> {
    if let Some(custom) = order {
        let normalized = custom
            .iter()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| {
                matches!(
                    value.as_str(),
                    "domain"
                        | "tags"
                        | "source_type"
                        | "source_group"
                        | "relative_path_prefix"
                        | "source_file"
                )
            })
            .collect::<Vec<_>>();
        if !normalized.is_empty() {
            return normalized;
        }
    }
    DEFAULT_OKF_PREFILTER_ORDER
        .iter()
        .map(|value| value.to_string())
        .collect()
}

fn stage0_okf_prefilter_candidates(
    index: &BM25Index,
    filter: &SearchFilter,
    prefilter_order: Option<&[String]>,
) -> HashSet<String> {
    if filter.is_empty() {
        return all_doc_ids(index);
    }

    let mut candidates = all_doc_ids(index);
    let mut applied = false;
    for key in normalized_okf_prefilter_order(prefilter_order) {
        let narrowed = index
            .documents
            .iter()
            .filter(|doc| candidates.contains(&doc.doc_id))
            .filter(|doc| match key.as_str() {
                "domain" => filter
                    .domain
                    .as_deref()
                    .is_none_or(|domain| doc.domain.as_deref() == Some(domain)),
                "tags" => {
                    if filter.tags.is_empty() {
                        true
                    } else {
                        filter.tags.iter().any(|tag| doc.tags.contains(tag))
                    }
                }
                "source_type" => filter
                    .source_type
                    .is_none_or(|source_type| doc.source_type == source_type),
                "source_group" => filter
                    .source_group
                    .as_deref()
                    .is_none_or(|group| doc.source_group.as_deref() == Some(group)),
                "relative_path_prefix" => filter.relative_path_prefix.as_deref().is_none_or(
                    |prefix| {
                        doc.relative_path
                            .as_deref()
                            .is_some_and(|path| path.starts_with(prefix))
                    },
                ),
                "source_file" => filter
                    .source_file
                    .as_deref()
                    .is_none_or(|source_file| doc.source_file.as_deref() == Some(source_file)),
                _ => true,
            })
            .map(|doc| doc.doc_id.clone())
            .collect::<HashSet<_>>();
        if narrowed.is_empty() {
            continue;
        }
        candidates = narrowed;
        applied = true;
    }

    if !applied {
        let strict = index
            .documents
            .iter()
            .filter(|doc| filter.matches(doc))
            .map(|doc| doc.doc_id.clone())
            .collect::<HashSet<_>>();
        if !strict.is_empty() {
            return strict;
        }
        return all_doc_ids(index);
    }

    candidates
}

fn workspace_config_dir_from_bm25_path(bm25_index_path: &Path) -> Option<&Path> {
    bm25_index_path.parent()?.parent()
}

fn experience_thread_id(doc_id: &str) -> Option<&str> {
    doc_id.strip_prefix("exp:")
}

fn experience_priority_score(index: &super::index::ExperienceIndex, doc_id: &str) -> f64 {
    experience_thread_id(doc_id)
        .and_then(|thread_id| index.priority_score(thread_id))
        .unwrap_or(0.0)
}

fn rerank_experience_candidates(
    bm25_index_path: &Path,
    mut candidates: Vec<SearchResult>,
) -> Vec<SearchResult> {
    if candidates.len() <= 1 {
        return candidates;
    }

    let Some(workspace_config_dir) = workspace_config_dir_from_bm25_path(bm25_index_path) else {
        return candidates;
    };

    let experiences_dir = super::experiences_dir(workspace_config_dir);
    let experience_index = super::index::ExperienceIndex::load(&experiences_dir);

    let mut sorted_experiences: Vec<SearchResult> = candidates
        .iter()
        .filter(|r| r.source_type == SourceType::Experience)
        .cloned()
        .collect();

    if sorted_experiences.len() <= 1 {
        return candidates;
    }

    sorted_experiences.sort_by(|a, b| {
        let priority_a = experience_priority_score(&experience_index, &a.doc_id);
        let priority_b = experience_priority_score(&experience_index, &b.doc_id);

        priority_b
            .partial_cmp(&priority_a)
            .unwrap_or(Ordering::Equal)
            .then_with(|| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal))
            .then_with(|| a.doc_id.cmp(&b.doc_id))
    });

    let mut sorted_iter = sorted_experiences.into_iter();
    for candidate in &mut candidates {
        if candidate.source_type == SourceType::Experience {
            if let Some(next) = sorted_iter.next() {
                *candidate = next;
            }
        }
    }

    candidates
}

fn okf_extension_string(frontmatter: &super::okf::OkfFrontmatter, key: &str) -> Option<String> {
    frontmatter
        .extensions
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn okf_extension_usize(frontmatter: &super::okf::OkfFrontmatter, key: &str) -> Option<usize> {
    frontmatter
        .extensions
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .map(|value| value as usize)
}

/// Search both experience and knowledge using BM25.
pub fn unified_search(
    bm25_index_path: &Path,
    query: &str,
    top_k: usize,
) -> Vec<SmartBrainSearchResult> {
    if top_k == 0 {
        return Vec::new();
    }

    let index = BM25Index::load(bm25_index_path);
    // 第一阶段：全局高召回，确保不会错过明显相关文档。
    let stage1_results = index.search(query, stage1_recall_k(top_k));
    // 第二阶段：局部结构化扩展 + 候选池重排。
    let recalled = two_stage_recall(&index, query, top_k, stage1_results, None, None);
    rerank_experience_candidates(bm25_index_path, recalled)
        .into_iter()
        .take(top_k)
        .map(|r| SmartBrainSearchResult::from_result(r, &index))
        .collect()
}

/// Search with structured filter on OKF metadata.
pub fn unified_search_with_filter(
    bm25_index_path: &Path,
    query: &str,
    top_k: usize,
    filter: SearchFilter,
) -> Vec<SmartBrainSearchResult> {
    unified_search_with_filter_with_policy(
        bm25_index_path,
        query,
        top_k,
        filter,
        true,
        None,
    )
}

pub fn unified_search_with_filter_with_policy(
    bm25_index_path: &Path,
    query: &str,
    top_k: usize,
    filter: SearchFilter,
    okf_prefilter_enabled: bool,
    okf_prefilter_order: Option<&[String]>,
) -> Vec<SmartBrainSearchResult> {
    if top_k == 0 {
        return Vec::new();
    }

    let index = BM25Index::load(bm25_index_path);
    let mut candidate_scope: Option<HashSet<String>> = None;
    let mut active_filter: Option<&SearchFilter> = Some(&filter);
    // Stage-0: 基于 OKF 元信息先做候选文档分流，再进入 BM25 排序。
    let mut stage1_results = if okf_prefilter_enabled {
        let stage0_candidates = stage0_okf_prefilter_candidates(&index, &filter, okf_prefilter_order);
        if stage0_candidates.len() < index.documents.len() {
            let narrowed =
                index.search_subset_with_filter(query, stage1_recall_k(top_k), &stage0_candidates, &filter);
            if narrowed.is_empty() {
                // 预筛选过严时回退，避免空召回。
                index.search_with_filter(query, stage1_recall_k(top_k), &filter)
            } else {
                candidate_scope = Some(stage0_candidates);
                narrowed
            }
        } else {
            index.search_with_filter(query, stage1_recall_k(top_k), &filter)
        }
    } else {
        index.search_with_filter(query, stage1_recall_k(top_k), &filter)
    };
    if stage1_results.is_empty() {
        // 过滤条件过严导致无结果时，回退到全局 BM25，保证检索可用性。
        stage1_results = index.search(query, stage1_recall_k(top_k));
        candidate_scope = None;
        active_filter = None;
    }
    let recalled = two_stage_recall(
        &index,
        query,
        top_k,
        stage1_results,
        active_filter,
        candidate_scope.as_ref(),
    );
    rerank_experience_candidates(bm25_index_path, recalled)
        .into_iter()
        .take(top_k)
        .map(|r| SmartBrainSearchResult::from_result(r, &index))
        .collect()
}

/// Rebuild the BM25 index from all experience and knowledge files on disk.
pub fn rebuild_index(workspace_config_dir: &Path, bm25_index_path: &Path) {
    use super::bm25_index::SourceType;
    use super::index::ExperienceIndex;
    use super::knowledge::KnowledgeIndex;
    use super::okf;

    let mut bm25 = BM25Index::default();

    let experiences_dir = super::experiences_dir(workspace_config_dir);
    let exp_index = ExperienceIndex::load(&experiences_dir);
    let raw_dir = experiences_dir.join("raw");

    for entry in &exp_index.entries {
        let raw_path = raw_dir.join(format!("{}.md", entry.thread_id));
        if let Ok(content) = std::fs::read_to_string(&raw_path) {
            if !content.trim().is_empty() {
                let body = okf::extract_body(&content);
                let title = entry
                    .summary_slug
                    .clone()
                    .unwrap_or_else(|| entry.thread_id.clone());
                let doc = super::bm25_index::build_document_with_locator_metadata(
                    format!("exp:{}", entry.thread_id),
                    SourceType::Experience,
                    format!("experiences/raw/{}.md", entry.thread_id),
                    title,
                    &body,
                    entry.extracted_at,
                    entry.categories.clone(),
                    Some("Experience".to_string()),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    false,
                );
                bm25.add_document(doc);
            }
        }
    }

    let knowledge_dir = super::knowledge_dir(workspace_config_dir);
    let _ = super::knowledge::backfill_legacy_metadata(&knowledge_dir);
    let know_index = KnowledgeIndex::load(&knowledge_dir);
    let docs_dir = knowledge_dir.join("docs");

    for entry in &know_index.entries {
        let chunk_paths = super::knowledge::list_chunk_doc_paths(&docs_dir, &entry.doc_id);
        if !chunk_paths.is_empty() {
            let chunk_total_fallback = entry.chunk_count.max(chunk_paths.len()).max(1);
            for (pos, chunk_path) in chunk_paths.iter().enumerate() {
                let Ok(content) = std::fs::read_to_string(chunk_path) else {
                    continue;
                };
                if content.trim().is_empty() {
                    continue;
                }
                let (body, frontmatter) = if let Some(doc) = okf::parse_document(&content) {
                    (doc.body, Some(doc.frontmatter))
                } else {
                    (content, None)
                };
                let chunk_index = frontmatter
                    .as_ref()
                    .and_then(|fm| okf_extension_usize(fm, "chunk_index"))
                    .unwrap_or(pos + 1);
                let chunk_total = frontmatter
                    .as_ref()
                    .and_then(|fm| okf_extension_usize(fm, "chunk_total"))
                    .unwrap_or(chunk_total_fallback);
                let title = frontmatter
                    .as_ref()
                    .and_then(|fm| fm.title.clone())
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| format!("{} [{}/{}]", entry.title, chunk_index, chunk_total));
                let tags = frontmatter
                    .as_ref()
                    .map(|fm| fm.tags.clone())
                    .filter(|tags| !tags.is_empty())
                    .unwrap_or_else(|| entry.categories.clone());
                let concept_type = frontmatter
                    .as_ref()
                    .map(|fm| fm.concept_type.clone())
                    .filter(|value| !value.trim().is_empty())
                    .or_else(|| Some("Knowledge".to_string()));
                let domain = frontmatter
                    .as_ref()
                    .and_then(|fm| okf_extension_string(fm, "domain"))
                    .or_else(|| entry.domain.clone());
                let source_group = frontmatter
                    .as_ref()
                    .and_then(|fm| okf_extension_string(fm, "source_group"))
                    .or_else(|| entry.source_group.clone());
                let relative_path = frontmatter
                    .as_ref()
                    .and_then(|fm| okf_extension_string(fm, "relative_path"))
                    .or_else(|| entry.relative_path.clone());
                let source_file = frontmatter
                    .as_ref()
                    .and_then(|fm| okf_extension_string(fm, "source_file"))
                    .or_else(|| Some(entry.source_file.clone()));
                let file_name = chunk_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| format!("{}.md", entry.doc_id));
                let doc = super::bm25_index::build_document_with_locator_metadata(
                    super::knowledge::chunk_bm25_doc_id(&entry.doc_id, chunk_index),
                    SourceType::Knowledge,
                    format!("knowledge/docs/{file_name}"),
                    title,
                    &body,
                    entry.added_at,
                    tags,
                    concept_type,
                    domain,
                    source_group,
                    relative_path,
                    source_file,
                    Some(entry.doc_id.clone()),
                    Some(chunk_index),
                    Some(chunk_total),
                    true,
                );
                bm25.add_document(doc);
            }
            continue;
        }

        let doc_path = docs_dir.join(format!("{}.md", entry.doc_id));
        if let Ok(content) = std::fs::read_to_string(&doc_path) {
            if !content.trim().is_empty() {
                let (body, frontmatter) = if let Some(doc) = okf::parse_document(&content) {
                    (doc.body, Some(doc.frontmatter))
                } else {
                    (content, None)
                };
                let tags = frontmatter
                    .as_ref()
                    .map(|fm| fm.tags.clone())
                    .filter(|tags| !tags.is_empty())
                    .unwrap_or_else(|| entry.categories.clone());
                let concept_type = frontmatter
                    .as_ref()
                    .map(|fm| fm.concept_type.clone())
                    .filter(|value| !value.trim().is_empty())
                    .or_else(|| Some("Knowledge".to_string()));
                let domain = frontmatter
                    .as_ref()
                    .and_then(|fm| okf_extension_string(fm, "domain"))
                    .or_else(|| entry.domain.clone());
                let source_group = frontmatter
                    .as_ref()
                    .and_then(|fm| okf_extension_string(fm, "source_group"))
                    .or_else(|| entry.source_group.clone());
                let relative_path = frontmatter
                    .as_ref()
                    .and_then(|fm| okf_extension_string(fm, "relative_path"))
                    .or_else(|| entry.relative_path.clone());
                let source_file = frontmatter
                    .as_ref()
                    .and_then(|fm| okf_extension_string(fm, "source_file"))
                    .or_else(|| Some(entry.source_file.clone()));
                let title = frontmatter
                    .as_ref()
                    .and_then(|fm| fm.title.clone())
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| entry.title.clone());
                let doc = super::bm25_index::build_document_with_locator_metadata(
                    format!("know:{}", entry.doc_id),
                    SourceType::Knowledge,
                    format!("knowledge/docs/{}.md", entry.doc_id),
                    title,
                    &body,
                    entry.added_at,
                    tags,
                    concept_type,
                    domain,
                    source_group,
                    relative_path,
                    source_file,
                    None,
                    None,
                    None,
                    false,
                );
                bm25.add_document(doc);
            }
        }
    }

    if let Err(e) = bm25.save(bm25_index_path) {
        tracing::error!("Failed to save rebuilt BM25 index: {e}");
    } else {
        tracing::info!(
            "BM25 index rebuilt: {} documents ({} experience, {} knowledge)",
            bm25.document_count(),
            exp_index.entries.len(),
            know_index.entries.len(),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::smartbrain::bm25_index::{build_document, build_document_with_metadata};
    use crate::smartbrain::index::{ExperienceEntry, ExperienceIndex};

    fn setup_workspace() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let temp_dir = tempfile::tempdir().unwrap();
        let workspace_config_dir = temp_dir.path().join("codey");
        let memories_dir = workspace_config_dir.join("memories");
        std::fs::create_dir_all(&memories_dir).unwrap();
        let bm25_path = memories_dir.join("smartbrain_index.json");
        (temp_dir, workspace_config_dir, bm25_path)
    }

    fn save_experience_index(workspace_config_dir: &Path) {
        let experiences_dir = crate::smartbrain::experiences_dir(workspace_config_dir);
        let mut index = ExperienceIndex::default();
        index.entries = vec![
            ExperienceEntry {
                thread_id: "thread-low".to_string(),
                extracted_at: 1_700_000_000,
                source_updated_at: 1_700_000_000,
                usage_count: 0,
                last_used_at: Some(1_700_000_000),
                summary_slug: Some("thread-low".to_string()),
                title: Some("low".to_string()),
                summary: Some("low priority".to_string()),
                categories: vec!["debugging".to_string()],
            },
            ExperienceEntry {
                thread_id: "thread-high".to_string(),
                extracted_at: 1_700_000_000,
                source_updated_at: 1_700_000_000,
                usage_count: 12,
                last_used_at: Some(1_800_000_000),
                summary_slug: Some("thread-high".to_string()),
                title: Some("high".to_string()),
                summary: Some("high priority".to_string()),
                categories: vec!["debugging".to_string()],
            },
        ];
        index.save(&experiences_dir).unwrap();
    }

    fn save_bm25_index(path: &Path) {
        let mut bm25 = BM25Index::default();
        bm25.add_document(build_document(
            "exp:thread-low".to_string(),
            SourceType::Experience,
            "experiences/raw/thread-low.md".to_string(),
            "thread-low".to_string(),
            "rust fix compiler error rust fix compiler error rust fix compiler error",
            1_700_000_000,
        ));
        bm25.add_document(build_document(
            "exp:thread-high".to_string(),
            SourceType::Experience,
            "experiences/raw/thread-high.md".to_string(),
            "thread-high".to_string(),
            "rust fix workaround",
            1_700_000_000,
        ));
        bm25.add_document(build_document(
            "know:rust-book".to_string(),
            SourceType::Knowledge,
            "knowledge/docs/rust-book.md".to_string(),
            "Rust book".to_string(),
            "rust ownership and borrowing guide",
            1_700_000_000,
        ));

        let recalled = bm25.search("rust fix compiler error", 10);
        assert_eq!(recalled[0].doc_id, "exp:thread-low");

        bm25.save(path).unwrap();
    }

    #[test]
    fn unified_search_reranks_experience_results_by_priority() {
        let (_temp_dir, workspace_config_dir, bm25_path) = setup_workspace();
        save_experience_index(&workspace_config_dir);
        save_bm25_index(&bm25_path);

        let results = unified_search(&bm25_path, "rust fix compiler error", 2);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].doc_id, "exp:thread-high");
        assert_eq!(results[1].doc_id, "exp:thread-low");
    }

    #[test]
    fn unified_search_with_filter_reranks_experience_results_by_priority() {
        let (_temp_dir, workspace_config_dir, bm25_path) = setup_workspace();
        save_experience_index(&workspace_config_dir);
        save_bm25_index(&bm25_path);

        let results = unified_search_with_filter(
            &bm25_path,
            "rust fix compiler error",
            2,
            SearchFilter {
                source_type: Some(SourceType::Experience),
                ..SearchFilter::default()
            },
        );
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].doc_id, "exp:thread-high");
        assert_eq!(results[1].doc_id, "exp:thread-low");
    }

    #[test]
    fn rebuild_index_backfills_legacy_domain_metadata_for_knowledge() {
        let (_temp_dir, workspace_config_dir, bm25_path) = setup_workspace();
        let knowledge_dir = crate::smartbrain::knowledge_dir(&workspace_config_dir);
        std::fs::create_dir_all(knowledge_dir.join("docs")).unwrap();

        let mut know_index = crate::smartbrain::knowledge::KnowledgeIndex::default();
        know_index
            .entries
            .push(crate::smartbrain::knowledge::KnowledgeEntry {
                doc_id: "legacy-knowledge".to_string(),
                source_file: "legacy.md".to_string(),
                source_type: "md".to_string(),
                title: "Legacy knowledge".to_string(),
                description: None,
                added_at: 1_700_000_000,
                chunk_count: 1,
                categories: vec!["legacy".to_string()],
                domain: None,
                relative_path: None,
                source_group: None,
            });
        know_index.save(&knowledge_dir).unwrap();
        std::fs::write(
            knowledge_dir.join("docs").join("legacy-knowledge.md"),
            "legacy content for rebuild test",
        )
        .unwrap();

        rebuild_index(&workspace_config_dir, &bm25_path);

        let reloaded = crate::smartbrain::knowledge::KnowledgeIndex::load(&knowledge_dir);
        assert_eq!(reloaded.entries[0].domain.as_deref(), Some("legacy"));
        assert_eq!(
            reloaded.entries[0].relative_path.as_deref(),
            Some("legacy.md")
        );

        let rebuilt_bm25 = BM25Index::load(&bm25_path);
        let rebuilt_doc = rebuilt_bm25
            .documents
            .iter()
            .find(|doc| doc.doc_id == "know:legacy-knowledge")
            .unwrap();
        assert_eq!(rebuilt_doc.domain.as_deref(), Some("legacy"));
    }

    #[test]
    fn rebuild_index_prefers_chunk_documents_for_split_knowledge() {
        let (_temp_dir, workspace_config_dir, bm25_path) = setup_workspace();
        let knowledge_dir = crate::smartbrain::knowledge_dir(&workspace_config_dir);
        std::fs::create_dir_all(knowledge_dir.join("docs")).unwrap();

        let mut know_index = crate::smartbrain::knowledge::KnowledgeIndex::default();
        know_index
            .entries
            .push(crate::smartbrain::knowledge::KnowledgeEntry {
                doc_id: "split-doc".to_string(),
                source_file: "split-doc.txt".to_string(),
                source_type: "txt".to_string(),
                title: "Split Doc".to_string(),
                description: None,
                added_at: 1_700_000_000,
                chunk_count: 2,
                categories: vec!["guide".to_string()],
                domain: Some("backend".to_string()),
                relative_path: Some("split-doc.txt".to_string()),
                source_group: Some("group-1".to_string()),
            });
        know_index.save(&knowledge_dir).unwrap();

        std::fs::write(
            knowledge_dir.join("docs").join("split-doc.md"),
            "parent full body",
        )
        .unwrap();
        std::fs::write(
            knowledge_dir.join("docs").join("split-doc__chunk_0001.md"),
            "---\ntype: Knowledge\ntitle: Split Doc [1/2]\ntags: [guide]\nsource_file: split-doc.txt\nsource_type: txt\ndomain: backend\nsource_group: group-1\nrelative_path: split-doc.txt\nis_chunk: true\nparent_doc_id: split-doc\nchunk_index: 1\nchunk_total: 2\n---\n\nchunk one body",
        )
        .unwrap();
        std::fs::write(
            knowledge_dir.join("docs").join("split-doc__chunk_0002.md"),
            "---\ntype: Knowledge\ntitle: Split Doc [2/2]\ntags: [guide]\nsource_file: split-doc.txt\nsource_type: txt\ndomain: backend\nsource_group: group-1\nrelative_path: split-doc.txt\nis_chunk: true\nparent_doc_id: split-doc\nchunk_index: 2\nchunk_total: 2\n---\n\nchunk two body",
        )
        .unwrap();

        rebuild_index(&workspace_config_dir, &bm25_path);

        let rebuilt_bm25 = BM25Index::load(&bm25_path);
        assert!(
            rebuilt_bm25
                .documents
                .iter()
                .any(|doc| doc.doc_id == "know:split-doc::chunk:0001"),
            "chunk document 1 should be indexed"
        );
        assert!(
            rebuilt_bm25
                .documents
                .iter()
                .any(|doc| doc.doc_id == "know:split-doc::chunk:0002"),
            "chunk document 2 should be indexed"
        );
        assert!(
            rebuilt_bm25
                .documents
                .iter()
                .all(|doc| doc.doc_id != "know:split-doc"),
            "parent document should not be indexed when chunk docs exist"
        );
    }

    #[test]
    fn second_stage_candidate_pool_expands_by_domain_and_tags() {
        let mut bm25 = BM25Index::default();
        bm25.add_document(build_document_with_metadata(
            "know:anchor".to_string(),
            SourceType::Knowledge,
            "knowledge/docs/anchor.md".to_string(),
            "Anchor".to_string(),
            "database migration rollback strategy",
            1_700_000_000,
            vec!["db".to_string(), "migration".to_string()],
            Some("Knowledge".to_string()),
            Some("backend".to_string()),
        ));
        bm25.add_document(build_document_with_metadata(
            "know:domain-sibling".to_string(),
            SourceType::Knowledge,
            "knowledge/docs/domain-sibling.md".to_string(),
            "Domain sibling".to_string(),
            "high availability replication notes",
            1_700_000_000,
            vec!["ops".to_string()],
            Some("Knowledge".to_string()),
            Some("backend".to_string()),
        ));
        bm25.add_document(build_document_with_metadata(
            "know:tag-sibling".to_string(),
            SourceType::Knowledge,
            "knowledge/docs/tag-sibling.md".to_string(),
            "Tag sibling".to_string(),
            "schema evolution and checklist",
            1_700_000_000,
            vec!["db".to_string()],
            Some("Knowledge".to_string()),
            Some("storage".to_string()),
        ));
        bm25.add_document(build_document_with_metadata(
            "know:unrelated".to_string(),
            SourceType::Experience,
            "knowledge/docs/unrelated.md".to_string(),
            "Unrelated".to_string(),
            "frontend css animation guide",
            1_700_000_000,
            vec!["ui".to_string()],
            Some("Knowledge".to_string()),
            Some("frontend".to_string()),
        ));

        let stage1_results = bm25.search("database migration", stage1_recall_k(2));
        let stage1_ids: HashSet<String> = stage1_results
            .iter()
            .map(|result| result.doc_id.clone())
            .collect();
        let (candidate_pool, expanded) =
            build_second_stage_candidate_pool(&bm25, &stage1_results, None);

        assert!(expanded, "存在同 domain / tag 文档时应触发扩展");
        assert!(
            candidate_pool.contains("know:domain-sibling"),
            "同 domain 文档应进入候选池"
        );
        assert!(
            candidate_pool.contains("know:tag-sibling"),
            "共享 tag 文档应进入候选池"
        );
        assert!(
            !candidate_pool.contains("know:unrelated") || stage1_ids.contains("know:unrelated"),
            "无关联文档不应被扩展逻辑引入（除非它本来就在 stage1）"
        );
    }

    #[test]
    fn two_stage_recall_falls_back_to_stage1_when_metadata_is_sparse() {
        let mut bm25 = BM25Index::default();
        bm25.add_document(build_document(
            "d1".to_string(),
            SourceType::Knowledge,
            "knowledge/docs/d1.md".to_string(),
            "Doc 1".to_string(),
            "rust ownership lifetime borrow checker",
            100,
        ));
        bm25.add_document(build_document(
            "d2".to_string(),
            SourceType::Knowledge,
            "knowledge/docs/d2.md".to_string(),
            "Doc 2".to_string(),
            "rust trait object generic bounds",
            100,
        ));
        bm25.add_document(build_document(
            "d3".to_string(),
            SourceType::Knowledge,
            "knowledge/docs/d3.md".to_string(),
            "Doc 3".to_string(),
            "rust async await pin future",
            100,
        ));

        let stage1_results = bm25.search("rust", stage1_recall_k(2));
        let stage1_ids: Vec<String> = stage1_results
            .iter()
            .map(|result| result.doc_id.clone())
            .collect();

        let recalled = two_stage_recall(&bm25, "rust", 2, stage1_results, None, None);
        let recalled_ids: Vec<String> = recalled
            .iter()
            .map(|result| result.doc_id.clone())
            .collect();

        assert_eq!(
            recalled_ids, stage1_ids,
            "元信息无法扩展时应回退到第一阶段结果，保持检索稳定"
        );
    }

    #[test]
    fn unified_search_with_filter_falls_back_to_global_bm25_when_filter_is_too_strict() {
        let (_temp_dir, _workspace_config_dir, bm25_path) = setup_workspace();
        let mut bm25 = BM25Index::default();
        bm25.add_document(build_document_with_metadata(
            "know:rust-guide".to_string(),
            SourceType::Knowledge,
            "knowledge/docs/rust-guide.md".to_string(),
            "Rust guide".to_string(),
            "rust ownership borrow checker guide",
            1_700_000_000,
            vec!["rust".to_string()],
            Some("Knowledge".to_string()),
            Some("backend".to_string()),
        ));
        bm25.add_document(build_document_with_metadata(
            "know:python-guide".to_string(),
            SourceType::Knowledge,
            "knowledge/docs/python-guide.md".to_string(),
            "Python guide".to_string(),
            "python asyncio event loop guide",
            1_700_000_000,
            vec!["python".to_string()],
            Some("Knowledge".to_string()),
            Some("backend".to_string()),
        ));
        bm25.save(&bm25_path).unwrap();

        let results = unified_search_with_filter_with_policy(
            &bm25_path,
            "rust ownership",
            3,
            SearchFilter {
                domain: Some("non-existent-domain".to_string()),
                ..SearchFilter::default()
            },
            true,
            None,
        );
        assert!(
            !results.is_empty(),
            "strict filter with no matches should degrade to global BM25 results"
        );
        assert_eq!(results[0].doc_id, "know:rust-guide");
    }
}
