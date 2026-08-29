//! 语音模式模型清单定义。
//!
//! 三个本地模型（ASR / TTS / VAD）均采用 sherpa-onnx CPU int8 版本：
//! - ASR: Qwen3-ASR-0.6B int8（约 0.9 GB，tar.bz2 打包）
//! - TTS: Kokoro-82M int8 多语言版（约 147 MB，GitHub 打包 / ModelScope 目录）
//! - VAD: silero-vad（约 0.6 MB，单 onnx 文件）
//!
//! 下载主源为 ModelScope（国内直连速度快），备用源为 GitHub Release。
//! 下载成功后写入 manifest.json 记录版本与状态，供启动时快速校验。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MANIFEST_FILE: &str = "manifest.json";

/// 单个语音模型的下载源。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceModelSource {
    /// ModelScope 主源直链（文件或目录 API 列表根）。
    pub modelscope: String,
    /// GitHub Release 备用源。
    pub github: Option<String>,
}

/// 单个语音模型的定义。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceModelDef {
    /// 模型标识：asr / tts / vad。
    pub id: String,
    /// 展示名称。
    pub label: String,
    /// 安装目录名（位于 voice-models/ 下）。
    pub dir: String,
    /// 下载类型：archive = tar.bz2 打包解压；single = 单文件；manifest = 目录清单批量下载。
    pub kind: String,
    /// 模型总大小（字节，估算值，用于进度展示）。
    pub total_size: u64,
    /// 下载源列表（主源在前）。
    pub sources: Vec<VoiceModelSource>,
    /// manifest 下载模式下要拉取的文件相对路径列表（含 espeak-ng-data 等）。
    #[serde(default)]
    pub files: Vec<String>,
    /// 就绪校验文件（存在即认为模型完整，不含 manifest.json）。
    pub ready_marker: String,
}

/// 语音模型清单（持久化到 voice-models/manifest.json）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VoiceModelsManifest {
    /// 各模型状态：pending / downloading / ready / failed。
    pub models: std::collections::HashMap<String, VoiceModelStatusEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceModelStatusEntry {
    pub status: String,
    /// 完成时间（unix 毫秒）。
    pub ready_at: Option<u64>,
    /// 使用的下载源。
    pub source_used: Option<String>,
}

impl VoiceModelsManifest {
    pub fn load(models_dir: &std::path::Path) -> Self {
        let path = models_dir.join(MANIFEST_FILE);
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(m) = serde_json::from_str::<VoiceModelsManifest>(&content) {
                return m;
            }
        }
        Self::default()
    }

    pub fn save(&self, models_dir: &std::path::Path) -> std::io::Result<()> {
        std::fs::create_dir_all(models_dir)?;
        let path = models_dir.join(MANIFEST_FILE);
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, json)
    }

    pub fn set_status(&mut self, id: &str, status: &str, source: Option<&str>) {
        let entry = VoiceModelStatusEntry {
            status: status.to_string(),
            ready_at: if status == "ready" { Some(now_ms()) } else { None },
            source_used: source.map(|s| s.to_string()),
        };
        self.models.insert(id.to_string(), entry);
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// ModelScope 仓库文件清单 API 响应结构（仅取所需字段）。
#[derive(Debug, Deserialize)]
pub struct ModelScopeFileList {
    pub data: Option<ModelScopeFileListData>,
}

#[derive(Debug, Deserialize)]
pub struct ModelScopeFileListData {
    pub files: Option<Vec<ModelScopeFileEntry>>,
}

#[derive(Debug, Deserialize)]
pub struct ModelScopeFileEntry {
    pub path: String,
    #[serde(default)]
    pub size: u64,
    #[serde(rename = "Type", default)]
    pub entry_type: String,
}

/// 构建三个内置模型定义。清单文件列表在运行时从 ModelScope API 拉取，
/// 这里只登记静态已知的关键文件；目录类文件（espeak-ng-data/dict）
/// 通过 `?Recursive=true` API 动态获取。
pub fn builtin_model_defs() -> Vec<VoiceModelDef> {
    vec![
        VoiceModelDef {
            id: "asr".to_string(),
            label: "Qwen3-ASR-0.6B (int8)".to_string(),
            dir: "asr-qwen3-0.6b-int8".to_string(),
            kind: "archive".to_string(),
            total_size: 875_028_824,
            sources: vec![
                VoiceModelSource {
                    modelscope: "https://modelscope.cn/models/zhaochaoqun/sherpa-onnx-asr-models/resolve/master/Qwen3-ASR-0.6B.tar.bz2".to_string(),
                    github: Some("https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25.tar.bz2".to_string()),
                },
            ],
            files: vec![],
            ready_marker: "encoder.int8.onnx".to_string(),
        },
        VoiceModelDef {
            id: "tts".to_string(),
            label: "Kokoro-82M int8 (multi-lang)".to_string(),
            dir: "tts-kokoro-int8".to_string(),
            kind: "manifest".to_string(),
            total_size: 148_000_000,
            sources: vec![
                VoiceModelSource {
                    modelscope: "https://modelscope.cn/models/gomodels/sherpa/resolve/master".to_string(),
                    github: Some("https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/kokoro-int8-multi-lang-v1_1.tar.bz2".to_string()),
                },
            ],
            files: vec![
                "kokoro-int8-multi-lang-v1_1/model.int8.onnx".to_string(),
                "kokoro-int8-multi-lang-v1_1/voices.bin".to_string(),
                "kokoro-int8-multi-lang-v1_1/tokens.txt".to_string(),
                "kokoro-int8-multi-lang-v1_1/lexicon-zh.txt".to_string(),
                "kokoro-int8-multi-lang-v1_1/lexicon-us-en.txt".to_string(),
                "kokoro-int8-multi-lang-v1_1/lexicon-gb-en.txt".to_string(),
                "kokoro-int8-multi-lang-v1_1/dict".to_string(),
                "kokoro-int8-multi-lang-v1_1/espeak-ng-data".to_string(),
                "kokoro-int8-multi-lang-v1_1/date-zh.fst".to_string(),
                "kokoro-int8-multi-lang-v1_1/number-zh.fst".to_string(),
                "kokoro-int8-multi-lang-v1_1/phone-zh.fst".to_string(),
            ],
            ready_marker: "model.int8.onnx".to_string(),
        },
        VoiceModelDef {
            id: "vad".to_string(),
            label: "Silero VAD".to_string(),
            dir: "vad".to_string(),
            kind: "single".to_string(),
            total_size: 643_854,
            sources: vec![
                VoiceModelSource {
                    modelscope: "https://modelscope.cn/models/gomodels/sherpa/resolve/master/vad/silero_vad.onnx".to_string(),
                    github: Some("https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx".to_string()),
                },
            ],
            files: vec![],
            ready_marker: "silero_vad.onnx".to_string(),
        },
    ]
}

/// 返回 voice-models 根目录。
pub fn voice_models_root(workspace_config_dir: &std::path::Path) -> PathBuf {
    workspace_config_dir.join("voice-models")
}

/// 判断某模型是否就绪（ready_marker 存在 + manifest 标记 ready）。
pub fn is_model_ready(models_dir: &std::path::Path, def: &VoiceModelDef) -> bool {
    let marker = models_dir.join(&def.dir).join(&def.ready_marker);
    if !marker.exists() {
        return false;
    }
    // ASR 的 encoder 可能是 int8 或非 int8 命名，做一次兜底扫描。
    if def.id == "asr" && !marker.exists() {
        let dir = models_dir.join(&def.dir);
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with("encoder") && name.ends_with(".onnx") {
                    return true;
                }
            }
        }
        return false;
    }
    true
}
