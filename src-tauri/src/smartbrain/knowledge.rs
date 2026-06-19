use std::path::Path;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use crate::adapter::types::{InternalMessage, text_content};
use crate::adapter::{self, types::StreamEvent};
use crate::config_system::ConfigToml;

use super::bm25_index::{self, BM25Index, SourceType};
use super::index::now_secs;
use super::okf::{OkfDocument, OkfFrontmatter};
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

/// Ingest a document file into the knowledge system.
///
/// Copies the source to `knowledge/sources/`, parses text, optionally calls
/// LLM to organize, writes processed content to `knowledge/docs/`, and
/// updates the knowledge index and BM25 index.
pub async fn ingest_document(
    http: &reqwest::Client,
    config: &ConfigToml,
    knowledge_dir: &Path,
    bm25_index_path: &Path,
    source_path: &Path,
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

    let doc_id = slug_from_name(&file_stem);

    let sources_dir = knowledge_dir.join("sources");
    let docs_dir = knowledge_dir.join("docs");
    let _ = std::fs::create_dir_all(&sources_dir);
    let _ = std::fs::create_dir_all(&docs_dir);

    let dest_path = sources_dir.join(&file_name);
    if source_path != dest_path {
        std::fs::copy(source_path, &dest_path)
            .map_err(|e| format!("Failed to copy source file: {e}"))?;
    }

    let raw_text = extract_text_from_file(&dest_path, &extension)?;
    if raw_text.trim().is_empty() {
        return Err("Document is empty or could not be parsed".to_string());
    }

    let sb_config = config.smartbrain_config();
    let (organized_text, hierarchy_fragment) = if sb_config.auto_organize {
        match organize_via_llm(http, config, &raw_text, &file_name).await {
            Ok((text, hier)) => (text, hier),
            Err(e) => {
                warn!("LLM organization failed for {file_name}, using raw text: {e}");
                (raw_text.clone(), None)
            }
        }
    } else {
        (raw_text.clone(), None)
    };

    let chunks = chunk_text(&organized_text, sb_config.max_chunk_tokens);
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
    let frontmatter = OkfFrontmatter::new("Knowledge")
        .with_title(&file_stem)
        .with_tags(categories.clone())
        .with_timestamp(timestamp)
        .with_extension("source_file", serde_json::json!(file_name))
        .with_extension("source_type", serde_json::json!(extension))
        .with_extension("chunk_count", serde_json::json!(chunk_count));

    let doc_path = docs_dir.join(format!("{doc_id}.md"));
    let okf_doc = OkfDocument::new(frontmatter, &organized_text);
    okf_doc
        .write_to(&doc_path)
        .map_err(|e| format!("Failed to write knowledge doc: {e}"))?;

    let mut knowledge_index = KnowledgeIndex::load(knowledge_dir);
    knowledge_index.add_entry(KnowledgeEntry {
        doc_id: doc_id.clone(),
        source_file: file_name.clone(),
        source_type: extension.clone(),
        title: file_stem.clone(),
        added_at: timestamp,
        chunk_count,
        categories: categories.clone(),
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
        &organized_text,
        timestamp,
        categories.clone(),
        Some("Knowledge".to_string()),
    );
    bm25.add_document(indexed_doc);
    if let Err(e) = bm25.save(bm25_index_path) {
        error!("Failed to save BM25 index after knowledge ingest: {e}");
    }

    if let Some(hier_value) = hierarchy_fragment {
        update_hierarchy(knowledge_dir, &doc_id, &hier_value);
    }

    info!("Knowledge ingested: {file_name} -> {doc_id} ({chunk_count} chunks)");

    let workspace_config_dir = knowledge_dir
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(knowledge_dir);
    super::regenerate_knowledge_index_md(workspace_config_dir);
    super::append_log(
        knowledge_dir,
        "Creation",
        &format!("Ingested [{file_name}](docs/{doc_id}.md)"),
    );

    Ok(doc_id)
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
        let source_path = knowledge_dir.join("sources").join(&entry.source_file);
        let _ = std::fs::remove_file(&source_path);
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

    let workspace_config_dir = knowledge_dir
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(knowledge_dir);
    super::regenerate_knowledge_index_md(workspace_config_dir);
    super::append_log(
        knowledge_dir,
        "Deletion",
        &format!("Removed knowledge document `{doc_id}`"),
    );

    Ok(())
}

/// Scan sources/ for new or modified files and ingest them.
pub async fn scan_and_ingest_new(
    http: &reqwest::Client,
    config: &ConfigToml,
    knowledge_dir: &Path,
    bm25_index_path: &Path,
) {
    let sources_dir = knowledge_dir.join("sources");
    if !sources_dir.exists() {
        return;
    }

    let knowledge_index = KnowledgeIndex::load(knowledge_dir);
    let existing_sources: std::collections::HashSet<String> = knowledge_index
        .entries
        .iter()
        .map(|e| e.source_file.clone())
        .collect();

    let entries = match std::fs::read_dir(&sources_dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().to_string();
        if existing_sources.contains(&file_name) {
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

fn extract_text_from_file(path: &Path, extension: &str) -> Result<String, String> {
    match extension {
        "md" | "markdown" | "txt" | "text" | "json" | "toml" | "yaml" | "yml" | "rs" | "py"
        | "js" | "ts" | "tsx" | "jsx" | "html" | "css" | "xml" | "csv" | "sh" | "bat" | "ps1"
        | "cfg" | "ini" | "log" => {
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
