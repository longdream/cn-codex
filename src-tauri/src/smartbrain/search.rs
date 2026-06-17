use std::path::Path;

use serde::Serialize;

use super::bm25_index::{BM25Index, SearchResult, SourceType};

#[derive(Debug, Clone, Serialize)]
pub struct SmartBrainSearchResult {
    pub doc_id: String,
    pub source_type: String,
    pub file_path: String,
    pub title: String,
    pub score: f64,
}

impl From<SearchResult> for SmartBrainSearchResult {
    fn from(r: SearchResult) -> Self {
        Self {
            doc_id: r.doc_id,
            source_type: match r.source_type {
                SourceType::Experience => "experience".to_string(),
                SourceType::Knowledge => "knowledge".to_string(),
            },
            file_path: r.file_path,
            title: r.title,
            score: r.score,
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
        .map(SmartBrainSearchResult::from)
        .collect()
}

/// Rebuild the BM25 index from all experience and knowledge files on disk.
pub fn rebuild_index(
    workspace_config_dir: &Path,
    bm25_index_path: &Path,
) {
    use super::bm25_index::{build_document, SourceType};
    use super::index::ExperienceIndex;
    use super::knowledge::KnowledgeIndex;

    let mut bm25 = BM25Index::default();

    let experiences_dir = super::experiences_dir(workspace_config_dir);
    let exp_index = ExperienceIndex::load(&experiences_dir);
    let raw_dir = experiences_dir.join("raw");

    for entry in &exp_index.entries {
        let raw_path = raw_dir.join(format!("{}.md", entry.thread_id));
        if let Ok(content) = std::fs::read_to_string(&raw_path) {
            if !content.trim().is_empty() {
                let title = entry
                    .summary_slug
                    .clone()
                    .unwrap_or_else(|| entry.thread_id.clone());
                let doc = build_document(
                    format!("exp:{}", entry.thread_id),
                    SourceType::Experience,
                    format!("experiences/raw/{}.md", entry.thread_id),
                    title,
                    &content,
                    entry.extracted_at,
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
                let doc = build_document(
                    format!("know:{}", entry.doc_id),
                    SourceType::Knowledge,
                    format!("knowledge/docs/{}.md", entry.doc_id),
                    entry.title.clone(),
                    &content,
                    entry.added_at,
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
