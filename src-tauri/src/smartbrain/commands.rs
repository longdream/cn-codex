use tauri::State;

use crate::error::AppResult;
use crate::state::AppState;

use super::bm25_index::{BM25Index, SearchFilter, SourceType};
use super::index::ExperienceIndex;
use super::knowledge::KnowledgeIndex;

#[tauri::command]
pub async fn smartbrain_list_experiences(
    state: State<'_, AppState>,
) -> AppResult<serde_json::Value> {
    let experiences_dir = super::experiences_dir(&state.workspace_config_dir);
    let index = ExperienceIndex::load(&experiences_dir);

    let entries: Vec<serde_json::Value> = index
        .entries
        .iter()
        .filter(|e| e.title.is_some() || e.summary_slug.is_some())
        .map(|e| {
            serde_json::json!({
                "thread_id": e.thread_id,
                "extracted_at": e.extracted_at,
                "usage_count": e.usage_count,
                "last_used_at": e.last_used_at,
                "summary_slug": e.summary_slug,
                "title": e.title,
                "summary": e.summary,
                "categories": e.categories,
            })
        })
        .collect();

    Ok(serde_json::json!({ "entries": entries }))
}

#[tauri::command]
pub async fn smartbrain_read_experience(
    state: State<'_, AppState>,
    thread_id: String,
) -> AppResult<serde_json::Value> {
    let experiences_dir = super::experiences_dir(&state.workspace_config_dir);
    let raw_path = experiences_dir.join("raw").join(format!("{thread_id}.md"));

    let raw_content = std::fs::read_to_string(&raw_path).unwrap_or_default();
    let (content, frontmatter) = if let Some(doc) = super::okf::parse_document(&raw_content) {
        (
            doc.body,
            serde_json::json!({
                "type": doc.frontmatter.concept_type,
                "title": doc.frontmatter.title,
                "description": doc.frontmatter.description,
                "tags": doc.frontmatter.tags,
                "timestamp": doc.frontmatter.timestamp,
            }),
        )
    } else {
        (raw_content, serde_json::Value::Null)
    };

    Ok(serde_json::json!({
        "thread_id": thread_id,
        "content": content,
        "frontmatter": frontmatter,
    }))
}

#[tauri::command]
pub async fn smartbrain_delete_experience(
    state: State<'_, AppState>,
    thread_id: String,
) -> AppResult<serde_json::Value> {
    let experiences_dir = super::experiences_dir(&state.workspace_config_dir);
    let bm25_path = super::bm25_index_path(&state.workspace_config_dir);

    let raw_path = experiences_dir.join("raw").join(format!("{thread_id}.md"));
    let _ = std::fs::remove_file(&raw_path);

    let mut exp_index = ExperienceIndex::load(&experiences_dir);
    exp_index.remove_entry(&thread_id);
    let _ = exp_index.save(&experiences_dir);

    let mut bm25 = BM25Index::load(&bm25_path);
    bm25.remove_document(&format!("exp:{thread_id}"));
    let _ = bm25.save(&bm25_path);

    Ok(serde_json::json!({ "status": "ok" }))
}

#[tauri::command]
pub async fn smartbrain_list_knowledge(state: State<'_, AppState>) -> AppResult<serde_json::Value> {
    let knowledge_dir = super::knowledge_dir(&state.workspace_config_dir);
    let index = KnowledgeIndex::load(&knowledge_dir);

    let entries: Vec<serde_json::Value> = index
        .entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "doc_id": e.doc_id,
                "source_file": e.source_file,
                "source_type": e.source_type,
                "title": e.title,
                "added_at": e.added_at,
                "chunk_count": e.chunk_count,
                "categories": e.categories,
            })
        })
        .collect();

    Ok(serde_json::json!({ "entries": entries }))
}

#[tauri::command]
pub async fn smartbrain_read_knowledge(
    state: State<'_, AppState>,
    doc_id: String,
) -> AppResult<serde_json::Value> {
    let knowledge_dir = super::knowledge_dir(&state.workspace_config_dir);
    let doc_path = knowledge_dir.join("docs").join(format!("{doc_id}.md"));

    let raw_content = std::fs::read_to_string(&doc_path).unwrap_or_default();
    let (content, frontmatter) = if let Some(doc) = super::okf::parse_document(&raw_content) {
        (
            doc.body,
            serde_json::json!({
                "type": doc.frontmatter.concept_type,
                "title": doc.frontmatter.title,
                "description": doc.frontmatter.description,
                "tags": doc.frontmatter.tags,
                "timestamp": doc.frontmatter.timestamp,
            }),
        )
    } else {
        (raw_content, serde_json::Value::Null)
    };

    Ok(serde_json::json!({
        "doc_id": doc_id,
        "content": content,
        "frontmatter": frontmatter,
    }))
}

#[tauri::command]
pub async fn smartbrain_delete_knowledge(
    state: State<'_, AppState>,
    doc_id: String,
) -> AppResult<serde_json::Value> {
    let knowledge_dir = super::knowledge_dir(&state.workspace_config_dir);
    let bm25_path = super::bm25_index_path(&state.workspace_config_dir);

    super::knowledge::remove_knowledge(&knowledge_dir, &bm25_path, &doc_id)
        .map_err(|e| crate::error::AppError::Custom(e))?;

    Ok(serde_json::json!({ "status": "ok" }))
}

