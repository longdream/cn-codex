//! API 格式适配层
//! 根据供应商的 wire_api 类型选择对应的 adapter 进行请求/响应格式转换。
//! 支持: chat (OpenAI Chat Completions), responses (OpenAI Responses API),
//!       anthropic (Anthropic Messages API), gemini (Google Gemini API)

pub mod anthropic;
pub mod chat_completions;
pub mod google;
pub mod responses;
pub mod types;

use async_trait::async_trait;
use reqwest::Url;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::collections::HashMap;
use types::{InternalMessage, StreamEvent};

/// 所有供应商 adapter 必须实现的 trait
#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    /// 构建请求 URL
    fn build_url(&self, base_url: &str, model: &str) -> String;

    /// 构建请求 headers
    fn build_headers(&self, api_key: &str) -> HeaderMap;

    /// 将内部统一消息格式转换为供应商 API 请求 body
    fn build_body(
        &self,
        model: &str,
        messages: &[InternalMessage],
        tools: Option<&[serde_json::Value]>,
        max_tokens: Option<i64>,
    ) -> serde_json::Value;

    /// 判断 SSE 行是否表示流结束
    fn is_stream_done(&self, line: &str) -> bool;

    /// 解析单条 SSE data 行，返回零或多个 StreamEvent
    fn parse_stream_line(&self, line: &str) -> Vec<StreamEvent>;
}

/// 根据 wire_api 字符串选择对应的 adapter 实例
pub fn get_adapter(wire_api: &str) -> Box<dyn ProviderAdapter> {
    match wire_api {
        "responses" => Box::new(responses::ResponsesAdapter),
        "anthropic" => Box::new(anthropic::AnthropicAdapter),
        "gemini" => Box::new(google::GoogleAdapter),
        // 默认使用 OpenAI Chat Completions
        _ => Box::new(chat_completions::ChatCompletionsAdapter),
    }
}

/// 合并 provider 级别的 query/header 覆盖项（用于网关白名单、反抓取 header 等场景）
pub fn apply_request_overrides(
    url: String,
    mut headers: HeaderMap,
    query_params: Option<&HashMap<String, String>>,
    extra_headers: Option<&HashMap<String, String>>,
) -> Result<(String, HeaderMap), String> {
    let mut parsed_url =
        Url::parse(&url).map_err(|e| format!("Invalid request URL '{url}': {e}"))?;
    if let Some(query_params) = query_params {
        let mut qp = parsed_url.query_pairs_mut();
        for (key, value) in query_params {
            if key.trim().is_empty() {
                continue;
            }
            qp.append_pair(key, value);
        }
    }

    if let Some(extra_headers) = extra_headers {
        for (key, value) in extra_headers {
            if key.trim().is_empty() {
                continue;
            }
            let name = HeaderName::from_bytes(key.as_bytes())
                .map_err(|e| format!("Invalid header name '{key}': {e}"))?;
            let header_value = HeaderValue::from_str(value)
                .map_err(|e| format!("Invalid header value for '{key}': {e}"))?;
            headers.insert(name, header_value);
        }
    }

    Ok((parsed_url.to_string(), headers))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_request_overrides_merges_query_and_headers() {
        let mut query = HashMap::new();
        query.insert("api-version".to_string(), "2024-01-01".to_string());
        let mut extra_headers = HashMap::new();
        extra_headers.insert("x-test-header".to_string(), "yes".to_string());

        let headers = HeaderMap::new();
        let (url, headers) = apply_request_overrides(
            "https://example.com/v1/chat/completions".to_string(),
            headers,
            Some(&query),
            Some(&extra_headers),
        )
        .unwrap();

        assert!(url.contains("api-version=2024-01-01"));
        assert_eq!(
            headers
                .get("x-test-header")
                .and_then(|v| v.to_str().ok())
                .unwrap_or(""),
            "yes"
        );
    }
}
