//! 语音会话：管理麦克风采集流与 VAD 会话状态。
//!
//! 录音采用 Rust 侧 cpal 直采（16kHz 单声道 f32），VAD 检测到完整语音段后
//! 自动送 ASR 识别，识别结果通过事件 `voice://asr-final` 推送前端。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use serde::Serialize;
use tokio::sync::Mutex as AsyncMutex;
use tracing::{info, warn};

use super::engine::{self, SubEngineState};

pub const EVENT_ASR_FINAL: &str = "voice://asr-final";
pub const EVENT_LISTEN_STATE: &str = "voice://listen-state";
pub const TARGET_SAMPLE_RATE: u32 = 16000;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ListenState {
    Idle,
    Listening,
    Processing,
}

pub struct VoiceSessionManager {
    state: Arc<SessionState>,
    stream: AsyncMutex<Option<cpal::Stream>>,
    /// 会话序号（自增，防止旧事件污染新会话）。
    session_seq: AtomicU64,
}

struct SessionState {
    listen_state: std::sync::Mutex<ListenState>,
    /// 当前累积的 16kHz 采样（VAD 窗口之间的缓冲）。
    pending: std::sync::Mutex<PendingBuffer>,
}

struct PendingBuffer {
    samples: Vec<f32>,
    /// 是否已检测到语音开始。
    speech_started: bool,
}

impl PendingBuffer {
    fn new() -> Self {
        Self {
            samples: Vec::new(),
            speech_started: false,
        }
    }
}

impl VoiceSessionManager {
    pub fn new() -> Self {
        Self {
            state: Arc::new(SessionState {
                listen_state: std::sync::Mutex::new(ListenState::Idle),
                pending: std::sync::Mutex::new(PendingBuffer::new()),
            }),
            stream: AsyncMutex::new(None),
            session_seq: AtomicU64::new(0),
        }
    }

    pub fn listen_state(&self) -> ListenState {
        self.state.listen_state.lock().unwrap().clone()
    }

    fn set_listen_state(&self, app: &tauri::AppHandle, state: ListenState) {
        {
            let mut guard = self.state.listen_state.lock().unwrap();
            if *guard == state {
                return;
            }
            *guard = state.clone();
        }
        use tauri::Emitter;
        let _ = app.emit(EVENT_LISTEN_STATE, &state);
    }

    /// 检查前置条件并启动麦克风监听。
    pub async fn start_listening(&self, app: tauri::AppHandle) -> Result<(), String> {
        let eng = engine::engine().ok_or("voice engine not initialized")?;
        if eng.state_of("asr") != SubEngineState::Ready {
            return Err("ASR_NOT_READY".to_string());
        }
        if eng.state_of("vad") != SubEngineState::Ready {
            return Err("VAD_NOT_READY".to_string());
        }

        let mut stream_guard = self.stream.lock().await;
        if stream_guard.is_some() {
            return Ok(()); // 已在监听
        }

        // 重置会话。
        self.session_seq.fetch_add(1, Ordering::SeqCst);
        {
            let mut pending = self.state.pending.lock().unwrap();
            pending.samples.clear();
            pending.speech_started = false;
        }
        eng.vad_reset();
        eng.vad_clear();

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or("no default input device")?;
        let supported = device
            .default_input_config()
            .map_err(|e| format!("input config: {e}"))?;
        let sample_format = supported.sample_format();
        let cfg = supported.config();
        let channels = cfg.channels as usize;
        let in_rate = cfg.sample_rate.0;
        info!(
            "[voice] mic input: format={sample_format:?}, rate={in_rate}, channels={channels}"
        );

        let session = self.state.clone();
        let app_for_cb = app.clone();
        let err_fn = move |err| warn!("[voice] audio stream error: {err}");

        let stream = match sample_format {
            SampleFormat::F32 => device.build_input_stream(
                &cfg,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    feed_samples(&app_for_cb, &session, data, channels, in_rate);
                },
                err_fn,
                None,
            ),
            SampleFormat::I16 => device.build_input_stream(
                &cfg,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let f32_data: Vec<f32> = data.iter().map(|s| *s as f32 / 32768.0).collect();
                    feed_samples(&app_for_cb, &session, &f32_data, channels, in_rate);
                },
                err_fn,
                None,
            ),
            other => return Err(format!("unsupported sample format {other:?}")),
        }
        .map_err(|e| format!("build input stream: {e}"))?;

        stream.play().map_err(|e| format!("stream play: {e}"))?;
        *stream_guard = Some(stream);
        self.set_listen_state(&app, ListenState::Listening);
        Ok(())
    }

    pub async fn stop_listening(&self, app: &tauri::AppHandle) {
        let mut stream_guard = self.stream.lock().await;
        if let Some(stream) = stream_guard.take() {
            drop(stream);
        }
        if let Some(eng) = engine::engine() {
            eng.vad_reset();
            eng.vad_clear();
        }
        {
            let mut pending = self.state.pending.lock().unwrap();
            pending.samples.clear();
            pending.speech_started = false;
        }
        self.set_listen_state(app, ListenState::Idle);
    }

    /// 标记处理状态（ASR 期间暂停送 VAD）。
    pub fn set_processing(&self, app: &tauri::AppHandle, processing: bool) {
        if processing {
            self.set_listen_state(app, ListenState::Processing);
        } else {
            self.set_listen_state(app, ListenState::Listening);
        }
    }
}

