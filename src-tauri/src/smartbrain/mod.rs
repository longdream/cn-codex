pub mod bm25_index;
pub mod commands;
pub mod consolidator;
pub mod db_query;
pub mod extractor;
pub mod index;
pub mod knowledge;
pub mod mysql_native;
pub mod okf;
pub mod permissions;
pub mod postgres_native;
pub mod prompts;
pub mod search;
pub mod sqlserver_native;
pub mod ssh;
pub mod summarizer;

use std::path::{Path, PathBuf};

/// Root directory for all memory data: `codey/memories/`.
pub fn memories_dir(workspace_config_dir: &Path) -> PathBuf {
    workspace_config_dir.join("memories")
}

/// Root directory for experience data: `codey/memories/experiences/`.
pub fn experiences_dir(workspace_config_dir: &Path) -> PathBuf {
    workspace_config_dir.join("memories").join("experiences")
}

/// Root directory for knowledge data: `codey/memories/knowledge/`.
pub fn knowledge_dir(workspace_config_dir: &Path) -> PathBuf {
    workspace_config_dir.join("memories").join("knowledge")
}

/// Directory for raw uploaded knowledge sources.
///
/// This lives outside `knowledge/` so non-OKF source markdown files do not
/// become concept documents inside the OKF bundle tree.
pub fn knowledge_sources_dir(workspace_config_dir: &Path) -> PathBuf {
    workspace_config_dir
        .join("memories")
        .join("knowledge_sources")
}

/// Path to the experience summary injected into sessions.
pub fn summary_path(workspace_config_dir: &Path) -> PathBuf {
    experiences_dir(workspace_config_dir).join("experience_summary.md")
}

/// Path to the unified BM25 index file.
pub fn bm25_index_path(workspace_config_dir: &Path) -> PathBuf {
    workspace_config_dir
        .join("memories")
        .join("smartbrain_index.json")
}

/// Load the experience summary text if it exists and is non-empty.
/// Strips OKF frontmatter if present, returning only the body content.
pub fn load_summary(workspace_config_dir: &Path) -> Option<String> {
    let path = summary_path(workspace_config_dir);
    std::fs::read_to_string(&path)
        .ok()
        .map(|s| okf::extract_body(&s).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Load the knowledge hierarchy if it exists.
pub fn load_hierarchy(workspace_config_dir: &Path) -> Option<knowledge::KnowledgeHierarchy> {
    let path = knowledge_dir(workspace_config_dir).join("hierarchy.json");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

/// Regenerate the OKF `index.md` for the knowledge directory.
pub fn regenerate_knowledge_index_md(workspace_config_dir: &Path) {
    let kdir = knowledge_dir(workspace_config_dir);
    let know_index = knowledge::KnowledgeIndex::load(&kdir);

    let entries: Vec<(String, String, Option<String>)> = know_index
        .entries
        .iter()
        .map(|e| {
            let path = format!("docs/{}.md", e.doc_id);
            let desc = if e.categories.is_empty() {
                None
            } else {
                Some(e.categories.join(", "))
            };
            (path, e.title.clone(), desc)
        })
        .collect();

    let content = okf::generate_index_md("Knowledge Base", &entries, None);
    let index_path = kdir.join("index.md");
    if let Err(e) = std::fs::write(&index_path, &content) {
        tracing::error!("Failed to write knowledge index.md: {e}");
    }
}

/// Regenerate the OKF `index.md` for the experiences directory.
pub fn regenerate_experiences_index_md(workspace_config_dir: &Path) {
    let edir = experiences_dir(workspace_config_dir);
    let exp_index = index::ExperienceIndex::load(&edir);

    let entries: Vec<(String, String, Option<String>)> = exp_index
        .entries
        .iter()
        .filter(|e| e.summary_slug.is_some())
        .map(|e| {
            let path = format!("raw/{}.md", e.thread_id);
            let title = e
                .summary_slug
                .clone()
                .unwrap_or_else(|| e.thread_id.clone());
            let desc = if e.categories.is_empty() {
                None
            } else {
                Some(e.categories.join(", "))
            };
            (path, title, desc)
        })
        .collect();

    let content = okf::generate_index_md("Experiences", &entries, None);
    let index_path = edir.join("index.md");
    let _ = std::fs::create_dir_all(&edir);
    if let Err(e) = std::fs::write(&index_path, &content) {
        tracing::error!("Failed to write experiences index.md: {e}");
    }
}

/// Append a log entry to the `log.md` in the specified directory.
pub fn append_log(dir: &Path, action: &str, description: &str) {
    let log_path = dir.join("log.md");
    let existing = std::fs::read_to_string(&log_path).unwrap_or_default();
    let updated = okf::append_log_entry(&existing, action, description);
    if let Err(e) = std::fs::write(&log_path, &updated) {
        tracing::error!("Failed to write log.md: {e}");
    }
}

/// Regenerate the bundle-root `index.md` at `codey/memories/index.md`.
pub fn regenerate_root_index_md(workspace_config_dir: &Path) {
    let mdir = memories_dir(workspace_config_dir);
    let _ = std::fs::create_dir_all(&mdir);

    let entries = vec![
        (
            "knowledge/".to_string(),
            "Knowledge Base".to_string(),
            Some("Ingested documents and reference material".to_string()),
        ),
        (
            "experiences/".to_string(),
            "Experiences".to_string(),
            Some("Learned patterns from past sessions".to_string()),
        ),
    ];

    let content = okf::generate_index_md("Memory Bundle", &entries, Some("0.1"));
    let index_path = mdir.join("index.md");
    if let Err(e) = std::fs::write(&index_path, &content) {
        tracing::error!("Failed to write root memories index.md: {e}");
    }
}
