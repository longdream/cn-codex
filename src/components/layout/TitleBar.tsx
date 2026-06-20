import { useState } from "react";
import { useIntl } from "react-intl";
import {
  windowClose,
  windowMinimize,
  windowToggleMaximize,
} from "../../api/window";
import { useAppStore } from "../../stores/appStore";
import { IconLayoutSidebarRight, IconQrcode } from "@tabler/icons-react";
import { QrCodePopover } from "../common/QrCodePopover";

function runWindowAction(action: () => Promise<void>, label: string) {
  const onError = (err: unknown) => {
    console.error(`Window ${label} failed:`, err);
  };

  try {
    void action().catch(onError);
  } catch (err) {
    onError(err);
  }
}

export function TitleBar() {
  const intl = useIntl();
  const rightPanelVisible = useAppStore((s) => s.rightPanelVisible);
  const rightPanelTab = useAppStore((s) => s.rightPanelTab);
  const setRightPanelTab = useAppStore((s) => s.setRightPanelTab);
  const setRightPanelVisible = useAppStore((s) => s.setRightPanelVisible);
  const [showQr, setShowQr] = useState(false);

  const handleDoubleClick = () => {
    runWindowAction(windowToggleMaximize, "toggle maximize");
  };

  return (
    <div className="relative z-[60] flex h-8 w-full flex-shrink-0 select-none items-center justify-between border-b border-[var(--border-subtle)] bg-[var(--surface-sidebar)]">
      <div
        data-tauri-drag-region
        className="flex h-full flex-1 items-center gap-2 pl-3"
        onDoubleClick={handleDoubleClick}
      >
        <span
          data-tauri-drag-region
          className="h-2.5 w-2.5 rounded-full bg-[var(--accent)]"
        />
        <span
          data-tauri-drag-region
          className="text-[11px] font-medium text-[var(--text-muted)]"
        >
          CN-Codex
        </span>
      </div>

      <div className="flex h-full items-center">
        <div className="relative">
          <button
            type="button"
            aria-label={intl.formatMessage({ id: "titleBar.qrCode" })}
            onClick={() => setShowQr(!showQr)}
            className={`flex h-full w-11 items-center justify-center transition-colors ${
              showQr
                ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                : "text-[var(--text-muted)] hover:bg-[var(--surface-elevated)]"
            }`}
          >
            <IconQrcode size={15} stroke={1.8} />
          </button>
          {showQr && <QrCodePopover onClose={() => setShowQr(false)} />}
        </div>
        <button
          type="button"
          aria-label={intl.formatMessage({ id: "titleBar.togglePanel" })}
          onClick={() => {
            if (rightPanelVisible && rightPanelTab === "project") {
              setRightPanelVisible(false);
            } else {
              setRightPanelTab("project");
            }
          }}
          className={`flex h-full w-11 items-center justify-center transition-colors ${
            rightPanelVisible
              ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
              : "text-[var(--text-muted)] hover:bg-[var(--surface-elevated)]"
          }`}
        >
          <IconLayoutSidebarRight size={15} stroke={1.8} />
        </button>
        <button
          type="button"
          aria-label={intl.formatMessage({ id: "titleBar.minimize" })}
          onClick={() => runWindowAction(windowMinimize, "minimize")}
          className="flex h-full w-11 items-center justify-center text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-elevated)]"
        >
          <svg width="10" height="1" viewBox="0 0 10 1" fill="currentColor">
            <rect width="10" height="1" />
          </svg>
        </button>
        <button
          type="button"
          aria-label={intl.formatMessage({ id: "titleBar.maximize" })}
          onClick={() =>
            runWindowAction(windowToggleMaximize, "toggle maximize")
          }
          className="flex h-full w-11 items-center justify-center text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-elevated)]"
        >
          <svg
            width="10"
            height="10"
            viewBox="0 0 10 10"
            fill="none"
            stroke="currentColor"
            strokeWidth="1"
          >
            <rect x="0.5" y="0.5" width="9" height="9" />
          </svg>
        </button>
        <button
          type="button"
          aria-label={intl.formatMessage({ id: "titleBar.close" })}
          onClick={() => runWindowAction(windowClose, "close")}
          className="flex h-full w-11 items-center justify-center text-[var(--text-muted)] transition-colors hover:bg-[#e81123] hover:text-white"
        >
          <svg
            width="10"
            height="10"
            viewBox="0 0 10 10"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.2"
          >
            <line x1="0" y1="0" x2="10" y2="10" />
            <line x1="10" y1="0" x2="0" y2="10" />
          </svg>
        </button>
      </div>
    </div>
  );
}
