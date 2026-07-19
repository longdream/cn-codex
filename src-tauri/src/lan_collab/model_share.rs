//! 模型共享：共享方本地 OpenAI 兼容代理 + 远端零 Key 接入。
//!
//! 原则：
//! - 只共享推理能力，不共享 API Key
//! - 共享方用本机 ConfigManager 真实供应商转发
//! - 接收方通过 `http://host:proxy_port/v1` 调用，鉴权用 share token

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::config_system::{ConfigManager, ModelProviderInfo};

use super::types::SharedModelOffer;

const PROXY_PORT_START: u16 = 47900;
const PROXY_PORT_END: u16 = 47920;
const DEFAULT_MAX_CONCURRENT: usize = 2;
const DEFAULT_DAILY_REQUESTS: u64 = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedModelConfig {
    pub share_id: String,
    pub model_id: String,
    pub display_name: String,
    pub provider_id: String,
    pub upstream_model: String,
    pub group_id: Option<String>,
    pub max_concurrent: usize,
    pub daily_request_limit: u64,
    pub enabled: bool,
    pub created_at: i64,
    /// 仅本机内存：上游 base_url（永不进入 Wire/Offer）
    #[serde(skip)]
    pub upstream_base_url: Option<String>,
    /// 仅本机内存：上游 api key（永不进入 Wire/Offer）
    #[serde(skip)]
    pub upstream_api_key: Option<String>,
}

#[derive(Debug, Clone)]
struct UsageCounter {
    day_key: String,
    requests: u64,
}

#[derive(Debug)]
struct ProxyInner {
    enabled: bool,
    bind_port: u16,
    token: String,
    shares: Vec<SharedModelConfig>,
    /// share_id -> concurrent in-flight
    inflight: HashMap<String, Arc<AtomicUsize>>,
    /// share_id -> daily usage
    usage: HashMap<String, UsageCounter>,
    listener_task: Option<JoinHandle<()>>,
}

#[derive(Clone)]
pub struct ModelShareService {
    config_manager: ConfigManager,
    http: reqwest::Client,
    inner: Arc<RwLock<ProxyInner>>,
    start_lock: Arc<Mutex<()>>,
}

#[derive(Clone)]
struct ProxyState {
    service: ModelShareService,
}

