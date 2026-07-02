use std::collections::{HashMap, HashSet};
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
    #[serde(default)]
    pub domain: Option<String>,
}

/// Filter criteria for structured search on OKF metadata.
#[derive(Debug, Clone, Default)]
pub struct SearchFilter {
    pub concept_type: Option<String>,
    pub tags: Vec<String>,
    pub domain: Option<String>,
    pub source_type: Option<SourceType>,
    pub timestamp_after: Option<i64>,
    pub timestamp_before: Option<i64>,
}

impl SearchFilter {
    pub fn is_empty(&self) -> bool {
        self.concept_type.is_none()
            && self.tags.is_empty()
            && self.domain.is_none()
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
        if let Some(domain) = &self.domain {
            if doc.domain.as_deref() != Some(domain.as_str()) {
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

    /// 在候选集合内执行 BM25 检索（不做额外结构化过滤）。
    ///
    /// 设计目的：
    /// - 第一阶段通常先做全局召回；
    /// - 第二阶段只在“扩展后的候选池”里重排；
    /// - 这样能保持召回稳定，同时避免全量文档再次扫描带来的噪音。
    pub fn search_subset(
        &self,
        query: &str,
        top_k: usize,
        candidate_doc_ids: &HashSet<String>,
    ) -> Vec<SearchResult> {
        if self.documents.is_empty() || candidate_doc_ids.is_empty() {
            return Vec::new();
        }

        let query_tokens = tokenize(query);
        if query_tokens.is_empty() {
            return Vec::new();
        }

        // 仅保留候选池中的文档参与打分，确保二阶段重排的范围可控。
        let candidate_docs: Vec<&IndexedDocument> = self
            .documents
            .iter()
            .filter(|doc| candidate_doc_ids.contains(&doc.doc_id))
            .collect();
        if candidate_docs.is_empty() {
            return Vec::new();
        }

        // 在候选子集上重新计算统计量（平均长度、IDF）。
        // 这是“二阶段重排”的关键：分值反映的是局部竞争关系，而不是全局竞争关系。
        let total_docs = candidate_docs.len() as f64;
        let avg_doc_length = candidate_docs
            .iter()
            .map(|doc| doc.token_count as f64)
            .sum::<f64>()
            / total_docs;
        let idf = compute_idf_from_refs(&query_tokens, &candidate_docs);

        let mut scored: Vec<SearchResult> = candidate_docs
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
            .filter(|result| result.score > 0.0)
            .collect();

        scored.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(top_k);
        scored
    }

    /// 在候选集合内执行 BM25 检索，并应用结构化过滤条件。
    ///
    /// 该方法用于“二阶段重排 + 过滤”场景，避免扩展候选时打破过滤约束。
    pub fn search_subset_with_filter(
        &self,
        query: &str,
        top_k: usize,
        candidate_doc_ids: &HashSet<String>,
        filter: &SearchFilter,
    ) -> Vec<SearchResult> {
        if self.documents.is_empty() || candidate_doc_ids.is_empty() {
            return Vec::new();
        }

        // 第一步：先做候选池过滤；
        // 第二步：再叠加结构化过滤；
        // 两层过滤都通过的文档才进入二阶段重排。
        let candidate_docs: Vec<&IndexedDocument> = self
            .documents
            .iter()
            .filter(|doc| candidate_doc_ids.contains(&doc.doc_id))
            .filter(|doc| filter.is_empty() || filter.matches(doc))
            .collect();
        if candidate_docs.is_empty() {
            return Vec::new();
        }

        if query.trim().is_empty() {
            return candidate_docs
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

        let total_docs = candidate_docs.len() as f64;
        let avg_doc_length = candidate_docs
            .iter()
            .map(|doc| doc.token_count as f64)
            .sum::<f64>()
            / total_docs;
        let idf = compute_idf_from_refs(&query_tokens, &candidate_docs);

        let mut scored: Vec<SearchResult> = candidate_docs
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
            .filter(|result| result.score > 0.0)
            .collect();

        scored.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
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
        domain: None,
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
    domain: Option<String>,
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
        domain,
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
    fn search_with_filter_can_filter_by_domain() {
        let mut index = BM25Index::default();
        index.add_document(build_document_with_metadata(
            "k1".to_string(),
            SourceType::Knowledge,
            "docs/k1.md".to_string(),
            "Database schema".to_string(),
            "postgres schema table index",
            100,
            vec!["database".to_string()],
            Some("Knowledge".to_string()),
            Some("db-export".to_string()),
        ));
        index.add_document(build_document_with_metadata(
            "k2".to_string(),
            SourceType::Knowledge,
            "docs/k2.md".to_string(),
            "Frontend guide".to_string(),
            "react component styling guide",
            100,
            vec!["frontend".to_string()],
            Some("Knowledge".to_string()),
            Some("frontend".to_string()),
        ));

        let filtered = index.search_with_filter(
            "schema",
            10,
            &SearchFilter {
                domain: Some("db-export".to_string()),
                ..SearchFilter::default()
            },
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].doc_id, "k1");
    }

    #[test]
    fn search_with_filter_domain_is_backward_compatible_with_missing_domain() {
        let mut index = BM25Index::default();
        index.add_document(build_document(
            "legacy-doc".to_string(),
            SourceType::Knowledge,
            "docs/legacy.md".to_string(),
            "Legacy doc".to_string(),
            "legacy content",
            100,
        ));

        let filtered = index.search_with_filter(
            "legacy",
            10,
            &SearchFilter {
                domain: Some("legacy".to_string()),
                ..SearchFilter::default()
            },
        );
        assert!(filtered.is_empty());

        let no_filter = index.search_with_filter("legacy", 10, &SearchFilter::default());
        assert_eq!(no_filter.len(), 1);
        assert_eq!(no_filter[0].doc_id, "legacy-doc");
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

    #[test]
    fn search_subset_only_scores_documents_inside_candidate_pool() {
        let mut index = BM25Index::default();
        index.add_document(build_document(
            "d1".to_string(),
            SourceType::Knowledge,
            "docs/d1.md".to_string(),
            "React routing fix".to_string(),
            "react router navigation fix useeffect",
            100,
        ));
        index.add_document(build_document(
            "d2".to_string(),
            SourceType::Knowledge,
            "docs/d2.md".to_string(),
            "React testing".to_string(),
            "react component test mocking",
            100,
        ));
        index.add_document(build_document(
            "d3".to_string(),
            SourceType::Knowledge,
            "docs/d3.md".to_string(),
            "High score out-of-pool".to_string(),
            "react router react router react router",
            100,
        ));

        let mut candidate_ids = HashSet::new();
        candidate_ids.insert("d1".to_string());
        candidate_ids.insert("d2".to_string());

        let subset_results = index.search_subset("react router", 10, &candidate_ids);
        assert_eq!(subset_results.len(), 2);
        assert_eq!(subset_results[0].doc_id, "d1");
        assert!(
            subset_results.iter().all(|result| result.doc_id != "d3"),
            "不在候选池的文档不应进入结果"
        );
    }

    #[test]
    fn search_subset_with_filter_respects_filter_inside_candidate_pool() {
        let mut index = BM25Index::default();
        index.add_document(build_document_with_metadata(
            "db-doc".to_string(),
            SourceType::Knowledge,
            "docs/db-doc.md".to_string(),
            "DB doc".to_string(),
            "schema migration index",
            100,
            vec!["database".to_string()],
            Some("Knowledge".to_string()),
            Some("database".to_string()),
        ));
        index.add_document(build_document_with_metadata(
            "web-doc".to_string(),
            SourceType::Knowledge,
            "docs/web-doc.md".to_string(),
            "Web doc".to_string(),
            "react router ui",
            100,
            vec!["frontend".to_string()],
            Some("Knowledge".to_string()),
            Some("frontend".to_string()),
        ));

        let candidate_ids = HashSet::from(["db-doc".to_string(), "web-doc".to_string()]);
        let filter = SearchFilter {
            domain: Some("database".to_string()),
            ..SearchFilter::default()
        };

        let filtered_results = index.search_subset_with_filter("", 10, &candidate_ids, &filter);
        assert_eq!(filtered_results.len(), 1);
        assert_eq!(filtered_results[0].doc_id, "db-doc");
        assert_eq!(filtered_results[0].score, 1.0);
    }
}
