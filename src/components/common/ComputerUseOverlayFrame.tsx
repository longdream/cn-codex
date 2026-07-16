import { useEffect, useState } from "react";
import { useIntl } from "react-intl";

const CURSOR_SVG = encodeURIComponent(
  `
<svg xmlns="http://www.w3.org/2000/svg" width="56" height="56" viewBox="0 0 56 56" fill="none">
  <defs>
    <linearGradient id="g" x1="10" y1="6" x2="46" y2="50" gradientUnits="userSpaceOnUse">
      <stop stop-color="#93c5fd"/>
      <stop offset="0.55" stop-color="#60a5fa"/>
      <stop offset="1" stop-color="#3b82f6"/>
    </linearGradient>
    <filter id="glow" x="-50%" y="-50%" width="200%" height="200%">
      <feGaussianBlur stdDeviation="2.2" result="blur"/>
      <feMerge>
        <feMergeNode in="blur"/>
        <feMergeNode in="SourceGraphic"/>
      </feMerge>
    </filter>
  </defs>
  <path d="M14 10 L14 40 L22.4 32.2 L28.8 47.2 L34.4 44.8 L28 29.8 L39 29.8 Z"
    fill="url(#g)" stroke="rgba(255,255,255,0.92)" stroke-width="1.8" stroke-linejoin="round" filter="url(#glow)"/>
  <circle cx="40" cy="16" r="8.4" fill="rgba(8,14,24,0.62)" stroke="#93c5fd" stroke-width="1.7"/>
  <path d="M40 11.8 V20.2 M35.8 16 H44.2" stroke="#dbeafe" stroke-width="1.8" stroke-linecap="round"/>
</svg>
`.trim(),
);

export const COMPUTER_USE_CURSOR_CSS = `url("data:image/svg+xml,${CURSOR_SVG}") 14 10, crosshair`;

interface ComputerUseOverlayFrameProps {
  active: boolean;
  /** screen = 整屏覆盖窗；app = 主窗内本地回退。 */
  variant?: "screen" | "app";
  applyCursor?: boolean;
}

/**
 * Computer Use 激活态视觉：
 * 整屏边缘泛色光晕 + 状态提示 + 受控光标，不使用直线描边和四角扩散圆。
 */
export function ComputerUseOverlayFrame({
  active,
  variant = "screen",
  applyCursor = true,
}: ComputerUseOverlayFrameProps) {
  const intl = useIntl();
  const [entered, setEntered] = useState(false);

  useEffect(() => {
    if (!active) {
      setEntered(false);
      if (applyCursor) {
        document.documentElement.style.removeProperty("--computer-use-cursor");
        document.body.classList.remove("computer-use-controlling");
      }
      return;
    }

    if (applyCursor) {
      document.documentElement.style.setProperty("--computer-use-cursor", COMPUTER_USE_CURSOR_CSS);
      document.body.classList.add("computer-use-controlling");
    }

    const raf = window.requestAnimationFrame(() => setEntered(true));
    return () => {
      window.cancelAnimationFrame(raf);
      if (applyCursor) {
        document.documentElement.style.removeProperty("--computer-use-cursor");
        document.body.classList.remove("computer-use-controlling");
      }
    };
  }, [active, applyCursor]);

  if (!active) {
    return null;
  }

  const title = intl.formatMessage({ id: "computerUse.overlay.title" });
  const subtitle = intl.formatMessage({ id: "computerUse.overlay.subtitle" });

  return (
    <div
      className={`computer-use-overlay computer-use-overlay--${variant}${entered ? " is-active" : ""}`}
      aria-live="polite"
      aria-atomic="true"
      role="status"
    >
      <div className="computer-use-overlay-frame" aria-hidden="true">
        <span className="computer-use-overlay-haze computer-use-overlay-haze-top" />
        <span className="computer-use-overlay-haze computer-use-overlay-haze-right" />
        <span className="computer-use-overlay-haze computer-use-overlay-haze-bottom" />
        <span className="computer-use-overlay-haze computer-use-overlay-haze-left" />
        <span className="computer-use-overlay-bloom computer-use-overlay-bloom-tl" />
        <span className="computer-use-overlay-bloom computer-use-overlay-bloom-tr" />
        <span className="computer-use-overlay-bloom computer-use-overlay-bloom-bl" />
        <span className="computer-use-overlay-bloom computer-use-overlay-bloom-br" />
        <span className="computer-use-overlay-wave computer-use-overlay-wave-a" />
        <span className="computer-use-overlay-wave computer-use-overlay-wave-b" />
      </div>

      <div className="computer-use-overlay-badge">
        <span className="computer-use-overlay-badge-dot" aria-hidden="true" />
        <div className="computer-use-overlay-badge-copy">
          <strong>{title}</strong>
          <span>{subtitle}</span>
        </div>
      </div>
    </div>
  );
}
