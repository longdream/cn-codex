use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tauri::State;
use tracing::info;

use crate::adapter;
use crate::adapter::types::{InternalMessage, text_content};
use crate::config_system::ConfigToml;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

use super::bm25_index::{BM25Index, SearchFilter, SourceType};
use super::index::ExperienceIndex;
use super::knowledge::KnowledgeIndex;

const FOLDER_IMPORT_CONFIRM_THRESHOLD: usize = 200;
const FOLDER_IMPORT_PREVIEW_LIMIT: usize = 8;
const DEFAULT_EXPERIENCE_PAGE_SIZE: u32 = 20;
const MAX_EXPERIENCE_PAGE_SIZE: u32 = 100;
const DATABASE_PARSE_SYSTEM_PROMPT: &str = "You extract structured database connection information.\nReturn only a valid JSON object.\nDo not include the raw full connection string in the output.\nIf a field is missing, use null or an empty object.\nUse this exact schema:\n{\n  \"host\": string | null,\n  \"port\": number | null,\n  \"databaseName\": string | null,\n  \"username\": string | null,\n  \"password\": string | null,\n  \"filePath\": string | null,\n  \"schema\": string | null,\n  \"queryParams\": { [key: string]: string }\n}";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct SmartbrainDatabaseParseResult {
    host: Option<String>,
    port: Option<u16>,
    #[serde(rename = "databaseName", alias = "database_name", alias = "database")]
    database_name: Option<String>,
    username: Option<String>,
    password: Option<String>,
    #[serde(rename = "filePath", alias = "file_path")]
    file_path: Option<String>,
    schema: Option<String>,
    #[serde(rename = "queryParams", alias = "query_params")]
    query_params: HashMap<String, String>,
}

impl SmartbrainDatabaseParseResult {
    fn sanitize(self) -> Self {
        let clean_opt = |value: Option<String>| {
            value
                .map(|entry| entry.trim().to_string())
                .filter(|entry| !entry.is_empty())
        };

        Self {
            host: clean_opt(self.host),
            port: self.port.filter(|port| *port > 0),
            database_name: clean_opt(self.database_name),
            username: clean_opt(self.username),
            password: clean_opt(self.password),
            file_path: clean_opt(self.file_path),
            schema: clean_opt(self.schema),
            query_params: self
                .query_params
                .into_iter()
                .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
                .filter(|(key, value)| !key.is_empty() && !value.is_empty())
                .collect(),
        }
    }
}

fn build_database_parse_messages(db_type: &str, connection_uri: &str) -> Vec<InternalMessage> {
    let user_prompt = format!(
        "Parse this database connection string.\nDatabase type: {db_type}\nConnection string:\n{connection_uri}\n\nRules:\n- Recognize URL, JDBC, ADO.NET, DSN, key-value, and vendor-specific formats.\n- Extract password when present so the UI can auto-fill it.\n- Put non-sensitive extra parameters into queryParams.\n- If the connection targets SQLite, prefer filePath and leave host/database fields null when appropriate.\n- Return JSON only."
    );

    vec![
        InternalMessage {
            role: "system".to_string(),
            content: text_content(DATABASE_PARSE_SYSTEM_PROMPT),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
        InternalMessage {
            role: "user".to_string(),
            content: text_content(user_prompt),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
    ]
}

fn resolve_active_llm_endpoint(
    config: &ConfigToml,
) -> Result<
    (
        String,
        String,
        String,
        String,
        Option<HashMap<String, String>>,
        Option<HashMap<String, String>>,
    ),
    String,
> {
    let default_model = config.resolve_model();
    if !config.model_endpoints.is_empty() {
        let idx = config.active_endpoint_index.unwrap_or(0);
        let endpoint = &config.model_endpoints[idx.min(config.model_endpoints.len() - 1)];
        let (_, provider) = config.resolve_provider();
        let wire_api = endpoint
            .wire_api
            .as_deref()
            .or(provider.wire_api.as_deref())
            .unwrap_or("chat")
            .to_string();
        let model = endpoint
            .model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(default_model.as_str())
            .to_string();
        return Ok((
            endpoint.url.clone(),
            endpoint.api_key.clone().unwrap_or_default(),
            wire_api,
            model,
            None,
            None,
        ));
    }

    let (provider_id, provider) = config.resolve_provider();
    let base_url = provider
        .resolve_base_url()
        .ok_or_else(|| format!("No base URL configured for provider '{provider_id}'"))?;
    let api_key = provider.resolve_api_key().unwrap_or_default();
    if default_model.is_empty() {
        return Err("No active model configured for intelligent database parsing".to_string());
    }

    Ok((
        base_url,
        api_key,
        provider.wire_api.as_deref().unwrap_or("chat").to_string(),
        default_model,
        provider.query_params.clone(),
        provider.http_headers.clone(),
    ))
}

async fn run_text_prompt_via_active_llm(
    config: &ConfigToml,
    messages: &[InternalMessage],
) -> Result<String, String> {
    let (base_url, api_key, wire_api, model, query_params, extra_headers) =
        resolve_active_llm_endpoint(config)?;
    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(20))
        .read_timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|error| format!("Failed to create HTTP client: {error}"))?;

    let adapter = adapter::get_adapter(&wire_api);
    let url = adapter.build_url(&base_url, &model);
    let headers = adapter.build_headers(&api_key);
    let (url, headers) = adapter::apply_request_overrides(
        url,
        headers,
        query_params.as_ref(),
        extra_headers.as_ref(),
    )?;
    let mut body = adapter::build_non_stream_body(
        &*adapter,
        &model,
        messages,
        None,
        config.max_output_tokens.or(Some(800)),
    );
    if let Some(obj) = body.as_object_mut() {
        if wire_api == "chat" {
            obj.insert(
                "response_format".to_string(),
                serde_json::json!({ "type": "json_object" }),
            );
        }
    }

    let response = http
        .post(&url)
        .headers(headers)
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("Database parse request failed: {error}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        return Err(format!(
            "Database parse model error ({status}): {body_text}"
        ));
    }

    let raw_body = response
        .text()
        .await
        .map_err(|error| format!("Failed to read database parse response body: {error}"))?;
    let result_text = crate::standalone::extract_non_streaming_fortune_text(&raw_body)
        .unwrap_or_else(|_| raw_body.trim().to_string());
    if result_text.is_empty() {
        return Err("Database parse model returned empty content".to_string());
    }

    Ok(result_text)
}

