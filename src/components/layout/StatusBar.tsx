import { IconCpu, IconFolder, IconLoader2, IconPlugConnected, IconRefresh } from "@tabler/icons-react";
import { useIntl } from "react-intl";
import { useAppStore } from "../../stores/appStore";

function getLeafName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

export function StatusBar() {
  const intl = useIntl();
  const initialized = useAppStore((state) => state.initialized);
  const initError = useAppStore((state) => state.initError);
  const retryInit = useAppStore((state) => state.retryInit);
  const currentModel = useAppStore((state) => state.currentModel);
  const configDir = useAppStore((state) => state.configDir);
  const workspaceCwd = useAppStore((state) => state.workspaceCwd);
  const smartbrainExtractionRunning = useAppStore((state) => state.smartbrainExtractionRunning);
  const smartbrainExtractionProgress = useAppStore((state) => state.smartbrainExtractionProgress);
  const defaultProvider = intl.formatMessage({ id: "app.defaultProvider" });
  const workspaceName = workspaceCwd ? getLeafName(workspaceCwd) : null;

  const statusDot = initialized
    ? "bg-[var(--accent)]"
    : initError
      ? "bg-[var(--danger)]"
      : "bg-[var(--warning)] animate-pulse";

  const statusLabel = initialized
    ? intl.formatMessage({ id: "status.connected" })
    : initError
      ? intl.formatMessage({ id: "status.initFailed" })
      : intl.formatMessage({ id: "status.initializing" });

  const extractionLabel = smartbrainExtractionProgress
    ? intl.formatMessage(
      { id: "status.smartbrain.extractingProgress" },
      {
        current: Math.min(
          smartbrainExtractionProgress.total,
          Math.max(0, smartbrainExtractionProgress.current),
        ),
        total: smartbrainExtractionProgress.total,
      },
    )
    : intl.formatMessage({ id: "status.smartbrain.extracting" });

  return (
    <div className="flex items-center justify-between border-t border-[var(--border-subtle)] bg-[var(--surface-panel)] px-4 py-1.5 text-[11px]">
      <div className="flex items-center gap-3">
        <span
          className={`flex items-center gap-1.5 ${initError ? "text-[var(--danger)]" : "text-[var(--text-faint)]"}`}
          title={initError ?? undefined}
        >
          <span className={`h-1.5 w-1.5 rounded-full ${statusDot}`} />
          <IconPlugConnected size={12} stroke={1.8} />
          {statusLabel}
        </span>

        {initError && retryInit && (
          <button
            onClick={retryInit}
            className="flex items-center gap-1 rounded px-1.5 py-0.5 text-[var(--accent)] transition-colors hover:bg-[var(--accent-soft)]"
          >
            <IconRefresh size={11} stroke={2} />
            {intl.formatMessage({ id: "status.retry" })}
          </button>
        )}

        <span className="flex items-center gap-1.5 text-[var(--text-faint)]">
          <IconCpu size={12} stroke={1.8} />
          {currentModel ?? defaultProvider}
        </span>

        {smartbrainExtractionRunning && (
          <span className="flex items-center gap-1.5 text-[var(--text-faint)]">
            <IconLoader2 size={12} stroke={1.8} className="animate-spin" />
            {extractionLabel}
          </span>
        )}

        {configDir && (
          <span className="flex items-center gap-1.5 text-[var(--text-faint)]">
            <IconFolder size={12} stroke={1.8} />
            codey
          </span>
        )}
      </div>

      <div className="flex items-center gap-3">
        {workspaceName && (
          <span className="flex items-center gap-1.5 text-[var(--text-faint)]" title={workspaceCwd ?? undefined}>
            <IconFolder size={12} stroke={1.8} />
            <span className="max-w-[16rem] truncate">{workspaceName}</span>
          </span>
        )}
        <span className="text-[var(--text-faint)]">
          v0.1.0
        </span>
      </div>
    </div>
  );
}
