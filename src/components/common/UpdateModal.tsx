import { IconDownload, IconRefresh, IconX } from "@tabler/icons-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useIntl } from "react-intl";
import { updateCheck, updateStart, type UpdateCheckResult } from "../../api/update";
import { useAppStore } from "../../stores/appStore";

export function UpdateModal() {
  const intl = useIntl();
  const initialized = useAppStore((s) => s.initialized);
  const [info, setInfo] = useState<UpdateCheckResult | null>(null);
  const [visible, setVisible] = useState(false);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const checkedRef = useRef(false);

  useEffect(() => {
    if (!initialized || checkedRef.current) {
      return;
    }
    checkedRef.current = true;

    void (async () => {
      try {
        const result = await updateCheck();
        if (result.updateAvailable && result.url) {
          setInfo(result);
          setVisible(true);
        }
      } catch (err) {
        // Auto-update is best-effort and should never block app startup.
        console.warn("[update] check failed:", err);
      }
    })();
  }, [initialized]);

  const handleLater = useCallback(() => {
    setVisible(false);
  }, []);

  const handleUpdate = useCallback(async () => {
    if (!info?.url) {
      return;
    }
    setStarting(true);
    setError(null);
    try {
      await updateStart(info.url, info.sha256);
      // Main process will exit shortly after updater starts.
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setError(message);
      setStarting(false);
    }
  }, [info]);

  if (!visible || !info) {
    return null;
  }

  return (
    <div className="fixed bottom-0 left-0 right-0 top-8 z-50 flex items-center justify-center bg-black/50 p-4 backdrop-blur-sm">
      <div className="app-shell-panel w-full max-w-lg overflow-hidden">
        <div className="flex items-center justify-between gap-3 border-b border-[var(--border-subtle)] px-5 py-3">
          <div className="flex items-center gap-3">
            <div className="flex h-8 w-8 items-center justify-center rounded-[var(--radius-sm)] bg-[rgba(79,140,255,0.12)] text-[var(--accent)]">
              <IconRefresh size={16} stroke={2} />
            </div>
            <div>
              <p className="text-[11px] font-medium uppercase tracking-wide text-[var(--text-faint)]">
                {intl.formatMessage({ id: "update.badge" })}
              </p>
              <h3 className="text-sm font-semibold text-[var(--text-strong)]">
                {intl.formatMessage({ id: "update.title" })}
              </h3>
            </div>
          </div>
          <button
            type="button"
            onClick={handleLater}
            className="rounded-full p-1 text-[var(--text-faint)] transition-colors hover:bg-white/10 hover:text-[var(--text-muted)]"
            aria-label={intl.formatMessage({ id: "common.close" })}
          >
            <IconX size={16} stroke={2} />
          </button>
        </div>

        <div className="space-y-3 px-5 py-4">
          <p className="text-sm text-[var(--text-base)]">
            {intl.formatMessage(
              { id: "update.description" },
              {
                current: info.currentVersion,
                latest: info.latestVersion,
              },
            )}
          </p>

          {info.notes?.trim() ? (
            <div className="rounded-[var(--radius-md)] border border-[var(--border-subtle)] bg-[var(--surface-contrast)] p-3">
              <p className="mb-1 text-[11px] font-medium uppercase tracking-wide text-[var(--text-faint)]">
                {intl.formatMessage({ id: "update.notes" })}
              </p>
              <p className="whitespace-pre-wrap text-sm text-[var(--text-muted)]">
                {info.notes}
              </p>
            </div>
          ) : null}

          <p className="break-all text-xs text-[var(--text-faint)]">
            {intl.formatMessage({ id: "update.url" })}: {info.url}
          </p>

          {error ? (
            <p className="rounded-[var(--radius-sm)] border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-300">
              {error}
            </p>
          ) : null}
        </div>

        <div className="flex items-center justify-end gap-3 border-t border-[var(--border-subtle)] px-5 py-3">
          <button
            type="button"
            onClick={handleLater}
            disabled={starting}
            className="secondary-button px-4"
          >
            {intl.formatMessage({ id: "update.later" })}
          </button>
          <button
            type="button"
            onClick={handleUpdate}
            disabled={starting}
            className="primary-button flex items-center gap-2 px-4"
          >
            <IconDownload size={16} stroke={1.8} />
            {starting
              ? intl.formatMessage({ id: "update.starting" })
              : intl.formatMessage({ id: "update.now" })}
          </button>
        </div>
      </div>
    </div>
  );
}
