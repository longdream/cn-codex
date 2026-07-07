//! 用量记录器：统一入口，将 adapter 返回的 UsageInfo 写入 SQLite

use std::sync::Arc;

use tracing::info;

use super::db::UsageDb;
use super::pricing::PricingTable;
use crate::adapter::types::UsageInfo;

/// 用量记录器，在 agent 完成一次 LLM 调用后调用
pub struct UsageRecorder {
    db: Arc<UsageDb>,
    pricing: Arc<std::sync::RwLock<PricingTable>>,
}

impl UsageRecorder {
    pub fn new(db: Arc<UsageDb>, pricing: Arc<std::sync::RwLock<PricingTable>>) -> Self {
        Self { db, pricing }
    }

    /// 记录一次 LLM 调用的用量
    pub fn record(&self, provider: &str, model: &str, thread_id: &str, usage: &UsageInfo) {
        let cost = {
            let pricing = self.pricing.read().unwrap_or_else(|e| e.into_inner());
            pricing.calculate_cost(model, usage.prompt_tokens, usage.completion_tokens)
        };

        match self.db.insert_record(
            provider,
            model,
            thread_id,
            usage.prompt_tokens,
            usage.completion_tokens,
            usage.total_tokens,
            usage.cached_tokens,
            usage.cache_creation_tokens,
            usage.reasoning_tokens,
            cost,
        ) {
            Ok(id) => {
                info!(
                    "Usage recorded (id={}): provider={}, model={}, tokens={}/{}/{}, cached={}, cache_creation={}, reasoning={}, cost=${:.6}",
                    id,
                    provider,
                    model,
                    usage.prompt_tokens,
                    usage.completion_tokens,
                    usage.total_tokens,
                    usage.cached_tokens,
                    usage.cache_creation_tokens,
                    usage.reasoning_tokens,
                    cost
                );
            }
            Err(e) => {
                tracing::error!("Failed to record usage: {e}");
            }
        }
    }
}
