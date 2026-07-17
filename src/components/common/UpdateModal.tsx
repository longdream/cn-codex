import {
  IconArrowRight,
  IconDownload,
  IconLoader2,
  IconRocket,
  IconShieldCheck,
  IconSparkles,
  IconX,
} from "@tabler/icons-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useIntl } from "react-intl";
import { updateCheck, updateStart } from "../../api/update";
import { useAppStore } from "../../stores/appStore";
import { useUpdateStore } from "../../stores/updateStore";

function formatPublishedAt(value?: string, locale?: string): string | null {
  const raw = value?.trim();
  if (!raw) {
    return null;
  }

  const date = new Date(raw);
  if (Number.isNaN(date.getTime())) {
    return raw;
  }

  try {
    return new Intl.DateTimeFormat(locale || undefined, {
      year: "numeric",
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    }).format(date);
  } catch {
    return raw;
  }
}

export function UpdateModal() {
  const intl = useIntl();
  const initialized = useAppStore((s) => s.initialized);
  const info = useUpdateStore((s) => s.info);
  const showModal = useUpdateStore((s) => s.showModal);
  const checked = useUpdateStore((s) => s.checked);
  const setInfo = useUpdateStore((s) => s.setInfo);
  const setShowModal = useUpdateStore((s) => s.setShowModal);
  const setChecked = useUpdateStore((s) => s.setChecked);
  const dismiss = useUpdateStore((s) => s.dismiss);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showUrl, setShowUrl] = useState(false);

  useEffect(() => {
    if (!initialized || checked) {
      return;
    }
    setChecked(true);

    void (async () => {
      try {
        const result = await updateCheck();
        console.info("[update] check result:", result);
        if (result.updateAvailable && result.url?.trim()) {
          setInfo(result);
          // 有新版本时直接在右上角显示按钮，不强制弹窗打断用户。
          setShowModal(false);
        } else {
          setInfo(null);
        }
      } catch (err) {
        // Auto-update is best-effort and should never block app startup.
        console.warn("[update] check failed:", err);
        setInfo(null);
      }
    })();
  }, [checked, initialized, setChecked, setInfo, setShowModal]);

  useEffect(() => {
    if (!showModal) {
      setError(null);
      setStarting(false);
      setShowUrl(false);
    }
  }, [showModal]);

  const publishedLabel = useMemo(
    () => formatPublishedAt(info?.publishedAt, intl.locale),
    [info?.publishedAt, intl.locale],
  );

  const handleLater = useCallback(() => {
    if (starting) {
      return;
    }
    dismiss();
  }, [dismiss, starting]);

  const handleUpdate = useCallback(async () => {
    if (!info?.url || starting) {
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
  }, [info, starting]);

  if (!showModal || !info) {
    return null;
  }

  return (
    <div className="fixed bottom-0 left-0 right-0 top-8 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm">
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="update-modal-title"
        className="app-shell-panel relative w-full max-w-[460px] overflow-hidden shadow-[0_24px_80px_rgba(0,0,0,0.45)]"
      >
        <div className="pointer-events-none absolute inset-x-0 top-0 h-28 bg-[radial-gradient(ellipse_at_top,rgba(34,197,94,0.18),transparent_70%)]" />
        <div className="pointer-events-none absolute -right-10 -top-10 h-36 w-36 rounded-full bg-[rgba(34,197,94,0.08)] blur-2xl" />

        <div className="relative flex items-start justify-between gap-3 border-b border-[var(--border-subtle)] px-5 py-4">
          <div className="flex items-start gap-3">
            <div className="relative mt-0.5 flex h-11 w-11 items-center justify-center rounded-2xl border border-[var(--accent-border)] bg-[var(--accent-soft)] text-[var(--accent-strong)] shadow-[0_0_24px_rgba(34,197,94,0.18)]">
              <IconRocket size={20} stroke={1.9} />
              <span className="absolute -bottom-1 -right-1 flex h-4 w-4 items-center justify-center rounded-full bg-[var(--accent)] text-white">
                <IconSparkles size={10} stroke={2.2} />
              </span>
            </div>
            <div className="min-w-0">
              <div className="mb-1 flex flex-wrap items-center gap-2">
                <span className="rounded-full border border-[var(--accent-border)] bg-[var(--accent-soft)] px-2 py-0.5 text-[10px] font-semibold uppercase tracking-[0.08em] text-[var(--accent-strong)]">
                  {intl.formatMessage({ id: "update.badge" })}
                </span>
                {info.force ? (
                  <span className="rounded-full border border-[rgba(245,158,11,0.35)] bg-[rgba(245,158,11,0.12)] px-2 py-0.5 text-[10px] font-semibold text-[var(--warning)]">
                    {intl.formatMessage({ id: "update.force" })}
                  </span>
                ) : null}
              </div>
              <h3
                id="update-modal-title"
                className="text-[15px] font-semibold tracking-[-0.01em] text-[var(--text-strong)]"
              >
                {intl.formatMessage({ id: "update.title" })}
              </h3>
              <p className="mt-1 text-[12px] leading-5 text-[var(--text-muted)]">
                {intl.formatMessage({ id: "update.subtitle" })}
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={handleLater}
            disabled={starting}
            className="icon-button rounded-full p-1.5 text-[var(--text-faint)] disabled:cursor-not-allowed disabled:opacity-40"
            aria-label={intl.formatMessage({ id: "common.close" })}
          >
            <IconX size={16} stroke={2} />
          </button>
        </div>

        <div className="relative space-y-4 px-5 py-4">
          <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-3 rounded-2xl border border-[var(--border-subtle)] bg-[linear-gradient(180deg,rgba(48,48,48,0.55),rgba(28,28,28,0.9))] p-3.5">
            <div className="rounded-xl border border-[var(--border-subtle)] bg-[rgba(0,0,0,0.18)] px-3 py-2.5">
              <p className="text-[10px] font-semibold uppercase tracking-[0.08em] text-[var(--text-faint)]">
                {intl.formatMessage({ id: "update.currentVersion" })}
              </p>
              <p className="mt-1 truncate font-mono text-[13px] font-medium text-[var(--text-base)]">
                v{info.currentVersion}
              </p>
            </div>
            <div className="flex h-8 w-8 items-center justify-center rounded-full border border-[var(--accent-border)] bg-[var(--accent-soft)] text-[var(--accent-strong)]">
              <IconArrowRight size={15} stroke={2} />
            </div>
            <div className="rounded-xl border border-[var(--accent-border)] bg-[var(--accent-soft)] px-3 py-2.5">
              <p className="text-[10px] font-semibold uppercase tracking-[0.08em] text-[var(--accent-strong)]">
                {intl.formatMessage({ id: "update.latestVersion" })}
              </p>
              <p className="mt-1 truncate font-mono text-[13px] font-semibold text-[var(--accent-strong)]">
                v{info.latestVersion}
              </p>
            </div>
          </div>

          <p className="text-[13px] leading-6 text-[var(--text-base)]">
            {intl.formatMessage(
              { id: "update.description" },
              {
                current: info.currentVersion,
                latest: info.latestVersion,
              },
            )}
          </p>

          {info.notes?.trim() ? (
            <div className="overflow-hidden rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface-panel)]">
              <div className="flex items-center justify-between border-b border-[var(--border-subtle)] px-3.5 py-2.5">
                <p className="text-[11px] font-semibold uppercase tracking-[0.06em] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "update.notes" })}
                </p>
                {publishedLabel ? (
                  <p className="text-[11px] text-[var(--text-faint)]">
                    {intl.formatMessage(
                      { id: "update.publishedAt" },
                      { date: publishedLabel },
                    )}
                  </p>
                ) : null}
              </div>
              <div className="thin-scrollbar max-h-36 overflow-y-auto px-3.5 py-3">
                <p className="whitespace-pre-wrap text-[13px] leading-6 text-[var(--text-muted)]">
                  {info.notes}
                </p>
              </div>
            </div>
          ) : publishedLabel ? (
            <p className="text-[12px] text-[var(--text-faint)]">
              {intl.formatMessage(
                { id: "update.publishedAt" },
                { date: publishedLabel },
              )}
            </p>
          ) : null}

          <div className="flex items-start gap-2.5 rounded-xl border border-[var(--border-subtle)] bg-[rgba(34,197,94,0.06)] px-3 py-2.5">
            <IconShieldCheck
              size={16}
              stroke={1.8}
              className="mt-0.5 shrink-0 text-[var(--accent-strong)]"
            />
            <div className="min-w-0 flex-1">
              <p className="text-[12px] leading-5 text-[var(--text-muted)]">
                {intl.formatMessage({ id: "update.safeHint" })}
              </p>
              <button
                type="button"
                onClick={() => setShowUrl((value) => !value)}
                className="mt-1 text-[11px] font-medium text-[var(--accent-strong)] transition-colors hover:text-[var(--accent)]"
              >
                {showUrl
                  ? intl.formatMessage({ id: "update.hideUrl" })
                  : intl.formatMessage({ id: "update.showUrl" })}
              </button>
              {showUrl ? (
                <p className="mt-1.5 break-all font-mono text-[11px] leading-5 text-[var(--text-faint)]">
                  {info.url}
                </p>
              ) : null}
            </div>
          </div>

          {error ? (
            <p className="rounded-xl border border-red-500/30 bg-red-500/10 px-3 py-2.5 text-[12px] leading-5 text-red-300">
              {error}
            </p>
          ) : null}
        </div>

        <div className="relative flex items-center justify-end gap-2.5 border-t border-[var(--border-subtle)] bg-[rgba(0,0,0,0.12)] px-5 py-3.5">
          <button
            type="button"
            onClick={handleLater}
            disabled={starting}
            className="secondary-button rounded-xl px-4 py-2 text-[13px] disabled:cursor-not-allowed disabled:opacity-50"
          >
            {intl.formatMessage({ id: "update.later" })}
          </button>
          <button
            type="button"
            onClick={handleUpdate}
            disabled={starting}
            className="primary-button flex min-w-[132px] items-center justify-center gap-2 rounded-xl px-4 py-2 text-[13px] shadow-[0_8px_24px_rgba(34,197,94,0.25)]"
          >
            {starting ? (
              <>
                <IconLoader2 size={16} stroke={1.9} className="animate-spin" />
                {intl.formatMessage({ id: "update.starting" })}
              </>
            ) : (
              <>
                <IconDownload size={16} stroke={1.9} />
                {intl.formatMessage({ id: "update.now" })}
              </>
            )}
          </button>
        </div>
      </div>
    </div>
  );
}
