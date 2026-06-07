use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelProviderInfo {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub env_key: Option<String>,
    #[serde(default)]
    pub experimental_bearer_token: Option<String>,
    #[serde(default)]
    pub wire_api: Option<String>,
    #[serde(default)]
    pub requires_openai_auth: Option<bool>,
    #[serde(default)]
    pub query_params: Option<HashMap<String, String>>,
    #[serde(default)]
    pub http_headers: Option<HashMap<String, String>>,
}

impl ModelProviderInfo {
    pub fn resolve_api_key(&self) -> Option<String> {
        if let Some(ref token) = self.experimental_bearer_token {
            if !token.is_empty() {
                return Some(token.clone());
            }
        }

        if let Some(ref env_key) = self.env_key {
            if let Ok(val) = std::env::var(env_key) {
                if !val.is_empty() {
                    return Some(val);
                }
            }
        }

        if let Ok(val) = std::env::var("OPENAI_API_KEY") {
            if !val.is_empty() {
                return Some(val);
            }
        }

        None
    }

    pub fn resolve_base_url(&self) -> Option<String> {
        self.base_url.as_ref().filter(|u| !u.is_empty()).cloned()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConfigToml {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub model_provider: Option<String>,
    #[serde(default)]
    pub model_reasoning_effort: Option<String>,
    #[serde(default)]
    pub model_context_window: Option<i64>,
    #[serde(default)]
    pub approval_policy: Option<String>,
    #[serde(default)]
    pub web_search: Option<String>,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default)]
    pub sandbox: Option<String>,
    #[serde(default)]
    pub model_providers: HashMap<String, ModelProviderInfo>,
    #[serde(default)]
    pub mcp_servers: HashMap<String, toml::Value>,
}

fn builtin_providers() -> HashMap<String, ModelProviderInfo> {
    let mut map = HashMap::new();
    map.insert(
        "openai".to_string(),
        ModelProviderInfo {
            name: Some("OpenAI".to_string()),
            base_url: Some("https://api.openai.com/v1".to_string()),
            env_key: Some("OPENAI_API_KEY".to_string()),
            wire_api: Some("responses".to_string()),
            requires_openai_auth: Some(true),
            ..Default::default()
        },
    );
    map.insert(
        "anthropic".to_string(),
        ModelProviderInfo {
            name: Some("Anthropic".to_string()),
            base_url: Some("https://api.anthropic.com/v1".to_string()),
            env_key: Some("ANTHROPIC_API_KEY".to_string()),
            wire_api: Some("chat".to_string()),
            requires_openai_auth: Some(false),
            ..Default::default()
        },
    );
    map.insert(
        "google".to_string(),
        ModelProviderInfo {
            name: Some("Google Gemini".to_string()),
            base_url: Some("https://generativelanguage.googleapis.com/v1beta/openai".to_string()),
            env_key: Some("GEMINI_API_KEY".to_string()),
            wire_api: Some("chat".to_string()),
            requires_openai_auth: Some(false),
            ..Default::default()
        },
    );
    map.insert(
        "deepseek".to_string(),
        ModelProviderInfo {
            name: Some("DeepSeek".to_string()),
            base_url: Some("https://api.deepseek.com/v1".to_string()),
            env_key: Some("DEEPSEEK_API_KEY".to_string()),
            wire_api: Some("chat".to_string()),
            requires_openai_auth: Some(false),
            ..Default::default()
        },
    );
    map.insert(
        "volcengine".to_string(),
        ModelProviderInfo {
            name: Some("Volcengine Ark".to_string()),
            base_url: Some("https://ark.cn-beijing.volces.com/api/v3".to_string()),
            env_key: Some("ARK_API_KEY".to_string()),
            wire_api: Some("chat".to_string()),
            requires_openai_auth: Some(false),
            ..Default::default()
        },
    );
    map.insert(
        "qwen".to_string(),
        ModelProviderInfo {
            name: Some("Qwen (Tongyi)".to_string()),
            base_url: Some("https://dashscope.aliyuncs.com/compatible-mode/v1".to_string()),
            env_key: Some("DASHSCOPE_API_KEY".to_string()),
            wire_api: Some("chat".to_string()),
            requires_openai_auth: Some(false),
            ..Default::default()
        },
    );
    map.insert(
        "zhipu".to_string(),
        ModelProviderInfo {
            name: Some("Zhipu AI".to_string()),
            base_url: Some("https://open.bigmodel.cn/api/paas/v4".to_string()),
            env_key: Some("ZHIPU_API_KEY".to_string()),
            wire_api: Some("chat".to_string()),
            requires_openai_auth: Some(false),
            ..Default::default()
        },
    );
    map.insert(
        "moonshot".to_string(),
        ModelProviderInfo {
            name: Some("Moonshot AI".to_string()),
            base_url: Some("https://api.moonshot.cn/v1".to_string()),
            env_key: Some("MOONSHOT_API_KEY".to_string()),
            wire_api: Some("chat".to_string()),
            requires_openai_auth: Some(false),
            ..Default::default()
        },
    );
    map.insert(
        "siliconflow".to_string(),
        ModelProviderInfo {
            name: Some("SiliconFlow".to_string()),
            base_url: Some("https://api.siliconflow.cn/v1".to_string()),
            env_key: Some("SILICONFLOW_API_KEY".to_string()),
            wire_api: Some("chat".to_string()),
            requires_openai_auth: Some(false),
            ..Default::default()
        },
    );
    map.insert(
        "baichuan".to_string(),
        ModelProviderInfo {
            name: Some("Baichuan".to_string()),
            base_url: Some("https://api.baichuan-ai.com/v1".to_string()),
            env_key: Some("BAICHUAN_API_KEY".to_string()),
            wire_api: Some("chat".to_string()),
            requires_openai_auth: Some(false),
            ..Default::default()
        },
    );
    map.insert(
        "ollama".to_string(),
        ModelProviderInfo {
            name: Some("Ollama".to_string()),
            base_url: Some("http://localhost:11434/v1".to_string()),
            wire_api: Some("chat".to_string()),
            requires_openai_auth: Some(false),
            ..Default::default()
        },
    );
    map.insert(
        "lmstudio".to_string(),
        ModelProviderInfo {
            name: Some("LM Studio".to_string()),
            base_url: Some("http://localhost:1234/v1".to_string()),
            wire_api: Some("chat".to_string()),
            requires_openai_auth: Some(false),
            ..Default::default()
        },
    );
    map
}

