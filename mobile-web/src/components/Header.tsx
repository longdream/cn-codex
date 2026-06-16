import { useMobileStore } from "../stores/mobileStore";

export function Header() {
  const view = useMobileStore((s) => s.view);
  const connected = useMobileStore((s) => s.connected);
  const setView = useMobileStore((s) => s.setView);
  const currentThreadId = useMobileStore((s) => s.currentThreadId);
  const threads = useMobileStore((s) => s.threads);

  const currentThread = threads.find((t) => t.id === currentThreadId);
  const title = view === "chat" && currentThread
    ? (currentThread.name || currentThread.preview || "对话")
    : "CN-Codex";

  return (
    <header className="app-header">
      <div className="header-left">
        {view === "chat" && (
          <button onClick={() => setView("threads")} className="back-btn">
            ‹
          </button>
        )}
        <span className="header-title">{title}</span>
      </div>
      <div className="header-right">
        <span className={`status-dot ${connected ? "online" : "offline"}`} />
        <span className="status-text">{connected ? "已连接" : "断开"}</span>
      </div>
    </header>
  );
}
