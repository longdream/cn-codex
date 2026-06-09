//! 用量追踪 Tauri commands

use tauri::State;

use crate::error::AppResult;
use crate::state::AppState;
use crate::usage::db::{DailyUsage, ModelUsage, UsageRecord, UsageStats};

/// 获取全局汇总统计
/// since_timestamp: 可选，Unix 秒，只统计此时间之后的数据
#[tauri::command]
pub async fn usage_get_stats(
    state: State<'_, AppState>,
    since_timestamp: Option<i64>,
) -> AppResult<UsageStats> {
    state.usage_db.get_stats(since_timestamp)
}

/// 获取按天分组的用量（最近 N 天）
#[tauri::command]
pub async fn usage_get_daily(
    state: State<'_, AppState>,
    days: Option<u32>,
) -> AppResult<Vec<DailyUsage>> {
    state.usage_db.get_daily_usage(days.unwrap_or(30))
}

/// 获取按模型分组的用量统计
#[tauri::command]
pub async fn usage_get_by_model(
    state: State<'_, AppState>,
    since_timestamp: Option<i64>,
) -> AppResult<Vec<ModelUsage>> {
    state.usage_db.get_model_usage(since_timestamp)
}

/// 获取最近 N 条用量记录
#[tauri::command]
pub async fn usage_get_recent(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> AppResult<Vec<UsageRecord>> {
    state.usage_db.get_recent_records(limit.unwrap_or(50))
}

/// 设置/更新模型价格
#[tauri::command]
pub async fn usage_set_pricing(
    state: State<'_, AppState>,
    model_pattern: String,
    prompt_price_per_1m: f64,
    completion_price_per_1m: f64,
) -> AppResult<()> {
    let mut pricing = state
        .pricing_table
        .write()
        .map_err(|e| crate::error::AppError::Custom(format!("Lock error: {e}")))?;
    pricing.set_price(model_pattern, prompt_price_per_1m, completion_price_per_1m);
    Ok(())
}

/// 获取所有定价信息
#[tauri::command]
pub async fn usage_get_pricing(state: State<'_, AppState>) -> AppResult<Vec<serde_json::Value>> {
    let pricing = state
        .pricing_table
        .read()
        .map_err(|e| crate::error::AppError::Custom(format!("Lock error: {e}")))?;
    let prices: Vec<serde_json::Value> = pricing
        .all_prices()
        .iter()
        .map(|p| {
            serde_json::json!({
                "modelPattern": p.model_pattern,
                "promptPricePer1m": p.prompt_price_per_1m,
                "completionPricePer1m": p.completion_price_per_1m,
            })
        })
        .collect();
    Ok(prices)
}
