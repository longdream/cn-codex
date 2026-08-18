use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartBrainConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub extraction_start_at: Option<i64>,
    // Experience sub-settings
    // 经验相关开关默认关闭，避免无意义 token 消耗；用户可在设置中手动开启。
    #[serde(default)]
    pub auto_extract: bool,
    #[serde(default)]
    pub auto_consolidate: bool,
    #[serde(default)]
    pub inject_summary: bool,
    #[serde(default = "default_max_raw_experiences")]
    pub max_raw_experiences: usize,
    #[serde(default = "default_max_consolidation_entries")]
    pub max_consolidation_entries: usize,
    #[serde(default)]
    pub auto_summarize_enabled: bool,
    #[serde(default = "default_auto_summarize_threshold")]
    pub auto_summarize_threshold: usize,
    #[serde(default = "default_max_unused_days")]
    pub max_unused_days: i64,
    #[serde(default = "default_max_rollouts_per_startup")]
    pub max_rollouts_per_startup: usize,
    #[serde(default = "default_min_session_messages")]
    pub min_session_messages: usize,
    #[serde(default = "default_summary_max_tokens")]
    pub summary_max_tokens: usize,
    // Knowledge sub-settings
    #[serde(default = "default_true")]
    pub knowledge_enabled: bool,
    #[serde(default = "default_max_knowledge_docs")]
    pub max_knowledge_docs: usize,
    #[serde(default = "default_max_chunk_tokens")]
    pub max_chunk_tokens: usize,
    #[serde(default = "default_true")]
    pub auto_organize: bool,
    #[serde(default = "default_true")]
    pub knowledge_chunk_files_enabled: bool,
    #[serde(default = "default_true")]
    pub search_okf_prefilter_enabled: bool,
    #[serde(default = "default_search_okf_prefilter_order")]
    pub search_okf_prefilter_order: Vec<String>,
    #[serde(default = "default_search_locator_priority")]
    pub search_locator_priority: Vec<String>,
}

fn default_true() -> bool {
    true
}
fn default_max_raw_experiences() -> usize {
    100
}
fn default_max_consolidation_entries() -> usize {
    50
}
fn default_auto_summarize_threshold() -> usize {
    10
}
fn default_max_unused_days() -> i64 {
    30
}
fn default_max_rollouts_per_startup() -> usize {
    5
}
fn default_min_session_messages() -> usize {
    3
}
fn default_summary_max_tokens() -> usize {
    2000
}
fn default_max_knowledge_docs() -> usize {
    200
}
fn default_max_chunk_tokens() -> usize {
    500
}
fn default_search_okf_prefilter_order() -> Vec<String> {
    vec![
        "domain".to_string(),
        "tags".to_string(),
        "source_type".to_string(),
    ]
}
fn default_search_locator_priority() -> Vec<String> {
    vec![
        "domain".to_string(),
        "source_group".to_string(),
        "relative_path".to_string(),
    ]
}

impl Default for SmartBrainConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            extraction_start_at: None,
            auto_extract: false,
            auto_consolidate: false,
            inject_summary: false,
            max_raw_experiences: default_max_raw_experiences(),
            max_consolidation_entries: default_max_consolidation_entries(),
            auto_summarize_enabled: false,
            auto_summarize_threshold: default_auto_summarize_threshold(),
            max_unused_days: default_max_unused_days(),
            max_rollouts_per_startup: default_max_rollouts_per_startup(),
            min_session_messages: default_min_session_messages(),
            summary_max_tokens: default_summary_max_tokens(),
            knowledge_enabled: default_true(),
            max_knowledge_docs: default_max_knowledge_docs(),
            max_chunk_tokens: default_max_chunk_tokens(),
            auto_organize: default_true(),
            knowledge_chunk_files_enabled: default_true(),
            search_okf_prefilter_enabled: default_true(),
            search_okf_prefilter_order: default_search_okf_prefilter_order(),
            search_locator_priority: default_search_locator_priority(),
        }
    }
}

impl SmartBrainConfig {
    pub fn is_active(&self) -> bool {
        self.enabled
    }