impl ModelShareService {
    pub fn new(config_manager: ConfigManager) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(600))
            // Upstream LLM calls should bypass system proxies so LAN/shared endpoints
            // do not fail with opaque 502 Bad Gateway responses.
            .no_proxy()
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            config_manager,
            http,
            inner: Arc::new(RwLock::new(ProxyInner {
                enabled: false,
                bind_port: 0,
                token: format!("lan_{}", Uuid::new_v4().simple()),
                shares: Vec::new(),
                inflight: HashMap::new(),
                usage: HashMap::new(),
                listener_task: None,
            })),
            start_lock: Arc::new(Mutex::new(())),
        }
    }

    pub async fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        if enabled {
            self.ensure_proxy_running().await?;
        } else {
            self.stop_proxy().await;
        }
        Ok(())
    }

    pub async fn proxy_port(&self) -> u16 {
        self.inner.read().await.bind_port
    }

    pub async fn token(&self) -> String {
        self.inner.read().await.token.clone()
    }

    pub async fn list_local_shares(&self) -> Vec<SharedModelConfig> {
        self.inner.read().await.shares.clone()
    }

    pub async fn local_offers(
        &self,
        host_node_id: &str,
        host_display_name: &str,
        host_address: &str,
        allowed_group_ids: Option<&HashSet<String>>,
    ) -> Vec<SharedModelOffer> {
        let guard = self.inner.read().await;
        if !guard.enabled || guard.bind_port == 0 {
            return Vec::new();
        }
        guard
            .shares
            .iter()
            .filter(|s| s.enabled)
            .filter(|s| group_allowed(&s.group_id, allowed_group_ids))
            .map(|s| SharedModelOffer {
                share_id: s.share_id.clone(),
                host_node_id: host_node_id.to_string(),
                host_display_name: host_display_name.to_string(),
                host_address: host_address.to_string(),
                proxy_port: guard.bind_port,
                model_id: s.model_id.clone(),
                display_name: s.display_name.clone(),
                provider_id: s.provider_id.clone(),
                upstream_model: s.upstream_model.clone(),
                group_id: s.group_id.clone(),
                access_token: guard.token.clone(),
                online: true,
            })
            .collect()
    }

    pub async fn share_model(
        &self,
        model_id: String,
        display_name: String,
        provider_id: String,
        upstream_model: String,
        group_id: Option<String>,
        upstream_base_url: Option<String>,
        upstream_api_key: Option<String>,
    ) -> Result<SharedModelConfig, String> {
        let model_id = model_id.trim().to_string();
        let provider_id = provider_id.trim().to_string();
        let upstream_model = if upstream_model.trim().is_empty() {
            model_id.clone()
        } else {
            upstream_model.trim().to_string()
        };
        let display_name = if display_name.trim().is_empty() {
            format!("共享 / {upstream_model}")
        } else {
            display_name.trim().to_string()
        };
        if model_id.is_empty() || provider_id.is_empty() {
            return Err("modelId / providerId 不能为空".to_string());
        }
        let group_id = Some(require_group_id(group_id)?);

        // 优先用前端传入的本机上游配置；否则回落 ConfigManager（已激活供应商）
        let (upstream_base_url, upstream_api_key) = {
            let from_args_base = upstream_base_url
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let from_args_key = upstream_api_key
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());

            if let Some(base) = from_args_base {
                (Some(base), from_args_key)
            } else {
                let config = self.config_manager.read().map_err(|e| e.to_string())?;
                // 尝试实例 id，再尝试 type
                let provider = {
                    let by_id = config.resolve_provider_by_id(&provider_id);
                    if by_id.resolve_base_url().filter(|s| !s.is_empty()).is_some() {
                        by_id
                    } else {
                        config.resolve_provider_by_id(
                            provider_id.split(':').next().unwrap_or(&provider_id),
                        )
                    }
                };
                let base = provider
                    .resolve_base_url()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| {
                        format!(
                            "本机供应商「{provider_id}」未配置 base_url。请先在设置中配置并激活该供应商后再共享。"
                        )
                    })?;
                let key = provider.resolve_api_key().unwrap_or_default();
                if key.is_empty() && provider.requires_openai_auth.unwrap_or(true) {
                    return Err(format!(
                        "本机供应商「{provider_id}」未配置 API Key。Key 仅保存在本机，请先在设置中填写。"
                    ));
                }
                (Some(base), if key.is_empty() { None } else { Some(key) })
            }
        };

        self.ensure_proxy_running().await?;

        let mut guard = self.inner.write().await;
        if let Some(existing) = guard.shares.iter_mut().find(|s| {
            s.provider_id == provider_id
                && s.upstream_model == upstream_model
                && s.group_id == group_id
        }) {
            existing.enabled = true;
            existing.display_name = display_name;
            existing.model_id = model_id;
            existing.upstream_base_url = upstream_base_url;
            existing.upstream_api_key = upstream_api_key;
            return Ok(existing.clone());
        }

        let share = SharedModelConfig {
            share_id: format!("mshare_{}", Uuid::new_v4().simple()),
            model_id,
            display_name,
            provider_id,
            upstream_model,
            group_id,
            max_concurrent: DEFAULT_MAX_CONCURRENT,
            daily_request_limit: DEFAULT_DAILY_REQUESTS,
            enabled: true,
            created_at: chrono::Utc::now().timestamp(),
            upstream_base_url,
            upstream_api_key,
        };
        guard
            .inflight
            .insert(share.share_id.clone(), Arc::new(AtomicUsize::new(0)));
        guard.shares.push(share.clone());
        Ok(share)
    }

    pub async fn unshare_model(&self, share_id: String) -> Result<(), String> {
        let mut guard = self.inner.write().await;
        let before = guard.shares.len();
        guard.shares.retain(|s| s.share_id != share_id);
        guard.inflight.remove(&share_id);
        guard.usage.remove(&share_id);
        if guard.shares.len() == before {
            return Err("共享项不存在".to_string());
        }
        Ok(())
    }

    pub async fn share_group_id(&self, share_id: &str) -> Result<Option<String>, String> {
        let guard = self.inner.read().await;
        guard
            .shares
            .iter()
            .find(|s| s.enabled && s.share_id == share_id)
            .map(|s| s.group_id.clone())
            .ok_or_else(|| "共享项不存在".to_string())
    }

    async fn ensure_proxy_running(&self) -> Result<(), String> {
        {
            let guard = self.inner.read().await;
            if guard.enabled && guard.bind_port > 0 {
                return Ok(());
            }
        }
        let _lock = self.start_lock.lock().await;
        {
            let guard = self.inner.read().await;
            if guard.enabled && guard.bind_port > 0 {
                return Ok(());
            }
        }

        let mut bound = None;
        for port in PROXY_PORT_START..=PROXY_PORT_END {
            match TcpListener::bind(("0.0.0.0", port)).await {
                Ok(listener) => {
                    bound = Some((listener, port));
                    break;
                }
                Err(err) => {
                    tracing::debug!("[lan_model_share] bind proxy :{port} failed: {err}");
                }
            }
        }
        let (listener, port) = bound
            .ok_or_else(|| format!("无法绑定模型代理端口 {PROXY_PORT_START}-{PROXY_PORT_END}"))?;

        let state = ProxyState {
            service: self.clone(),
        };
        let app = Router::new()
            .route("/health", get(health_handler))
            .route("/v1/models", get(list_models_handler))
            .route("/v1/models/{share_id}", get(get_model_handler))
            .route("/v1/chat/completions", post(chat_completions_handler))
            .with_state(state);

        let task = tokio::spawn(async move {
            if let Err(err) = axum::serve(listener, app).await {
                tracing::warn!("[lan_model_share] proxy server stopped: {err}");
            }
        });

        let mut guard = self.inner.write().await;
        guard.enabled = true;
        guard.bind_port = port;
        guard.listener_task = Some(task);
        tracing::info!("[lan_model_share] proxy listening on :{port}");
        Ok(())
    }

    async fn stop_proxy(&self) {
        let task = {
            let mut guard = self.inner.write().await;
            guard.enabled = false;
            guard.bind_port = 0;
            guard.listener_task.take()
        };
        if let Some(task) = task {
            task.abort();
        }
        tracing::info!("[lan_model_share] proxy stopped");
    }

    async fn authorized(&self, headers: &HeaderMap) -> Result<(), (StatusCode, String)> {
        let guard = self.inner.read().await;
        if !guard.enabled {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                "model share disabled".into(),
            ));
        }
        let expected = format!("Bearer {}", guard.token);
        let auth = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if auth != expected {
            return Err((StatusCode::UNAUTHORIZED, "invalid share token".into()));
        }
        Ok(())
    }

    async fn find_share_for_model(&self, model: &str) -> Option<SharedModelConfig> {
        let guard = self.inner.read().await;
        guard
            .shares
            .iter()
            .find(|s| {
                s.enabled
                    && (s.share_id == model
                        || s.model_id == model
                        || s.upstream_model == model
                        || s.display_name == model)
            })
            .cloned()
    }

    async fn acquire_quota(
        &self,
        share: &SharedModelConfig,
    ) -> Result<Arc<AtomicUsize>, (StatusCode, String)> {
        let counter = {
            let mut guard = self.inner.write().await;
            let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
            let usage = guard
                .usage
                .entry(share.share_id.clone())
                .or_insert_with(|| UsageCounter {
                    day_key: day.clone(),
                    requests: 0,
                });
            if usage.day_key != day {
                usage.day_key = day;
                usage.requests = 0;
            }
            if usage.requests >= share.daily_request_limit {
                return Err((
                    StatusCode::TOO_MANY_REQUESTS,
                    "share daily request quota exceeded".into(),
                ));
            }
            usage.requests = usage.requests.saturating_add(1);
            guard
                .inflight
                .entry(share.share_id.clone())
                .or_insert_with(|| Arc::new(AtomicUsize::new(0)))
                .clone()
        };

        let current = counter.fetch_add(1, Ordering::SeqCst);
        if current >= share.max_concurrent {
            counter.fetch_sub(1, Ordering::SeqCst);
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                "share concurrent limit exceeded".into(),
            ));
        }
        Ok(counter)
    }

    async fn resolve_upstream(
        &self,
        share: &SharedModelConfig,
    ) -> Result<(ModelProviderInfo, String), (StatusCode, String)> {
        // 优先使用共享时缓存的本机凭证（不离开本机）
        if let Some(base) = share
            .upstream_base_url
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            let key = share.upstream_api_key.clone().unwrap_or_default();
            let mut provider = ModelProviderInfo::default();
            provider.base_url = Some(base);
            provider.experimental_bearer_token = if key.is_empty() { None } else { Some(key) };
            provider.requires_openai_auth = Some(
                !share
                    .upstream_api_key
                    .as_ref()
                    .map(|s| s.is_empty())
                    .unwrap_or(true),
            );
            provider.wire_api = Some("chat".to_string());
            return Ok((provider, share.upstream_model.clone()));
        }

        let config = self.config_manager.read().map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("read local provider config failed: {e}"),
            )
        })?;
        // provider_id 可能是前端实例 id 或 type（openai/deepseek）
        let provider = config.resolve_provider_by_id(&share.provider_id);
        let base = provider.resolve_base_url().ok_or_else(|| {
            (
                StatusCode::BAD_GATEWAY,
                format!("upstream provider '{}' has no base_url", share.provider_id),
            )
        })?;
        let key = provider.resolve_api_key().unwrap_or_default();
        if key.is_empty() && provider.requires_openai_auth.unwrap_or(true) {
            return Err((
                StatusCode::BAD_GATEWAY,
                format!("upstream provider '{}' has no api key", share.provider_id),
            ));
        }
        let mut provider = provider;
        provider.base_url = Some(base);
        provider.experimental_bearer_token = Some(key);
        Ok((provider, share.upstream_model.clone()))
    }
}

