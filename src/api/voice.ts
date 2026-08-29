import { invoke } from "@tauri-apps/api/core";

/** 单个语音模型状态 */
export interface VoiceModelStatus {
  id: string;
  label: string;
  /** not_installed / downloading / ready / failed / loading / engine_failed */
  status: string;
  totalSize: number;
  readyMarkerOk: boolean;
}

/** 下载进度事件载荷（voice://download-progress） */
export interface VoiceDownloadProgress {
  modelId: string;
  label: string;
  status: "downloading" | "extracting" | "ready" | "failed";
  downloaded: number;
  total: number;
  speed: number;
  error?: string | null;
}

/** ASR 最终结果事件载荷（voice://asr-final） */
export interface VoiceAsrFinal {
  text: string;
  durationMs: number;
}

export async function voiceGetModelStatuses(): Promise<VoiceModelStatus[]> {
  return invoke("voice_get_model_statuses");
}

export async function voiceDownloadModel(modelId: string): Promise<void> {
  return invoke("voice_download_model", { modelId });
}

export async function voiceStartupCheck(): Promise<void> {
  return invoke("voice_startup_check");
}

export async function voiceStartListening(): Promise<void> {
  return invoke("voice_start_listening");
}

export async function voiceStopListening(): Promise<void> {
  return invoke("voice_stop_listening");
}

export async function voiceTtsGenerate(
  text: string,
  speed = 1.0,
  sid = 0,
): Promise<{ samples: number[]; sampleRate: number }> {
  return invoke("voice_tts_generate", { request: { text, speed, sid } });
}