    pub fn knowledge_is_active(&self) -> bool {
        self.enabled && self.knowledge_enabled
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ImageGenerationConfig {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
}

impl ImageGenerationConfig {
    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }
}

/// 资源池模型的单个后端端点
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEndpointInfo {
    pub url: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub wire_api: Option<String>,
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
    pub max_output_tokens: Option<i64>,
    #[serde(default)]
    pub model_supports_vision: Option<bool>,
    #[serde(default)]
    pub vision_fallback_kind: Option<String>,
    #[serde(default)]
    pub vision_fallback_provider: Option<String>,
    #[serde(default)]
    pub vision_fallback_model: Option<String>,
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
    #[serde(default)]
    pub model_auto_compact_token_limit: Option<i64>,
    #[serde(default)]
    pub hooks: HashMap<String, toml::Value>,
    #[serde(default)]
    pub relay_server_url: Option<String>,
    #[serde(default, alias = "experience")]
    pub smartbrain: Option<SmartBrainConfig>,
    #[serde(default)]
    pub image_generation: Option<ImageGenerationConfig>,
    /// 对话级子智能体开关（仅内存覆盖，不写回全局 config.toml）。
    #[serde(default)]
    pub subagent_enabled: Option<bool>,
    /// 当前选中的 local-pool 模型的端点列表（仅 local-pool 类型供应商使用）
    #[serde(default)]
    pub model_endpoints: Vec<ModelEndpointInfo>,
    /// 当前正在使用的端点索引（持久化以便重启后恢复）
    #[serde(default)]
    pub active_endpoint_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub cwd: Option<String>,
    pub url: Option<String>,
    pub headers: HashMap<String, String>,
    pub disabled: bool,
}

impl McpServerConfig {
    pub fn is_http_transport(&self) -> bool {
        self.transport == "http"
    }

    pub fn is_sse_transport(&self) -> bool {
        self.transport == "sse"
    }

