//! 语音模式 Tauri 命令层。
//!
//! 前端通过这些命令控制模型下载、语音会话与 TTS 播放。

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, State};
use tracing::info;

use crate::error::AppResult;
use crate::state::AppState;

use super::downloader::VoiceDownloadManager;
use super::engine::{self, SubEngineState};
use super::models::{self, VoiceModelDef};
use super::session::VoiceSessionManager;

pub struct VoiceState {
    pub manager: VoiceDownloadManager,
    pub session: VoiceSessionManager,
    defs: Vec<VoiceModelDef>,
}

impl VoiceState {
    pub fn new(workspace_config_dir: &std::path::Path) -> Self {
        let models_dir = models::voice_models_root(workspace_config_dir);
        Self {
            manager: VoiceDownloadManager::new(models_dir),
            session: VoiceSessionManager::new(),
            defs: models::builtin_model_defs(),
        }
    }

    pub fn defs(&self) -> &[VoiceModelDef] {
        &self.defs
    }

    pub fn find_def(&self, id: &str) -> Option<VoiceModelDef> {
        self.defs.iter().find(|d| d.id == id).cloned()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceModelStatus {
    pub id: String,
    pub label: String,
    /// not_installed / downloading / ready / failed / loading / engine_failed
    pub status: String,
    pub total_size: u64,
    pub ready_marker_ok: bool,
}

#[tauri::command]
pub async fn voice_get_model_statuses(
    state: State<'_, VoiceState>,
) -> AppResult<Vec<VoiceModelStatus>> {
    let manifest = models::VoiceModelsManifest::load(state.manager.models_dir());
    let eng = engine::engine();
    let list = state
        .defs()
        .iter()
        .map(|def| {
            let marker_ok = models::is_model_ready(state.manager.models_dir(), def);
            let manifest_status = manifest
                .models
                .get(&def.id)
                .map(|s| s.status.clone())
                .unwrap_or_else(|| "not_installed".to_string());
            let status = if marker_ok && manifest_status == "ready" {
                match eng {
                    Some(e) => match e.state_of(&def.id) {
                        SubEngineState::Ready => "ready".to_string(),
                        SubEngineState::Failed => "engine_failed".to_string(),
                        SubEngineState::Loading => "loading".to_string(),
                        SubEngineState::NotReady => "loading".to_string(),
                    },
                    None => "ready".to_string(),
                }
            } else {
                manifest_status
            };
            VoiceModelStatus {
                id: def.id.clone(),
                label: def.label.clone(),
                status,
                total_size: def.total_size,
                ready_marker_ok: marker_ok,
            }
        })
        .collect();
    Ok(list)
}

/// 手动触发（或重试）某模型下载。
#[tauri::command]
pub async fn voice_download_model(
    app: AppHandle,
    state: State<'_, VoiceState>,
    model_id: String,
) -> AppResult<()> {
    let def = state
        .find_def(&model_id)
        .ok_or_else(|| crate::error::AppError::Custom(format!("unknown model {model_id}")))?;
    let manager = &state.manager;
    manager
        .download_model(app, def)
        .await
        .map_err(crate::error::AppError::Custom)?;
    // 下载完成后立即加载引擎。
    if let Some(eng) = engine::engine() {
        if let Some(def) = state.find_def(&model_id) {
            let result = match model_id.as_str() {
                "asr" => eng.load_asr(&def),
                "tts" => eng.load_tts(&def),
                "vad" => eng.load_vad(&def),
                _ => Ok(()),
            };
            if let Err(err) = result {
                info!("[voice] engine load {model_id} failed: {err}");
            }
        }
    }
    Ok(())
}

/// 启动监听麦克风（VAD 自动断句）。
#[tauri::command]
pub async fn voice_start_listening(
    app: AppHandle,
    state: State<'_, VoiceState>,
) -> AppResult<()> {
    state
        .session
        .start_listening(app)
        .await
        .map_err(|e| match e.as_str() {
            "ASR_NOT_READY" => crate::error::AppError::Custom("ASR_NOT_READY".to_string()),
            "VAD_NOT_READY" => crate::error::AppError::Custom("VAD_NOT_READY".to_string()),
            other => crate::error::AppError::Custom(other.to_string()),
        })
}

/// 停止监听。
#[tauri::command]
pub async fn voice_stop_listening(
    app: AppHandle,
    state: State<'_, VoiceState>,
) -> AppResult<()> {
    state.session.stop_listening(&app).await;
    Ok(())
}

#[tauri::command]
pub async fn voice_get_listen_state(state: State<'_, VoiceState>) -> AppResult<String> {
    let s = state.session.listen_state();
    Ok(serde_json::to_string(&s).unwrap_or_else(|_| "\"idle\"".to_string()))
}

#[derive(Debug, Deserialize)]
pub struct VoiceTtsRequest {
    pub text: String,
    #[serde(default = "default_speed")]
    pub speed: f32,
    #[serde(default)]
    pub sid: i32,
}

fn default_speed() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceTtsResult {
    pub samples: Vec<f32>,
    pub sample_rate: i32,
}

/// 合成语音并返回 PCM（前端通过 WebAudio 播放，天然支持打断）。
#[tauri::command]
pub async fn voice_tts_generate(
    _state: State<'_, VoiceState>,
    request: VoiceTtsRequest,
) -> AppResult<VoiceTtsResult> {
    let eng = engine::engine().ok_or_else(|| {
        crate::error::AppError::Custom("TTS_NOT_READY".to_string())
    })?;
    if eng.state_of("tts") != SubEngineState::Ready {
        return Err(crate::error::AppError::Custom("TTS_NOT_READY".to_string()));
    }
    let (samples, rate) = eng
        .synthesize(&request.text, request.speed, request.sid)
        .map_err(crate::error::AppError::Custom)?;
    Ok(VoiceTtsResult {
        samples,
        sample_rate: rate,
    })
}

/// 启动时调用：初始化引擎并自动检查下载缺失模型。
#[tauri::command]
pub async fn voice_startup_check(app: AppHandle, state: State<'_, VoiceState>) -> AppResult<()> {
    let defs = state.defs().to_vec();
    let missing = state.manager.find_missing(&defs);
    info!("[voice] startup check: {} models missing", missing.len());

    // 引擎初始化 + 已就绪模型自动加载。
    let models_dir: PathBuf = state.manager.models_dir().to_path_buf();
    let eng = engine::init_engine(models_dir);
    eng.autoload(&defs);

    // 缺失模型自动后台下载。
    for def in missing {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let _ = download_def(app, def).await;
        });
    }
    Ok(())
}

async fn download_def(app: AppHandle, def: VoiceModelDef) -> Result<(), String> {
    // 通过全局 VoiceState 下载（从 app.state 拿）。
    use tauri::Manager;
    let state: tauri::State<VoiceState> = app.state();
    let manager = &state.manager;
    let result = manager.download_model(app.clone(), def.clone()).await;
    if result.is_ok() {
        if let Some(eng) = engine::engine() {
            let load_result = match def.id.as_str() {
                "asr" => eng.load_asr(&def),
                "tts" => eng.load_tts(&def),
                "vad" => eng.load_vad(&def),
                _ => Ok(()),
            };
            if let Err(err) = load_result {
                info!("[voice] engine load {} failed: {err}", def.id);
            }
        }
    }
    result
}

// VoiceState 从 AppState 派生，需要在 setup 中 manage。
// 这里提供一个从 AppState 构造的帮助函数。
impl VoiceState {
    pub fn from_app_state(state: &AppState) -> Self {
        Self::new(&state.workspace_config_dir)
    }
}

use serde::Deserialize;
