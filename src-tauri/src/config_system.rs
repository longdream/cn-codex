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
pub struct SmartBrainConfig {
    #[serde(default)]
    pub enabled: bool,
    // Experience sub-settings
    #[serde(default = "default_true")]
    pub auto_extract: bool,
    #[serde(default = "default_true")]
    pub auto_consolidate: bool,
    #[serde(default = "default_true")]
    pub inject_summary: bool,
    #[serde(default = "default_max_raw_experiences")]
    pub max_raw_experiences: usize,
    #[serde(default = "default_max_consolidation_entries")]
    pub max_consolidation_entries: usize,
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

impl SmartBrainConfig {
    pub fn is_active(&self) -> bool {
        self.enabled
    }
}

/// 资源池模型的单个后端端点
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEndpointInfo {
    pub url: String,
    #[serde(default)]
    pub label: Option<String>,
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
    "openai",
    "anthropic",
    "google",
    "deepseek",
    "volcengine",
    "qwen",
    "zhipu",
    "moonshot",
    "siliconflow",
    "baichuan",
    "ollama",
    "lmstudio",
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

    pub fn apply_edit(&mut self, key_path: &str, value: &serde_json::Value) {
        match key_path {
            "model" => self.model = value.as_str().map(String::from),
            "model_provider" => self.model_provider = value.as_str().map(String::from),
            "model_reasoning_effort" => {
                self.model_reasoning_effort = value.as_str().map(String::from);
            }
            "model_context_window" => self.model_context_window = value.as_i64(),
            "max_output_tokens" => self.max_output_tokens = value.as_i64(),
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
            other if other.starts_with("model_providers.") => {
                let provider_key = &other["model_providers.".len()..];
                if let Ok(info) = serde_json::from_value::<ModelProviderInfo>(value.clone()) {
                    self.model_providers.insert(provider_key.to_string(), info);
                }
            }
            "model_endpoints" => {
                if value.is_null() || value.is_array() && value.as_array().is_some_and(|a| a.is_empty()) {
                    self.model_endpoints.clear();
                } else if let Ok(endpoints) = serde_json::from_value::<Vec<ModelEndpointInfo>>(value.clone()) {
                    self.model_endpoints = endpoints;
                }
            }
            "active_endpoint_index" => {
                self.active_endpoint_index = value.as_u64().map(|v| v as usize);
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
    if transport == "http" && !is_supported_mcp_http_url(url.as_deref()) {
        return None;
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
        Some("http" | "streamable_http" | "sse") => "http".to_string(),
        Some("stdio" | "local") => "stdio".to_string(),
        Some(_) => "stdio".to_string(),
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

#[derive(Clone)]
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

#[cfg(test)]
mod tests {
    use super::*;

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

            [mcp_servers.empty]
            args = ["missing-command"]
            "#,
        )
        .unwrap();

        let servers = config.resolved_mcp_servers();
        assert_eq!(servers.len(), 2);
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
            api_key = "sk-aaa"
            wire_api = "chat"

            [[model_endpoints]]
            url = "http://10.0.0.2:8080/v1"
            label = "节点2"
            api_key = "sk-bbb"
            wire_api = "responses"
            "#,
        )
        .unwrap();

        assert_eq!(config.model_endpoints.len(), 2);
        assert_eq!(config.model_endpoints[0].url, "http://10.0.0.1:8080/v1");
        assert_eq!(config.model_endpoints[0].label.as_deref(), Some("节点1"));
        assert_eq!(config.model_endpoints[0].api_key.as_deref(), Some("sk-aaa"));
        assert_eq!(config.model_endpoints[0].wire_api.as_deref(), Some("chat"));
        assert_eq!(config.model_endpoints[1].url, "http://10.0.0.2:8080/v1");
        assert_eq!(config.model_endpoints[1].wire_api.as_deref(), Some("responses"));
    }

    #[test]
    fn apply_edit_model_endpoints() {
        let mut config = ConfigToml::default();
        let endpoints_json = serde_json::json!([
            { "url": "http://10.0.0.1:8080/v1", "label": "Node A", "api_key": "sk-1", "wire_api": "chat" },
            { "url": "http://10.0.0.2:8080/v1", "label": "Node B" },
        ]);
        config.apply_edit("model_endpoints", &endpoints_json);
        assert_eq!(config.model_endpoints.len(), 2);
        assert_eq!(config.model_endpoints[0].url, "http://10.0.0.1:8080/v1");
        assert_eq!(config.model_endpoints[0].api_key.as_deref(), Some("sk-1"));
        assert_eq!(config.model_endpoints[1].label.as_deref(), Some("Node B"));
        assert!(config.model_endpoints[1].api_key.is_none());

        config.apply_edit("model_endpoints", &serde_json::Value::Null);
        assert!(config.model_endpoints.is_empty());
    }
}
