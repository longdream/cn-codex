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
use reqwest::header::HeaderMap;
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