#[tauri::command]
pub async fn smartbrain_upload_knowledge(
    state: State<'_, AppState>,
    file_path: String,
) -> AppResult<serde_json::Value> {
    let config = state.config_manager.read()?;
    let knowledge_dir = super::knowledge_dir(&state.workspace_config_dir);
    let bm25_path = super::bm25_index_path(&state.workspace_config_dir);
    let source_path = std::path::PathBuf::from(&file_path);

    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .read_timeout(std::time::Duration::from_secs(300))
        .build()
        .unwrap_or_default();

    let doc_id =
        super::knowledge::ingest_document(&http, &config, &knowledge_dir, &bm25_path, &source_path)
            .await
            .map_err(|e| crate::error::AppError::Custom(e))?;

    Ok(serde_json::json!({
        "status": "ok",
        "doc_id": doc_id,
    }))
}

#[tauri::command]
pub async fn smartbrain_search(
    state: State<'_, AppState>,
    query: String,
    top_k: Option<usize>,
    concept_type: Option<String>,
    tags: Option<Vec<String>>,
    source_type: Option<String>,
) -> AppResult<serde_json::Value> {
    let config = state.config_manager.read()?;
    let sb_config = config.smartbrain_config();
    if !sb_config.is_active() {
        return Ok(serde_json::json!({
            "results": [],
            "error": "SmartBrain is disabled. Enable it in Settings.",
        }));
    }

    let bm25_path = super::bm25_index_path(&state.workspace_config_dir);

    let has_filter = concept_type.is_some()
        || tags.as_ref().is_some_and(|t| !t.is_empty())
        || source_type.is_some();

    let results = if has_filter {
        let filter = SearchFilter {
            concept_type,
            tags: tags.unwrap_or_default(),
            source_type: source_type.as_deref().map(|s| match s {
                "experience" => SourceType::Experience,
                _ => SourceType::Knowledge,
            }),
            timestamp_after: None,
            timestamp_before: None,
        };
        super::search::unified_search_with_filter(&bm25_path, &query, top_k.unwrap_or(10), filter)
    } else {
        super::search::unified_search(&bm25_path, &query, top_k.unwrap_or(10))
    };

    Ok(serde_json::json!({ "results": results }))
}

#[tauri::command]
pub async fn smartbrain_rebuild_index(state: State<'_, AppState>) -> AppResult<serde_json::Value> {
    let bm25_path = super::bm25_index_path(&state.workspace_config_dir);
    super::search::rebuild_index(&state.workspace_config_dir, &bm25_path);
    Ok(serde_json::json!({ "status": "ok" }))
}

/// Migrate existing non-OKF markdown files to OKF format by adding frontmatter.
#[tauri::command]
pub async fn smartbrain_migrate_to_okf(state: State<'_, AppState>) -> AppResult<serde_json::Value> {
    use super::okf::{self, OkfDocument, OkfFrontmatter};

    let knowledge_dir = super::knowledge_dir(&state.workspace_config_dir);
    let experiences_dir = super::experiences_dir(&state.workspace_config_dir);
    let mut migrated_knowledge = 0u32;
    let mut migrated_experiences = 0u32;

    let know_index = KnowledgeIndex::load(&knowledge_dir);
    let docs_dir = knowledge_dir.join("docs");
    for entry in &know_index.entries {
        let doc_path = docs_dir.join(format!("{}.md", entry.doc_id));
        if let Ok(content) = std::fs::read_to_string(&doc_path) {
            if okf::has_frontmatter(&content) {
                continue;
            }
            let frontmatter = OkfFrontmatter::new("Knowledge")
                .with_title(&entry.title)
                .with_tags(entry.categories.clone())
                .with_timestamp(entry.added_at)
                .with_extension("source_file", serde_json::json!(entry.source_file))
                .with_extension("source_type", serde_json::json!(entry.source_type))
                .with_extension("chunk_count", serde_json::json!(entry.chunk_count));

            let okf_doc = OkfDocument::new(frontmatter, &content);
            if okf_doc.write_to(&doc_path).is_ok() {
                migrated_knowledge += 1;
            }
        }
    }

    let exp_index = ExperienceIndex::load(&experiences_dir);
    let raw_dir = experiences_dir.join("raw");
    for entry in &exp_index.entries {
        let raw_path = raw_dir.join(format!("{}.md", entry.thread_id));
        if let Ok(content) = std::fs::read_to_string(&raw_path) {
            if okf::has_frontmatter(&content) {
                continue;
            }
            let mut frontmatter = OkfFrontmatter::new("Experience")
                .with_tags(entry.categories.clone())
                .with_timestamp(entry.extracted_at)
                .with_extension("thread_id", serde_json::json!(entry.thread_id))
                .with_extension("usage_count", serde_json::json!(entry.usage_count));

            if let Some(slug) = &entry.summary_slug {
                frontmatter = frontmatter.with_title(slug);
            }
            if let Some(last_used) = entry.last_used_at {
                frontmatter =
                    frontmatter.with_extension("last_used_at", serde_json::json!(last_used));
            }

            let okf_doc = OkfDocument::new(frontmatter, &content);
            if okf_doc.write_to(&raw_path).is_ok() {
                migrated_experiences += 1;
            }
        }
    }

    super::regenerate_knowledge_index_md(&state.workspace_config_dir);
    super::regenerate_experiences_index_md(&state.workspace_config_dir);
    super::regenerate_root_index_md(&state.workspace_config_dir);

    let bm25_path = super::bm25_index_path(&state.workspace_config_dir);
    super::search::rebuild_index(&state.workspace_config_dir, &bm25_path);

    super::append_log(
        &super::memories_dir(&state.workspace_config_dir),
        "Update",
        &format!(
            "Migrated to OKF format: {migrated_knowledge} knowledge docs, {migrated_experiences} experience docs"
        ),
    );

    Ok(serde_json::json!({
        "status": "ok",
        "migrated_knowledge": migrated_knowledge,
        "migrated_experiences": migrated_experiences,
    }))
}
