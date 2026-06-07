use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadStartParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_policy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSummary {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadStartResponse {
    pub thread: ThreadSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadResumeParams {
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadResumeResponse {
    pub thread: ThreadSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ThreadListParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadListResponse {
    pub data: Vec<ThreadSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadReadParams {
    pub thread_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_turns: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadReadResponse {
    pub thread: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadArchiveParams {
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ThreadArchiveResponse {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadUnarchiveParams {
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadUnarchiveResponse {
    pub thread: ThreadSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSetNameParams {
    pub thread_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSetNameResponse {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadRollbackParams {
    pub thread_id: String,
    pub drop_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadRollbackResponse {
    pub thread: ThreadSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadUnsubscribeParams {
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadUnsubscribeResponse {
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInput {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnStartParams {
    pub thread_id: String,
    pub input: Vec<UserInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnStartResponse {
    pub turn: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnSteerParams {
    pub thread_id: String,
    pub input: Vec<UserInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_turn_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnSteerResponse {
    pub turn_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnInterruptParams {
    pub thread_id: String,
    pub turn_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TurnInterruptResponse {}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConfigReadParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_layers: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigReadResponse {
    pub config: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layers: Option<Vec<Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigValueWriteParams {
    pub key_path: String,
    pub value: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_strategy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigEdit {
    pub key_path: String,
    pub value: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_strategy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigBatchWriteParams {
    pub edits: Vec<ConfigEdit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigWriteResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAccountParams {
    #[serde(default)]
    pub refresh_token: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAccountResponse {
    #[serde(default)]
    pub account: Option<Value>,
    #[serde(default)]
    pub requires_openai_auth: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginAccountParams {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LoginAccountResponse(pub Value);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelLoginAccountParams {
    pub login_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CancelLoginAccountResponse(pub Value);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LogoutAccountResponse(pub Value);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GetAccountRateLimitsResponse(pub Value);

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelListParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_hidden: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelListResponse {
    pub data: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

pub struct RpcCall {
    pub method: &'static str,
    pub params: Value,
}

fn map_merge_strategy(value: Option<&str>) -> &'static str {
    match value {
        Some("merge") => "upsert",
        _ => "replace",
    }
}

fn map_user_input(input: &UserInput) -> Value {
    match input.kind.as_str() {
        "image" => json!({
            "type": "image",
            "url": input.image_url.clone().unwrap_or_default()
        }),
        _ => json!({
            "type": "text",
            "text": input.text.clone().unwrap_or_default(),
            "text_elements": []
        }),
    }
}

pub fn thread_start_rpc(params: &ThreadStartParams) -> RpcCall {
    RpcCall {
        method: "thread/start",
        params: json!({
            "model": params.model,
            "modelProvider": params.model_provider,
            "cwd": params.cwd,
            "approvalPolicy": params.approval_policy,
            "baseInstructions": params.instructions,
            "experimentalRawEvents": false,
            "persistExtendedHistory": true
        }),
    }
}

pub fn thread_resume_rpc(params: &ThreadResumeParams) -> RpcCall {
    RpcCall {
        method: "thread/resume",
        params: json!({ "threadId": params.thread_id }),
    }
}

pub fn thread_list_rpc(params: &ThreadListParams) -> RpcCall {
    RpcCall {
        method: "thread/list",
        params: serde_json::to_value(params).unwrap_or_default(),
    }
}

pub fn thread_read_rpc(params: &ThreadReadParams) -> RpcCall {
    RpcCall {
        method: "thread/read",
        params: json!({
            "threadId": params.thread_id,
            "includeTurns": params.include_turns.unwrap_or(true)
        }),
    }
}

pub fn thread_archive_rpc(params: &ThreadArchiveParams) -> RpcCall {
    RpcCall {
        method: "thread/archive",
        params: json!({ "threadId": params.thread_id }),
    }
}

pub fn thread_unarchive_rpc(params: &ThreadUnarchiveParams) -> RpcCall {
    RpcCall {
        method: "thread/unarchive",
        params: json!({ "threadId": params.thread_id }),
    }
}

pub fn thread_set_name_rpc(params: &ThreadSetNameParams) -> RpcCall {
    RpcCall {
        method: "thread/name/set",
        params: json!({
            "threadId": params.thread_id,
            "name": params.name
        }),
    }
}

pub fn thread_rollback_rpc(params: &ThreadRollbackParams) -> RpcCall {
    RpcCall {
        method: "thread/rollback",
        params: json!({
            "threadId": params.thread_id,
            "numTurns": params.drop_count.max(1)
        }),
    }
}

pub fn thread_unsubscribe_rpc(params: &ThreadUnsubscribeParams) -> RpcCall {
    RpcCall {
        method: "thread/unsubscribe",
        params: json!({ "threadId": params.thread_id }),
    }
}

pub fn turn_start_rpc(params: &TurnStartParams) -> RpcCall {
    let input = params.input.iter().map(map_user_input).collect::<Vec<_>>();
    RpcCall {
        method: "turn/start",
        params: json!({
            "threadId": params.thread_id,
            "input": input,
            "model": params.model,
            "effort": params.effort
        }),
    }
}

pub fn turn_steer_rpc(params: &TurnSteerParams) -> RpcCall {
    let input = params.input.iter().map(map_user_input).collect::<Vec<_>>();
    RpcCall {
        method: "turn/steer",
        params: json!({
            "threadId": params.thread_id,
            "input": input,
            "expectedTurnId": params.expected_turn_id
        }),
    }
}

pub fn turn_interrupt_rpc(params: &TurnInterruptParams) -> RpcCall {
    RpcCall {
        method: "turn/interrupt",
        params: json!({
            "threadId": params.thread_id,
            "turnId": params.turn_id
        }),
    }
}

pub fn config_read_rpc(params: &ConfigReadParams) -> RpcCall {
    RpcCall {
        method: "config/read",
        params: json!({
            "includeLayers": params.include_layers.unwrap_or(false),
            "cwd": params.cwd
        }),
    }
}

pub fn config_value_write_rpc(params: &ConfigValueWriteParams) -> RpcCall {
    RpcCall {
        method: "config/value/write",
        params: json!({
            "keyPath": params.key_path,
            "value": params.value,
            "mergeStrategy": map_merge_strategy(params.merge_strategy.as_deref()),
            "filePath": params.file_path
        }),
    }
}

pub fn config_batch_write_rpc(params: &ConfigBatchWriteParams) -> RpcCall {
    let edits = params
        .edits
        .iter()
        .map(|edit| {
            json!({
                "keyPath": edit.key_path,
                "value": edit.value,
                "mergeStrategy": map_merge_strategy(edit.merge_strategy.as_deref())
            })
        })
        .collect::<Vec<_>>();
    RpcCall {
        method: "config/batchWrite",
        params: json!({
            "edits": edits,
            "filePath": params.file_path,
            "reloadUserConfig": true
        }),
    }
}

pub fn get_account_rpc(params: &GetAccountParams) -> RpcCall {
    RpcCall {
        method: "account/read",
        params: json!({ "refreshToken": params.refresh_token }),
    }
}

pub fn login_account_rpc(params: &LoginAccountParams) -> RpcCall {
    RpcCall {
        method: "account/login/start",
        params: serde_json::to_value(params).unwrap_or_default(),
    }
}

pub fn cancel_login_rpc(params: &CancelLoginAccountParams) -> RpcCall {
    RpcCall {
        method: "account/login/cancel",
        params: json!({ "loginId": params.login_id }),
    }
}

pub fn logout_account_rpc() -> RpcCall {
    RpcCall {
        method: "account/logout",
        params: Value::Null,
    }
}

pub fn get_rate_limits_rpc() -> RpcCall {
    RpcCall {
        method: "account/rateLimits/read",
        params: Value::Null,
    }
}

pub fn model_list_rpc(params: &ModelListParams) -> RpcCall {
    RpcCall {
        method: "model/list",
        params: serde_json::to_value(params).unwrap_or_default(),
    }
}
