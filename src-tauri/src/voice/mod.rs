//! 语音模式模块。
//!
//! 包含本地模型下载管理、sherpa-onnx CPU 推理引擎、麦克风会话与 Tauri 命令。

pub mod commands;
pub mod downloader;
pub mod engine;
pub mod models;
pub mod session;

pub use commands::VoiceState;