fn extract_json_object_candidate(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(stripped) = trimmed.strip_prefix("```json") {
        let candidate = stripped.trim();
        if let Some(end) = candidate.rfind("```") {
            return Some(candidate[..end].trim().to_string());
        }
    }

    if let Some(stripped) = trimmed.strip_prefix("```") {
        let candidate = stripped.trim();
        if let Some(end) = candidate.rfind("```") {
            return Some(candidate[..end].trim().to_string());
        }
    }

    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    if end < start {
        return None;
    }
    Some(trimmed[start..=end].trim().to_string())
}

fn parse_database_parse_result(text: &str) -> Result<SmartbrainDatabaseParseResult, String> {
    let candidate = extract_json_object_candidate(text).unwrap_or_else(|| text.trim().to_string());
    serde_json::from_str::<SmartbrainDatabaseParseResult>(&candidate)
        .map(SmartbrainDatabaseParseResult::sanitize)
        .map_err(|error| format!("Failed to parse intelligent database JSON: {error}"))
}

fn experience_entry_to_json(e: &super::index::ExperienceEntry) -> serde_json::Value {
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
}

fn delete_experiences_impl(
    experiences_dir: &std::path::Path,
    bm25_path: &std::path::Path,
    thread_ids: &[String],
) -> u32 {
    if thread_ids.is_empty() {
        return 0;
    }

    let mut exp_index = ExperienceIndex::load(experiences_dir);
    let mut bm25 = BM25Index::load(bm25_path);
    let mut deleted = 0u32;

    for thread_id in thread_ids {
        let raw_path = experiences_dir.join("raw").join(format!("{thread_id}.md"));
        let _ = std::fs::remove_file(&raw_path);
        if exp_index.remove_entry(thread_id) {
            deleted += 1;
        }
        bm25.remove_document(&format!("exp:{thread_id}"));
    }

    if deleted > 0 {
        if let Err(e) = exp_index.save(experiences_dir) {
            tracing::warn!("Failed to save experience index after delete: {e}");
        }
        if let Err(e) = bm25.save(bm25_path) {
            tracing::warn!("Failed to save BM25 index after delete: {e}");
        }
    }

    if exp_index.entries.is_empty() {
        for name in [
            "experience_summary.md",
            "experience_handbook.md",
            "index.md",
            "log.md",
        ] {
            let _ = std::fs::remove_file(experiences_dir.join(name));
        }
        exp_index.last_consolidated_at = None;
        let _ = exp_index.save(experiences_dir);
    }

    deleted
}

