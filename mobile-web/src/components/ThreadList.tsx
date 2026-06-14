import { useMobileStore } from "../stores/mobileStore";
import { fetchThreadMessages } from "../api/http";

function relativeTime(ts: number): string {
  const now = Math.floor(Date.now() / 1000);
  const diff = now - ts;
  if (diff < 60) return "刚刚";
  if (diff < 3600) return `${Math.floor(diff / 60)} 分钟前`;
  if (diff < 86400) return `${Math.floor(diff / 3600)} 小时前`;
  if (diff < 604800) return `${Math.floor(diff / 86400)} 天前`;
  return new Date(ts * 1000).toLocaleDateString("zh-CN");
}

export function ThreadList() {
  const threads = useMobileStore((s) => s.threads);
  const activeThreadId = useMobileStore((s) => s.activeThreadId);
  const setCurrentThread = useMobileStore((s) => s.setCurrentThread);

  const handleSelect = (id: string) => {
    setCurrentThread(id);
    void fetchThreadMessages(id);
  };

  if (threads.length === 0) {
    return (
      <div className="empty-state">
        <p>暂无对话</p>
        <p className="empty-hint">在 PC 端开始对话后将自动同步</p>
      </div>
    );
  }

  return (
    <div className="thread-list">
      {threads.map((thread) => {
        const isActive = thread.id === activeThreadId;
        return (
          <button
            key={thread.id}
            onClick={() => handleSelect(thread.id)}
            className={`thread-item ${isActive ? "is-active" : ""}`}
          >
            <div className="thread-item-header">
              <span className="thread-item-title">
                {isActive && <span className="active-dot" />}
                {thread.name || thread.preview || "未命名对话"}
              </span>
              <span className="thread-item-time">{relativeTime(thread.updatedAt)}</span>
            </div>
            {thread.preview && thread.name && (
              <p className="thread-item-preview">{thread.preview}</p>
            )}
            <div className="thread-item-meta">
              {isActive && <span className="active-badge">当前活跃</span>}
              <span className="thread-item-count">{thread.messageCount} 条消息</span>
            </div>
          </button>
        );
      })}
    </div>
  );
}
