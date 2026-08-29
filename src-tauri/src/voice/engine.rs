//! sherpa-onnx 本地推理引擎封装（纯 CPU）。
//!
//! - ASR: Qwen3-ASR-0.6B int8 离线识别
//! - TTS: Kokoro-82M int8 多语言合成
//! - VAD: silero-vad 语音活动检测
//!
//! 引擎在模型就绪后按需惰性创建并常驻内存，通过 Arc 在多命令间共享。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use tracing::{info, warn};

use super::models::{self, VoiceModelDef};

/// 引擎全局单例。
static ENGINE: OnceLock<VoiceEngine> = OnceLock::new();

pub fn engine() -> Option<&'static VoiceEngine> {
    ENGINE.get()
}

/// 初始化引擎（启动时调用一次；模型未就绪时各子引擎保持未加载）。
pub fn init_engine(models_dir: PathBuf) -> &'static VoiceEngine {
    ENGINE.get_or_init(|| VoiceEngine::new(models_dir))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SubEngineState {
    NotReady,
    Loading,
    Ready,
    Failed,
}

pub struct VoiceEngine {
    models_dir: PathBuf,
    asr_state: Mutex<SubEngineState>,
    tts_state: Mutex<SubEngineState>,
    vad_state: Mutex<SubEngineState>,
    asr: Mutex<Option<sherpa_onnx::OfflineRecognizer>>,
    tts: Mutex<Option<sherpa_onnx::OfflineTts>>,
    vad: Mutex<Option<sherpa_onnx::VoiceActivityDetector>>,
}

impl VoiceEngine {
    fn new(models_dir: PathBuf) -> Self {
        Self {
            models_dir,
            asr_state: Mutex::new(SubEngineState::NotReady),
            tts_state: Mutex::new(SubEngineState::NotReady),
            vad_state: Mutex::new(SubEngineState::NotReady),
            asr: Mutex::new(None),
            tts: Mutex::new(None),
            vad: Mutex::new(None),
        }
    }

    pub fn models_dir(&self) -> &Path {
        &self.models_dir
    }

    pub fn state_of(&self, id: &str) -> SubEngineState {
        match id {
            "asr" => *self.asr_state.lock().unwrap(),
            "tts" => *self.tts_state.lock().unwrap(),
            "vad" => *self.vad_state.lock().unwrap(),
            _ => SubEngineState::NotReady,
        }
    }

    fn set_state(&self, id: &str, state: SubEngineState) {
        match id {
            "asr" => *self.asr_state.lock().unwrap() = state,
            "tts" => *self.tts_state.lock().unwrap() = state,
            "vad" => *self.vad_state.lock().unwrap() = state,
            _ => {}
        }
    }

    /// 模型就绪后加载 ASR 引擎（CPU，2 线程）。
    pub fn load_asr(&self, def: &VoiceModelDef) -> Result<(), String> {
        let mut guard = self.asr.lock().unwrap();
        if guard.is_some() {
            return Ok(());
        }
        self.set_state("asr", SubEngineState::Loading);
        let dir = self.models_dir.join(&def.dir);
        // 解包目录可能带顶层前缀，做一次拍平。
        models::is_model_ready(&self.models_dir, def);
        super::downloader::flatten_archive_dir(&dir, &def.ready_marker);

        let find = |names: &[&str]| -> Option<PathBuf> {
            for name in names {
                let p = dir.join(name);
                if p.exists() {
                    return Some(p);
                }
            }
            // 兜底：按前缀扫描。
            let prefix = names[0].split('.').next().unwrap_or("");
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for e in entries.flatten() {
                    let n = e.file_name().to_string_lossy().to_string();
                    if n.starts_with(prefix) && n.ends_with(".onnx") {
                        return Some(e.path());
                    }
                }
            }
            None
        };

        let conv = find(&["conv_frontend.onnx"]).ok_or("conv_frontend.onnx missing")?;
        let encoder = find(&["encoder.int8.onnx", "encoder.onnx"])
            .ok_or("encoder onnx missing")?;
        let decoder = find(&["decoder.int8.onnx", "decoder.onnx"])
            .ok_or("decoder onnx missing")?;
        let tokenizer = dir.join("tokenizer").join("Qwen3-ASR");
        let tokenizer = if tokenizer.exists() {
            tokenizer
        } else {
            dir.join("tokenizer")
        };
        if !tokenizer.exists() {
            self.set_state("asr", SubEngineState::Failed);
            return Err("tokenizer dir missing".to_string());
        }

