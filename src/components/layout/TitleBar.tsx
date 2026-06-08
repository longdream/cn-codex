import { getCurrentWindow } from "@tauri-apps/api/window";

/**
 * 自定义标题栏组件
 * 替代 Windows 系统白色标题栏，使其与深色/浅色主题统一
 * 注意：按钮不能放在 data-tauri-drag-region 内部，否则 click 会被 drag region 拦截
 */
export function TitleBar() {
  const appWindow = getCurrentWindow();

  return (
    <div className="flex h-8 w-full flex-shrink-0 select-none items-center justify-between bg-[var(--surface-sidebar)] border-b border-[var(--border-subtle)]">
      {/* 左侧应用标题 — 可拖拽区域 */}
      <div data-tauri-drag-region className="flex flex-1 items-center gap-2 pl-3 h-full">
        <span className="h-2.5 w-2.5 rounded-full bg-[var(--accent)]" />
        <span className="text-[11px] font-medium text-[var(--text-muted)]">CN-Codex</span>
      </div>

      {/* 右侧窗口控制按钮 — 不在 drag-region 内 */}
      <div className="flex h-full">
        <button
          onClick={() => appWindow.minimize()}
          className="flex h-full w-11 items-center justify-center text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-elevated)]"
        >
          <svg width="10" height="1" viewBox="0 0 10 1" fill="currentColor">
            <rect width="10" height="1" />
          </svg>
        </button>
        <button
          onClick={() => appWindow.toggleMaximize()}
          className="flex h-full w-11 items-center justify-center text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-elevated)]"
        >
          <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1">
            <rect x="0.5" y="0.5" width="9" height="9" />
          </svg>
        </button>
        <button
          onClick={() => appWindow.close()}
          className="flex h-full w-11 items-center justify-center text-[var(--text-muted)] transition-colors hover:bg-[#e81123] hover:text-white"
        >
          <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.2">
            <line x1="0" y1="0" x2="10" y2="10" />
            <line x1="10" y1="0" x2="0" y2="10" />
          </svg>
        </button>
      </div>
    </div>
  );
}
