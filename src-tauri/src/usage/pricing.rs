//! 模型定价表
//! 支持用户自定义价格，也提供内置默认价格

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// 单个模型的定价（每百万 token）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPricing {
    /// 模型匹配模式（可以是精确名称或 glob 通配符）
    pub model_pattern: String,
    /// 输入 token 价格（USD / 1M tokens）
    pub prompt_price_per_1m: f64,
    /// 输出 token 价格（USD / 1M tokens）
    pub completion_price_per_1m: f64,
}

/// 价格表管理器
pub struct PricingTable {
    /// 用户自定义价格（优先级高于默认）
    custom_prices: HashMap<String, ModelPricing>,
    /// 内置默认价格
    default_prices: Vec<ModelPricing>,
}

impl PricingTable {
    pub fn new() -> Self {
        Self {
            custom_prices: HashMap::new(),
            default_prices: builtin_prices(),
        }
    }

    /// 计算费用
    pub fn calculate_cost(&self, model: &str, prompt_tokens: u64, completion_tokens: u64) -> f64 {
        let pricing = self.find_pricing(model);
        let prompt_cost = (prompt_tokens as f64 / 1_000_000.0) * pricing.prompt_price_per_1m;
        let completion_cost =
            (completion_tokens as f64 / 1_000_000.0) * pricing.completion_price_per_1m;
        prompt_cost + completion_cost
    }

    /// 查找模型对应的定价
    fn find_pricing(&self, model: &str) -> &ModelPricing {
        // 优先匹配用户自定义
        if let Some(p) = self.custom_prices.get(model) {
            return p;
        }
        // 前缀匹配用户自定义
        for (pattern, pricing) in &self.custom_prices {
            if model.starts_with(pattern.trim_end_matches('*')) {
                return pricing;
            }
        }
        // 匹配内置默认
        for p in &self.default_prices {
            if model == p.model_pattern || model.starts_with(p.model_pattern.trim_end_matches('*'))
            {
                return p;
            }
        }
        // 未知模型返回零价格
        &ZERO_PRICING
    }

    /// 设置/更新自定义价格
    pub fn set_price(&mut self, model_pattern: String, prompt_per_1m: f64, completion_per_1m: f64) {
        self.custom_prices.insert(
            model_pattern.clone(),
            ModelPricing {
                model_pattern,
                prompt_price_per_1m: prompt_per_1m,
                completion_price_per_1m: completion_per_1m,
            },
        );
    }

    /// 获取所有定价（合并自定义和默认）
    pub fn all_prices(&self) -> Vec<&ModelPricing> {
        let mut result: Vec<&ModelPricing> = self.custom_prices.values().collect();
        for p in &self.default_prices {
            if !self.custom_prices.contains_key(&p.model_pattern) {
                result.push(p);
            }
        }
        result
    }
}

/// 零定价（未知模型兜底）
static ZERO_PRICING: ModelPricing = ModelPricing {
    model_pattern: String::new(),
    prompt_price_per_1m: 0.0,
    completion_price_per_1m: 0.0,
};

/// 内置常见模型定价（2025 年 Q2 价格基准）
fn builtin_prices() -> Vec<ModelPricing> {
    vec![
        // OpenAI
        ModelPricing {
            model_pattern: "gpt-4o".to_string(),
            prompt_price_per_1m: 2.5,
            completion_price_per_1m: 10.0,
        },
        ModelPricing {
            model_pattern: "gpt-4o-mini".to_string(),
            prompt_price_per_1m: 0.15,
            completion_price_per_1m: 0.6,
        },
        ModelPricing {
            model_pattern: "gpt-4.1".to_string(),
            prompt_price_per_1m: 2.0,
            completion_price_per_1m: 8.0,
        },
        ModelPricing {
            model_pattern: "gpt-4.1-mini".to_string(),
            prompt_price_per_1m: 0.4,
            completion_price_per_1m: 1.6,
        },
        ModelPricing {
            model_pattern: "o3-mini".to_string(),
            prompt_price_per_1m: 1.1,
            completion_price_per_1m: 4.4,
        },
        // Anthropic
        ModelPricing {
            model_pattern: "claude-sonnet-4*".to_string(),
            prompt_price_per_1m: 3.0,
            completion_price_per_1m: 15.0,
        },
        ModelPricing {
            model_pattern: "claude-opus-4*".to_string(),
            prompt_price_per_1m: 15.0,
            completion_price_per_1m: 75.0,
        },
        // DeepSeek
        ModelPricing {
            model_pattern: "deepseek-chat".to_string(),
            prompt_price_per_1m: 0.14,
            completion_price_per_1m: 0.28,
        },
        ModelPricing {
            model_pattern: "deepseek-reasoner".to_string(),
            prompt_price_per_1m: 0.55,
            completion_price_per_1m: 2.19,
        },
        // Google
        ModelPricing {
            model_pattern: "gemini-2.5-pro".to_string(),
            prompt_price_per_1m: 1.25,
            completion_price_per_1m: 10.0,
        },
        // 通义千问
        ModelPricing {
            model_pattern: "qwen-max".to_string(),
            prompt_price_per_1m: 2.4,
            completion_price_per_1m: 9.6,
        },
        // 智谱
        ModelPricing {
            model_pattern: "glm-4-plus".to_string(),
            prompt_price_per_1m: 5.0,
            completion_price_per_1m: 5.0,
        },
    ]
}
