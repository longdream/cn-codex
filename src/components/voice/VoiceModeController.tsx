/**
 * 语音模式核心控制器（无 UI）。
 *
 * 职责：
 * - 应用启动时触发模型自动检查/下载（需求 3.1）；
 * - 监听 ASR 结果事件 → 意图识别（纯 LLM）→ 分发 chat/solve（需求 4.x）；
 * - chat：TTS 播放回复，不写对话界面；
 * - solve：弹出确认卡片，用户确认后 summary 注入主链路、同时 TTS 播放 ack。
 */
import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useIntl } from "react-intl";
import { voiceStartListening, voiceStopListening, voiceTtsGenerate, voiceDownloadModel } from "../../api/voice";
import { useVoiceStore } from "../../stores/voiceStore";
import { useAppStore } from "../../stores/appStore";
import { classifyVoiceIntent } from "../../utils/voiceIntent";

/**
 * 确认卡片打开期间的语音回答 → 纯 LLM 判定 confirm/cancel。
 * 不使用正则或关键词规则。
 */
async function classifyConfirmDecision(
  text: string,
): Promise<"confirm" | "cancel" | null> {
  const { invoke } = await import("@tauri-apps/api/core");
  const { resolveFortuneLlmConfig } = await import("../../utils/fortune");
  const store = useAppStore.getState();
  const resolved = resolveFortuneLlmConfig({
    provider: store.getActiveProvider(),
    activeModel: store.getActiveModel(),
    currentModel: store.currentModel,
    activeEndpointIndex: store.activeEndpointIndex,
  });
  const system =
    '用户看到一张"确认发送任务总结"的卡片，现口头回答。判断回答是确认还是取消。只输出 JSON：{"decision":"confirm"} 或 {"decision":"cancel"}。';
  try {
    const raw = await invoke<string>("fortune_llm_call", {
      baseUrl: resolved.baseUrl,
      apiKey: resolved.apiKey,
      model: resolved.modelName,
      wireApi: resolved.wireApi,
      prompt: text,
      systemPrompt: system,
    });
    const start = raw.indexOf("{");
    const end = raw.lastIndexOf("}");
    if (start !== -1 && end > start) {
      const obj = JSON.parse(raw.slice(start, end + 1));
      if (obj?.decision === "confirm") return "confirm";
      if (obj?.decision === "cancel") return "cancel";
    }
    return null;
  } catch (err) {
    console.error("[voice] confirm decision llm failed:", err);
    return null;
  }
}

const EVENT_ASR_FINAL = "voice://asr-final";

interface AsrFinalPayload {
  text: string;
  durationMs: number;
}

/** 播放 TTS（WebAudio），返回 stop 函数。 */
async function playTts(
  text: string,
  speed: number,
  sid: number,
  onEnd?: () => void,
): Promise<() => void> {
  const audioCtx = new AudioContext();
  let stopped = false;
  let source: AudioBufferSourceNode | null = null;
  const stop = () => {
    if (stopped) return;
    stopped = true;
    try {
      source?.stop();
    } catch {
      // already stopped
    }
    void audioCtx.close();
  };
  try {
    const { samples, sampleRate } = await voiceTtsGenerate(text, speed, sid);
    if (stopped) return stop;
    const buffer = audioCtx.createBuffer(1, samples.length, sampleRate);
    const channel = buffer.getChannelData(0);
    channel.set(samples);
    source = audioCtx.createBufferSource();
    source.buffer = buffer;
    source.connect(audioCtx.destination);
    source.onended = () => {
      void audioCtx.close();
      onEnd?.();
    };
    source.start();
  } catch (err) {
    console.error("[voice] tts play failed:", err);
    void audioCtx.close();
    onEnd?.();
  }
  return stop;
}