/// 音频回调：降混单声道 → 重采样 16k → VAD。
fn feed_samples(
    app: &tauri::AppHandle,
    session: &Arc<SessionState>,
    data: &[f32],
    channels: usize,
    in_rate: u32,
) {
    let eng = match engine::engine() {
        Some(e) => e,
        None => return,
    };
    if eng.state_of("vad") != SubEngineState::Ready
        || eng.state_of("asr") != SubEngineState::Ready
    {
        return;
    }
    // 处理中（ASR 正在跑）时丢弃新音频，避免把 TTS 回声收进去。
    {
        let listen = session.listen_state.lock().unwrap();
        if *listen == ListenState::Processing {
            return;
        }
    }

    // 降混为单声道。
    let mono: Vec<f32> = if channels <= 1 {
        data.to_vec()
    } else {
        data.chunks(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect()
    };

    // 简单线性重采样到 16kHz。
    let samples: Vec<f32> = if in_rate == TARGET_SAMPLE_RATE {
        mono
    } else {
        resample_linear(&mono, in_rate, TARGET_SAMPLE_RATE)
    };

    const WINDOW: usize = 512;
    let mut pending = session.pending.lock().unwrap();
    pending.samples.extend_from_slice(&samples);

    // 静音时限制缓冲长度（最多 10 秒），避免无限增长。
    if !pending.speech_started && pending.samples.len() > TARGET_SAMPLE_RATE as usize * 10 {
        let drop_n = pending.samples.len() - TARGET_SAMPLE_RATE as usize * 2;
        pending.samples.drain(..drop_n);
    }

    while pending.samples.len() >= WINDOW {
        let window: Vec<f32> = pending.samples.drain(..WINDOW).collect();
        eng.vad_feed(&window);
        if !pending.speech_started && eng.vad_speech_detected() {
            pending.speech_started = true;
        }
        // 完整语音段就绪 → 送 ASR。
        if let Some(segment) = eng.vad_pop_segment() {
            pending.speech_started = false;
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                run_asr(app, segment).await;
            });
        }
    }
}

/// 后台执行 ASR 并推送结果事件。
async fn run_asr(app: tauri::AppHandle, segment: Vec<f32>) {
    let eng = match engine::engine() {
        Some(e) => e,
        None => return,
    };
    // VAD 段可能仍是其他采样率？不会：VAD 按 16k 配置，段即 16k。
    let text = match eng.transcribe(&segment) {
        Ok(t) => t,
        Err(err) => {
            warn!("[voice] asr failed: {err}");
            return;
        }
    };
    if text.is_empty() {
        return;
    }
    info!("[voice] asr final: {text}");
    use tauri::Emitter;
    let _ = app.emit(
        EVENT_ASR_FINAL,
        serde_json::json!({ "text": text, "durationMs": (segment.len() as f64 / TARGET_SAMPLE_RATE as f64 * 1000.0) as u64 }),
    );
}

/// 线性插值重采样。
fn resample_linear(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let out_len = ((input.len() as f64) / ratio) as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let pos = i as f64 * ratio;
        let idx = pos as usize;
        let frac = (pos - idx as f64) as f32;
        let s0 = input[idx];
        let s1 = input.get(idx + 1).copied().unwrap_or(s0);
        out.push(s0 + (s1 - s0) * frac);
    }
    out
}
