use tauri::State;

use crate::error::AppResult;
use crate::state::AppState;

use super::bm25_index::BM25Index;
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
        .map(|e| {
            serde_json::json!({
                "thread_id": e.thread_id,
                "extracted_at": e.extracted_at,
                "usage_count": e.usage_count,
                "last_used_at": e.last_used_at,
                "summary_slug": e.summary_slug,
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

    let content = std::fs::read_to_string(&raw_path).unwrap_or_default();
    Ok(serde_json::json!({
        "thread_id": thread_id,
        "content": content,
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
pub async fn smartbrain_list_knowledge(
    state: State<'_, AppState>,
) -> AppResult<serde_json::Value> {
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

    let content = std::fs::read_to_string(&doc_path).unwrap_or_default();
    Ok(serde_json::json!({
        "doc_id": doc_id,
        "content": content,
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

    let doc_id = super::knowledge::ingest_document(
        &http,
        &config,
        &knowledge_dir,
        &bm25_path,
        &source_path,
    )
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
    let results = super::search::unified_search(&bm25_path, &query, top_k.unwrap_or(10));

    Ok(serde_json::json!({ "results": results }))
}

#[tauri::command]
pub async fn smartbrain_rebuild_index(
    state: State<'_, AppState>,
) -> AppResult<serde_json::Value> {
    let bm25_path = super::bm25_index_path(&state.workspace_config_dir);
    super::search::rebuild_index(&state.workspace_config_dir, &bm25_path);
    Ok(serde_json::json!({ "status": "ok" }))
}