async fn health_handler() -> impl IntoResponse {
    Json(json!({ "ok": true, "service": "lan-model-share" }))
}

async fn list_models_handler(State(state): State<ProxyState>, headers: HeaderMap) -> Response {
    if let Err((code, msg)) = state.service.authorized(&headers).await {
        return error_response(code, msg);
    }
    let shares = state.service.list_local_shares().await;
    let data: Vec<Value> = shares
        .into_iter()
        .filter(|s| s.enabled)
        .map(|s| {
            json!({
                "id": s.share_id,
                "object": "model",
                "created": s.created_at,
                "owned_by": "lan-share",
                "root": s.upstream_model,
                "display_name": s.display_name,
            })
        })
        .collect();
    Json(json!({ "object": "list", "data": data })).into_response()
}

async fn get_model_handler(
    State(state): State<ProxyState>,
    headers: HeaderMap,
    Path(share_id): Path<String>,
) -> Response {
    if let Err((code, msg)) = state.service.authorized(&headers).await {
        return error_response(code, msg);
    }
    let shares = state.service.list_local_shares().await;
    if let Some(s) = shares
        .into_iter()
        .find(|s| s.share_id == share_id && s.enabled)
    {
        return Json(json!({
            "id": s.share_id,
            "object": "model",
            "created": s.created_at,
            "owned_by": "lan-share",
            "root": s.upstream_model,
            "display_name": s.display_name,
        }))
        .into_response();
    }
    error_response(StatusCode::NOT_FOUND, "model not found".into())
}

