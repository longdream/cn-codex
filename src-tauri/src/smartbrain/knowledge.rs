use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use crate::adapter::types::{InternalMessage, text_content};
use crate::adapter::{self, types::StreamEvent};
use crate::config_system::ConfigToml;

use super::bm25_index::{self, BM25Index, SourceType};
use super::index::now_secs;
use super::okf::{self, OkfDocument, OkfFrontmatter};
use super::prompts;

const CHUNK_FILE_SEPARATOR: &str = "__chunk_";
const CHUNK_BM25_SEPARATOR: &str = "::chunk:";
const EFFECTIVE_CHUNK_TOKEN_FALLBACK: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEntry {
    pub doc_id: String,
    pub source_file: String,
    pub source_type: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub added_at: i64,
    pub chunk_count: usize,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_group: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct KnowledgeIngestContext {
    pub domain: Option<String>,
    pub relative_path: Option<String>,
    pub source_group: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FolderIngestCandidate {
    pub source_path: PathBuf,
    pub relative_path: String,
}

#[derive(Debug, Clone, Default)]
pub struct FolderCandidateCollection {
    pub candidates: Vec<FolderIngestCandidate>,
    pub skipped_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct FolderIngestFailure {
    pub relative_path: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FolderIngestSummary {
    pub imported_count: usize,
    pub skipped_count: usize,
    pub failed_count: usize,
    pub failures: Vec<FolderIngestFailure>,
}

#[derive(Debug, Clone, Default)]
pub struct KnowledgeMetadataUpdate {
    pub title: Option<String>,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub domain: Option<String>,
    pub source_group: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeIndex {
    pub version: u32,
    pub entries: Vec<KnowledgeEntry>,
}

impl Default for KnowledgeIndex {
    fn default() -> Self {
        Self {
            version: 1,
            entries: Vec::new(),
        }
    }
}

impl KnowledgeIndex {
    pub fn load(knowledge_dir: &Path) -> Self {
        let index_path = knowledge_dir.join("index.json");
        if !index_path.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(&index_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_else(|e| {
                error!("Failed to parse knowledge index: {e}");
                Self::default()
            }),
            Err(e) => {
                error!("Failed to read knowledge index: {e}");
                Self::default()
            }
        }
    }

    pub fn save(&self, knowledge_dir: &Path) -> Result<(), String> {
        let index_path = knowledge_dir.join("index.json");
        if let Some(parent) = index_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize knowledge index: {e}"))?;
        std::fs::write(&index_path, content)
            .map_err(|e| format!("Failed to write knowledge index: {e}"))?;
        Ok(())
    }

    pub fn has_entry(&self, doc_id: &str) -> bool {
        self.entries.iter().any(|e| e.doc_id == doc_id)
    }

    pub fn add_entry(&mut self, entry: KnowledgeEntry) {
        self.remove_entry(&entry.doc_id);
        self.entries.push(entry);
    }

    pub fn remove_entry(&mut self, doc_id: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.doc_id != doc_id);
        self.entries.len() < before
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeCategory {
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub children: Vec<KnowledgeCategory>,
    #[serde(default)]
    pub docs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeHierarchy {
    pub categories: Vec<KnowledgeCategory>,
}

impl Default for KnowledgeHierarchy {
    fn default() -> Self {
        Self {
            categories: Vec::new(),
        }
    }
}

impl KnowledgeHierarchy {
    pub fn load(knowledge_dir: &Path) -> Self {
        let path = knowledge_dir.join("hierarchy.json");
        if !path.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, knowledge_dir: &Path) -> Result<(), String> {
        let path = knowledge_dir.join("hierarchy.json");
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize hierarchy: {e}"))?;
        std::fs::write(&path, content).map_err(|e| format!("Failed to write hierarchy: {e}"))?;
        Ok(())
    }

    /// Produce a compact text summary of the category tree for prompt injection.
    pub fn summary_text(&self) -> String {
        let mut lines = Vec::new();
        for cat in &self.categories {
            collect_hierarchy_lines(cat, 0, &mut lines);
        }
        lines.join("\n")
    }
}

fn collect_hierarchy_lines(cat: &KnowledgeCategory, depth: usize, lines: &mut Vec<String>) {
    let indent = "  ".repeat(depth);
    let doc_count = cat.docs.len();
    let suffix = if doc_count > 0 {
        format!(" ({doc_count} docs)")
    } else {
        String::new()
    };
    lines.push(format!("{indent}- {}{suffix}", cat.name));
    for child in &cat.children {
        collect_hierarchy_lines(child, depth + 1, lines);
    }
}

pub const DEFAULT_FOLDER_UPLOAD_EXTENSIONS: &[&str] = &[
    "pdf", "docx", "xlsx", "xls", "md", "txt", "json", "yaml", "yml", "toml", "csv", "html", "xml",
];

pub fn default_folder_upload_extensions() -> Vec<String> {
    DEFAULT_FOLDER_UPLOAD_EXTENSIONS
        .iter()
        .map(|ext| ext.to_string())
        .collect()
}

pub fn normalize_allowed_extensions(input: &[String]) -> HashSet<String> {
    input
        .iter()
        .map(|ext| ext.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|ext| !ext.is_empty())
        .collect()
}

fn workspace_config_dir_from_knowledge_dir(knowledge_dir: &Path) -> PathBuf {
    knowledge_dir
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(knowledge_dir)
        .to_path_buf()
}

fn knowledge_sources_dir_from_knowledge_dir(knowledge_dir: &Path) -> PathBuf {
    let workspace_config_dir = workspace_config_dir_from_knowledge_dir(knowledge_dir);
    super::knowledge_sources_dir(&workspace_config_dir)
}

fn legacy_knowledge_sources_dir(knowledge_dir: &Path) -> PathBuf {
    knowledge_dir.join("sources")
}

/// Ingest a document file into the knowledge system.
///
/// Copies the source to `memories/knowledge_sources/` (outside the OKF bundle),
/// parses text, optionally calls LLM to organize, writes processed content to
/// `knowledge/docs/`, and updates the knowledge index and BM25 index.
pub async fn ingest_document(
    http: &reqwest::Client,
    config: &ConfigToml,
    knowledge_dir: &Path,
    bm25_index_path: &Path,
    source_path: &Path,
) -> Result<String, String> {
    ingest_document_with_context(
        http,
        config,
        knowledge_dir,
        bm25_index_path,
        source_path,
        &KnowledgeIngestContext::default(),
    )
    .await
}

pub async fn ingest_document_with_context(
    http: &reqwest::Client,
    config: &ConfigToml,
    knowledge_dir: &Path,
    bm25_index_path: &Path,
    source_path: &Path,
    context: &KnowledgeIngestContext,
) -> Result<String, String> {
    ingest_document_inner(
        http,
        config,
        knowledge_dir,
        bm25_index_path,
        source_path,
        context,
        true,
    )
    .await
}

async fn ingest_document_inner(
    http: &reqwest::Client,
    config: &ConfigToml,
    knowledge_dir: &Path,
    bm25_index_path: &Path,
    source_path: &Path,
    context: &KnowledgeIngestContext,
    run_post_hooks: bool,
) -> Result<String, String> {
    let file_name = source_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let file_stem = source_path
        .file_stem()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let extension = source_path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    let sources_dir = knowledge_sources_dir_from_knowledge_dir(knowledge_dir);
    let docs_dir = knowledge_dir.join("docs");
    let _ = std::fs::create_dir_all(&sources_dir);
    let _ = std::fs::create_dir_all(&docs_dir);

    let relative_path = context
        .relative_path
        .as_deref()
        .and_then(normalize_relative_display_path);
    let source_file = relative_path.clone().unwrap_or_else(|| file_name.clone());
    let mut knowledge_index = KnowledgeIndex::load(knowledge_dir);
    let doc_id = if let Some(existing) = knowledge_index
        .entries
        .iter()
        .find(|entry| entry.source_file == source_file)
    {
        existing.doc_id.clone()
    } else {
        unique_doc_id(&slug_from_name(&file_stem), &knowledge_index)
    };

    let dest_path = sources_dir.join(relative_display_path_to_pathbuf(&source_file));
    if let Some(parent) = dest_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if source_path != dest_path {
        std::fs::copy(source_path, &dest_path)
            .map_err(|e| format!("Failed to copy source file: {e}"))?;
    }

    let raw_text = extract_text_from_file(&dest_path, &extension)?;
    if raw_text.trim().is_empty() {
        return Err("Document is empty or could not be parsed".to_string());
    }

    let sb_config = config.smartbrain_config();
    let inferred_domain = normalize_optional_text(context.domain.clone())
        .or_else(|| infer_domain_from_relative_path(relative_path.as_deref()))
        .or_else(|| {
            source_path
                .parent()
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().to_string())
                .and_then(|name| normalize_optional_text(Some(name)))
        });
    let source_group = normalize_optional_text(context.source_group.clone());
    let source_label = build_source_label_for_prompt(
        &file_name,
        inferred_domain.as_deref(),
        relative_path.as_deref(),
    );
    let llm_output = match organize_via_llm(http, config, &raw_text, &source_label).await {
        Ok(parsed) => Some(parsed),
        Err(e) => {
            warn!("LLM knowledge enrichment failed for {file_name}, using fallback metadata: {e}");
            None
        }
    };
    let (organized_text, hierarchy_fragment, metadata_fragment) = if let Some(parsed) = llm_output {
        let prompts::ParsedKnowledgeOrganizeOutput {
            organized_markdown,
            hierarchy,
            metadata,
        } = parsed;
        let organized = if sb_config.auto_organize {
            organized_markdown
        } else {
            raw_text.clone()
        };
        (organized, hierarchy, metadata)
    } else {
        (raw_text.clone(), None, None)
    };
    if !sb_config.auto_organize {
        info!(
            "smartbrain.auto_organize=false for {file_name}; keeping raw text while still applying AI metadata when available"
        );
    }

    let ai_title = metadata_fragment
        .as_ref()
        .and_then(|metadata| normalize_optional_text(metadata.title.clone()));
    let ai_description = metadata_fragment
        .as_ref()
        .and_then(|metadata| normalize_optional_text(metadata.description.clone()));
    let ai_domain = metadata_fragment
        .as_ref()
        .and_then(|metadata| normalize_optional_text(metadata.domain.clone()));
    let resolved_title = ai_title.unwrap_or_else(|| file_stem.clone());
    let resolved_description = ai_description;
    let domain = ai_domain.or(inferred_domain);

    let normalized_text = okf::extract_body(&organized_text);
    let effective_chunk_tokens = if sb_config.max_chunk_tokens == 0 {
        info!(
            "smartbrain.max_chunk_tokens=0 for {file_name}; using fallback chunk size {}",
            EFFECTIVE_CHUNK_TOKEN_FALLBACK
        );
        EFFECTIVE_CHUNK_TOKEN_FALLBACK
    } else {
        sb_config.max_chunk_tokens
    };
    let chunks = chunk_text(&normalized_text, effective_chunk_tokens);
    let chunk_count = chunks.len().max(1);

    let mut categories = hierarchy_fragment
        .as_ref()
        .and_then(|v| v.get("categories"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|c| c.get("name").and_then(|n| n.as_str()))
                .map(String::from)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if let Some(metadata) = metadata_fragment.as_ref() {
        categories.extend(metadata.tags.clone());
    }
    let categories = normalize_tag_list(categories);

    let timestamp = now_secs();
    let mut frontmatter = OkfFrontmatter::new("Knowledge")
        .with_title(&resolved_title)
        .with_tags(categories.clone())
        .with_timestamp(timestamp)
        .with_extension("okf_profile", serde_json::json!("smartbrain-knowledge-v1"))
        .with_extension("source_file", serde_json::json!(source_file))
        .with_extension("source_type", serde_json::json!(extension))
        .with_extension("is_chunk", serde_json::json!(false))
        .with_extension("chunk_count", serde_json::json!(chunk_count));
    if let Some(value) = resolved_description.clone() {
        frontmatter.description = Some(value);
    }
    if let Some(value) = domain.clone() {
        frontmatter = frontmatter.with_extension("domain", serde_json::json!(value));
    }
    if let Some(value) = relative_path.clone() {
        frontmatter = frontmatter.with_extension("relative_path", serde_json::json!(value));
    }
    if let Some(value) = source_group.clone() {
        frontmatter = frontmatter.with_extension("source_group", serde_json::json!(value));
    }

    let doc_path = docs_dir.join(format!("{doc_id}.md"));
    remove_chunk_files(&docs_dir, &doc_id)
        .map_err(|e| format!("Failed to cleanup previous chunk files: {e}"))?;
    let okf_doc = OkfDocument::new(frontmatter, &normalized_text);
    okf_doc
        .write_to(&doc_path)
        .map_err(|e| format!("Failed to write knowledge doc: {e}"))?;

    let split_into_chunk_files = sb_config.knowledge_chunk_files_enabled && chunk_count > 1;
    if split_into_chunk_files {
        for (idx, chunk) in chunks.iter().enumerate() {
            let chunk_index = idx + 1;
            let chunk_file_name = chunk_file_name(&doc_id, chunk_index);
            let chunk_doc_path = docs_dir.join(&chunk_file_name);
            let mut chunk_frontmatter = OkfFrontmatter::new("Knowledge")
                .with_title(format!("{resolved_title} [{chunk_index}/{chunk_count}]"))
                .with_tags(categories.clone())
                .with_timestamp(timestamp)
                .with_extension("okf_profile", serde_json::json!("smartbrain-knowledge-v1"))
                .with_extension("source_file", serde_json::json!(source_file))
                .with_extension("source_type", serde_json::json!(extension))
                .with_extension("chunk_count", serde_json::json!(chunk_count))
                .with_extension("is_chunk", serde_json::json!(true))
                .with_extension("parent_doc_id", serde_json::json!(doc_id.clone()))
                .with_extension("chunk_index", serde_json::json!(chunk_index))
                .with_extension("chunk_total", serde_json::json!(chunk_count));
            if let Some(value) = resolved_description.clone() {
                chunk_frontmatter.description = Some(value);
            }
            if let Some(value) = domain.clone() {
                chunk_frontmatter =
                    chunk_frontmatter.with_extension("domain", serde_json::json!(value));
            }
            if let Some(value) = relative_path.clone() {
                chunk_frontmatter =
                    chunk_frontmatter.with_extension("relative_path", serde_json::json!(value));
            }
            if let Some(value) = source_group.clone() {
                chunk_frontmatter =
                    chunk_frontmatter.with_extension("source_group", serde_json::json!(value));
            }
            OkfDocument::new(chunk_frontmatter, chunk.as_str())
                .write_to(&chunk_doc_path)
                .map_err(|e| format!("Failed to write chunk knowledge doc: {e}"))?;
        }
    }

    knowledge_index.add_entry(KnowledgeEntry {
        doc_id: doc_id.clone(),
        source_file: source_file.clone(),
        source_type: extension.clone(),
        title: resolved_title.clone(),
        description: resolved_description.clone(),
        added_at: timestamp,
        chunk_count,
        categories: categories.clone(),
        domain: domain.clone(),
        relative_path: relative_path.clone(),
        source_group: source_group.clone(),
    });
    knowledge_index
        .save(knowledge_dir)
        .map_err(|e| format!("Failed to save knowledge index: {e}"))?;

    let mut bm25 = BM25Index::load(bm25_index_path);
    bm25.remove_document(&format!("know:{doc_id}"));
    remove_chunk_docs_from_bm25(&mut bm25, &doc_id);
    if split_into_chunk_files {
        for (idx, chunk) in chunks.iter().enumerate() {
            let chunk_index = idx + 1;
            let chunk_doc_id = chunk_bm25_doc_id(&doc_id, chunk_index);
            let chunk_title = format!("{resolved_title} [{chunk_index}/{chunk_count}]");
            let chunk_path = format!("knowledge/docs/{}", chunk_file_name(&doc_id, chunk_index));
            let indexed_doc = bm25_index::build_document_with_locator_metadata(
                chunk_doc_id,
                SourceType::Knowledge,
                chunk_path,
                chunk_title,
                chunk,
                timestamp,
                categories.clone(),
                Some("Knowledge".to_string()),
                domain.clone(),
                source_group.clone(),
                relative_path.clone(),
                Some(source_file.clone()),
                Some(doc_id.clone()),
                Some(chunk_index),
                Some(chunk_count),
                true,
            );
            bm25.add_document(indexed_doc);
        }
    } else {
        let indexed_doc = bm25_index::build_document_with_locator_metadata(
            format!("know:{doc_id}"),
            SourceType::Knowledge,
            format!("knowledge/docs/{doc_id}.md"),
            resolved_title.clone(),
            &normalized_text,
            timestamp,
            categories.clone(),
            Some("Knowledge".to_string()),
            domain.clone(),
            source_group.clone(),
            relative_path.clone(),
            Some(source_file.clone()),
            None,
            None,
            None,
            false,
        );
        bm25.add_document(indexed_doc);
    }
    if let Err(e) = bm25.save(bm25_index_path) {
        error!("Failed to save BM25 index after knowledge ingest: {e}");
    }

    if let Some(hier_value) = hierarchy_fragment {
        update_hierarchy(knowledge_dir, &doc_id, &hier_value);
    }

    info!("Knowledge ingested: {file_name} -> {doc_id} ({chunk_count} chunks)");

    if run_post_hooks {
        let workspace_config_dir = workspace_config_dir_from_knowledge_dir(knowledge_dir);
        super::regenerate_knowledge_index_md(&workspace_config_dir);
        super::append_log(
            knowledge_dir,
            "Creation",
            &format!("Ingested [{source_file}](docs/{doc_id}.md)"),
        );
    }

    Ok(doc_id)
}

pub fn collect_folder_candidates(
    folder_path: &Path,
    recursive: bool,
    allowed_extensions: &HashSet<String>,
) -> Result<FolderCandidateCollection, String> {
    if !folder_path.exists() {
        return Err(format!(
            "Folder does not exist: {}",
            folder_path.to_string_lossy()
        ));
    }
    if !folder_path.is_dir() {
        return Err(format!(
            "Path is not a folder: {}",
            folder_path.to_string_lossy()
        ));
    }
    if allowed_extensions.is_empty() {
        return Err("Allowed extensions list is empty".to_string());
    }

    let mut collection = FolderCandidateCollection::default();
    collect_folder_candidates_from_dir(
        folder_path,
        folder_path,
        recursive,
        allowed_extensions,
        &mut collection,
    )?;
    collection
        .candidates
        .sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(collection)
}

pub async fn ingest_folder_candidates(
    http: &reqwest::Client,
    config: &ConfigToml,
    knowledge_dir: &Path,
    bm25_index_path: &Path,
    candidates: Vec<FolderIngestCandidate>,
    skipped_count: usize,
    domain: Option<String>,
    source_group: Option<String>,
) -> FolderIngestSummary {
    let mut imported_count = 0usize;
    let mut failures = Vec::new();

    for candidate in candidates {
        let context = KnowledgeIngestContext {
            domain: domain.clone(),
            relative_path: Some(candidate.relative_path.clone()),
            source_group: source_group.clone(),
        };
        match ingest_document_inner(
            http,
            config,
            knowledge_dir,
            bm25_index_path,
            &candidate.source_path,
            &context,
            false,
        )
        .await
        {
            Ok(_) => imported_count += 1,
            Err(error) => failures.push(FolderIngestFailure {
                relative_path: candidate.relative_path,
                error,
            }),
        }
    }

    if imported_count > 0 {
        let workspace_config_dir = workspace_config_dir_from_knowledge_dir(knowledge_dir);
        super::regenerate_knowledge_index_md(&workspace_config_dir);
        super::append_log(
            knowledge_dir,
            "Batch Import",
            &format!(
                "Imported {} files from folder (domain: {}, source_group: {}).",
                imported_count,
                domain.clone().unwrap_or_else(|| "unknown".to_string()),
                source_group.clone().unwrap_or_else(|| "none".to_string())
            ),
        );
    }

    FolderIngestSummary {
        imported_count,
        skipped_count,
        failed_count: failures.len(),
        failures,
    }
}

pub fn backfill_legacy_metadata(knowledge_dir: &Path) -> usize {
    let mut knowledge_index = KnowledgeIndex::load(knowledge_dir);
    let mut changed_count = 0usize;

    for entry in &mut knowledge_index.entries {
        if entry.domain.is_none() {
            entry.domain = infer_domain_from_relative_path(Some(entry.source_file.as_str()))
                .or_else(|| Some("legacy".to_string()));
            changed_count += 1;
        }
        if entry.relative_path.is_none() {
            entry.relative_path = normalize_relative_display_path(&entry.source_file);
            changed_count += 1;
        }
    }

    if changed_count > 0 {
        if let Err(error) = knowledge_index.save(knowledge_dir) {
            error!("Failed to backfill knowledge metadata: {error}");
            return 0;
        }
    }

    changed_count
}

/// Update knowledge metadata (title/description/tags/domain/source_group) in place
/// and keep BM25 index synchronized.
pub fn update_knowledge_metadata(
    knowledge_dir: &Path,
    bm25_index_path: &Path,
    doc_id: &str,
    update: KnowledgeMetadataUpdate,
) -> Result<(), String> {
    let docs_dir = knowledge_dir.join("docs");
    let parent_doc_path = docs_dir.join(format!("{doc_id}.md"));

    let update_title = update.title.is_some();
    let update_description = update.description.is_some();
    let update_domain = update.domain.is_some();
    let update_source_group = update.source_group.is_some();

    let normalized_title = if let Some(title) = update.title {
        let trimmed = title.trim();
        if trimmed.is_empty() {
            return Err("title must not be empty".to_string());
        }
        Some(trimmed.to_string())
    } else {
        None
    };
    let normalized_description = normalize_optional_text(update.description);
    let normalized_tags = update.tags.map(normalize_tag_list);
    let normalized_domain = normalize_optional_text(update.domain);
    let normalized_source_group = normalize_optional_text(update.source_group);

    let mut knowledge_index = KnowledgeIndex::load(knowledge_dir);
    let updated_entry = {
        let Some(entry) = knowledge_index
            .entries
            .iter_mut()
            .find(|entry| entry.doc_id == doc_id)
        else {
            return Err(format!("Knowledge document not found: {doc_id}"));
        };

        if let Some(title) = normalized_title.clone() {
            entry.title = title;
        }
        if update_description {
            entry.description = normalized_description.clone();
        }
        if let Some(tags) = normalized_tags.clone() {
            entry.categories = tags;
        }
        if update_domain {
            entry.domain = normalized_domain.clone();
        }
        if update_source_group {
            entry.source_group = normalized_source_group.clone();
        }
        entry.clone()
    };

    knowledge_index.save(knowledge_dir)?;

    let mut doc_paths = vec![parent_doc_path.clone()];
    doc_paths.extend(list_chunk_doc_paths(&docs_dir, doc_id));
    let chunk_total_fallback = updated_entry
        .chunk_count
        .max(doc_paths.len().saturating_sub(1))
        .max(1);

    for path in &doc_paths {
        if !path.exists() {
            continue;
        }
        let raw_content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
        let mut parsed = if let Some(doc) = okf::parse_document(&raw_content) {
            doc
        } else {
            OkfDocument::new(
                OkfFrontmatter::new("Knowledge"),
                okf::extract_body(&raw_content),
            )
        };

        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let chunk_index = okf_extension_usize(&parsed.frontmatter, "chunk_index")
            .or_else(|| chunk_index_from_file_name(doc_id, file_name))
            .unwrap_or(1);
        let chunk_total =
            okf_extension_usize(&parsed.frontmatter, "chunk_total").unwrap_or(chunk_total_fallback);
        let is_chunk_doc = path != &parent_doc_path
            || parsed
                .frontmatter
                .extensions
                .get("is_chunk")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);

        parsed.frontmatter.concept_type = "Knowledge".to_string();
        parsed.frontmatter.tags = updated_entry.categories.clone();
        parsed.frontmatter.extensions.insert(
            "okf_profile".to_string(),
            serde_json::json!("smartbrain-knowledge-v1"),
        );
        parsed.frontmatter.extensions.insert(
            "source_file".to_string(),
            serde_json::json!(updated_entry.source_file.clone()),
        );
        parsed.frontmatter.extensions.insert(
            "source_type".to_string(),
            serde_json::json!(updated_entry.source_type.clone()),
        );
        parsed.frontmatter.extensions.insert(
            "chunk_count".to_string(),
            serde_json::json!(updated_entry.chunk_count.max(1)),
        );

        set_optional_extension(
            &mut parsed.frontmatter,
            "domain",
            updated_entry.domain.clone(),
        );
        set_optional_extension(
            &mut parsed.frontmatter,
            "relative_path",
            updated_entry.relative_path.clone(),
        );
        set_optional_extension(
            &mut parsed.frontmatter,
            "source_group",
            updated_entry.source_group.clone(),
        );

        if is_chunk_doc {
            parsed
                .frontmatter
                .extensions
                .insert("is_chunk".to_string(), serde_json::json!(true));
            parsed.frontmatter.extensions.insert(
                "parent_doc_id".to_string(),
                serde_json::json!(doc_id.to_string()),
            );
            parsed
                .frontmatter
                .extensions
                .insert("chunk_index".to_string(), serde_json::json!(chunk_index));
            parsed
                .frontmatter
                .extensions
                .insert("chunk_total".to_string(), serde_json::json!(chunk_total));
            if update_title {
                parsed.frontmatter.title = Some(format!(
                    "{} [{}/{}]",
                    updated_entry.title, chunk_index, chunk_total
                ));
            }
        } else {
            parsed
                .frontmatter
                .extensions
                .insert("is_chunk".to_string(), serde_json::json!(false));
            parsed.frontmatter.extensions.remove("parent_doc_id");
            parsed.frontmatter.extensions.remove("chunk_index");
            parsed.frontmatter.extensions.remove("chunk_total");
            if update_title {
                parsed.frontmatter.title = Some(updated_entry.title.clone());
            }
        }
        if update_description {
            parsed.frontmatter.description = updated_entry.description.clone();
        }

        parsed
            .write_to(path)
            .map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
    }

    let mut bm25 = BM25Index::load(bm25_index_path);
    bm25.remove_document(&format!("know:{doc_id}"));
    remove_chunk_docs_from_bm25(&mut bm25, doc_id);

    let chunk_paths = list_chunk_doc_paths(&docs_dir, doc_id);
    if !chunk_paths.is_empty() {
        let chunk_total = updated_entry.chunk_count.max(chunk_paths.len()).max(1);
        for (pos, chunk_path) in chunk_paths.iter().enumerate() {
            let raw_content = std::fs::read_to_string(chunk_path)
                .map_err(|e| format!("Failed to read {}: {e}", chunk_path.display()))?;
            if raw_content.trim().is_empty() {
                continue;
            }
            let (body, frontmatter) = if let Some(doc) = okf::parse_document(&raw_content) {
                (doc.body, Some(doc.frontmatter))
            } else {
                (raw_content, None)
            };
            let file_name = chunk_path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| format!("Invalid chunk file name: {}", chunk_path.display()))?;
            let chunk_index = frontmatter
                .as_ref()
                .and_then(|fm| okf_extension_usize(fm, "chunk_index"))
                .or_else(|| chunk_index_from_file_name(doc_id, file_name))
                .unwrap_or(pos + 1);
            let indexed_doc = bm25_index::build_document_with_locator_metadata(
                chunk_bm25_doc_id(doc_id, chunk_index),
                SourceType::Knowledge,
                format!("knowledge/docs/{file_name}"),
                frontmatter
                    .as_ref()
                    .and_then(|fm| fm.title.clone())
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| {
                        format!("{} [{}/{}]", updated_entry.title, chunk_index, chunk_total)
                    }),
                &body,
                updated_entry.added_at,
                frontmatter
                    .as_ref()
                    .map(|fm| fm.tags.clone())
                    .filter(|tags| !tags.is_empty())
                    .unwrap_or_else(|| updated_entry.categories.clone()),
                Some("Knowledge".to_string()),
                frontmatter
                    .as_ref()
                    .and_then(|fm| okf_extension_string(fm, "domain"))
                    .or_else(|| updated_entry.domain.clone()),
                frontmatter
                    .as_ref()
                    .and_then(|fm| okf_extension_string(fm, "source_group"))
                    .or_else(|| updated_entry.source_group.clone()),
                frontmatter
                    .as_ref()
                    .and_then(|fm| okf_extension_string(fm, "relative_path"))
                    .or_else(|| updated_entry.relative_path.clone()),
                frontmatter
                    .as_ref()
                    .and_then(|fm| okf_extension_string(fm, "source_file"))
                    .or_else(|| Some(updated_entry.source_file.clone())),
                Some(doc_id.to_string()),
                Some(chunk_index),
                Some(chunk_total),
                true,
            );
            bm25.add_document(indexed_doc);
        }
    } else if parent_doc_path.exists() {
        let parent_content = std::fs::read_to_string(&parent_doc_path)
            .map_err(|e| format!("Failed to read {}: {e}", parent_doc_path.display()))?;
        if !parent_content.trim().is_empty() {
            let (body, frontmatter) = if let Some(doc) = okf::parse_document(&parent_content) {
                (doc.body, Some(doc.frontmatter))
            } else {
                (parent_content, None)
            };
            let indexed_doc = bm25_index::build_document_with_locator_metadata(
                format!("know:{doc_id}"),
                SourceType::Knowledge,
                format!("knowledge/docs/{doc_id}.md"),
                frontmatter
                    .as_ref()
                    .and_then(|fm| fm.title.clone())
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| updated_entry.title.clone()),
                &body,
                updated_entry.added_at,
                frontmatter
                    .as_ref()
                    .map(|fm| fm.tags.clone())
                    .filter(|tags| !tags.is_empty())
                    .unwrap_or_else(|| updated_entry.categories.clone()),
                Some("Knowledge".to_string()),
                frontmatter
                    .as_ref()
                    .and_then(|fm| okf_extension_string(fm, "domain"))
                    .or_else(|| updated_entry.domain.clone()),
                frontmatter
                    .as_ref()
                    .and_then(|fm| okf_extension_string(fm, "source_group"))
                    .or_else(|| updated_entry.source_group.clone()),
                frontmatter
                    .as_ref()
                    .and_then(|fm| okf_extension_string(fm, "relative_path"))
                    .or_else(|| updated_entry.relative_path.clone()),
                frontmatter
                    .as_ref()
                    .and_then(|fm| okf_extension_string(fm, "source_file"))
                    .or_else(|| Some(updated_entry.source_file.clone())),
                None,
                None,
                None,
                false,
            );
            bm25.add_document(indexed_doc);
        }
    }

    if let Err(e) = bm25.save(bm25_index_path) {
        error!("Failed to save BM25 index after metadata update: {e}");
    }

    let workspace_config_dir = workspace_config_dir_from_knowledge_dir(knowledge_dir);
    super::regenerate_knowledge_index_md(&workspace_config_dir);
    super::append_log(
        knowledge_dir,
        "Update",
        &format!("Updated knowledge metadata `{doc_id}`"),
    );

    Ok(())
}

/// Remove a knowledge document and all its artifacts.
pub fn remove_knowledge(
    knowledge_dir: &Path,
    bm25_index_path: &Path,
    doc_id: &str,
) -> Result<(), String> {
    let mut knowledge_index = KnowledgeIndex::load(knowledge_dir);
    let entry = knowledge_index
        .entries
        .iter()
        .find(|e| e.doc_id == doc_id)
        .cloned();

    if let Some(entry) = &entry {
        let source_path =
            knowledge_sources_dir_from_knowledge_dir(knowledge_dir).join(&entry.source_file);
        let _ = std::fs::remove_file(&source_path);
        let legacy_source_path =
            legacy_knowledge_sources_dir(knowledge_dir).join(&entry.source_file);
        let _ = std::fs::remove_file(&legacy_source_path);
    }

    let doc_path = knowledge_dir.join("docs").join(format!("{doc_id}.md"));
    let _ = std::fs::remove_file(&doc_path);
    remove_chunk_files(&knowledge_dir.join("docs"), doc_id)?;

    knowledge_index.remove_entry(doc_id);
    knowledge_index.save(knowledge_dir)?;

    let mut bm25 = BM25Index::load(bm25_index_path);
    bm25.remove_document(&format!("know:{doc_id}"));
    remove_chunk_docs_from_bm25(&mut bm25, doc_id);
    if let Err(e) = bm25.save(bm25_index_path) {
        error!("Failed to save BM25 index after knowledge removal: {e}");
    }

    info!("Knowledge removed: {doc_id}");

    let workspace_config_dir = workspace_config_dir_from_knowledge_dir(knowledge_dir);
    super::regenerate_knowledge_index_md(&workspace_config_dir);
    super::append_log(
        knowledge_dir,
        "Deletion",
        &format!("Removed knowledge document `{doc_id}`"),
    );

    Ok(())
}

/// Scan source stores for new or modified files and ingest them.
pub async fn scan_and_ingest_new(
    http: &reqwest::Client,
    config: &ConfigToml,
    knowledge_dir: &Path,
    bm25_index_path: &Path,
) {
    let source_roots = [
        knowledge_sources_dir_from_knowledge_dir(knowledge_dir),
        legacy_knowledge_sources_dir(knowledge_dir),
    ];
    if source_roots.iter().all(|dir| !dir.exists()) {
        return;
    }

    let knowledge_index = KnowledgeIndex::load(knowledge_dir);
    let existing_sources: std::collections::HashSet<String> = knowledge_index
        .entries
        .iter()
        .map(|e| e.source_file.clone())
        .collect();

    let mut queued_source_files: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for sources_dir in source_roots {
        if !sources_dir.exists() {
            continue;
        }
        let entries = match std::fs::read_dir(&sources_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if existing_sources.contains(&file_name) {
                continue;
            }
            if !queued_source_files.insert(file_name.clone()) {
                continue;
            }

            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            match ingest_document(http, config, knowledge_dir, bm25_index_path, &path).await {
                Ok(doc_id) => info!("Auto-ingested new knowledge source: {file_name} -> {doc_id}"),
                Err(e) => warn!("Failed to auto-ingest {file_name}: {e}"),
            }
        }
    }
}

fn extract_text_from_file(path: &Path, extension: &str) -> Result<String, String> {
    match extension {
        "md" | "markdown" => std::fs::read_to_string(path)
            .map(|content| okf::extract_body(&content))
            .map_err(|e| format!("Failed to read markdown file: {e}")),
        "txt" | "text" | "json" | "toml" | "yaml" | "yml" | "rs" | "py" | "js" | "ts" | "tsx"
        | "jsx" | "html" | "css" | "xml" | "csv" | "sh" | "bat" | "ps1" | "cfg" | "ini" | "log" => {
            std::fs::read_to_string(path).map_err(|e| format!("Failed to read text file: {e}"))
        }
        "pdf" => {
            let bytes = std::fs::read(path).map_err(|e| format!("Failed to read PDF file: {e}"))?;
            pdf_extract::extract_text_from_mem(&bytes)
                .map_err(|e| format!("Failed to extract PDF text: {e}"))
        }
        "docx" => extract_docx_text(path),
        "xlsx" | "xls" => extract_excel_text(path),
        _ => std::fs::read_to_string(path)
            .map_err(|e| format!("Unsupported or unreadable file type '{extension}': {e}")),
    }
}

fn extract_docx_text(path: &Path) -> Result<String, String> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;
    use std::io::Read;

    let file = std::fs::File::open(path).map_err(|e| format!("Failed to open DOCX file: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("Failed to read DOCX as ZIP: {e}"))?;

    let mut xml_content = String::new();
    {
        let mut doc_part = archive
            .by_name("word/document.xml")
            .map_err(|e| format!("Failed to find document.xml in DOCX: {e}"))?;
        doc_part
            .read_to_string(&mut xml_content)
            .map_err(|e| format!("Failed to read document.xml: {e}"))?;
    }

    let mut reader = Reader::from_str(&xml_content);
    let mut text = String::new();
    let mut in_paragraph = false;
    let mut paragraph_buf = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let local = e.local_name();
                if local.as_ref() == b"p" {
                    in_paragraph = true;
                    paragraph_buf.clear();
                } else if local.as_ref() == b"br" && in_paragraph {
                    paragraph_buf.push('\n');
                } else if local.as_ref() == b"tab" && in_paragraph {
                    paragraph_buf.push('\t');
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_paragraph {
                    if let Ok(t) = e.unescape() {
                        paragraph_buf.push_str(&t);
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                if e.local_name().as_ref() == b"p" {
                    if !paragraph_buf.trim().is_empty() {
                        if !text.is_empty() {
                            text.push_str("\n\n");
                        }
                        text.push_str(paragraph_buf.trim());
                    }
                    in_paragraph = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML parse error in DOCX: {e}")),
            _ => {}
        }
    }

    Ok(text)
}

fn extract_excel_text(path: &Path) -> Result<String, String> {
    use calamine::{Reader, open_workbook_auto};

    let mut workbook =
        open_workbook_auto(path).map_err(|e| format!("Failed to open Excel file: {e}"))?;

    let mut markdown = String::new();
    let sheet_names: Vec<String> = workbook.sheet_names().to_vec();

    for sheet_name in &sheet_names {
        if let Ok(range) = workbook.worksheet_range(sheet_name) {
            if !markdown.is_empty() {
                markdown.push_str("\n\n");
            }
            markdown.push_str(&format!("## {sheet_name}\n\n"));

            let mut rows_iter = range.rows();
            if let Some(header) = rows_iter.next() {
                let header_cells: Vec<String> =
                    header.iter().map(|cell| cell.to_string()).collect();
                markdown.push_str("| ");
                markdown.push_str(&header_cells.join(" | "));
                markdown.push_str(" |\n");
                markdown.push_str("|");
                for _ in &header_cells {
                    markdown.push_str(" --- |");
                }
                markdown.push('\n');

                for row in rows_iter {
                    let cells: Vec<String> = row.iter().map(|cell| cell.to_string()).collect();
                    markdown.push_str("| ");
                    markdown.push_str(&cells.join(" | "));
                    markdown.push_str(" |\n");
                }
            }
        }
    }

    Ok(markdown)
}

fn unique_doc_id(base: &str, index: &KnowledgeIndex) -> String {
    if !index.has_entry(base) {
        return base.to_string();
    }

    let mut counter = 2usize;
    loop {
        let candidate = format!("{base}-{counter}");
        if !index.has_entry(&candidate) {
            return candidate;
        }
        counter += 1;
    }
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn normalize_tag_list(tags: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut normalized = Vec::new();
    for tag in tags {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lowered = trimmed.to_ascii_lowercase();
        if seen.insert(lowered) {
            normalized.push(trimmed.to_string());
        }
    }
    normalized
}

fn okf_extension_string(frontmatter: &OkfFrontmatter, key: &str) -> Option<String> {
    frontmatter
        .extensions
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn okf_extension_usize(frontmatter: &OkfFrontmatter, key: &str) -> Option<usize> {
    frontmatter
        .extensions
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .map(|value| value as usize)
}

fn set_optional_extension(frontmatter: &mut OkfFrontmatter, key: &str, value: Option<String>) {
    if let Some(value) = value {
        frontmatter
            .extensions
            .insert(key.to_string(), serde_json::json!(value));
    } else {
        frontmatter.extensions.remove(key);
    }
}

fn normalize_relative_display_path(input: &str) -> Option<String> {
    let raw = input.trim();
    if raw.is_empty() {
        return None;
    }
    let path = Path::new(raw);
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().to_string()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

fn relative_display_path_to_pathbuf(relative_path: &str) -> PathBuf {
    let mut path = PathBuf::new();
    for part in relative_path.split('/') {
        if !part.trim().is_empty() {
            path.push(part);
        }
    }
    path
}

fn infer_domain_from_relative_path(relative_path: Option<&str>) -> Option<String> {
    let path = relative_path.and_then(normalize_relative_display_path)?;
    if !path.contains('/') {
        return None;
    }
    path.split('/').next().map(str::to_string)
}

fn build_source_label_for_prompt(
    file_name: &str,
    domain: Option<&str>,
    relative_path: Option<&str>,
) -> String {
    match (domain, relative_path) {
        (Some(domain), Some(relative_path)) => {
            format!("{file_name} (domain: {domain}, relative_path: {relative_path})")
        }
        (Some(domain), None) => format!("{file_name} (domain: {domain})"),
        (None, Some(relative_path)) => format!("{file_name} (relative_path: {relative_path})"),
        (None, None) => file_name.to_string(),
    }
}

fn collect_folder_candidates_from_dir(
    root: &Path,
    current_dir: &Path,
    recursive: bool,
    allowed_extensions: &HashSet<String>,
    collection: &mut FolderCandidateCollection,
) -> Result<(), String> {
    let entries = std::fs::read_dir(current_dir).map_err(|error| {
        format!(
            "Failed to read folder {}: {error}",
            current_dir.to_string_lossy()
        )
    })?;

    for entry in entries {
        let entry = match entry {
            Ok(value) => value,
            Err(_) => {
                collection.skipped_count += 1;
                continue;
            }
        };
        let path = entry.path();
        if path.is_dir() {
            if recursive {
                collect_folder_candidates_from_dir(
                    root,
                    &path,
                    recursive,
                    allowed_extensions,
                    collection,
                )?;
            }
            continue;
        }
        if !path.is_file() {
            collection.skipped_count += 1;
            continue;
        }

        let extension = path
            .extension()
            .map(|value| value.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if !allowed_extensions.contains(&extension) {
            collection.skipped_count += 1;
            continue;
        }

        let relative = match path
            .strip_prefix(root)
            .ok()
            .and_then(|value| normalize_relative_display_path(&value.to_string_lossy()))
        {
            Some(value) => value,
            None => {
                collection.skipped_count += 1;
                continue;
            }
        };

        collection.candidates.push(FolderIngestCandidate {
            source_path: path,
            relative_path: relative,
        });
    }

    Ok(())
}

fn slug_from_name(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else if c == ' ' {
                '-'
            } else {
                '_'
            }
        })
        .collect();
    let slug = slug
        .trim_matches(|c: char| c == '-' || c == '_')
        .to_string();
    if slug.is_empty() {
        format!("doc-{}", now_secs())
    } else {
        slug
    }
}

pub(crate) fn chunk_file_prefix(doc_id: &str) -> String {
    format!("{doc_id}{CHUNK_FILE_SEPARATOR}")
}

pub(crate) fn chunk_file_name(doc_id: &str, chunk_index: usize) -> String {
    format!("{}{:04}.md", chunk_file_prefix(doc_id), chunk_index)
}

fn chunk_index_from_file_name(doc_id: &str, file_name: &str) -> Option<usize> {
    let prefix = chunk_file_prefix(doc_id);
    let suffix = file_name.strip_prefix(&prefix)?.strip_suffix(".md")?;
    suffix.parse::<usize>().ok()
}

pub(crate) fn chunk_bm25_doc_prefix(doc_id: &str) -> String {
    format!("know:{doc_id}{CHUNK_BM25_SEPARATOR}")
}

pub(crate) fn chunk_bm25_doc_id(doc_id: &str, chunk_index: usize) -> String {
    format!("{}{:04}", chunk_bm25_doc_prefix(doc_id), chunk_index)
}

pub(crate) fn list_chunk_doc_paths(docs_dir: &Path, doc_id: &str) -> Vec<PathBuf> {
    if !docs_dir.exists() {
        return Vec::new();
    }
    let prefix = chunk_file_prefix(doc_id);
    let mut paths = std::fs::read_dir(docs_dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&prefix) && name.ends_with(".md"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn remove_chunk_files(docs_dir: &Path, doc_id: &str) -> Result<(), String> {
    for path in list_chunk_doc_paths(docs_dir, doc_id) {
        if let Err(e) = std::fs::remove_file(&path) {
            return Err(format!(
                "Failed to remove chunk file {}: {e}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn remove_chunk_docs_from_bm25(bm25: &mut BM25Index, doc_id: &str) {
    let prefix = chunk_bm25_doc_prefix(doc_id);
    bm25.documents
        .retain(|doc| !doc.doc_id.starts_with(&prefix));
}

fn chunk_text(text: &str, max_chunk_tokens: usize) -> Vec<String> {
    if max_chunk_tokens == 0 {
        return vec![text.to_string()];
    }

    let normalized_text = text.replace("\r\n", "\n").replace('\r', "\n");
    let paragraphs: Vec<&str> = normalized_text.split("\n\n").collect();
    let mut chunks = Vec::new();
    let mut current_chunk = String::new();
    let mut current_tokens = 0usize;

    for para in paragraphs {
        let para_tokens =
            para.split_whitespace().count() + para.chars().filter(|c| is_cjk_char(*c)).count();
        if current_tokens + para_tokens > max_chunk_tokens && !current_chunk.is_empty() {
            chunks.push(std::mem::take(&mut current_chunk));
            current_tokens = 0;
        }
        if !current_chunk.is_empty() {
            current_chunk.push_str("\n\n");
        }
        current_chunk.push_str(para);
        current_tokens += para_tokens;
    }

    if !current_chunk.is_empty() {
        chunks.push(current_chunk);
    }

    if chunks.is_empty() {
        chunks.push(normalized_text);
    }

    chunks
}

fn is_cjk_char(ch: char) -> bool {
    matches!(ch, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}')
}

async fn organize_via_llm(
    http: &reqwest::Client,
    config: &ConfigToml,
    raw_text: &str,
    source_name: &str,
) -> Result<prompts::ParsedKnowledgeOrganizeOutput, String> {
    let (_, provider) = config.resolve_provider();
    let base_url = provider
        .resolve_base_url()
        .ok_or("No base URL configured")?;
    let api_key = provider.resolve_api_key().unwrap_or_default();
    if api_key.is_empty() {
        return Err("No API key configured".to_string());
    }
    let model = config.resolve_model();
    if model.is_empty() {
        return Err("No model configured".to_string());
    }
    let wire_api = provider.wire_api.as_deref().unwrap_or("chat");

    let prompt_messages = prompts::build_knowledge_organize_messages(raw_text, source_name);
    let internal_messages: Vec<InternalMessage> = prompt_messages
        .into_iter()
        .map(|(role, content)| InternalMessage {
            role,
            content: text_content(content),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        })
        .collect();

    let adapter = adapter::get_adapter(wire_api);
    let url = adapter.build_url(&base_url, &model);
    let headers = adapter.build_headers(&api_key);
    let (url, headers) = adapter::apply_request_overrides(
        url,
        headers,
        provider.query_params.as_ref(),
        provider.http_headers.as_ref(),
    )?;
    let body = adapter.build_body(&model, &internal_messages, None, config.max_output_tokens);

    let response = http
        .post(&url)
        .headers(headers)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Knowledge organization HTTP error: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        return Err(format!(
            "Knowledge organization LLM error ({status}): {body_text}"
        ));
    }

    let mut result_text = String::new();
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut utf8_decoder = crate::utf8_stream::Utf8StreamDecoder::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Stream error: {e}"))?;
        utf8_decoder.push(&mut buffer, &chunk);

        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim().to_string();
            buffer = buffer[line_end + 1..].to_string();

            if line.is_empty() {
                continue;
            }

            if adapter.is_stream_done(&line) {
                break;
            }

            for event in adapter.parse_stream_line(&line) {
                if let StreamEvent::TextDelta(delta) = event {
                    result_text.push_str(&delta);
                }
            }
        }
    }

    let result_text = result_text.trim().to_string();
    if result_text.is_empty() {
        return Err("Knowledge organization returned empty response".to_string());
    }

    Ok(prompts::parse_knowledge_organize_output(&result_text))
}

fn update_hierarchy(knowledge_dir: &Path, doc_id: &str, hierarchy_fragment: &serde_json::Value) {
    let mut hierarchy = KnowledgeHierarchy::load(knowledge_dir);

    if let Some(new_cats) = hierarchy_fragment
        .get("categories")
        .and_then(|v| v.as_array())
    {
        for cat_val in new_cats {
            if let (Some(name), summary) = (
                cat_val.get("name").and_then(|n| n.as_str()),
                cat_val
                    .get("summary")
                    .and_then(|s| s.as_str())
                    .unwrap_or(""),
            ) {
                if let Some(existing) = hierarchy.categories.iter_mut().find(|c| c.name == name) {
                    if !existing.docs.contains(&doc_id.to_string()) {
                        existing.docs.push(doc_id.to_string());
                    }
                } else {
                    hierarchy.categories.push(KnowledgeCategory {
                        name: name.to_string(),
                        summary: summary.to_string(),
                        children: Vec::new(),
                        docs: vec![doc_id.to_string()],
                    });
                }
            }
        }
    }

    if let Err(e) = hierarchy.save(knowledge_dir) {
        error!("Failed to save knowledge hierarchy: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_system::{ConfigToml, SmartBrainConfig};
    use crate::smartbrain::bm25_index::{SourceType, build_document};
    use reqwest::Client;

    #[test]
    fn collect_folder_candidates_respects_recursive_and_extension_filter() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path().join("dataset");
        let nested = root.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(root.join("a.txt"), "alpha").unwrap();
        std::fs::write(root.join("b.md"), "beta").unwrap();
        std::fs::write(nested.join("c.json"), "{\"k\":1}").unwrap();
        std::fs::write(nested.join("d.bin"), "raw").unwrap();

        let allowed = normalize_allowed_extensions(&["txt".to_string(), "json".to_string()]);
        let recursive = collect_folder_candidates(&root, true, &allowed).unwrap();
        let recursive_paths: Vec<String> = recursive
            .candidates
            .iter()
            .map(|candidate| candidate.relative_path.clone())
            .collect();
        assert_eq!(recursive_paths, vec!["a.txt", "nested/c.json"]);
        assert_eq!(recursive.skipped_count, 2);

        let non_recursive = collect_folder_candidates(&root, false, &allowed).unwrap();
        let non_recursive_paths: Vec<String> = non_recursive
            .candidates
            .iter()
            .map(|candidate| candidate.relative_path.clone())
            .collect();
        assert_eq!(non_recursive_paths, vec!["a.txt"]);
        assert_eq!(non_recursive.skipped_count, 1);
    }

    #[test]
    fn normalize_relative_path_rejects_parent_segments() {
        assert_eq!(
            normalize_relative_display_path("../secrets.txt"),
            None,
            "parent traversal should be rejected"
        );
    }

    #[test]
    fn backfill_legacy_metadata_sets_domain_and_relative_path() {
        let temp_dir = tempfile::tempdir().unwrap();
        let knowledge_dir = temp_dir.path().join("memories").join("knowledge");
        std::fs::create_dir_all(&knowledge_dir).unwrap();
        let mut index = KnowledgeIndex::default();
        index.entries.push(KnowledgeEntry {
            doc_id: "legacy-doc".to_string(),
            source_file: "legacy.txt".to_string(),
            source_type: "txt".to_string(),
            title: "Legacy".to_string(),
            description: None,
            added_at: 1_700_000_000,
            chunk_count: 1,
            categories: Vec::new(),
            domain: None,
            relative_path: None,
            source_group: None,
        });
        index.save(&knowledge_dir).unwrap();

        let changed = backfill_legacy_metadata(&knowledge_dir);
        assert_eq!(changed, 2);

        let reloaded = KnowledgeIndex::load(&knowledge_dir);
        assert_eq!(reloaded.entries.len(), 1);
        assert_eq!(reloaded.entries[0].domain.as_deref(), Some("legacy"));
        assert_eq!(
            reloaded.entries[0].relative_path.as_deref(),
            Some("legacy.txt")
        );
    }

    #[test]
    fn extract_text_from_markdown_strips_okf_frontmatter() {
        let temp_dir = tempfile::tempdir().unwrap();
        let markdown_path = temp_dir.path().join("doc.md");
        std::fs::write(
            &markdown_path,
            "---\ntype: Knowledge\ntitle: Test\n---\n\n# Body\n\nhello\n",
        )
        .unwrap();

        let extracted = extract_text_from_file(&markdown_path, "md").unwrap();
        assert_eq!(extracted, "# Body\n\nhello\n");
    }

    #[test]
    fn knowledge_sources_storage_is_outside_okf_bundle() {
        let temp_dir = tempfile::tempdir().unwrap();
        let knowledge_dir = temp_dir
            .path()
            .join("codey")
            .join("memories")
            .join("knowledge");
        let sources_dir = knowledge_sources_dir_from_knowledge_dir(&knowledge_dir);

        assert!(
            !sources_dir.starts_with(&knowledge_dir),
            "raw source store should be outside knowledge bundle: {}",
            sources_dir.display()
        );
    }

    #[tokio::test]
    async fn ingest_document_splits_long_text_into_chunk_files_when_enabled() {
        let temp_dir = tempfile::tempdir().unwrap();
        let workspace_dir = temp_dir.path().join("workspace");
        let memories_dir = workspace_dir.join("codey").join("memories");
        let knowledge_dir = memories_dir.join("knowledge");
        let bm25_path = memories_dir.join("smartbrain_index.json");
        std::fs::create_dir_all(&knowledge_dir).unwrap();

        let source_path = temp_dir.path().join("long-note.txt");
        std::fs::write(
            &source_path,
            "第一段 alpha beta gamma\n\n第二段 delta epsilon zeta\n\n第三段 eta theta iota",
        )
        .unwrap();

        let mut config = ConfigToml::default();
        config.smartbrain = Some(SmartBrainConfig {
            auto_organize: false,
            max_chunk_tokens: 3,
            knowledge_chunk_files_enabled: true,
            ..SmartBrainConfig::default()
        });

        let http = Client::new();
        let doc_id = ingest_document(&http, &config, &knowledge_dir, &bm25_path, &source_path)
            .await
            .expect("ingest should succeed");

        let docs_dir = knowledge_dir.join("docs");
        let parent_doc = docs_dir.join(format!("{doc_id}.md"));
        assert!(
            parent_doc.exists(),
            "parent doc should be kept for compatibility"
        );

        let chunk_prefix = format!("{doc_id}__chunk_");
        let chunk_files = std::fs::read_dir(&docs_dir)
            .unwrap()
            .flatten()
            .filter_map(|entry| entry.file_name().to_str().map(ToString::to_string))
            .filter(|name| name.starts_with(&chunk_prefix))
            .collect::<Vec<_>>();
        assert!(
            !chunk_files.is_empty(),
            "expected chunk files with prefix {chunk_prefix}"
        );
        let first_chunk_path = docs_dir.join(&chunk_files[0]);
        let first_chunk_content = std::fs::read_to_string(&first_chunk_path).unwrap();
        let parsed_chunk = okf::parse_document(&first_chunk_content)
            .expect("chunk file should be valid OKF markdown");
        assert_eq!(
            parsed_chunk
                .frontmatter
                .extensions
                .get("okf_profile")
                .and_then(serde_json::Value::as_str),
            Some("smartbrain-knowledge-v1")
        );
        assert_eq!(
            parsed_chunk
                .frontmatter
                .extensions
                .get("is_chunk")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );

        let bm25 = BM25Index::load(&bm25_path);
        let chunk_doc_prefix = format!("know:{doc_id}::chunk:");
        assert!(
            bm25.documents
                .iter()
                .any(|doc| doc.doc_id.starts_with(&chunk_doc_prefix)),
            "expected chunk docs to be indexed with prefix {chunk_doc_prefix}"
        );
    }

    #[tokio::test]
    async fn ingest_document_uses_chunk_fallback_when_max_chunk_tokens_is_zero() {
        let temp_dir = tempfile::tempdir().unwrap();
        let workspace_dir = temp_dir.path().join("workspace");
        let memories_dir = workspace_dir.join("codey").join("memories");
        let knowledge_dir = memories_dir.join("knowledge");
        let bm25_path = memories_dir.join("smartbrain_index.json");
        std::fs::create_dir_all(&knowledge_dir).unwrap();

        let source_path = temp_dir.path().join("long-note.txt");
        let paragraph = (0..260)
            .map(|idx| format!("token-{idx}"))
            .collect::<Vec<_>>()
            .join(" ");
        let content = format!("{paragraph}\n\n{paragraph}\n\n{paragraph}");
        std::fs::write(&source_path, content).unwrap();

        let mut config = ConfigToml::default();
        config.smartbrain = Some(SmartBrainConfig {
            auto_organize: false,
            max_chunk_tokens: 0,
            knowledge_chunk_files_enabled: true,
            ..SmartBrainConfig::default()
        });

        let http = Client::new();
        let doc_id = ingest_document(&http, &config, &knowledge_dir, &bm25_path, &source_path)
            .await
            .expect("ingest should succeed");

        let docs_dir = knowledge_dir.join("docs");
        let chunk_prefix = format!("{doc_id}__chunk_");
        let chunk_files = std::fs::read_dir(&docs_dir)
            .unwrap()
            .flatten()
            .filter_map(|entry| entry.file_name().to_str().map(ToString::to_string))
            .filter(|name| name.starts_with(&chunk_prefix))
            .collect::<Vec<_>>();
        assert!(
            !chunk_files.is_empty(),
            "fallback chunk size should split long doc even when max_chunk_tokens is zero"
        );

        let index = KnowledgeIndex::load(&knowledge_dir);
        let entry = index
            .entries
            .iter()
            .find(|item| item.doc_id == doc_id)
            .expect("knowledge entry should exist");
        assert!(
            entry.chunk_count > 1,
            "chunk_count should be greater than 1 when fallback splitting is active"
        );
    }

    #[test]
    fn chunk_text_handles_crlf_paragraph_breaks() {
        let text =
            "第一段 alpha beta gamma\r\n\r\n第二段 delta epsilon zeta\r\n\r\n第三段 eta theta iota";
        let chunks = chunk_text(text, 3);
        assert!(chunks.len() > 1, "CRLF paragraph breaks should be chunked");
        assert!(chunks.iter().all(|chunk| !chunk.contains('\r')));
    }

    #[test]
    fn chunk_text_splits_crlf_separated_paragraphs() {
        let text = "first one two\r\n\r\nsecond three four\r\n\r\nthird five six";
        let chunks = chunk_text(text, 4);
        assert!(
            chunks.len() >= 2,
            "crlf paragraphs should be split into multiple chunks"
        );
        assert!(chunks.iter().any(|chunk| chunk.contains("second")));
        assert!(chunks.iter().all(|chunk| !chunk.contains('\r')));
    }

    #[test]
    fn remove_knowledge_removes_chunk_files_and_chunk_bm25_docs() {
        let temp_dir = tempfile::tempdir().unwrap();
        let workspace_dir = temp_dir.path().join("workspace");
        let memories_dir = workspace_dir.join("codey").join("memories");
        let knowledge_dir = memories_dir.join("knowledge");
        let docs_dir = knowledge_dir.join("docs");
        let sources_dir = memories_dir.join("knowledge_sources");
        let bm25_path = memories_dir.join("smartbrain_index.json");
        std::fs::create_dir_all(&docs_dir).unwrap();
        std::fs::create_dir_all(&sources_dir).unwrap();

        let mut index = KnowledgeIndex::default();
        index.entries.push(KnowledgeEntry {
            doc_id: "test-doc".to_string(),
            source_file: "test-doc.txt".to_string(),
            source_type: "txt".to_string(),
            title: "Test Doc".to_string(),
            description: None,
            added_at: 1_700_000_000,
            chunk_count: 2,
            categories: vec!["alpha".to_string()],
            domain: Some("knowledge".to_string()),
            relative_path: Some("test-doc.txt".to_string()),
            source_group: Some("group-a".to_string()),
        });
        index.save(&knowledge_dir).unwrap();

        std::fs::write(docs_dir.join("test-doc.md"), "parent").unwrap();
        std::fs::write(docs_dir.join("test-doc__chunk_0001.md"), "chunk 1").unwrap();
        std::fs::write(docs_dir.join("test-doc__chunk_0002.md"), "chunk 2").unwrap();
        std::fs::write(sources_dir.join("test-doc.txt"), "source content").unwrap();

        let mut bm25 = BM25Index::default();
        bm25.add_document(build_document(
            "know:test-doc".to_string(),
            SourceType::Knowledge,
            "knowledge/docs/test-doc.md".to_string(),
            "test-doc".to_string(),
            "parent body",
            1_700_000_000,
        ));
        bm25.add_document(build_document(
            "know:test-doc::chunk:0001".to_string(),
            SourceType::Knowledge,
            "knowledge/docs/test-doc__chunk_0001.md".to_string(),
            "test-doc chunk 1".to_string(),
            "chunk body 1",
            1_700_000_000,
        ));
        bm25.add_document(build_document(
            "know:test-doc::chunk:0002".to_string(),
            SourceType::Knowledge,
            "knowledge/docs/test-doc__chunk_0002.md".to_string(),
            "test-doc chunk 2".to_string(),
            "chunk body 2",
            1_700_000_000,
        ));
        bm25.save(&bm25_path).unwrap();

        remove_knowledge(&knowledge_dir, &bm25_path, "test-doc").expect("remove should succeed");

        assert!(!docs_dir.join("test-doc.md").exists());
        assert!(!docs_dir.join("test-doc__chunk_0001.md").exists());
        assert!(!docs_dir.join("test-doc__chunk_0002.md").exists());

        let bm25_after = BM25Index::load(&bm25_path);
        assert!(
            bm25_after
                .documents
                .iter()
                .all(|doc| !doc.doc_id.starts_with("know:test-doc")),
            "all parent and chunk docs should be removed"
        );
    }

    #[test]
    fn update_knowledge_metadata_updates_index_and_preserves_body() {
        let temp_dir = tempfile::tempdir().unwrap();
        let workspace_dir = temp_dir.path().join("workspace");
        let memories_dir = workspace_dir.join("codey").join("memories");
        let knowledge_dir = memories_dir.join("knowledge");
        let docs_dir = knowledge_dir.join("docs");
        let bm25_path = memories_dir.join("smartbrain_index.json");
        std::fs::create_dir_all(&docs_dir).unwrap();

        let mut index = KnowledgeIndex::default();
        index.entries.push(KnowledgeEntry {
            doc_id: "meta-doc".to_string(),
            source_file: "meta-doc.txt".to_string(),
            source_type: "txt".to_string(),
            title: "Old Title".to_string(),
            description: None,
            added_at: 1_700_000_000,
            chunk_count: 2,
            categories: vec!["old".to_string()],
            domain: Some("legacy".to_string()),
            relative_path: Some("meta-doc.txt".to_string()),
            source_group: Some("legacy-group".to_string()),
        });
        index.save(&knowledge_dir).unwrap();

        std::fs::write(
            docs_dir.join("meta-doc.md"),
            "---\ntype: Knowledge\ntitle: Old Title\ntags: [old]\nsource_file: meta-doc.txt\nsource_type: txt\ndomain: legacy\nsource_group: legacy-group\nis_chunk: false\nchunk_count: 2\n---\n\nparent body unchanged",
        )
        .unwrap();
        std::fs::write(
            docs_dir.join("meta-doc__chunk_0001.md"),
            "---\ntype: Knowledge\ntitle: Old Title [1/2]\ntags: [old]\nsource_file: meta-doc.txt\nsource_type: txt\ndomain: legacy\nsource_group: legacy-group\nis_chunk: true\nparent_doc_id: meta-doc\nchunk_index: 1\nchunk_total: 2\n---\n\nchunk body 1",
        )
        .unwrap();
        std::fs::write(
            docs_dir.join("meta-doc__chunk_0002.md"),
            "---\ntype: Knowledge\ntitle: Old Title [2/2]\ntags: [old]\nsource_file: meta-doc.txt\nsource_type: txt\ndomain: legacy\nsource_group: legacy-group\nis_chunk: true\nparent_doc_id: meta-doc\nchunk_index: 2\nchunk_total: 2\n---\n\nchunk body 2",
        )
        .unwrap();

        update_knowledge_metadata(
            &knowledge_dir,
            &bm25_path,
            "meta-doc",
            KnowledgeMetadataUpdate {
                title: Some("New Title".to_string()),
                description: Some("updated description".to_string()),
                tags: Some(vec!["updated".to_string(), "kb".to_string()]),
                domain: Some("backend".to_string()),
                source_group: Some("import-batch-1".to_string()),
            },
        )
        .unwrap();

        let refreshed = KnowledgeIndex::load(&knowledge_dir);
        let entry = refreshed
            .entries
            .iter()
            .find(|entry| entry.doc_id == "meta-doc")
            .unwrap();
        assert_eq!(entry.title, "New Title");
        assert_eq!(entry.description.as_deref(), Some("updated description"));
        assert_eq!(
            entry.categories,
            vec!["updated".to_string(), "kb".to_string()]
        );
        assert_eq!(entry.domain.as_deref(), Some("backend"));
        assert_eq!(entry.source_group.as_deref(), Some("import-batch-1"));

        let parent =
            okf::parse_document(&std::fs::read_to_string(docs_dir.join("meta-doc.md")).unwrap())
                .unwrap();
        assert_eq!(parent.body, "parent body unchanged");
        assert_eq!(parent.frontmatter.title.as_deref(), Some("New Title"));
        assert_eq!(
            parent.frontmatter.description.as_deref(),
            Some("updated description")
        );
        assert_eq!(
            parent.frontmatter.tags,
            vec!["updated".to_string(), "kb".to_string()]
        );

        let chunk = okf::parse_document(
            &std::fs::read_to_string(docs_dir.join("meta-doc__chunk_0001.md")).unwrap(),
        )
        .unwrap();
        assert_eq!(chunk.body, "chunk body 1");
        assert_eq!(chunk.frontmatter.title.as_deref(), Some("New Title [1/2]"));
        assert_eq!(
            chunk.frontmatter.tags,
            vec!["updated".to_string(), "kb".to_string()]
        );
        assert_eq!(
            chunk
                .frontmatter
                .extensions
                .get("domain")
                .and_then(serde_json::Value::as_str),
            Some("backend")
        );

        let bm25 = BM25Index::load(&bm25_path);
        assert!(
            bm25.documents
                .iter()
                .any(|doc| doc.doc_id == "know:meta-doc::chunk:0001"
                    && doc.title == "New Title [1/2]"),
            "chunk-1 index should reflect updated title"
        );
        assert!(
            bm25.documents
                .iter()
                .all(|doc| doc.doc_id != "know:meta-doc"),
            "parent index should not be used when chunk docs exist"
        );
    }
}
