use std::sync::Arc;

use tauri::{AppHandle, State};
use tokio::sync::RwLock;

use crate::state::AppState;

use super::runtime::LanCollabRuntime;
use super::types::{
    ChatMessage, CollabGroup, LanCollabStatus, NearbyPeer, NodeIdentity, RemoteKnowledgeDoc,
    RemoteKnowledgeHit, SharedKnowledgeDocMeta, SharedKnowledgeOffer, SharedModelOffer,
    SharedSkillOffer, SharedWorkflowOffer,
};
use super::workflow_share::WorkflowOriginSummary;

/// 进程内运行时缓存（按 workspace 懒加载）。
static RUNTIME: std::sync::OnceLock<Arc<RwLock<Option<LanCollabRuntime>>>> =
    std::sync::OnceLock::new();

fn runtime_slot() -> Arc<RwLock<Option<LanCollabRuntime>>> {
    RUNTIME.get_or_init(|| Arc::new(RwLock::new(None))).clone()
}

async fn ensure_runtime(state: &AppState, app: &AppHandle) -> Result<LanCollabRuntime, String> {
    let slot = runtime_slot();
    {
        let guard = slot.read().await;
        if let Some(runtime) = guard.as_ref() {
            runtime.attach_app_handle(app.clone()).await;
            return Ok(runtime.clone());
        }
    }
    let mut guard = slot.write().await;
    if let Some(runtime) = guard.as_ref() {
        runtime.attach_app_handle(app.clone()).await;
        return Ok(runtime.clone());
    }
    let data_dir = state.workspace_config_dir.join("lan_collab");
    let runtime = LanCollabRuntime::open(
        data_dir,
        state.config_manager.clone(),
        state.workspace_config_dir.clone(),
    )?;
    runtime.attach_app_handle(app.clone()).await;
    *guard = Some(runtime.clone());
    Ok(runtime)
}

#[tauri::command]
pub async fn lan_collab_status(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<LanCollabStatus, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    Ok(runtime.status().await)
}

#[tauri::command]
pub async fn lan_collab_set_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<LanCollabStatus, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    runtime.set_enabled(enabled).await
}

#[tauri::command]
pub async fn lan_collab_set_display_name(
    app: AppHandle,
    state: State<'_, AppState>,
    display_name: String,
) -> Result<NodeIdentity, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    runtime.set_display_name(display_name).await
}

#[tauri::command]
pub async fn lan_collab_list_peers(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<NearbyPeer>, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    Ok(runtime.list_peers().await)
}

#[tauri::command]
pub async fn lan_collab_connect_peer(
    app: AppHandle,
    state: State<'_, AppState>,
    host: String,
    port: u16,
) -> Result<NearbyPeer, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    runtime.connect_peer(host, port).await
}

#[tauri::command]
pub async fn lan_collab_refresh_scan(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<LanCollabStatus, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    runtime.refresh_scan().await
}

#[tauri::command]
pub async fn lan_collab_create_group(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<CollabGroup, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    runtime.create_group(name).await
}

#[tauri::command]
pub async fn lan_collab_join_group(
    app: AppHandle,
    state: State<'_, AppState>,
    invite_code: String,
) -> Result<CollabGroup, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    runtime.join_group(invite_code).await
}

#[tauri::command]
pub async fn lan_collab_list_groups(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<CollabGroup>, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    Ok(runtime.list_groups().await)
}

#[tauri::command]
pub async fn lan_collab_send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    group_id: String,
    text: String,
) -> Result<ChatMessage, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    runtime.send_message(group_id, text).await
}

#[tauri::command]
pub async fn lan_collab_list_messages(
    app: AppHandle,
    state: State<'_, AppState>,
    group_id: String,
) -> Result<Vec<ChatMessage>, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    runtime.list_messages(group_id).await
}

#[tauri::command]
pub async fn lan_collab_share_model(
    app: AppHandle,
    state: State<'_, AppState>,
    model_id: String,
    display_name: String,
    provider_id: String,
    upstream_model: String,
    group_id: Option<String>,
    #[allow(non_snake_case)] upstream_base_url: Option<String>,
    #[allow(non_snake_case)] upstream_api_key: Option<String>,
) -> Result<SharedModelOffer, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    runtime
        .share_model(
            model_id,
            display_name,
            provider_id,
            upstream_model,
            group_id,
            upstream_base_url,
            upstream_api_key,
        )
        .await
}

#[tauri::command]
pub async fn lan_collab_unshare_model(
    app: AppHandle,
    state: State<'_, AppState>,
    share_id: String,
) -> Result<(), String> {
    let runtime = ensure_runtime(&state, &app).await?;
    runtime.unshare_model(share_id).await
}

#[tauri::command]
pub async fn lan_collab_list_local_shared_models(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<SharedModelOffer>, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    Ok(runtime.list_local_shared_models().await)
}

#[tauri::command]
pub async fn lan_collab_list_remote_shared_models(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<SharedModelOffer>, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    Ok(runtime.list_remote_shared_models().await)
}

