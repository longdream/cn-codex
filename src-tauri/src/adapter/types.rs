//! Adapter 层统一类型定义
//! 所有 adapter 返回相同的 StreamEvent，由 agent.rs 统一处理

use serde::{Deserialize, Serialize};

/// 单次请求中 LLM 返回的 token 用量信息
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageInfo {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

/// tool call 累积器（逐步拼接 SSE 中的碎片）
#[derive(Debug, Clone, Default)]
pub struct ToolCallAccumulator {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// adapter 返回给 agent 的统一流式事件
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// 文本内容增量
    TextDelta(String),

    /// tool call 增量（index, id 可选, name 可选, arguments 片段）
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments: Option<String>,
    },

    /// 流结束，附带 finish reason
    Done {
        finish_reason: Option<String>,
    },

    /// 用量信息（通常在流结束时由最后一个 chunk 返回）
    Usage(UsageInfo),
}

/// adapter 处理完整个 stream 后返回的最终结果
#[derive(Debug)]
pub struct CompletionOutput {
    /// 完整文本内容
    pub text: String,
    /// 所有 tool call
    pub tool_calls: Vec<ToolCallResult>,
    /// finish reason
    pub finish_reason: Option<String>,
    /// token 用量（如果供应商返回了的话）
    pub usage: Option<UsageInfo>,
}

/// 最终解析出的单个 tool call
#[derive(Debug, Clone)]
pub struct ToolCallResult {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// 内部消息格式（从 agent.rs 的 ApiMessage 统一到此处）
#[derive(Debug, Clone, Serialize)]
pub struct InternalMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<InternalToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// 内部 tool call 表示
#[derive(Debug, Clone, Serialize)]
pub struct InternalToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: InternalFunctionCall,
}

/// 内部函数调用表示
#[derive(Debug, Clone, Serialize)]
pub struct InternalFunctionCall {
    pub name: String,
    pub arguments: String,
}
