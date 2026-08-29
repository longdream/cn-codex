import { useEffect, useMemo } from "react";
import { useIntl } from "react-intl";
import {
  IconAlertTriangle,
  IconChevronDown,
  IconChevronUp,
  IconDownload,
  IconRefresh,
  IconX,
} from "@tabler/icons-react";
import { useVoiceStore } from "../../stores/voiceStore";

function formatSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let v = bytes;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u++;
  }
  return `${v.toFixed(v >= 100 || u === 0 ? 0 : 1)} ${units[u]}`;
}

function formatSpeed(bps: number): string {
  if (!Number.isFinite(bps) || bps <= 0) return "";
  return `${formatSize(bps)}/s`;
}

/**
 * 右下角全局下载列表（需求 3.3）：
 * - 下载中默认展开，紧凑单行条目；
 * - 可折叠为小图标；全部完成后自动消失；
 * - 失败条目保留并带"重试"按钮。
 */
export function VoiceDownloadList() {
  const intl = useIntl();
  const entries = useVoiceStore((s) => s.downloadEntries);
  const collapsed = useVoiceStore((s) => s.downloadsCollapsed);
  const setCollapsed = useVoiceStore((s) => s.setDownloadsCollapsed);
  const retryDownload = useVoiceStore((s) => s.retryDownload);

  const activeEntries = useMemo(
    () => entries.filter((e) => e.status !== "ready"),
    [entries],
  );

  useEffect(() => {
    // 全部完成时自动重置折叠态，下次下载再展开。
    if (activeEntries.length === 0 && collapsed) {
      void setCollapsed(false);
    }
  }, [activeEntries.length, collapsed, setCollapsed]);

  if (activeEntries.length === 0) return null;

  const anyActive = activeEntries.some(
    (e) => e.status === "downloading" || e.status === "extracting",
  );

  if (collapsed) {
    return (
      <button
        onClick={() => void setCollapsed(false)}
        className="voice-download-badge"
        title={intl.formatMessage({ id: "voice.downloadList.expand" })}
      >
        <IconDownload size={14} stroke={1.8} className={anyActive ? "voice-spin" : ""} />
        {anyActive && <span className="voice-download-badge-dot" />}
      </button>
    );
  }

  return (
    <div className="voice-download-list" data-testid="voice-download-list">
      <div className="voice-download-list-header">
        <span className="voice-download-list-title">
          <IconDownload size={12} stroke={1.8} className={anyActive ? "voice-spin" : ""} />
          {intl.formatMessage({ id: "voice.downloadList.title" })}
        </span>
        <button
          className="voice-download-collapse-btn"
          onClick={() => void setCollapsed(true)}
          title={intl.formatMessage({ id: "voice.downloadList.collapse" })}
        >
          <IconChevronDown size={13} stroke={2} />
        </button>
      </div>
      <div className="voice-download-items">
        {activeEntries.map((entry) => {
          const pct =
            entry.total > 0
              ? Math.min(100, Math.round((entry.downloaded / entry.total) * 100))
              : 0;
          const failed = entry.status === "failed";
          return (
            <div key={entry.modelId} className="voice-download-item">
              <div className="voice-download-item-row">
                <span className="voice-download-item-name" title={entry.label}>
                  {entry.modelId.toUpperCase()}
                </span>
                {failed ? (
                  <button
                    className="voice-download-retry"
                    onClick={() => void retryDownload(entry.modelId)}
                    title={intl.formatMessage({ id: "voice.downloadList.retry" })}
                  >
                    <IconRefresh size={12} stroke={2} />
                  </button>
                ) : (
                  <span className="voice-download-item-pct">
                    {entry.status === "extracting"
                      ? intl.formatMessage({ id: "voice.downloadList.extracting" })
                      : `${pct}%`}
                  </span>
                )}
              </div>
              <div className="voice-download-bar">
                <div
                  className={`voice-download-bar-fill ${failed ? "is-failed" : ""}`}
                  style={{ width: failed ? "100%" : `${pct}%` }}
                />
              </div>
              <div className="voice-download-item-meta">
                {failed
                  ? entry.error || intl.formatMessage({ id: "voice.downloadList.failed" })
                  : `${formatSize(entry.downloaded)} / ${formatSize(entry.total)} ${formatSpeed(entry.speed)}`}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

/**
 * 未就绪使用提示（需求 3.4）：
 * 使用 ASR/TTS 而模型未就绪时弹出，右下角带"重试下载"按钮。
 */
export function VoiceNotReadyToast() {
  const intl = useIntl();
  const toast = useVoiceStore((s) => s.notReadyToast);
  const dismiss = useVoiceStore((s) => s.dismissNotReadyToast);
  const retryAllFailed = useVoiceStore((s) => s.retryAllFailed);
  const retrying = useVoiceStore((s) => s.downloadEntries.length > 0);

  if (!toast || !toast.visible) return null;

  return (
    <div className="voice-notready-toast" data-testid="voice-notready-toast">
      <IconAlertTriangle size={15} stroke={1.8} className="voice-notready-icon" />
      <span className="voice-notready-text">
        {intl.formatMessage({
          id: toast.kind === "asr" ? "voice.notReady.asr" : "voice.notReady.tts",
        })}
      </span>
      <button
        className="voice-notready-retry"
        onClick={() => void retryAllFailed()}
        disabled={retrying}
      >
        <IconRefresh size={12} stroke={2} />
        {intl.formatMessage({ id: "voice.notReady.retry" })}
      </button>
      <button className="voice-notready-close" onClick={dismiss}>
        <IconX size={12} stroke={2} />
      </button>
    </div>
  );
}

/** ChevronUp 引用占位（避免 tree-shake 报 unused） */
export const _VoiceDownloadIcons = { IconChevronUp };