#[tauri::command]
pub async fn smartbrain_parse_database_connection(
    state: State<'_, AppState>,
    db_type: String,
    connection_uri: String,
) -> AppResult<serde_json::Value> {
    let config = state.config_manager.read()?;
    let messages = build_database_parse_messages(&db_type, &connection_uri);
    let result_text = run_text_prompt_via_active_llm(&config, &messages)
        .await
        .map_err(AppError::Custom)?;
    let parsed = parse_database_parse_result(&result_text).map_err(AppError::Custom)?;

    info!(
        "smartbrain database parse completed: db_type={}, host_present={}, database_present={}, user_present={}",
        db_type,
        parsed.host.is_some(),
        parsed.database_name.is_some(),
        parsed.username.is_some(),
    );

    Ok(serde_json::json!({
        "host": parsed.host,
        "port": parsed.port,
        "databaseName": parsed.database_name,
        "username": parsed.username,
        "password": parsed.password,
        "filePath": parsed.file_path,
        "schema": parsed.schema,
        "queryParams": parsed.query_params,
    }))
}

#[tauri::command]
pub async fn smartbrain_list_experiences(
    state: State<'_, AppState>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> AppResult<serde_json::Value> {
    let experiences_dir = super::experiences_dir(&state.workspace_config_dir);
    let index = ExperienceIndex::load(&experiences_dir);

    let mut filtered: Vec<&super::index::ExperienceEntry> = index
        .entries
        .iter()
        .filter(|e| e.title.is_some() || e.summary_slug.is_some())
        .collect();
    filtered.sort_by(|a, b| b.extracted_at.cmp(&a.extracted_at));

    let total = filtered.len();
    let offset = offset.unwrap_or(0) as usize;
    let limit = limit
        .unwrap_or(DEFAULT_EXPERIENCE_PAGE_SIZE)
        .clamp(1, MAX_EXPERIENCE_PAGE_SIZE) as usize;

    let entries: Vec<serde_json::Value> = filtered
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(experience_entry_to_json)
        .collect();

    Ok(serde_json::json!({ "entries": entries, "total": total }))
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
    delete_experiences_impl(&experiences_dir, &bm25_path, &[thread_id]);
    Ok(serde_json::json!({ "status": "ok" }))
}

#[tauri::command]
pub async fn smartbrain_delete_experiences(
    state: State<'_, AppState>,
    thread_ids: Vec<String>,
) -> AppResult<serde_json::Value> {
    let experiences_dir = super::experiences_dir(&state.workspace_config_dir);
    let bm25_path = super::bm25_index_path(&state.workspace_config_dir);
    let deleted_count = delete_experiences_impl(&experiences_dir, &bm25_path, &thread_ids);
    Ok(serde_json::json!({
        "status": "ok",
        "deleted_count": deleted_count,
    }))
}

