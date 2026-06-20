pub mod consolidator;
pub mod extractor;
pub mod index;
pub mod prompts;

use std::path::{Path, PathBuf};

/// Return the root directory for experience data: `codey/memories/experiences/`.
pub fn experiences_dir(workspace_config_dir: &Path) -> PathBuf {
    workspace_config_dir.join("memories").join("experiences")
}

/// Return the path to the experience summary injected into sessions.
pub fn summary_path(workspace_config_dir: &Path) -> PathBuf {
    experiences_dir(workspace_config_dir).join("experience_summary.md")
}

/// Load the experience summary text if it exists and is non-empty.
pub fn load_summary(workspace_config_dir: &Path) -> Option<String> {
    let path = summary_path(workspace_config_dir);
    std::fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