async fn chat_completions_handler(
    State(state): State<ProxyState>,
    headers: HeaderMap,
    Json(mut body): Json<Value>,
) -> Response {
    if let Err((code, msg)) = state.service.authorized(&headers).await {
        return error_response(code, msg);
    }

    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if model.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "model is required".into());
    }

    let share = match state.service.find_share_for_model(&model).await {
        Some(s) => s,
        None => return error_response(StatusCode::NOT_FOUND, "shared model not found".into()),
    };

    let counter = match state.service.acquire_quota(&share).await {
        Ok(c) => c,
        Err((code, msg)) => return error_response(code, msg),
    };

    let (provider, upstream_model) = match state.service.resolve_upstream(&share).await {
        Ok(v) => v,
        Err((code, msg)) => {
            counter.fetch_sub(1, Ordering::SeqCst);
            return error_response(code, msg);
        }
    };

    // 强制走上游真实模型名；不把 share_id 传给上游
    if let Some(obj) = body.as_object_mut() {
        obj.insert("model".into(), Value::String(upstream_model.clone()));
    }

    let stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let base_url = provider.resolve_base_url().unwrap_or_default();
    let api_key = provider.resolve_api_key().unwrap_or_default();
    let url = build_chat_url(&base_url);

    let started = Instant::now();
    let request = state
        .service
        .http
        .post(&url)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {api_key}"))
        .json(&body);

    let upstream = match request.send().await {
        Ok(resp) => resp,
        Err(err) => {
            counter.fetch_sub(1, Ordering::SeqCst);
            tracing::warn!(
                "[lan_model_share] upstream request failed share={} model={} url={} err={err}",
                share.share_id,
                upstream_model,
                url
            );
            return error_response(
                StatusCode::BAD_GATEWAY,
                format!("upstream request failed: {err}"),
            );
        }
    };

    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("application/json"));

    if !stream {
        let bytes = match upstream.bytes().await {
            Ok(b) => b,
            Err(err) => {
                counter.fetch_sub(1, Ordering::SeqCst);
                return error_response(
                    StatusCode::BAD_GATEWAY,
                    format!("upstream body failed: {err}"),
                );
            }
        };
        counter.fetch_sub(1, Ordering::SeqCst);
        tracing::info!(
            "[lan_model_share] non-stream share={} model={} status={} elapsed_ms={}",
            share.share_id,
            upstream_model,
            status.as_u16(),
            started.elapsed().as_millis()
        );
        return Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from(bytes))
            .unwrap_or_else(|_| {
                error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "response build failed".into(),
                )
            });
    }

    // stream: 透传 SSE 字节流
    let byte_stream = upstream.bytes_stream();
    let mapped = futures_util::stream::unfold(
        (byte_stream, counter, share.share_id.clone(), started),
        |(mut stream, counter, share_id, started)| async move {
            use futures_util::StreamExt;
            match stream.next().await {
                Some(Ok(chunk)) => Some((
                    Ok::<_, std::io::Error>(chunk),
                    (stream, counter, share_id, started),
                )),
                Some(Err(err)) => {
                    counter.fetch_sub(1, Ordering::SeqCst);
                    tracing::warn!("[lan_model_share] stream error share={share_id}: {err}");
                    Some((
                        Err(std::io::Error::other(err.to_string())),
                        (stream, counter, share_id, started),
                    ))
                }
                None => {
                    counter.fetch_sub(1, Ordering::SeqCst);
                    tracing::info!(
                        "[lan_model_share] stream done share={share_id} elapsed_ms={}",
                        started.elapsed().as_millis()
                    );
                    None
                }
            }
        },
    );

    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(mapped))
        .unwrap_or_else(|_| {
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "stream response build failed".into(),
            )
        })
}