/// Manually trigger categorization and merging of experiences to reduce their count.
///
/// This is the backend counterpart of the "分类总结" button: it asks the LLM to
/// group similar experiences and replace them with fewer consolidated entries.
#[tauri::command]
pub async fn smartbrain_summarize_experiences(
    state: State<'_, AppState>,
) -> AppResult<serde_json::Value> {
    let config = state.config_manager.read()?;
    let experiences_dir = super::experiences_dir(&state.workspace_config_dir);
    let bm25_path = super::bm25_index_path(&state.workspace_config_dir);

    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .read_timeout(std::time::Duration::from_secs(300))
        .build()
        .unwrap_or_default();

    let stats =
        super::summarizer::run_summarize_merge(&http, &config, &experiences_dir, Some(&bm25_path))
            .await;

    Ok(serde_json::json!({
        "status": if stats.success { "ok" } else { "error" },
        "beforeCount": stats.before_count,
        "afterCount": stats.after_count,
        "error": stats.error,
        "skipReason": stats.skip_reason,
    }))
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
                "description": e.description,
                "added_at": e.added_at,
                "chunk_count": e.chunk_count,
                "categories": e.categories,
                "domain": e.domain,
                "relative_path": e.relative_path,
                "source_group": e.source_group,
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
pub async fn smartbrain_update_knowledge(
    state: State<'_, AppState>,
    doc_id: String,
    title: Option<String>,
    description: Option<String>,
    tags: Option<Vec<String>>,
    domain: Option<String>,
    source_group: Option<String>,
) -> AppResult<serde_json::Value> {
    let knowledge_dir = super::knowledge_dir(&state.workspace_config_dir);
    let bm25_path = super::bm25_index_path(&state.workspace_config_dir);
    super::knowledge::update_knowledge_metadata(
        &knowledge_dir,
        &bm25_path,
        &doc_id,
        super::knowledge::KnowledgeMetadataUpdate {
            title,
            description,
            tags,
            domain,
            source_group,
        },
    )
    .map_err(crate::error::AppError::Custom)?;

    Ok(serde_json::json!({
        "status": "ok",
        "doc_id": doc_id,
    }))
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
pub async fn smartbrain_upload_knowledge_folder(
    state: State<'_, AppState>,
    folder_path: String,
    recursive: Option<bool>,
    allowed_extensions: Option<Vec<String>>,
    confirmed: Option<bool>,
) -> AppResult<serde_json::Value> {
    let config = state.config_manager.read()?;
    let knowledge_dir = super::knowledge_dir(&state.workspace_config_dir);
    let bm25_path = super::bm25_index_path(&state.workspace_config_dir);
    let root_path = std::path::PathBuf::from(&folder_path);
    let recursive = recursive.unwrap_or(true);
    let confirmed = confirmed.unwrap_or(false);
    let allowed_extensions =
        allowed_extensions.unwrap_or_else(super::knowledge::default_folder_upload_extensions);
    let normalized_extensions = super::knowledge::normalize_allowed_extensions(&allowed_extensions);
    let collection =
        super::knowledge::collect_folder_candidates(&root_path, recursive, &normalized_extensions)
            .map_err(crate::error::AppError::Custom)?;
    let candidate_count = collection.candidates.len();
    let preview_paths: Vec<String> = collection
        .candidates
        .iter()
        .take(FOLDER_IMPORT_PREVIEW_LIMIT)
        .map(|candidate| candidate.relative_path.clone())
        .collect();

    if candidate_count > FOLDER_IMPORT_CONFIRM_THRESHOLD && !confirmed {
        return Ok(serde_json::json!({
            "status": "needs_confirmation",
            "requires_confirmation": true,
            "candidate_count": candidate_count,
            "skipped_count": collection.skipped_count,
            "threshold": FOLDER_IMPORT_CONFIRM_THRESHOLD,
            "preview_paths": preview_paths,
        }));
    }

    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .read_timeout(std::time::Duration::from_secs(300))
        .build()
        .unwrap_or_default();
    let domain = root_path
        .file_name()
        .map(|name| name.to_string_lossy().trim().to_string())
        .filter(|value| !value.is_empty());
    let source_group = Some(format!("folder-{}", super::index::now_secs()));
    let summary = super::knowledge::ingest_folder_candidates(
        &http,
        &config,
        &knowledge_dir,
        &bm25_path,
        collection.candidates,
        collection.skipped_count,
        domain.clone(),
        source_group.clone(),
    )
    .await;

    Ok(serde_json::json!({
        "status": "ok",
        "requires_confirmation": false,
        "candidate_count": candidate_count,
        "domain": domain,
        "source_group": source_group,
        "imported_count": summary.imported_count,
        "skipped_count": summary.skipped_count,
        "failed_count": summary.failed_count,
        "failures": summary.failures,
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
    domain: Option<String>,
    source_group: Option<String>,
    relative_path_prefix: Option<String>,
    source_file: Option<String>,
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
    let prefilter_enabled = sb_config.search_okf_prefilter_enabled;
    let prefilter_order = sb_config.search_okf_prefilter_order.clone();
    let top_k = top_k.unwrap_or(10);

    let has_filter = concept_type.is_some()
        || tags.as_ref().is_some_and(|t| !t.is_empty())
        || source_type.is_some()
        || domain.is_some()
        || source_group.is_some()
        || relative_path_prefix.is_some()
        || source_file.is_some();

    let results = if has_filter {
        let filter = SearchFilter {
            concept_type,
            tags: tags.unwrap_or_default(),
            domain,
            source_group,
            relative_path_prefix,
            source_file,
            source_type: source_type.as_deref().map(|s| match s {
                "experience" => SourceType::Experience,
                _ => SourceType::Knowledge,
            }),
            timestamp_after: None,
            timestamp_before: None,
        };
        super::search::unified_search_with_filter_with_policy(
            &bm25_path,
            &query,
            top_k,
            filter,
            prefilter_enabled,
            Some(&prefilter_order),
        )
    } else {
        super::search::unified_search(&bm25_path, &query, top_k)
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
            let frontmatter = if let Some(domain) = &entry.domain {
                frontmatter.with_extension("domain", serde_json::json!(domain))
            } else {
                frontmatter
            };
            let frontmatter = if let Some(relative_path) = &entry.relative_path {
                frontmatter.with_extension("relative_path", serde_json::json!(relative_path))
            } else {
                frontmatter
            };
            let frontmatter = if let Some(source_group) = &entry.source_group {
                frontmatter.with_extension("source_group", serde_json::json!(source_group))
            } else {
                frontmatter
            };

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
