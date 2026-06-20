import { useEffect, useRef, useState } from "react";
import { useIntl } from "react-intl";
import { invoke } from "@tauri-apps/api/core";

interface QrCodePopoverProps {
  onClose: () => void;
}

export function QrCodePopover({ onClose }: QrCodePopoverProps) {
  const intl = useIntl();
  const [svg, setSvg] = useState<string>("");
  const [url, setUrl] = useState<string>("");
  const [error, setError] = useState<string>("");
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const load = async () => {
      try {
        const serverUrl = await invoke<string>("get_mobile_server_url");
        setUrl(serverUrl);
        const qrSvg = await invoke<string>("get_qrcode_svg");
        setSvg(qrSvg);
      } catch (e) {
        setError(String(e));
      }
    };
    void load();
  }, []);

  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        onClose();
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [onClose]);

  return (
    <div
      ref={ref}
      className="absolute right-0 top-full z-[100] mt-1 w-64 rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-panel)] p-4 shadow-xl"
    >
      <p className="mb-3 text-center text-xs font-medium text-[var(--text-strong)]">
        {intl.formatMessage({ id: "qr.scanToAccess" })}
      </p>
      {error ? (
        <div className="text-center py-4">
          <p className="text-xs text-[var(--text-muted)] mb-2">
            {intl.formatMessage({ id: "qr.webServerNotStarted" })}
          </p>
          <p className="text-[11px] text-[var(--text-faint)]">
            {intl.formatMessage({ id: "qr.enableInSettings" })}
          </p>
        </div>
      ) : svg ? (
        <div
          className="mx-auto w-48 [&_svg]:w-full [&_svg]:h-auto"
          dangerouslySetInnerHTML={{ __html: svg }}
        />
      ) : (
        <div className="flex h-48 items-center justify-center">
          <span className="text-xs text-[var(--text-muted)]">
            {intl.formatMessage({ id: "common.loading" })}
          </span>
        </div>
      )}
      {url && (
        <p className="mt-3 break-all text-center font-mono text-[10px] text-[var(--text-muted)]">
          {url}
        </p>
      )}
    </div>
  );
}
