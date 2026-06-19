use std::path::Path;

use serde::Serialize;

use super::bm25_index::{BM25Index, SearchFilter, SearchResult, SourceType};

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

/// Search both experience and knowledge using BM25.
pub fn unified_search(
    bm25_index_path: &Path,
    query: &str,
    top_k: usize,
) -> Vec<SmartBrainSearchResult> {
    let index = BM25Index::load(bm25_index_path);
    index
        .search(query, top_k)
        .into_iter()
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
    let index = BM25Index::load(bm25_index_path);
    index
        .search_with_filter(query, top_k, &filter)
        .into_iter()
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
