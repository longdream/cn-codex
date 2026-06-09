use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TierLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmTierConfig {
    pub model: String,
    pub reasoning_effort: String,
    pub service_tier: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenBudget {
    pub daily_limit: u64,
    pub warning_threshold: f64,
    pub auto_downgrade: bool,
}

impl Default for TokenBudget {
    fn default() -> Self {
        Self {
            daily_limit: 0,
            warning_threshold: 0.8,
            auto_downgrade: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TierUsageStats {
    pub low_tokens: u64,
    pub medium_tokens: u64,
    pub high_tokens: u64,
    pub total_tokens: u64,
}

pub struct LlmTiersManager {
    pub tiers: HashMap<TierLevel, LlmTierConfig>,
    pub scenes: HashMap<String, TierLevel>,
    pub default_tier: TierLevel,
    pub budget: TokenBudget,
    pub daily_usage: std::sync::atomic::AtomicU64,
}

impl Default for LlmTiersManager {
    fn default() -> Self {
        let mut tiers = HashMap::new();
        tiers.insert(
            TierLevel::Low,
            LlmTierConfig {
                model: "gpt-4.1-mini".to_string(),
                reasoning_effort: "low".to_string(),
                service_tier: "default".to_string(),
                description: "快速响应，适合简单任务".to_string(),
            },
        );
        tiers.insert(
            TierLevel::Medium,
            LlmTierConfig {
                model: "gpt-4.1".to_string(),
                reasoning_effort: "medium".to_string(),
                service_tier: "default".to_string(),
                description: "日常开发，均衡性价比".to_string(),
            },
        );
        tiers.insert(
            TierLevel::High,
            LlmTierConfig {
                model: "o3".to_string(),
                reasoning_effort: "high".to_string(),
                service_tier: "priority".to_string(),
                description: "复杂推理，最高质量".to_string(),
            },
        );

        let mut scenes = HashMap::new();
        for s in ["chat", "explain", "translate"] {
            scenes.insert(s.to_string(), TierLevel::Low);
        }
        for s in ["code_edit", "bug_fix", "test_write", "refactor"] {
            scenes.insert(s.to_string(), TierLevel::Medium);
        }
        for s in ["architecture", "code_review", "complex_debug", "plan"] {
            scenes.insert(s.to_string(), TierLevel::High);
        }

        Self {
            tiers,
            scenes,
            default_tier: TierLevel::Medium,
            budget: TokenBudget::default(),
            daily_usage: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

impl LlmTiersManager {
    pub fn resolve_tier(&self, scene: Option<&str>) -> &LlmTierConfig {
        let level = scene
            .and_then(|s| self.scenes.get(s))
            .copied()
            .unwrap_or(self.default_tier);
        self.tiers.get(&level).unwrap()
    }

    pub fn get_usage_stats(&self) -> TierUsageStats {
        let total = self.daily_usage.load(std::sync::atomic::Ordering::Relaxed);
        TierUsageStats {
            low_tokens: 0,
            medium_tokens: 0,
            high_tokens: 0,
            total_tokens: total,
        }
    }

    pub fn record_usage(&self, tokens: u64) {
        self.daily_usage
            .fetch_add(tokens, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_level_serde_roundtrip() {
        let json = serde_json::to_string(&TierLevel::Low).unwrap();
        assert_eq!(json, "\"low\"");
        let parsed: TierLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, TierLevel::Low);
    }

    #[test]
    fn tier_config_serde_roundtrip() {
        let cfg = LlmTierConfig {
            model: "gpt-4.1".to_string(),
            reasoning_effort: "medium".to_string(),
            service_tier: "default".to_string(),
            description: "test".to_string(),
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: LlmTierConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.model, "gpt-4.1");
    }

    #[test]
    fn default_has_all_scenes() {
        let mgr = LlmTiersManager::default();
        assert_eq!(mgr.scenes.len(), 11);
    }

    #[test]
    fn concurrent_usage_recording() {
        let daily = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let threads: Vec<_> = (0..10)
            .map(|_| {
                let daily = std::sync::Arc::clone(&daily);
                std::thread::spawn(move || {
                    for _ in 0..100 {
                        daily.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(daily.load(std::sync::atomic::Ordering::Relaxed), 1000);
    }

    #[test]
    fn usage_stats_fields() {
        let stats = TierUsageStats {
            low_tokens: 10,
            medium_tokens: 20,
            high_tokens: 30,
            total_tokens: 60,
        };
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("\"totalTokens\":60"));
    }

    #[test]
    fn budget_serde() {
        let budget = TokenBudget::default();
        let json = serde_json::to_string(&budget).unwrap();
        let parsed: TokenBudget = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.daily_limit, 0);
        assert!(parsed.auto_downgrade);
    }
}