export function VoiceModeController() {
  const intl = useIntl();
  const voiceModeOn = useVoiceStore((s) => s.voiceModeOn);
  const pendingConfirm = useVoiceStore((s) => s.pendingConfirm);
  const persona = useVoiceStore((s) => s.persona);

  // 1. 启动检查（触发 Rust 侧自动下载）。
  useEffect(() => {
    void useVoiceStore.getState().runStartupCheck();
  }, []);

  // 2. ASR 结果 → 意图识别 → 分发。
  useEffect(() => {
    let disposed = false;
    let stopTts: (() => void) | null = null;
    let unlisten: (() => void) | null = null;

    void listen<AsrFinalPayload>(EVENT_ASR_FINAL, (event) => {
      const payload = event.payload;
      void handleAsrFinal(payload);
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });

    const handleAsrFinal = async (payload: AsrFinalPayload) => {
      const store = useVoiceStore.getState();
      const text = payload.text.trim();
      if (!text) return;

      // solve 确认卡片打开期间：把语音输入交给 LLM 判定确认/取消意图。
      const confirm = store.pendingConfirm;
      if (confirm) {
        await handleConfirmCardVoiceReply(text);
        return;
      }

      store.setListenState("processing");
      try {
        const result = await classifyVoiceIntent(
          text,
          store.persona,
          store.chatHistory,
        );
        const s2 = useVoiceStore.getState();
        if (result.intent === "chat" && result.reply) {
          // 闲聊：仅语音回复，不写入对话界面。
          s2.appendChatHistory("user", text);
          s2.appendChatHistory("assistant", result.reply);
          s2.setListenState("idle");
          stopTts = await playTts(
            result.reply,
            s2.persona.ttsSpeed,
            s2.persona.ttsSid,
            () => {
              useVoiceStore.getState().setListenState(
                useVoiceStore.getState().voiceModeOn ? "listening" : "idle",
              );
            },
          );
        } else if (result.intent === "solve") {
          // 解决问题：弹出确认卡片（summary + LLM ack）。
          s2.setPendingConfirm({
            id: crypto.randomUUID(),
            rawText: text,
            summary: result.summary ?? text,
            ack: result.ack ?? "",
          });
          s2.setListenState("idle");
        }
      } catch (err) {
        console.error("[voice] handle asr final failed:", err);
        useVoiceStore.getState().setListenState("idle");
      }
    };

    const handleConfirmCardVoiceReply = async (text: string) => {
      const store = useVoiceStore.getState();
      const confirm = store.pendingConfirm;
      if (!confirm) return;
      try {
        // 复用纯 LLM 意图管线判定确认/取消（无正则关键词）。
        const decision = await classifyConfirmDecision(text);
        if (decision === "confirm") {
          confirmSolve(confirm.summary, confirm.ack);
        } else if (decision === "cancel") {
          useVoiceStore.getState().setPendingConfirm(null);
        }
      } catch (err) {
        console.error("[voice] confirm voice reply failed:", err);
      }
    };

    return () => {
      disposed = true;
      unlisten?.();
      stopTts?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [persona]);

  // 3. 语音模式开关 → 启停监听。
  useEffect(() => {
    const store = useVoiceStore.getState();
    if (voiceModeOn) {
      const statuses = store.modelStatuses;
      if (statuses["asr"] !== "ready") {
        store.showNotReadyToast("asr");
        void voiceDownloadModel("asr").catch(() => {});
        void store.setVoiceMode(false);
        return;
      }
      voiceStartListening()
        .then(() => store.setListenState("listening"))
        .catch((err) => {
          console.error("[voice] start listening failed:", err);
          const msg = String(err);
          if (msg.includes("VAD_NOT_READY")) {
            store.showNotReadyToast("asr");
          } else {
            store.showNotReadyToast("asr");
          }
          void store.setVoiceMode(false);
        });
    } else {
      voiceStopListening().catch(() => {});
      store.setListenState("idle");
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [voiceModeOn]);

  if (pendingConfirm) {
    console.debug(intl.formatMessage({ id: "voice.confirm.title" }));
  }

  return null;
}

/** solve 确认：summary 注入主链路 + 同时 TTS 播放 ack。 */
export function confirmSolve(summary: string, ack: string) {
  const voiceStore = useVoiceStore.getState();
  voiceStore.setPendingConfirm(null);

  // 注入主链路：复用 requestChatSend 机制（ChatPage 监听后走 handleSend）。
  useAppStore.getState().requestChatSend(summary, "chat");

  // 同时播放 ack（LLM 生成的开始任务话术；为空时静默）。
  if (ack.trim()) {
    void playTts(ack, voiceStore.persona.ttsSpeed, voiceStore.persona.ttsSid).catch(() => {});
  }
}

export function cancelSolve() {
  useVoiceStore.getState().setPendingConfirm(null);
}