    pub fn is_remote_transport(&self) -> bool {
        self.is_http_transport() || self.is_sse_transport()
    }
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
        "rightcode".to_string(),
        ModelProviderInfo {
            name: Some("RightCode".to_string()),
            base_url: Some("https://right.codes/codex/v1".to_string()),
            env_key: Some("RIGHTCODE_API_KEY".to_string()),
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
    map.insert(
        "codebuddy".to_string(),
        ModelProviderInfo {
            name: Some("CodeBuddy".to_string()),
            base_url: Some("https://copilot.tencent.com/v2".to_string()),
            env_key: Some("CODEBUDDY_API_KEY".to_string()),
            wire_api: Some("chat".to_string()),
            requires_openai_auth: Some(false),
            ..Default::default()
        },
    );
    map
}

const RESERVED_PROVIDER_IDS: &[&str] = &[
    "openai",
    "anthropic",
    "google",
    "deepseek",
    "volcengine",
    "qwen",
    "zhipu",
    "moonshot",
    "siliconflow",
    "rightcode",
    "baichuan",
    "ollama",
    "lmstudio",
    "codebuddy",
    "amazon-bedrock",
];

/// 统一整理 relay 地址：
/// - 去除首尾空白
/// - 去除末尾 `/`
/// 这样在拼接 `/m/{room}` 与 `/pc/{room}` 时不会出现双斜杠。
fn normalize_relay_server_url(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed.trim_end_matches('/');
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn parse_json_string_list(value: &serde_json::Value) -> Option<Vec<String>> {
    let array = value.as_array()?;
    let values = array
        .iter()
        .filter_map(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    Some(values)
}

impl ConfigToml {
    pub fn load(path: &Path) -> AppResult<Self> {
        if !path.exists() {
            info!(
                "Config file not found at {}, using defaults",
                path.display()
            );
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path).map_err(|e| {
            AppError::Custom(format!("Failed to read config {}: {e}", path.display()))
        })?;
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

    pub fn apply_edit(&mut self, key_path: &str, value: &serde_json::Value) -> AppResult<()> {
        match key_path {
            "model" => self.model = value.as_str().map(String::from),
            "model_provider" => self.model_provider = value.as_str().map(String::from),
            "model_reasoning_effort" => {
                self.model_reasoning_effort = value.as_str().map(String::from);
            }
            "model_context_window" => self.model_context_window = value.as_i64(),
            "max_output_tokens" => self.max_output_tokens = value.as_i64(),
            "model_supports_vision" => self.model_supports_vision = value.as_bool(),
            "vision_fallback_kind" => {
                self.vision_fallback_kind = value.as_str().map(String::from);
            }
            "vision_fallback_provider" => {
                self.vision_fallback_provider = value.as_str().map(String::from);
            }
            "vision_fallback_model" => {
                self.vision_fallback_model = value.as_str().map(String::from);
            }
            "approval_policy" => self.approval_policy = value.as_str().map(String::from),
            "web_search" => self.web_search = value.as_str().map(String::from),
            "instructions" => self.instructions = value.as_str().map(String::from),
            "sandbox" => self.sandbox = value.as_str().map(String::from),
            "model_auto_compact_token_limit" => {
                self.model_auto_compact_token_limit = value.as_i64();
            }
            "relay_server_url" => {
                self.relay_server_url = value.as_str().and_then(normalize_relay_server_url);
            }
            "smartbrain.enabled" => {
                let enabled = value.as_bool().unwrap_or(false);
                let sb = self
                    .smartbrain
                    .get_or_insert_with(SmartBrainConfig::default);
                sb.enabled = enabled;
            }
            "smartbrain.extraction_start_at" => {
                let sb = self
                    .smartbrain
                    .get_or_insert_with(SmartBrainConfig::default);
                sb.extraction_start_at = value.as_i64();
            }
            "smartbrain.auto_extract" => {
                let sb = self
                    .smartbrain
                    .get_or_insert_with(SmartBrainConfig::default);
                sb.auto_extract = value.as_bool().unwrap_or(false);
            }
            "smartbrain.auto_summarize_enabled" => {
                let sb = self
                    .smartbrain
                    .get_or_insert_with(SmartBrainConfig::default);
                sb.auto_summarize_enabled = value.as_bool().unwrap_or(false);
            }
            "smartbrain.auto_summarize_threshold" => {
                let sb = self
                    .smartbrain
                    .get_or_insert_with(SmartBrainConfig::default);
                sb.auto_summarize_threshold = value
                    .as_u64()
                    .map(|v| v.max(2) as usize)
                    .unwrap_or(default_auto_summarize_threshold());
            }
            "smartbrain.search_okf_prefilter_enabled" => {
                let sb = self
                    .smartbrain
                    .get_or_insert_with(SmartBrainConfig::default);
                sb.search_okf_prefilter_enabled = value.as_bool().unwrap_or(true);
            }
            "smartbrain.search_okf_prefilter_order" => {
                let sb = self
                    .smartbrain
                    .get_or_insert_with(SmartBrainConfig::default);
                sb.search_okf_prefilter_order = if value.is_null() {
                    default_search_okf_prefilter_order()
                } else {
                    parse_json_string_list(value)
                        .filter(|values| !values.is_empty())
                        .unwrap_or_else(default_search_okf_prefilter_order)
                };
            }
            "smartbrain.search_locator_priority" => {
                let sb = self
                    .smartbrain
                    .get_or_insert_with(SmartBrainConfig::default);
                sb.search_locator_priority = if value.is_null() {
                    default_search_locator_priority()
                } else {
                    parse_json_string_list(value)
                        .filter(|values| !values.is_empty())
                        .unwrap_or_else(default_search_locator_priority)
                };
            }
            "smartbrain.knowledge_chunk_files_enabled" => {
                let sb = self
                    .smartbrain
                    .get_or_insert_with(SmartBrainConfig::default);
                sb.knowledge_chunk_files_enabled = value.as_bool().unwrap_or(true);
            }
            "image_generation" => {
                if value.is_null() {
                    self.image_generation = None;
                } else if let Ok(mut image_generation) =
                    serde_json::from_value::<ImageGenerationConfig>(value.clone())
                {
                    image_generation.model = normalize_optional_string(image_generation.model);
                    image_generation.base_url =
                        normalize_optional_string(image_generation.base_url);
                    image_generation.api_key = normalize_optional_string(image_generation.api_key);
                    self.image_generation = Some(image_generation);
                }
            }
            "image_generation.model" => {
                let image_generation = self
                    .image_generation
                    .get_or_insert_with(ImageGenerationConfig::default);
                image_generation.model =
                    normalize_optional_string(value.as_str().map(ToString::to_string));
            }
            "image_generation.base_url" => {
                let image_generation = self
                    .image_generation
                    .get_or_insert_with(ImageGenerationConfig::default);
                image_generation.base_url =
                    normalize_optional_string(value.as_str().map(ToString::to_string));
            }
            "image_generation.enabled" => {
                let image_generation = self
                    .image_generation
                    .get_or_insert_with(ImageGenerationConfig::default);
                image_generation.enabled = if value.is_null() {
                    None
                } else {
                    value.as_bool()
                };
            }
            "image_generation.api_key" => {
                let image_generation = self
                    .image_generation
                    .get_or_insert_with(ImageGenerationConfig::default);
                image_generation.api_key =
                    normalize_optional_string(value.as_str().map(ToString::to_string));
            }
            other if other.starts_with("model_providers.") => {
                let provider_key = &other["model_providers.".len()..];
                if let Ok(info) = serde_json::from_value::<ModelProviderInfo>(value.clone()) {
                    self.model_providers.insert(provider_key.to_string(), info);
                }
            }
            other if other.starts_with("mcp_servers.") => {
                self.apply_mcp_server_edit(other, value)?;
            }
            "model_endpoints" => {
                if value.is_null()
                    || value.is_array() && value.as_array().is_some_and(|a| a.is_empty())
                {
                    self.model_endpoints.clear();
                } else if let Ok(endpoints) =
                    serde_json::from_value::<Vec<ModelEndpointInfo>>(value.clone())
                {
                    self.model_endpoints = endpoints;
                }
            }
            "active_endpoint_index" => {
                self.active_endpoint_index = value.as_u64().map(|v| v as usize);
            }
            _ => {}
        }
        Ok(())
    }

    fn apply_mcp_server_edit(
        &mut self,
        key_path: &str,
        value: &serde_json::Value,
    ) -> AppResult<()> {
        let server_name = key_path["mcp_servers.".len()..].trim();
        if server_name.is_empty() {
            return Err(AppError::Custom(format!(
                "Invalid MCP key path '{key_path}'"
            )));
        }

        if value.is_null() {
            self.mcp_servers.remove(server_name);
            return Ok(());
        }

        if !value.is_object() {
            return Err(AppError::Custom(format!(
                "MCP server '{server_name}' config must be an object"
            )));
        }

        let table: toml::Table = serde_json::from_value(value.clone()).map_err(|error| {
            AppError::Custom(format!(
                "Invalid MCP server '{server_name}' config: {error}"
            ))
        })?;

        let has_command = table
            .get("command")
            .and_then(toml::Value::as_str)
            .is_some_and(|command| !command.trim().is_empty());
        let has_url = table
            .get("url")
            .or_else(|| table.get("server_url"))
            .and_then(toml::Value::as_str)
            .is_some_and(|url| !url.trim().is_empty());
        if !has_command && !has_url {
            return Err(AppError::Custom(format!(
                "Invalid MCP server '{server_name}' config: expected non-empty 'command' or 'url'"
            )));
        }

        let toml_value = toml::Value::Table(table);
        if parse_mcp_server(server_name, &toml_value).is_none() {
            return Err(AppError::Custom(format!(
                "Invalid MCP server '{server_name}' config: expected a valid stdio command or HTTP/SSE url"
            )));
        }

        self.mcp_servers.insert(server_name.to_string(), toml_value);
        Ok(())
    }

    pub fn resolve_provider(&self) -> (String, ModelProviderInfo) {
        let provider_id = self
            .model_provider
            .as_deref()
            .unwrap_or("openai")
            .to_string();
        let provider = self.resolve_provider_by_id(&provider_id);
        (provider_id, provider)
    }

    pub fn resolve_provider_by_id(&self, provider_id: &str) -> ModelProviderInfo {
        if let Some(user_provider) = self.model_providers.get(provider_id) {
            return user_provider.clone();
        }

        let builtins = builtin_providers();
        if let Some(builtin) = builtins.get(provider_id) {
            return builtin.clone();
        }

        ModelProviderInfo::default()
    }

    /// 解析当前配置的模型名称
    /// 如果 model 字段为空，返回空字符串（调用方负责报错）
    pub fn resolve_model(&self) -> String {
        self.model
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("")
            .to_string()
    }

    pub fn smartbrain_config(&self) -> SmartBrainConfig {
        self.smartbrain.clone().unwrap_or_default()
    }

    pub fn image_generation_config(&self) -> ImageGenerationConfig {
        self.image_generation.clone().unwrap_or_default()
    }

    pub fn web_search_enabled(&self) -> bool {
        matches!(
            self.web_search
                .as_deref()
                .map(str::trim)
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("live" | "cached" | "enabled" | "true" | "on" | "1")
        )
    }

    /// 对话级子智能体开关；默认关闭，仅在对话框显式开启时暴露 spawn_agent 工具族。
    pub fn subagent_enabled(&self) -> bool {
        self.subagent_enabled.unwrap_or(false)
    }

    pub fn resolved_mcp_servers(&self) -> HashMap<String, McpServerConfig> {
        self.mcp_servers
            .iter()
            .filter_map(|(name, value)| parse_mcp_server(name, value))
            .collect()
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }

    pub fn is_reserved_provider(id: &str) -> bool {
        RESERVED_PROVIDER_IDS.contains(&id)
    }
}

fn parse_mcp_server(name: &str, value: &toml::Value) -> Option<(String, McpServerConfig)> {
    let table = value.as_table()?;
    let url = table
        .get("url")
        .or_else(|| table.get("server_url"))
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(ToString::to_string);
    let transport = normalize_mcp_transport(
        table
            .get("type")
            .or_else(|| table.get("transport"))
            .and_then(toml::Value::as_str),
        url.as_deref(),
    );
    let command = table
        .get("command")
        .and_then(toml::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if transport == "stdio" && command.is_empty() {
        return None;
    }
    if transport == "http" || transport == "sse" {
        if !is_supported_mcp_http_url(url.as_deref()) {
            return None;
        }
    }

    let args = table
        .get("args")
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(toml::Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();

    let env = table
        .get("env")
        .and_then(toml::Value::as_table)
        .map(|items| {
            items
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|val| (key.to_string(), val.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    let headers = table
        .get("headers")
        .or_else(|| table.get("http_headers"))
        .and_then(toml::Value::as_table)
        .map(|items| {
            items
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|val| (key.to_string(), val.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();

    let cwd = table
        .get("cwd")
        .and_then(toml::Value::as_str)
        .map(ToString::to_string);
    let disabled = table
        .get("disabled")
        .and_then(toml::Value::as_bool)
        .or_else(|| {
            table
                .get("isActive")
                .or_else(|| table.get("is_active"))
                .or_else(|| table.get("enabled"))
                .and_then(toml::Value::as_bool)
                .map(|active| !active)
        })
        .unwrap_or(false);

    Some((
        name.to_string(),
        McpServerConfig {
            name: name.to_string(),
            transport,
            command,
            args,
            env,
            cwd,
            url,
            headers,
            disabled,
        },
    ))
}

pub fn normalize_mcp_transport(raw: Option<&str>, url: Option<&str>) -> String {
    let normalized = raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase().replace('-', "_"));
    match normalized.as_deref() {
        Some("http" | "streamable_http" | "streamablehttp") => "http".to_string(),
        Some("sse") => "sse".to_string(),
        Some("stdio" | "local") => "stdio".to_string(),
        Some(_) => "stdio".to_string(),
        None if url.is_some_and(url_looks_like_mcp_sse) => "sse".to_string(),
        None if url.is_some_and(|value| !value.trim().is_empty()) => "http".to_string(),
        None => "stdio".to_string(),
    }
}

pub fn is_supported_mcp_http_url(url: Option<&str>) -> bool {
    url.is_some_and(|value| {
        let trimmed = value.trim().to_ascii_lowercase();
        trimmed.starts_with("http://") || trimmed.starts_with("https://")
    })
}

pub fn url_looks_like_mcp_sse(url: &str) -> bool {
    let trimmed = url.trim().to_ascii_lowercase();
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return false;
    }
    let path = trimmed
        .split_once('?')
        .map(|(path, _)| path)
        .unwrap_or(trimmed.as_str());
    path.ends_with("/sse") || path.contains("/sse/")
}

#[derive(Clone)]
pub struct ConfigManager {
    config_path: PathBuf,
    access_lock: Arc<Mutex<()>>,
}

impl ConfigManager {
    pub fn new(config_path: PathBuf) -> Self {
        Self {
            config_path,
            access_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn read(&self) -> AppResult<ConfigToml> {
        let _guard = self
            .access_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        ConfigToml::load(&self.config_path)
    }

    pub fn write(&self, edits: &[(String, serde_json::Value)]) -> AppResult<ConfigToml> {
        let _guard = self
            .access_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut config = ConfigToml::load(&self.config_path)?;
        for (key, value) in edits {
            config.apply_edit(key, value)?;
        }
        config.save(&self.config_path)?;
        Ok(config)
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{self, RecvTimeoutError};
    use std::time::Duration;

    #[test]
    fn config_manager_clones_serialize_writes() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let manager = ConfigManager::new(temp_dir.path().join("config.toml"));
        let clone = manager.clone();
        assert!(Arc::ptr_eq(&manager.access_lock, &clone.access_lock));

        let held_guard = manager
            .access_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (started_tx, started_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            started_tx.send(()).expect("signal writer start");
            let result = clone.write(&[(
                "model".to_string(),
                serde_json::json!("new-model"),
            )]);
            finished_tx.send(result).expect("signal writer finish");
        });

        started_rx.recv().expect("writer started");
        assert!(matches!(
            finished_rx.recv_timeout(Duration::from_millis(50)),
            Err(RecvTimeoutError::Timeout)
        ));
        drop(held_guard);

        finished_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("writer should finish after lock release")
            .expect("write succeeds");
        writer.join().expect("writer thread");
        assert_eq!(
            manager.read().expect("read config").model.as_deref(),
            Some("new-model")
        );
    }

    #[test]
    fn web_search_enabled_requires_enabled_mode() {
        let mut config = ConfigToml::default();
        assert!(!config.web_search_enabled());

        for value in ["live", "cached", "enabled", "true", "on", "1"] {
            config.web_search = Some(value.to_string());
            assert!(
                config.web_search_enabled(),
                "{value} should enable web search"
            );
        }

        for value in ["disabled", "false", "off", "0", ""] {
            config.web_search = Some(value.to_string());
            assert!(
                !config.web_search_enabled(),
                "{value} should disable web search"
            );
        }
    }

    #[test]
    fn resolved_mcp_servers_parses_supported_fields() {
        let config: ConfigToml = toml::from_str(
            r#"
            [mcp_servers.docs]
            command = "node"
            args = ["server.js", "--stdio"]
            cwd = "tools/docs"
            disabled = false
            env = { TOKEN = "abc" }

            [mcp_servers.remote]
            type = "streamable-http"
            url = "https://example.com/mcp"
            headers = { Authorization = "Bearer token" }

            [mcp_servers.remote_sse]
            type = "sse"
            url = "http://10.136.128.2:30092/sse?id=cfb6552f-4d65-4192-8ebf-c5f58d386457"
            isActive = true

            [mcp_servers.empty]
            args = ["missing-command"]
            "#,
        )
        .unwrap();

        let servers = config.resolved_mcp_servers();
        assert_eq!(servers.len(), 3);
        let docs = servers.get("docs").unwrap();
        assert_eq!(docs.transport, "stdio");
        assert_eq!(docs.command, "node");
        assert_eq!(docs.args, vec!["server.js", "--stdio"]);
        assert_eq!(docs.cwd.as_deref(), Some("tools/docs"));
        assert_eq!(docs.env.get("TOKEN").map(String::as_str), Some("abc"));
        assert!(!docs.disabled);
        let remote = servers.get("remote").unwrap();
        assert_eq!(remote.transport, "http");
        assert_eq!(remote.url.as_deref(), Some("https://example.com/mcp"));
        assert_eq!(
            remote.headers.get("Authorization").map(String::as_str),
            Some("Bearer token")
        );
        let remote_sse = servers.get("remote_sse").unwrap();
        assert_eq!(remote_sse.transport, "sse");
        assert_eq!(
            remote_sse.url.as_deref(),
            Some("http://10.136.128.2:30092/sse?id=cfb6552f-4d65-4192-8ebf-c5f58d386457")
        );
        assert!(!remote_sse.disabled);
    }

    #[test]
    fn model_endpoints_parse_from_toml() {
        let config: ConfigToml = toml::from_str(
            r#"
            model = "deepseek-chat"
            model_provider = "local-pool"

            [model_providers.local-pool]
            wire_api = "chat"

            [[model_endpoints]]
            url = "http://10.0.0.1:8080/v1"
            label = "节点1"
            model = "qwen-router-a"
            api_key = "sk-aaa"
            wire_api = "chat"

            [[model_endpoints]]
            url = "http://10.0.0.2:8080/v1"
            label = "节点2"
            model = "qwen-router-b"
            api_key = "sk-bbb"
            wire_api = "responses"
            "#,
        )
        .unwrap();

        assert_eq!(config.model_endpoints.len(), 2);
        assert_eq!(config.model_endpoints[0].url, "http://10.0.0.1:8080/v1");
        assert_eq!(config.model_endpoints[0].label.as_deref(), Some("节点1"));
        assert_eq!(
            config.model_endpoints[0].model.as_deref(),
            Some("qwen-router-a")
        );
        assert_eq!(config.model_endpoints[0].api_key.as_deref(), Some("sk-aaa"));
        assert_eq!(config.model_endpoints[0].wire_api.as_deref(), Some("chat"));
        assert_eq!(config.model_endpoints[1].url, "http://10.0.0.2:8080/v1");
        assert_eq!(
            config.model_endpoints[1].model.as_deref(),
            Some("qwen-router-b")
        );
        assert_eq!(
            config.model_endpoints[1].wire_api.as_deref(),
            Some("responses")
        );
    }

    #[test]
    fn apply_edit_model_endpoints() {
        let mut config = ConfigToml::default();
        let endpoints_json = serde_json::json!([
            { "url": "http://10.0.0.1:8080/v1", "label": "Node A", "model": "qwen-router-a", "api_key": "sk-1", "wire_api": "chat" },
            { "url": "http://10.0.0.2:8080/v1", "label": "Node B", "model": "qwen-router-b" },
        ]);
        config
            .apply_edit("model_endpoints", &endpoints_json)
            .expect("apply model_endpoints");
        assert_eq!(config.model_endpoints.len(), 2);
        assert_eq!(config.model_endpoints[0].url, "http://10.0.0.1:8080/v1");
        assert_eq!(
            config.model_endpoints[0].model.as_deref(),
            Some("qwen-router-a")
        );
        assert_eq!(config.model_endpoints[0].api_key.as_deref(), Some("sk-1"));
        assert_eq!(config.model_endpoints[1].label.as_deref(), Some("Node B"));
        assert_eq!(
            config.model_endpoints[1].model.as_deref(),
            Some("qwen-router-b")
        );
        assert!(config.model_endpoints[1].api_key.is_none());

        config
            .apply_edit("model_endpoints", &serde_json::Value::Null)
            .expect("clear model_endpoints");
        assert!(config.model_endpoints.is_empty());
    }

    #[test]
    fn apply_edit_vision_fallback_fields() {
        let mut config = ConfigToml::default();
        config
            .apply_edit("model_supports_vision", &serde_json::json!(false))
            .expect("set model_supports_vision");
        config
            .apply_edit("vision_fallback_kind", &serde_json::json!("multimodal"))
            .expect("set vision_fallback_kind");
        config
            .apply_edit("vision_fallback_provider", &serde_json::json!("qwen"))
            .expect("set vision_fallback_provider");
        config
            .apply_edit("vision_fallback_model", &serde_json::json!("qwen-vl-max"))
            .expect("set vision_fallback_model");

        assert_eq!(config.model_supports_vision, Some(false));
        assert_eq!(config.vision_fallback_kind.as_deref(), Some("multimodal"));
        assert_eq!(config.vision_fallback_provider.as_deref(), Some("qwen"));
        assert_eq!(config.vision_fallback_model.as_deref(), Some("qwen-vl-max"));

        config
            .apply_edit("vision_fallback_kind", &serde_json::Value::Null)
            .expect("clear vision_fallback_kind");
        config
            .apply_edit("vision_fallback_provider", &serde_json::Value::Null)
            .expect("clear vision_fallback_provider");
        config
            .apply_edit("vision_fallback_model", &serde_json::Value::Null)
            .expect("clear vision_fallback_model");
        assert!(config.vision_fallback_kind.is_none());
        assert!(config.vision_fallback_provider.is_none());
        assert!(config.vision_fallback_model.is_none());
    }

    #[test]
    fn apply_edit_image_generation_fields() {
        let mut config = ConfigToml::default();
        config
            .apply_edit("image_generation.enabled", &serde_json::json!(false))
            .expect("set image_generation.enabled");
        config
            .apply_edit("image_generation.model", &serde_json::json!("gpt-image-2"))
            .expect("set image_generation.model");
        config
            .apply_edit(
                "image_generation.base_url",
                &serde_json::json!("https://api.openai.com/v1/"),
            )
            .expect("set image_generation.base_url");
        config
            .apply_edit("image_generation.api_key", &serde_json::json!("sk-test"))
            .expect("set image_generation.api_key");

        let image_generation = config.image_generation_config();
        assert_eq!(image_generation.enabled, Some(false));
        assert!(!image_generation.is_enabled());
        assert_eq!(image_generation.model.as_deref(), Some("gpt-image-2"));
        assert_eq!(
            image_generation.base_url.as_deref(),
            Some("https://api.openai.com/v1/")
        );
        assert_eq!(image_generation.api_key.as_deref(), Some("sk-test"));

        config
            .apply_edit("image_generation.api_key", &serde_json::Value::Null)
            .expect("clear image_generation.api_key");
        config
            .apply_edit("image_generation.enabled", &serde_json::Value::Null)
            .expect("clear image_generation.enabled");
        let image_generation = config.image_generation_config();
        assert!(image_generation.is_enabled());
        assert!(image_generation.api_key.is_none());
    }

    #[test]
    fn apply_edit_mcp_server_upsert_and_delete() {
        let mut config = ConfigToml::default();
        let server = serde_json::json!({
            "command": "npx",
            "args": ["-y", "@playwright/mcp@latest"],
            "disabled": false
        });

        config
            .apply_edit("mcp_servers.playwright", &server)
            .expect("upsert MCP server");
        assert!(config.mcp_servers.contains_key("playwright"));

        let resolved = config.resolved_mcp_servers();
        let playwright = resolved
            .get("playwright")
            .expect("playwright should be resolved");
        assert_eq!(playwright.command, "npx");
        assert_eq!(playwright.args, vec!["-y", "@playwright/mcp@latest"]);
        assert!(!playwright.disabled);

        config
            .apply_edit("mcp_servers.playwright", &serde_json::Value::Null)
            .expect("delete MCP server");
        assert!(!config.mcp_servers.contains_key("playwright"));
    }

    #[test]
    fn apply_edit_mcp_server_supports_sse_and_is_active() {
        let mut config = ConfigToml::default();
        let server = serde_json::json!({
            "type": "sse",
            "url": "http://10.136.128.2:30092/sse?id=cfb6552f-4d65-4192-8ebf-c5f58d386457",
            "isActive": true
        });

        config
            .apply_edit("mcp_servers.ISS.IPSA.ContractProApi", &server)
            .expect("upsert SSE MCP server");

        let resolved = config.resolved_mcp_servers();
        let remote = resolved
            .get("ISS.IPSA.ContractProApi")
            .expect("SSE server should resolve");
        assert_eq!(remote.transport, "sse");
        assert_eq!(
            remote.url.as_deref(),
            Some("http://10.136.128.2:30092/sse?id=cfb6552f-4d65-4192-8ebf-c5f58d386457")
        );
        assert!(!remote.disabled);
    }

    #[test]
    fn apply_edit_mcp_server_rejects_invalid_payload() {
        let mut config = ConfigToml::default();

        let err = config
            .apply_edit("mcp_servers.playwright", &serde_json::json!("invalid"))
            .expect_err("string payload should be rejected");
        assert!(
            err.to_string().contains("must be an object"),
            "unexpected error: {err}"
        );

        let err = config
            .apply_edit(
                "mcp_servers.playwright",
                &serde_json::json!({ "args": ["-y", "@playwright/mcp@latest"] }),
            )
            .expect_err("payload without command/url should be rejected");
        assert!(
            err.to_string()
                .contains("expected non-empty 'command' or 'url'"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn smartbrain_policy_defaults_are_applied() {
        let config = ConfigToml::default();
        let smartbrain = config.smartbrain_config();
        assert!(smartbrain.knowledge_chunk_files_enabled);
        assert!(smartbrain.knowledge_is_active() == smartbrain.enabled);
        assert!(smartbrain.search_okf_prefilter_enabled);
        assert_eq!(
            smartbrain.search_okf_prefilter_order,
            vec![
                "domain".to_string(),
                "tags".to_string(),
                "source_type".to_string()
            ]
        );
        assert_eq!(
            smartbrain.search_locator_priority,
            vec![
                "domain".to_string(),
                "source_group".to_string(),
                "relative_path".to_string()
            ]
        );
    }

    #[test]
    fn knowledge_enabled_gates_knowledge_features_without_disabling_smartbrain() {
        let mut smartbrain = SmartBrainConfig::default();
        smartbrain.enabled = true;
        smartbrain.knowledge_enabled = false;
        assert!(smartbrain.is_active());
        assert!(!smartbrain.knowledge_is_active());
    }

    #[test]
    fn apply_edit_smartbrain_policy_fields() {
        let mut config = ConfigToml::default();
        config
            .apply_edit(
                "smartbrain.search_okf_prefilter_enabled",
                &serde_json::json!(false),
            )
            .expect("set smartbrain.search_okf_prefilter_enabled");
        config
            .apply_edit(
                "smartbrain.search_okf_prefilter_order",
                &serde_json::json!(["tags", "domain"]),
            )
            .expect("set smartbrain.search_okf_prefilter_order");
        config
            .apply_edit(
                "smartbrain.search_locator_priority",
                &serde_json::json!(["relative_path", "domain"]),
            )
            .expect("set smartbrain.search_locator_priority");
        config
            .apply_edit(
                "smartbrain.knowledge_chunk_files_enabled",
                &serde_json::json!(false),
            )
            .expect("set smartbrain.knowledge_chunk_files_enabled");

        let smartbrain = config.smartbrain_config();
        assert!(!smartbrain.search_okf_prefilter_enabled);
        assert_eq!(
            smartbrain.search_okf_prefilter_order,
            vec!["tags".to_string(), "domain".to_string()]
        );
        assert_eq!(
            smartbrain.search_locator_priority,
            vec!["relative_path".to_string(), "domain".to_string()]
        );
        assert!(!smartbrain.knowledge_chunk_files_enabled);
    }

    #[test]
    fn apply_edit_smartbrain_auto_extract() {
        let mut config = ConfigToml::default();
        config
            .apply_edit("smartbrain.auto_extract", &serde_json::json!(false))
            .expect("set smartbrain.auto_extract");

        let smartbrain = config.smartbrain_config();
        assert!(!smartbrain.auto_extract);
    }

    #[test]
    fn apply_edit_smartbrain_auto_summarize_fields() {
        let mut config = ConfigToml::default();
        config
            .apply_edit(
                "smartbrain.auto_summarize_enabled",
                &serde_json::json!(false),
            )
            .expect("set smartbrain.auto_summarize_enabled");
        config
            .apply_edit(
                "smartbrain.auto_summarize_threshold",
                &serde_json::json!(20),
            )
            .expect("set smartbrain.auto_summarize_threshold");

        let smartbrain = config.smartbrain_config();
        assert!(!smartbrain.auto_summarize_enabled);
        assert_eq!(smartbrain.auto_summarize_threshold, 20);
    }

    #[test]
    fn apply_edit_smartbrain_auto_summarize_threshold_clamps_to_min() {
        let mut config = ConfigToml::default();
        config
            .apply_edit("smartbrain.auto_summarize_threshold", &serde_json::json!(1))
            .expect("set smartbrain.auto_summarize_threshold");
        let smartbrain = config.smartbrain_config();
        assert_eq!(smartbrain.auto_summarize_threshold, 2);
    }
}
