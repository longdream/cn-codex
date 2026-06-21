import { useRecording } from "../../hooks/useRecording";

function formatTime(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  return `${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")}`;
}

export function RecordingToggle() {
  const {
    state,
    elapsedSeconds,
    lastTrace,
    error,
    handleStart,
    handleStop,
    handleDismiss,
  } = useRecording();

  if (state === "hidden") {
    return null;
  }

  return (
    <div className="recording-toggle">
      {state === "ready" && (
        <div className="recording-toggle-content">
          <div className="recording-toggle-label">录制就绪</div>
          <button
            className="recording-toggle-btn recording-toggle-btn-start"
            onClick={handleStart}
          >
            <span className="recording-dot recording-dot-idle" />
            开始录制
          </button>
          <button
            className="recording-toggle-btn recording-toggle-btn-dismiss"
            onClick={handleDismiss}
          >
            关闭
          </button>
          {error && <div className="recording-toggle-error">{error}</div>}
        </div>
      )}

      {state === "recording" && (
        <div className="recording-toggle-content">
          <div className="recording-toggle-status">
            <span className="recording-dot recording-dot-active" />
            <span className="recording-toggle-timer">
              录制中 {formatTime(elapsedSeconds)}
            </span>
          </div>
          <button
            className="recording-toggle-btn recording-toggle-btn-stop"
            onClick={handleStop}
          >
            停止录制
          </button>
        </div>
      )}

      {state === "processing" && (
        <div className="recording-toggle-content">
          <div className="recording-toggle-status">
            <span className="recording-dot recording-dot-processing" />
            <span>处理中...</span>
          </div>
        </div>
      )}

      {state === "completed" && lastTrace && (
        <div className="recording-toggle-content">
          <div className="recording-toggle-status">
            <span className="recording-dot recording-dot-done" />
            <span>
              录制完成：{lastTrace.events.length} 个操作
            </span>
          </div>
          <button
            className="recording-toggle-btn recording-toggle-btn-dismiss"
            onClick={handleDismiss}
          >
            关闭
          </button>
        </div>
      )}
    </div>
  );
}
