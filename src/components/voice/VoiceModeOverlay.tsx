import { IconCheck, IconMicrophone, IconSquare, IconX } from "@tabler/icons-react";
import { useIntl } from "react-intl";
import { useVoiceStore } from "../../stores/voiceStore";
import { areModelsReady } from "../../stores/voiceStore";
import { confirmSolve, cancelSolve } from "./VoiceModeController";

/**
 * 语音模式浮层（对话页右下角，聊天输入框上方）：
 * - 闲聊/聆听状态只显示麦克风与声波动画，不显示任何文字（需求 4.2）；
 * - solve 确认卡片展示总结文本 + 确认/取消按钮（需求 4.3）。
 */
export function VoiceModeOverlay() {
  const intl = useIntl();
  const voiceModeOn = useVoiceStore((s) => s.voiceModeOn);
  const setVoiceMode = useVoiceStore((s) => s.setVoiceMode);
  const listenState = useVoiceStore((s) => s.listenState);
  const pendingConfirm = useVoiceStore((s) => s.pendingConfirm);
  const modelStatuses = useVoiceStore((s) => s.modelStatuses);
  const showNotReadyToast = useVoiceStore((s) => s.showNotReadyToast);

  const ready = areModelsReady(modelStatuses);

  const handleToggle = () => {
    if (!ready.asr) {
      showNotReadyToast("asr");
      return;
    }
    void setVoiceMode(!voiceModeOn);
  };

  return (
    <>
      {/* 语音模式开关 + 状态浮层（solve 卡片打开时隐藏开关避免误触） */}
      {!pendingConfirm && (
        <div className="voice-mode-overlay">
          {voiceModeOn && listenState !== "idle" && (
            <div className="voice-wave-wrap" data-state={listenState}>
              <span className="voice-wave-bar" />
              <span className="voice-wave-bar" />
              <span className="voice-wave-bar" />
              <span className="voice-wave-bar" />
              <span className="voice-wave-bar" />
            </div>
          )}
          <button
            className={`voice-mode-toggle ${voiceModeOn ? "is-on" : ""}`}
            onClick={handleToggle}
            title={intl.formatMessage({
              id: voiceModeOn ? "voice.mode.stop" : "voice.mode.start",
            })}
            data-testid="voice-mode-toggle"
          >
            {voiceModeOn ? <IconSquare size={14} stroke={2} /> : <IconMicrophone size={16} stroke={1.8} />}
          </button>
        </div>
      )}

      {/* solve 确认卡片 */}
      {pendingConfirm && (
        <div className="voice-confirm-card" data-testid="voice-confirm-card">
          <div className="voice-confirm-header">
            <span className="voice-confirm-title">
              {intl.formatMessage({ id: "voice.confirm.title" })}
            </span>
            <button className="voice-confirm-close" onClick={cancelSolve}>
              <IconX size={14} stroke={2} />
            </button>
          </div>
          <p className="voice-confirm-summary">{pendingConfirm.summary}</p>
          <div className="voice-confirm-actions">
            <button className="voice-confirm-cancel" onClick={cancelSolve}>
              <IconX size={13} stroke={2} />
              {intl.formatMessage({ id: "voice.confirm.cancel" })}
            </button>
            <button
              className="voice-confirm-send"
              onClick={() => confirmSolve(pendingConfirm.summary, pendingConfirm.ack)}
            >
              <IconCheck size={13} stroke={2} />
              {intl.formatMessage({ id: "voice.confirm.send" })}
            </button>
          </div>
        </div>
      )}
    </>
  );
}