#[tauri::command]
pub async fn lan_collab_share_knowledge(
    app: AppHandle,
    state: State<'_, AppState>,
    title: String,
    group_id: Option<String>,
    source_group: Option<String>,
    domain: Option<String>,
    doc_ids: Option<Vec<String>>,
) -> Result<SharedKnowledgeOffer, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    runtime
        .share_knowledge(
            title,
            group_id,
            source_group,
            domain,
            doc_ids.unwrap_or_default(),
        )
        .await
}

#[tauri::command]
pub async fn lan_collab_unshare_knowledge(
    app: AppHandle,
    state: State<'_, AppState>,
    share_id: String,
) -> Result<(), String> {
    let runtime = ensure_runtime(&state, &app).await?;
    runtime.unshare_knowledge(share_id).await
}

#[tauri::command]
pub async fn lan_collab_list_local_shared_knowledge(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<SharedKnowledgeOffer>, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    Ok(runtime.list_local_shared_knowledge().await)
}

#[tauri::command]
pub async fn lan_collab_list_remote_shared_knowledge(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<SharedKnowledgeOffer>, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    Ok(runtime.list_remote_shared_knowledge().await)
}

#[tauri::command]
pub async fn lan_collab_list_shareable_knowledge_docs(
    app: AppHandle,
    state: State<'_, AppState>,
    source_group: Option<String>,
    domain: Option<String>,
) -> Result<Vec<SharedKnowledgeDocMeta>, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    runtime
        .list_shareable_knowledge_docs(source_group, domain)
        .await
}

#[tauri::command]
pub async fn lan_collab_search_remote_knowledge(
    app: AppHandle,
    state: State<'_, AppState>,
    host_node_id: String,
    share_id: String,
    query: String,
    top_k: Option<usize>,
) -> Result<Vec<RemoteKnowledgeHit>, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    runtime
        .search_remote_knowledge(host_node_id, share_id, query, top_k)
        .await
}

#[tauri::command]
pub async fn lan_collab_fetch_remote_knowledge(
    app: AppHandle,
    state: State<'_, AppState>,
    host_node_id: String,
    share_id: String,
    doc_id: String,
) -> Result<RemoteKnowledgeDoc, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    runtime
        .fetch_remote_knowledge(host_node_id, share_id, doc_id)
        .await
}

#[tauri::command]
pub async fn lan_collab_share_skill(
    app: AppHandle,
    state: State<'_, AppState>,
    skill_id: String,
    group_id: Option<String>,
) -> Result<SharedSkillOffer, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    runtime.share_skill(skill_id, group_id).await
}

#[tauri::command]
pub async fn lan_collab_unshare_skill(
    app: AppHandle,
    state: State<'_, AppState>,
    share_id: String,
) -> Result<(), String> {
    let runtime = ensure_runtime(&state, &app).await?;
    runtime.unshare_skill(share_id).await
}

#[tauri::command]
pub async fn lan_collab_list_local_shared_skills(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<SharedSkillOffer>, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    Ok(runtime.list_local_shared_skills().await)
}

#[tauri::command]
pub async fn lan_collab_list_remote_shared_skills(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<SharedSkillOffer>, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    Ok(runtime.list_remote_shared_skills().await)
}

#[tauri::command]
pub async fn lan_collab_install_remote_skill(
    app: AppHandle,
    state: State<'_, AppState>,
    host_node_id: String,
    share_id: String,
    overwrite: Option<bool>,
    force_overwrite: Option<bool>,
) -> Result<String, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    runtime
        .install_remote_skill(host_node_id, share_id, overwrite, force_overwrite)
        .await
}

#[tauri::command]
pub async fn lan_collab_share_workflow(
    app: AppHandle,
    state: State<'_, AppState>,
    workflow_name: String,
    group_id: Option<String>,
) -> Result<SharedWorkflowOffer, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    runtime.share_workflow(workflow_name, group_id).await
}

#[tauri::command]
pub async fn lan_collab_unshare_workflow(
    app: AppHandle,
    state: State<'_, AppState>,
    share_id: String,
) -> Result<(), String> {
    let runtime = ensure_runtime(&state, &app).await?;
    runtime.unshare_workflow(share_id).await
}

#[tauri::command]
pub async fn lan_collab_list_local_shared_workflows(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<SharedWorkflowOffer>, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    Ok(runtime.list_local_shared_workflows().await)
}

#[tauri::command]
pub async fn lan_collab_list_remote_shared_workflows(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<SharedWorkflowOffer>, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    Ok(runtime.list_remote_shared_workflows().await)
}

#[tauri::command]
pub async fn lan_collab_install_remote_workflow(
    app: AppHandle,
    state: State<'_, AppState>,
    host_node_id: String,
    share_id: String,
    overwrite: Option<bool>,
    force_overwrite: Option<bool>,
    install_as: Option<String>,
) -> Result<String, String> {
    let runtime = ensure_runtime(&state, &app).await?;
    runtime
        .install_remote_workflow(
            host_node_id,
            share_id,
            overwrite,
            force_overwrite,
            install_as,
        )
        .await
}

#[tauri::command]
pub async fn lan_collab_list_workflow_share_origins(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<WorkflowOriginSummary>, String> {
    let _ = ensure_runtime(&state, &app).await?;
    // 直接从 workspace 扫描 origin，不依赖协作开关
    let service =
        super::workflow_share::WorkflowShareService::new(state.workspace_config_dir.clone());
    Ok(service.list_local_origin_summaries())
}