fn group_allowed(group_id: &Option<String>, allowed: Option<&HashSet<String>>) -> bool {
    match (group_id, allowed) {
        // 本机清单：不过滤
        (_, None) => true,
        // 对端目录：未绑定协作组的条目不再视为全员公开
        (None, Some(_)) => false,
        (Some(gid), Some(set)) => set.contains(gid),
    }
}

fn require_group_id(group_id: Option<String>) -> Result<String, String> {
    group_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "请选择要共享到的协作组（未选组的内容保持私有）".to_string())
}

fn build_chat_url(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/chat/completions") {
        return base.to_string();
    }
    if base.ends_with("/responses") {
        let prefix = &base[..base.len() - "/responses".len()];
        return format!("{prefix}/chat/completions");
    }
    format!("{base}/chat/completions")
}

fn error_response(status: StatusCode, message: String) -> Response {
    let body = json!({
        "error": {
            "message": message,
            "type": "lan_share_error",
        }
    });
    (status, Json(body)).into_response()
}

/// 解析 `host:port` / `IP:port` 用于接收方 base_url。
pub fn offer_base_url(host_address: &str, proxy_port: u16) -> String {
    // host_address 可能是 ip 或 ip:collab_port
    let host = host_address
        .split(':')
        .next()
        .unwrap_or(host_address)
        .trim();
    format!("http://{host}:{proxy_port}/v1")
}