const RESERVED_PROVIDER_IDS: &[&str] = &[
    "openai", "anthropic", "google", "deepseek", "volcengine",
    "qwen", "zhipu", "moonshot", "siliconflow", "baichuan",
    "ollama", "lmstudio", "amazon-bedrock",
];

impl ConfigToml {
    pub fn load(path: &Path) -> AppResult<Self> {
        if !path.exists() {
            info!("Config file not found at {}, using defaults", path.display());
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)
            .map_err(|e| AppError::Custom(format!("Failed to read config {}: {e}", path.display())))?;
        let config: Self = toml::from_str(&content)
            .map_err(|e| AppError::Custom(format!("Failed to parse config: {e}")))?;
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> AppResult<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::Custom(format!("Failed to create config dir: {e}")))?;
        }
        let content = toml::to_string_pretty(self)
            .map_err(|e| AppError::Custom(format!("Failed to serialize config: {e}")))?;
        std::fs::write(path, content)
            .map_err(|e| AppError::Custom(format!("Failed to write config: {e}")))?;
        info!("Config saved to {}", path.display());
        Ok(())
    }

    pub fn apply_edit(&mut self, key_path: &str, value: &serde_json::Value) {
        match key_path {
            "model" => self.model = value.as_str().map(String::from),
            "model_provider" => self.model_provider = value.as_str().map(String::from),
            "model_reasoning_effort" => {
                self.model_reasoning_effort = value.as_str().map(String::from);
            }
            "model_context_window" => self.model_context_window = value.as_i64(),
            "approval_policy" => self.approval_policy = value.as_str().map(String::from),
            "web_search" => self.web_search = value.as_str().map(String::from),
            "instructions" => self.instructions = value.as_str().map(String::from),
            "sandbox" => self.sandbox = value.as_str().map(String::from),
            other if other.starts_with("model_providers.") => {
                let provider_key = &other["model_providers.".len()..];
                if let Ok(info) = serde_json::from_value::<ModelProviderInfo>(value.clone()) {
                    self.model_providers.insert(provider_key.to_string(), info);
                }
            }
            _ => {}
        }
    }

    pub fn resolve_provider(&self) -> (String, ModelProviderInfo) {
        let provider_id = self
            .model_provider
            .as_deref()
            .unwrap_or("openai")
            .to_string();

        let builtins = builtin_providers();

        if let Some(user_provider) = self.model_providers.get(&provider_id) {
            return (provider_id, user_provider.clone());
        }

        if let Some(builtin) = builtins.get(&provider_id) {
            return (provider_id, builtin.clone());
        }

        (provider_id, ModelProviderInfo::default())
    }

    pub fn resolve_model(&self) -> String {
        self.model
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("gpt-4.1")
            .to_string()
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }

    pub fn is_reserved_provider(id: &str) -> bool {
        RESERVED_PROVIDER_IDS.contains(&id)
    }
}

pub struct ConfigManager {
    config_path: PathBuf,
}

impl ConfigManager {
    pub fn new(config_path: PathBuf) -> Self {
        Self { config_path }
    }

    pub fn read(&self) -> AppResult<ConfigToml> {
        ConfigToml::load(&self.config_path)
    }

    pub fn write(&self, edits: &[(String, serde_json::Value)]) -> AppResult<ConfigToml> {
        let mut config = self.read()?;
        for (key, value) in edits {
            config.apply_edit(key, value);
        }
        config.save(&self.config_path)?;
        Ok(config)
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }
}
