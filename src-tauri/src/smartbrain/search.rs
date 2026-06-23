use std::cmp::Ordering;
use std::path::Path;

use serde::Serialize;

use super::bm25_index::{BM25Index, SearchFilter, SearchResult, SourceType};

const RECALL_MULTIPLIER: usize = 3;
const MIN_CANDIDATE_K: usize = 10;

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
}

impl SmartBrainSearchResult {
    fn from_result(r: SearchResult, index: &BM25Index) -> Self {
        let (tags, concept_type) = index
            .documents
            .iter()
            .find(|d| d.doc_id == r.doc_id)
            .map(|d| (d.tags.clone(), d.concept_type.clone()))
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
    let recalled = index.search(query, expanded_candidate_k(top_k));
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
    if top_k == 0 {
        return Vec::new();
    }

    let index = BM25Index::load(bm25_index_path);
    let recalled = index.search_with_filter(query, expanded_candidate_k(top_k), &filter);
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
                let doc = super::bm25_index::build_document_with_metadata(
                    format!("exp:{}", entry.thread_id),
                    SourceType::Experience,
                    format!("experiences/raw/{}.md", entry.thread_id),
                    title,
                    &body,
                    entry.extracted_at,
                    entry.categories.clone(),
                    Some("Experience".to_string()),
                );
                bm25.add_document(doc);
            }
        }
    }

    let knowledge_dir = super::knowledge_dir(workspace_config_dir);
    let know_index = KnowledgeIndex::load(&knowledge_dir);
    let docs_dir = knowledge_dir.join("docs");

    for entry in &know_index.entries {
        let doc_path = docs_dir.join(format!("{}.md", entry.doc_id));
        if let Ok(content) = std::fs::read_to_string(&doc_path) {
            if !content.trim().is_empty() {
                let body = okf::extract_body(&content);
                let doc = super::bm25_index::build_document_with_metadata(
                    format!("know:{}", entry.doc_id),
                    SourceType::Knowledge,
                    format!("knowledge/docs/{}.md", entry.doc_id),
                    entry.title.clone(),
                    &body,
                    entry.added_at,
                    entry.categories.clone(),
                    Some("Knowledge".to_string()),
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
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::smartbrain::bm25_index::build_document;
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
}
