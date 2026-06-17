pub mod bm25_index;
pub mod commands;
pub mod consolidator;
pub mod extractor;
pub mod index;
pub mod knowledge;
pub mod prompts;
pub mod search;

use std::path::{Path, PathBuf};

/// Root directory for experience data: `codey/memories/experiences/`.
pub fn experiences_dir(workspace_config_dir: &Path) -> PathBuf {
    workspace_config_dir.join("memories").join("experiences")
}

/// Root directory for knowledge data: `codey/memories/knowledge/`.
pub fn knowledge_dir(workspace_config_dir: &Path) -> PathBuf {
    workspace_config_dir.join("memories").join("knowledge")
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
pub fn load_summary(workspace_config_dir: &Path) -> Option<String> {
    let path = summary_path(workspace_config_dir);
    std::fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Load the knowledge hierarchy if it exists.
pub fn load_hierarchy(workspace_config_dir: &Path) -> Option<knowledge::KnowledgeHierarchy> {
    let path = knowledge_dir(workspace_config_dir).join("hierarchy.json");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}
