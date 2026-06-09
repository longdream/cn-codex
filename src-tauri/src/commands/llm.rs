use tauri::State;

use crate::error::AppResult;
use crate::llm_tiers::{LlmTierConfig, TierLevel, TierUsageStats};
use crate::state::AppState;

#[tauri::command]
pub async fn llm_get_tiers(state: State<'_, AppState>) -> AppResult<serde_json::Value> {
    let mgr = state.llm_tiers.read().await;
    Ok(serde_json::json!({
        "tiers": mgr.tiers,
        "scenes": mgr.scenes,
        "budget": mgr.budget,
    }))
}

#[tauri::command]
pub async fn llm_set_tier(
    state: State<'_, AppState>,
    level: TierLevel,
    config: LlmTierConfig,
) -> AppResult<()> {
    let mut mgr = state.llm_tiers.write().await;
    mgr.tiers.insert(level, config);
    Ok(())
}

#[tauri::command]
pub async fn llm_resolve_tier(
    state: State<'_, AppState>,
    scene: String,
) -> AppResult<LlmTierConfig> {
    let mgr = state.llm_tiers.read().await;
    Ok(mgr.resolve_tier(Some(&scene)).clone())
}

#[tauri::command]
pub async fn llm_get_usage(state: State<'_, AppState>) -> AppResult<TierUsageStats> {
    let mgr = state.llm_tiers.read().await;
    Ok(mgr.get_usage_stats())
}

#[tauri::command]
pub async fn llm_record_usage(state: State<'_, AppState>, tokens: u64) -> AppResult<()> {
    let mgr = state.llm_tiers.read().await;
    mgr.record_usage(tokens);
    Ok(())
}

#[tauri::command]
pub async fn llm_bind_scene(
    state: State<'_, AppState>,
    scene: String,
    level: TierLevel,
) -> AppResult<()> {
    let mut mgr = state.llm_tiers.write().await;
    mgr.scenes.insert(scene, level);
    Ok(())
}
