use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceType {
    Experience,
    Knowledge,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedDocument {
    pub doc_id: String,
    pub source_type: SourceType,
    pub file_path: String,
    pub title: String,
    pub tokens: Vec<String>,
    pub token_count: usize,
    pub updated_at: i64,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub concept_type: Option<String>,
}

/// Filter criteria for structured search on OKF metadata.
#[derive(Debug, Clone, Default)]
pub struct SearchFilter {
    pub concept_type: Option<String>,
    pub tags: Vec<String>,
    pub source_type: Option<SourceType>,
    pub timestamp_after: Option<i64>,
    pub timestamp_before: Option<i64>,
}

impl SearchFilter {
    pub fn is_empty(&self) -> bool {
        self.concept_type.is_none()
            && self.tags.is_empty()
            && self.source_type.is_none()
            && self.timestamp_after.is_none()
            && self.timestamp_before.is_none()
    }

    fn matches(&self, doc: &IndexedDocument) -> bool {
        if let Some(ct) = &self.concept_type {
            if doc.concept_type.as_deref() != Some(ct.as_str()) {
                return false;
            }
        }
        if let Some(st) = &self.source_type {
            if doc.source_type != *st {
                return false;
            }
        }
        if !self.tags.is_empty() {
            let has_match = self.tags.iter().any(|t| doc.tags.contains(t));
            if !has_match {
                return false;
            }
        }
        if let Some(after) = self.timestamp_after {
            if doc.updated_at < after {
                return false;
            }
        }
        if let Some(before) = self.timestamp_before {
            if doc.updated_at > before {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub doc_id: String,
    pub source_type: SourceType,
    pub file_path: String,
    pub title: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BM25Index {
    pub version: u32,
    pub documents: Vec<IndexedDocument>,
}

impl Default for BM25Index {
    fn default() -> Self {
        Self {
            version: 1,
            documents: Vec::new(),
        }
    }
}

impl BM25Index {
    pub fn load(index_path: &Path) -> Self {
        if !index_path.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(index_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_else(|e| {
                error!("Failed to parse BM25 index: {e}");
                Self::default()
            }),
            Err(e) => {
                error!("Failed to read BM25 index: {e}");
                Self::default()
            }
        }
    }

    pub fn save(&self, index_path: &Path) -> Result<(), String> {
        if let Some(parent) = index_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let content = serde_json::to_string(self)
            .map_err(|e| format!("Failed to serialize BM25 index: {e}"))?;
        std::fs::write(index_path, content)
            .map_err(|e| format!("Failed to write BM25 index: {e}"))?;
        Ok(())
    }

    pub fn add_document(&mut self, doc: IndexedDocument) {
        self.remove_document(&doc.doc_id);
        self.documents.push(doc);
    }

    pub fn remove_document(&mut self, doc_id: &str) -> bool {
        let before = self.documents.len();
        self.documents.retain(|d| d.doc_id != doc_id);
        self.documents.len() < before
    }

    pub fn remove_by_source_type(&mut self, source_type: SourceType) {
        self.documents.retain(|d| d.source_type != source_type);
    }

    pub fn search(&self, query: &str, top_k: usize) -> Vec<SearchResult> {
        if self.documents.is_empty() {
            return Vec::new();
        }

        let query_tokens = tokenize(query);
        if query_tokens.is_empty() {
            return Vec::new();
        }

        let total_docs = self.documents.len() as f64;
        let avg_doc_length = if self.documents.is_empty() {
            1.0
        } else {
            self.documents
                .iter()
                .map(|d| d.token_count as f64)
                .sum::<f64>()
                / total_docs
        };

        let idf = compute_idf(&query_tokens, &self.documents);

        let mut scored: Vec<SearchResult> = self
            .documents
            .iter()
            .map(|doc| {
                let score = bm25_score(&query_tokens, &doc.tokens, avg_doc_length, &idf);
                SearchResult {
                    doc_id: doc.doc_id.clone(),
                    source_type: doc.source_type,
                    file_path: doc.file_path.clone(),
                    title: doc.title.clone(),
                    score,
                }
            })
            .filter(|r| r.score > 0.0)
            .collect();

        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(top_k);
        scored
    }

    /// Search with optional structured filter on OKF metadata.
    pub fn search_with_filter(
        &self,
        query: &str,
        top_k: usize,
        filter: &SearchFilter,
    ) -> Vec<SearchResult> {
        if self.documents.is_empty() {
            return Vec::new();
        }

        let filtered_docs: Vec<&IndexedDocument> = if filter.is_empty() {
            self.documents.iter().collect()
        } else {
            self.documents
                .iter()
                .filter(|d| filter.matches(d))
                .collect()
        };

        if filtered_docs.is_empty() {
            return Vec::new();
        }

        if query.trim().is_empty() {
            return filtered_docs
                .into_iter()
                .take(top_k)
                .map(|doc| SearchResult {
                    doc_id: doc.doc_id.clone(),
                    source_type: doc.source_type,
                    file_path: doc.file_path.clone(),
                    title: doc.title.clone(),
                    score: 1.0,
                })
                .collect();
        }

        let query_tokens = tokenize(query);
        if query_tokens.is_empty() {
            return Vec::new();
        }

        let total_docs = filtered_docs.len() as f64;
        let avg_doc_length = filtered_docs
            .iter()
            .map(|d| d.token_count as f64)
            .sum::<f64>()
            / total_docs;

        let idf = compute_idf_from_refs(&query_tokens, &filtered_docs);

        let mut scored: Vec<SearchResult> = filtered_docs
            .iter()
            .map(|doc| {
                let score = bm25_score(&query_tokens, &doc.tokens, avg_doc_length, &idf);
                SearchResult {
                    doc_id: doc.doc_id.clone(),
                    source_type: doc.source_type,
                    file_path: doc.file_path.clone(),
                    title: doc.title.clone(),
                    score,
                }
            })
            .filter(|r| r.score > 0.0)
            .collect();

        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(top_k);
        scored
    }

    pub fn has_document(&self, doc_id: &str) -> bool {
        self.documents.iter().any(|d| d.doc_id == doc_id)
    }

    pub fn document_count(&self) -> usize {
        self.documents.len()
    }
}

/// Tokenize text for BM25 indexing and querying.
/// Handles both English (whitespace/punctuation split) and CJK (character bigrams).
pub fn tokenize(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut tokens = Vec::new();
    let mut current_word = String::new();
    let mut cjk_chars: Vec<char> = Vec::new();

    for ch in lower.chars() {
        if is_cjk(ch) {
            if !current_word.is_empty() {
                tokens.push(std::mem::take(&mut current_word));
            }
            cjk_chars.push(ch);
            tokens.push(ch.to_string());
        } else {
            if !cjk_chars.is_empty() {
                for window in cjk_chars.windows(2) {
                    tokens.push(window.iter().collect());
                }
                cjk_chars.clear();
            }

            if ch.is_alphanumeric() || ch == '_' || ch == '-' {
                current_word.push(ch);
            } else if !current_word.is_empty() {
                tokens.push(std::mem::take(&mut current_word));
            }
        }
    }

    if !cjk_chars.is_empty() {
        for window in cjk_chars.windows(2) {
            tokens.push(window.iter().collect());
        }
    }
    if !current_word.is_empty() {
        tokens.push(current_word);
    }

    tokens
}

fn is_cjk(ch: char) -> bool {
    matches!(ch,
        '\u{4E00}'..='\u{9FFF}' |
        '\u{3400}'..='\u{4DBF}' |
        '\u{F900}'..='\u{FAFF}' |
        '\u{20000}'..='\u{2A6DF}' |
        '\u{2A700}'..='\u{2B73F}' |
        '\u{2B740}'..='\u{2B81F}' |
        '\u{2B820}'..='\u{2CEAF}'
    )
}

fn compute_idf_from_refs(
    query_tokens: &[String],
    documents: &[&IndexedDocument],
) -> HashMap<String, f64> {
    let total_docs = documents.len() as f64;
    let mut idf_map = HashMap::new();

    for token in query_tokens {
        if idf_map.contains_key(token) {
            continue;
        }
        let doc_freq = documents
            .iter()
            .filter(|d| d.tokens.contains(token))
            .count() as f64;
        let idf = ((total_docs - doc_freq + 0.5) / (doc_freq + 0.5) + 1.0).ln();
        idf_map.insert(token.clone(), idf.max(0.0));
    }

    idf_map
}

fn compute_idf(query_tokens: &[String], documents: &[IndexedDocument]) -> HashMap<String, f64> {
    let total_docs = documents.len() as f64;
    let mut idf_map = HashMap::new();

    for token in query_tokens {
        if idf_map.contains_key(token) {
            continue;
        }
        let doc_freq = documents
            .iter()
            .filter(|d| d.tokens.contains(token))
            .count() as f64;
        let idf = ((total_docs - doc_freq + 0.5) / (doc_freq + 0.5) + 1.0).ln();
        idf_map.insert(token.clone(), idf.max(0.0));
    }

    idf_map
}

fn bm25_score(
    query_tokens: &[String],
    document_tokens: &[String],
    avg_doc_length: f64,
    idf: &HashMap<String, f64>,
) -> f64 {
    if document_tokens.is_empty() || avg_doc_length <= 0.0 {
        return 0.0;
    }

    let mut frequencies: HashMap<&str, usize> = HashMap::new();
    for token in document_tokens {
        *frequencies.entry(token.as_str()).or_insert(0) += 1;
    }

    const K1: f64 = 1.5;
    const B: f64 = 0.75;
    let doc_length = document_tokens.len() as f64;
    let length_norm = K1 * (1.0 - B + B * doc_length / avg_doc_length);

    query_tokens.iter().fold(0.0, |score, token| {
        let Some(tf) = frequencies.get(token.as_str()).copied() else {
            return score;
        };
        let Some(idf_val) = idf.get(token) else {
            return score;
        };
        let tf = tf as f64;
        score + idf_val * (tf * (K1 + 1.0)) / (tf + length_norm)
    })
}

/// Build an IndexedDocument from raw text content.
pub fn build_document(
    doc_id: String,
    source_type: SourceType,
    file_path: String,
    title: String,
    content: &str,
    updated_at: i64,
) -> IndexedDocument {
    let tokens = tokenize(content);
    let token_count = tokens.len();
    IndexedDocument {
        doc_id,
        source_type,
        file_path,
        title,
        tokens,
        token_count,
        updated_at,
        tags: Vec::new(),
        concept_type: None,
    }
}

/// Build an IndexedDocument with OKF metadata (tags, concept_type).
pub fn build_document_with_metadata(
    doc_id: String,
    source_type: SourceType,
    file_path: String,
    title: String,
    content: &str,
    updated_at: i64,
    tags: Vec<String>,
    concept_type: Option<String>,
) -> IndexedDocument {
    let tokens = tokenize(content);
    let token_count = tokens.len();
    IndexedDocument {
        doc_id,
        source_type,
        file_path,
        title,
        tokens,
        token_count,
        updated_at,
        tags,
        concept_type,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_english() {
        let tokens = tokenize("Hello World! This is a test.");
        assert!(tokens.contains(&"hello".to_string()));
        assert!(tokens.contains(&"world".to_string()));
        assert!(tokens.contains(&"test".to_string()));
    }

    #[test]
    fn tokenize_cjk_bigrams() {
        let tokens = tokenize("修复React路由问题");
        assert!(tokens.contains(&"修".to_string()));
        assert!(tokens.contains(&"修复".to_string()));
        assert!(tokens.contains(&"react".to_string()));
        assert!(tokens.contains(&"路由".to_string()));
        assert!(tokens.contains(&"问题".to_string()));
    }

    #[test]
    fn search_returns_ranked_results() {
        let mut index = BM25Index::default();
        index.add_document(build_document(
            "d1".to_string(),
            SourceType::Experience,
            "raw/d1.md".to_string(),
            "React routing fix".to_string(),
            "Fixed react router navigation issue with useEffect",
            100,
        ));
        index.add_document(build_document(
            "d2".to_string(),
            SourceType::Knowledge,
            "docs/d2.md".to_string(),
            "Python testing".to_string(),
            "Unit testing with pytest and mocking",
            200,
        ));

        let results = index.search("react router", 10);
        assert!(!results.is_empty());
        assert_eq!(results[0].doc_id, "d1");
    }

    #[test]
    fn add_and_remove_document() {
        let mut index = BM25Index::default();
        index.add_document(build_document(
            "d1".to_string(),
            SourceType::Experience,
            "raw/d1.md".to_string(),
            "Test".to_string(),
            "content",
            100,
        ));
        assert!(index.has_document("d1"));
        assert!(index.remove_document("d1"));
        assert!(!index.has_document("d1"));
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.json");

        let mut index = BM25Index::default();
        index.add_document(build_document(
            "d1".to_string(),
            SourceType::Knowledge,
            "docs/d1.md".to_string(),
            "Test doc".to_string(),
            "some content here",
            100,
        ));
        index.save(&path).unwrap();

        let loaded = BM25Index::load(&path);
        assert_eq!(loaded.documents.len(), 1);
        assert_eq!(loaded.documents[0].doc_id, "d1");
    }

    #[test]
    fn empty_query_returns_empty() {
        let index = BM25Index::default();
        assert!(index.search("", 10).is_empty());
    }
}
