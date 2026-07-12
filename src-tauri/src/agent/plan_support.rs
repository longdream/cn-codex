use std::path::{Path, PathBuf};

use tracing::warn;

fn non_empty_trimmed(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub(super) fn extract_proposed_plan(text: &str) -> Option<String> {
    const OPEN_TAG: &str = "<proposed_plan>";
    const CLOSE_TAG: &str = "</proposed_plan>";

    let start = text.find(OPEN_TAG)?;
    let content_start = start + OPEN_TAG.len();
    let end = text[content_start..].find(CLOSE_TAG)?;
    non_empty_trimmed(&text[content_start..content_start + end])
}

pub(crate) fn resolve_effective_plan_content(
    turn_mode: &str,
    plan_text: Option<&str>,
    raw_text: &str,
    cleaned_text: &str,
) -> Option<String> {
    let stream_plan = plan_text.and_then(non_empty_trimmed);
    if turn_mode != "plan" {
        return stream_plan;
    }

    stream_plan
        .or_else(|| extract_proposed_plan(raw_text))
        .or_else(|| extract_proposed_plan(cleaned_text))
        .or_else(|| non_empty_trimmed(cleaned_text))
}

pub(crate) fn user_requested_new_plan_file(user_input: &str) -> bool {
    let normalized = user_input.trim().to_lowercase();
    let en_patterns = [
        "new plan",
        "new implementation plan",
        "create a new plan",
        "start a new plan",
        "write a new plan",
        "generate a new plan",
        "new version of plan",
        "another plan",
    ];
    if en_patterns
        .iter()
        .any(|pattern| normalized.contains(pattern))
    {
        return true;
    }

    let zh_patterns = [
        "新计划",
        "新建计划",
        "重新生成计划",
        "重新做计划",
        "重新写计划",
        "再来一份计划",
        "再生成一份计划",
        "换一份计划",
        "新版本计划",
    ];
    zh_patterns
        .iter()
        .any(|pattern| user_input.trim().contains(pattern))
}

pub(crate) fn resolve_plan_storage_path(stored_path: &str, workspace_root: &Path) -> PathBuf {
    let candidate = PathBuf::from(stored_path);
    if candidate.is_absolute() {
        candidate
    } else {
        workspace_root.join(candidate)
    }
}

pub(crate) fn read_plan_file_content(stored_path: &str, workspace_root: &Path) -> Option<String> {
    let path = resolve_plan_storage_path(stored_path, workspace_root);
    match std::fs::read_to_string(&path) {
        Ok(content) => non_empty_trimmed(&content),
        Err(err) => {
            warn!("Failed to read active plan file {}: {err}", path.display());
            None
        }
    }
}

pub(crate) fn plan_contents_equivalent(left: &str, right: &str) -> bool {
    normalize_plan_content_for_compare(left) == normalize_plan_content_for_compare(right)
}

fn normalize_plan_content_for_compare(text: &str) -> String {
    text.replace("\r\n", "\n").trim().to_string()
}

pub(crate) fn build_active_plan_context_prompt(path: &str, revision: u64, content: &str) -> String {
    const MAX_PLAN_CONTEXT_CHARS: usize = 12_000;
    let total_chars = content.chars().count();
    let truncated_content: String = if total_chars > MAX_PLAN_CONTEXT_CHARS {
        content.chars().take(MAX_PLAN_CONTEXT_CHARS).collect()
    } else {
        content.to_string()
    };
    let truncation_note = if total_chars > MAX_PLAN_CONTEXT_CHARS {
        "\n\nNOTE: The active plan content was truncated for context size. \
         Keep revisions compatible with the visible content and preserve existing structure."
    } else {
        ""
    };

    format!(
        "An active implementation plan already exists in this thread. \
         Unless the user explicitly requests a new plan/version, revise this plan in place. \
         Return the fully revised plan (not just a delta) inside <proposed_plan> tags.\n\
         \nActive plan path: {path}\n\
         Active plan revision: {}\n\
         \n<current_active_plan>\n{truncated_content}\n</current_active_plan>{truncation_note}",
        revision.max(1)
    )
}
