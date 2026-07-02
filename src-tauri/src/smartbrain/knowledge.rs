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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEntry {
    pub doc_id: String,
    pub source_file: String,
    pub source_type: String,
    pub title: String,
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
    let domain = normalize_optional_text(context.domain.clone())
        .or_else(|| infer_domain_from_relative_path(relative_path.as_deref()))
        .or_else(|| {
            source_path
                .parent()
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().to_string())
                .and_then(|name| normalize_optional_text(Some(name)))
        });
    let source_group = normalize_optional_text(context.source_group.clone());
    let source_label =
        build_source_label_for_prompt(&file_name, domain.as_deref(), relative_path.as_deref());
    let (organized_text, hierarchy_fragment) = if sb_config.auto_organize {
        match organize_via_llm(http, config, &raw_text, &source_label).await {
            Ok((text, hier)) => (text, hier),
            Err(e) => {
                warn!("LLM organization failed for {file_name}, using raw text: {e}");
                (raw_text.clone(), None)
            }
        }
    } else {
        (raw_text.clone(), None)
    };

    let normalized_text = okf::extract_body(&organized_text);
    let chunks = chunk_text(&normalized_text, sb_config.max_chunk_tokens);
    let chunk_count = chunks.len().max(1);

    let categories = hierarchy_fragment
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

    let timestamp = now_secs();
    let mut frontmatter = OkfFrontmatter::new("Knowledge")
        .with_title(&file_stem)
        .with_tags(categories.clone())
        .with_timestamp(timestamp)
        .with_extension("source_file", serde_json::json!(source_file))
        .with_extension("source_type", serde_json::json!(extension))
        .with_extension("chunk_count", serde_json::json!(chunk_count));
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
    let okf_doc = OkfDocument::new(frontmatter, &normalized_text);
    okf_doc
        .write_to(&doc_path)
        .map_err(|e| format!("Failed to write knowledge doc: {e}"))?;

    knowledge_index.add_entry(KnowledgeEntry {
        doc_id: doc_id.clone(),
        source_file: source_file.clone(),
        source_type: extension.clone(),
        title: file_stem.clone(),
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
    let indexed_doc = bm25_index::build_document_with_metadata(
        format!("know:{doc_id}"),
        SourceType::Knowledge,
        format!("knowledge/docs/{doc_id}.md"),
        file_stem,
        &normalized_text,
        timestamp,
        categories.clone(),
        Some("Knowledge".to_string()),
        domain.clone(),
    );
    bm25.add_document(indexed_doc);
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

    knowledge_index.remove_entry(doc_id);
    knowledge_index.save(knowledge_dir)?;

    let mut bm25 = BM25Index::load(bm25_index_path);
    bm25.remove_document(&format!("know:{doc_id}"));
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

fn chunk_text(text: &str, max_chunk_tokens: usize) -> Vec<String> {
    if max_chunk_tokens == 0 {
        return vec![text.to_string()];
    }

    let paragraphs: Vec<&str> = text.split("\n\n").collect();
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
        chunks.push(text.to_string());
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
) -> Result<(String, Option<serde_json::Value>), String> {
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

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Stream error: {e}"))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim().to_string();
            buffer = buffer[line_end + 1..].to_string();

            if line.is_empty() || !line.starts_with("data: ") {
                continue;
            }

            let data = &line[6..];
            if adapter.is_stream_done(data) {
                break;
            }

            for event in adapter.parse_stream_line(data) {
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
}