        let mut config = sherpa_onnx::OfflineRecognizerConfig::default();
        config.model_config.qwen3_asr = sherpa_onnx::OfflineQwen3ASRModelConfig {
            conv_frontend: Some(conv.to_string_lossy().to_string()),
            encoder: Some(encoder.to_string_lossy().to_string()),
            decoder: Some(decoder.to_string_lossy().to_string()),
            tokenizer: Some(tokenizer.to_string_lossy().to_string()),
            ..Default::default()
        };
        config.model_config.tokens = Some(String::new());
        config.model_config.provider = Some("cpu".to_string());
        config.model_config.num_threads = 2;

        match sherpa_onnx::OfflineRecognizer::create(&config) {
            Some(recognizer) => {
                *guard = Some(recognizer);
                self.set_state("asr", SubEngineState::Ready);
                info!("[voice] ASR engine loaded (cpu)");
                Ok(())
            }
            None => {
                self.set_state("asr", SubEngineState::Failed);
                Err("ASR engine create failed".to_string())
            }
        }
    }

    /// 加载 TTS 引擎（Kokoro int8，CPU）。
    pub fn load_tts(&self, def: &VoiceModelDef) -> Result<(), String> {
        let mut guard = self.tts.lock().unwrap();
        if guard.is_some() {
            return Ok(());
        }
        self.set_state("tts", SubEngineState::Loading);
        let dir = self.models_dir.join(&def.dir);
        let model = dir.join("model.int8.onnx");
        let voices = dir.join("voices.bin");
        let tokens = dir.join("tokens.txt");
        let data_dir = dir.join("espeak-ng-data");
        let dict_dir = dir.join("dict");
        let lexicon = dir.join("lexicon-zh.txt");
        for (name, p) in [
            ("model.int8.onnx", &model),
            ("voices.bin", &voices),
            ("tokens.txt", &tokens),
            ("espeak-ng-data", &data_dir),
        ] {
            if !p.exists() {
                self.set_state("tts", SubEngineState::Failed);
                return Err(format!("{name} missing"));
            }
        }

        let mut config = sherpa_onnx::OfflineTtsConfig::default();
        config.model.kokoro = sherpa_onnx::OfflineTtsKokoroModelConfig {
            model: Some(model.to_string_lossy().to_string()),
            voices: Some(voices.to_string_lossy().to_string()),
            tokens: Some(tokens.to_string_lossy().to_string()),
            data_dir: Some(data_dir.to_string_lossy().to_string()),
            dict_dir: if dict_dir.exists() {
                Some(dict_dir.to_string_lossy().to_string())
            } else {
                None
            },
            lexicon: if lexicon.exists() {
                Some(lexicon.to_string_lossy().to_string())
            } else {
                None
            },
            lang: Some("zh".to_string()),
            ..Default::default()
        };
        config.max_num_sentences = 2;

        match sherpa_onnx::OfflineTts::create(&config) {
            Some(tts) => {
                *guard = Some(tts);
                self.set_state("tts", SubEngineState::Ready);
                info!("[voice] TTS engine loaded (cpu)");
                Ok(())
            }
            None => {
                self.set_state("tts", SubEngineState::Failed);
                Err("TTS engine create failed".to_string())
            }
        }
    }

    /// 加载 VAD（silero）。
    pub fn load_vad(&self, def: &VoiceModelDef) -> Result<(), String> {
        let mut guard = self.vad.lock().unwrap();
        if guard.is_some() {
            return Ok(());
        }
        self.set_state("vad", SubEngineState::Loading);
        let model = self.models_dir.join(&def.dir).join(&def.ready_marker);
        if !model.exists() {
            self.set_state("vad", SubEngineState::Failed);
            return Err("silero_vad.onnx missing".to_string());
        }
        let config = sherpa_onnx::VadModelConfig {
            silero_vad: sherpa_onnx::SileroVadModelConfig {
                model: Some(model.to_string_lossy().to_string()),
                threshold: 0.5,
                min_silence_duration: 0.45,
                min_speech_duration: 0.2,
                window_size: 512,
                max_speech_duration: 20.0,
            },
            ten_vad: sherpa_onnx::TenVadModelConfig::default(),
            sample_rate: 16000,
            num_threads: 1,
            provider: Some("cpu".to_string()),
            debug: false,
        };
        match sherpa_onnx::VoiceActivityDetector::create(&config, 60.0) {
            Some(vad) => {
                *guard = Some(vad);
                self.set_state("vad", SubEngineState::Ready);
                info!("[voice] VAD engine loaded (cpu)");
                Ok(())
            }
            None => {
                self.set_state("vad", SubEngineState::Failed);
                Err("VAD engine create failed".to_string())
            }
        }
    }

    /// 同步识别一段 16kHz 单声道 PCM。
    pub fn transcribe(&self, samples: &[f32]) -> Result<String, String> {
        let guard = self.asr.lock().unwrap();
        let recognizer = guard.as_ref().ok_or("ASR not ready")?;
        let stream = recognizer.create_stream();
        stream.accept_waveform(16000, samples);
        recognizer.decode(&stream);
        match stream.get_result() {
            Some(result) => Ok(result.text.trim().to_string()),
            None => Ok(String::new()),
        }
    }

    /// 合成语音，返回 (samples, sample_rate)。speed: 语速倍率。
    pub fn synthesize(&self, text: &str, speed: f32, sid: i32) -> Result<(Vec<f32>, i32), String> {
        let guard = self.tts.lock().unwrap();
        let tts = guard.as_ref().ok_or("TTS not ready")?;
        let gen_cfg = sherpa_onnx::GenerationConfig {
            speed,
            sid,
            ..Default::default()
        };
        let audio = tts
            .generate_with_config(text, &gen_cfg, Option::<fn(&[f32], f32) -> bool>::None)
            .ok_or("tts generate failed")?;
        Ok((audio.samples().to_vec(), audio.sample_rate()))
    }

    /// 喂入一段 PCM 给 VAD；返回本段是否结束（有完整语音段产出）。
    pub fn vad_feed(&self, samples: &[f32]) -> bool {
        let guard = self.vad.lock().unwrap();
        let vad = match guard.as_ref() {
            Some(v) => v,
            None => return false,
        };
        vad.accept_waveform(samples);
        !vad.is_empty()
    }

    pub fn vad_speech_detected(&self) -> bool {
        let guard = self.vad.lock().unwrap();
        guard.as_ref().map(|v| v.detected()).unwrap_or(false)
    }

    /// 取出完整语音段。
    pub fn vad_pop_segment(&self) -> Option<Vec<f32>> {
        let guard = self.vad.lock().unwrap();
        let vad = guard.as_ref()?;
        if vad.is_empty() {
            return None;
        }
        let segment = vad.front()?;
        vad.pop();
        Some(segment.samples().to_vec())
    }

    pub fn vad_reset(&self) {
        let guard = self.vad.lock().unwrap();
        if let Some(v) = guard.as_ref() {
            v.reset();
        }
    }

    pub fn vad_clear(&self) {
        let guard = self.vad.lock().unwrap();
        if let Some(v) = guard.as_ref() {
            v.clear();
        }
    }

    /// 就绪后自动加载全部可用引擎（模型文件已存在时）。
    pub fn autoload(&self, defs: &[VoiceModelDef]) {
        for def in defs {
            let state = match self.state_of(&def.id) {
                SubEngineState::Ready => continue,
                s => s,
            };
            if state == SubEngineState::Failed {
                continue;
            }
            let result = match def.id.as_str() {
                "asr" => self.load_asr(def),
                "tts" => self.load_tts(def),
                "vad" => self.load_vad(def),
                _ => Ok(()),
            };
            if let Err(err) = result {
                warn!("[voice] autoload {} failed: {err}", def.id);
            }
        }
    }
}

/// 便捷函数：写 WAV 文件供调试（release 不使用）。
#[allow(dead_code)]
pub fn write_wav_debug(path: &Path, samples: &[f32], sample_rate: i32) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = sherpa_onnx::write(&path.to_string_lossy(), samples, sample_rate);
}
