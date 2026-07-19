import {
  IconMessageCircle,
  IconSend,
  IconUsersGroup,
  IconX,
} from "@tabler/icons-react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useIntl } from "react-intl";
import {
  lanCollabListMessages,
  lanCollabSendMessage,
  lanCollabStatus,
  type ChatMessage,
  type CollabGroup,
  type LanCollabStatus,
} from "../../api/lanCollab";
import { useAppStore } from "../../stores/appStore";

function formatTime(ts: number): string {
  if (!ts) return "";
  try {
    return new Date(ts * 1000).toLocaleString();
  } catch {
    return String(ts);
  }
}

/**
 * 主对话框右上角的组内聊天入口。
 * 未读消息时图标变色并闪动；点击后弹出独立聊天窗口，
 * 不再把聊天塞进右侧「局域网协作」长面板底部。
 */
export function LanGroupChatLauncher() {
  const intl = useIntl();
  const setRightPanelTab = useAppStore((s) => s.setRightPanelTab);

  const [open, setOpen] = useState(false);
  const [unreadCount, setUnreadCount] = useState(0);
  const [status, setStatus] = useState<LanCollabStatus | null>(null);
  const [selectedGroupId, setSelectedGroupId] = useState<string | null>(null);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [messageDraft, setMessageDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const openRef = useRef(open);
  const selectedGroupIdRef = useRef(selectedGroupId);
  const panelRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const selfNodeId = status?.identity.nodeId;
  const errorClearTimerRef = useRef<number | null>(null);

  const dismissError = useCallback(() => {
    if (errorClearTimerRef.current != null) {
      window.clearTimeout(errorClearTimerRef.current);
      errorClearTimerRef.current = null;
    }
    setError(null);
  }, []);

  const showError = useCallback(
    (message: string) => {
      setError(message);
      if (errorClearTimerRef.current != null) {
        window.clearTimeout(errorClearTimerRef.current);
      }
      errorClearTimerRef.current = window.setTimeout(() => {
        setError(null);
        errorClearTimerRef.current = null;
      }, 12000);
    },
    [],
  );

  useEffect(() => {
    openRef.current = open;
  }, [open]);

  useEffect(() => {
    selectedGroupIdRef.current = selectedGroupId;
  }, [selectedGroupId]);

  const refreshStatus = useCallback(async () => {
    try {
      const next = await lanCollabStatus();
      setStatus(next);
      setSelectedGroupId((prev) => {
        if (prev && next.groups.some((g) => g.groupId === prev)) return prev;
        return next.groups[0]?.groupId ?? null;
      });
    } catch (err) {
      showError(err instanceof Error ? err.message : String(err));
    }
  }, [showError]);

  const refreshMessages = useCallback(async (groupId: string | null) => {
    if (!groupId) {
      setMessages([]);
      return;
    }
    try {
      const list = await lanCollabListMessages(groupId);
      setMessages(list);
    } catch (err) {
      showError(err instanceof Error ? err.message : String(err));
    }
  }, [showError]);

  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);

  useEffect(() => {
    if (!open) return;
    void refreshMessages(selectedGroupId);
  }, [open, selectedGroupId, refreshMessages]);

  useEffect(() => {
    if (!open) return;
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [open, messages.length]);

  // 全局监听组消息：窗口关闭或非当前组时累计未读
  useEffect(() => {
    let disposed = false;
    const unlisteners: UnlistenFn[] = [];

    const setup = async () => {
      try {
        unlisteners.push(
          await listen<ChatMessage>("lan-collab-message", (event) => {
            if (disposed) return;
            const msg = event.payload;
            const isOpen = openRef.current;
            const currentGroupId = selectedGroupIdRef.current;

            if (isOpen && currentGroupId && msg.groupId === currentGroupId) {
              setMessages((prev) => {
                if (prev.some((m) => m.messageId === msg.messageId)) return prev;
                return [...prev, msg].sort((a, b) => a.createdAt - b.createdAt);
              });
              return;
            }

            // 自己发出的消息不计入未读
            if (selfNodeId && msg.fromNodeId === selfNodeId) return;
            setUnreadCount((n) => n + 1);
          }),
        );
        unlisteners.push(
          await listen("lan-collab-group", () => {
            if (!disposed) void refreshStatus();
          }),
        );
        unlisteners.push(
          await listen("lan-collab-peer", () => {
            if (!disposed) void refreshStatus();
          }),
        );
      } catch (err) {
        if (!disposed) {
          showError(err instanceof Error ? err.message : String(err));
        }
      }
    };

    void setup();
    return () => {
      disposed = true;
      for (const off of unlisteners) {
        try {
          off();
        } catch {
          // ignore
        }
      }
    };
  }, [refreshStatus, selfNodeId, showError]);

  useEffect(() => {
    return () => {
      if (errorClearTimerRef.current != null) {
        window.clearTimeout(errorClearTimerRef.current);
      }
    };
  }, []);

  // 打开时轮询兜底
  useEffect(() => {
    if (!open) return;
    const timer = window.setInterval(() => {
      void refreshStatus();
      if (selectedGroupIdRef.current) {
        void refreshMessages(selectedGroupIdRef.current);
      }
    }, 4000);
    return () => window.clearInterval(timer);
  }, [open, refreshStatus, refreshMessages]);

  // 点击外部关闭
  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: MouseEvent) => {
      const target = event.target as Node | null;
      if (!target) return;
      if (panelRef.current?.contains(target)) return;
      if (buttonRef.current?.contains(target)) return;
      setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  const selectedGroup: CollabGroup | null = useMemo(
    () => status?.groups.find((g) => g.groupId === selectedGroupId) ?? null,
    [status?.groups, selectedGroupId],
  );

  const hasGroups = (status?.groups.length ?? 0) > 0;
  const hasUnread = unreadCount > 0;

  const handleToggle = () => {
    setOpen((prev) => {
      const next = !prev;
      if (next) {
        setUnreadCount(0);
        void refreshStatus();
        void refreshMessages(selectedGroupIdRef.current);
      }
      return next;
    });
  };

  const handleSelectGroup = (groupId: string) => {
    setSelectedGroupId(groupId);
    setUnreadCount(0);
    void refreshMessages(groupId);
  };

  const handleSend = async () => {
    if (!selectedGroupId || !messageDraft.trim() || busy) return;
    setBusy(true);
    dismissError();
    try {
      await lanCollabSendMessage(selectedGroupId, messageDraft);
      setMessageDraft("");
      await refreshMessages(selectedGroupId);
    } catch (err) {
      showError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const openLanPanel = () => {
    setRightPanelTab("lan");
    setOpen(false);
  };

  return (
    <div className="relative shrink-0">
      <button
        ref={buttonRef}
        type="button"
        onClick={handleToggle}
        className={`relative flex h-6 w-6 items-center justify-center rounded-full border transition-colors ${
          open
            ? "border-[var(--accent-border)] bg-[var(--accent-soft)] text-[var(--accent-strong)]"
            : hasUnread
              ? "lan-chat-unread border-[var(--accent-border)] bg-[var(--accent-soft)] text-[var(--accent)]"
              : "border-[var(--chat-line)] bg-[var(--chat-chip)] text-[var(--chat-muted)] hover:text-[var(--chat-prose)]"
        }`}
        title={
          hasUnread
            ? intl.formatMessage(
                { id: "lanCollab.chatUnreadTitle" },
                { count: unreadCount },
              )
            : intl.formatMessage({ id: "lanCollab.chatLauncherTitle" })
        }
        aria-label={intl.formatMessage({ id: "lanCollab.chatLauncherTitle" })}
        aria-expanded={open}
      >
        <IconUsersGroup size={13} stroke={1.8} />
        {hasUnread && (
          <span className="absolute -right-1 -top-1 flex h-3.5 min-w-3.5 items-center justify-center rounded-full bg-[var(--accent)] px-0.5 text-[9px] font-semibold leading-none text-white">
            {unreadCount > 99 ? "99+" : unreadCount}
          </span>
        )}
      </button>

      {open && (
        <div
          ref={panelRef}
          className="lan-group-chat-panel absolute bottom-full right-0 z-40 mb-2 flex w-[min(380px,calc(100vw-2rem))] flex-col overflow-hidden rounded-[var(--radius-lg)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] shadow-[var(--shadow-strong)]"
          style={{ height: "min(460px, calc(100vh - 180px))" }}
        >
          <div className="flex items-center gap-2 border-b border-[var(--border-subtle)] px-3 py-2">
            <IconMessageCircle size={14} stroke={1.8} className="text-[var(--accent)]" />
            <div className="min-w-0 flex-1">
              <div className="truncate text-[12px] font-semibold text-[var(--text-strong)]">
                {selectedGroup
                  ? intl.formatMessage(
                      { id: "lanCollab.chatWith" },
                      { name: selectedGroup.name },
                    )
                  : intl.formatMessage({ id: "lanCollab.chatSection" })}
              </div>
              <div className="truncate text-[10px] text-[var(--text-faint)]">
                {status?.enabled
                  ? intl.formatMessage(
                      { id: "lanCollab.connectedCount" },
                      { count: status.connectedPeerCount ?? 0 },
                    )
                  : intl.formatMessage({ id: "lanCollab.disabled" })}
              </div>
            </div>
            <button
              type="button"
              onClick={() => setOpen(false)}
              className="flex h-6 w-6 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
              title={intl.formatMessage({ id: "lanCollab.chatClose" })}
            >
              <IconX size={14} stroke={1.8} />
            </button>
          </div>

          {!status?.enabled ? (
            <div className="flex flex-1 flex-col items-center justify-center gap-2 p-4 text-center">
              <p className="text-[12px] text-[var(--text-muted)]">
                {intl.formatMessage({ id: "lanCollab.chatNeedEnable" })}
              </p>
              <button
                type="button"
                onClick={openLanPanel}
                className="rounded-full border border-[var(--accent-border)] bg-[var(--accent-soft)] px-3 py-1 text-[11px] font-medium text-[var(--accent-strong)]"
              >
                {intl.formatMessage({ id: "lanCollab.chatOpenSettings" })}
              </button>
            </div>
          ) : !hasGroups ? (
            <div className="flex flex-1 flex-col items-center justify-center gap-2 p-4 text-center">
              <p className="text-[12px] text-[var(--text-muted)]">
                {intl.formatMessage({ id: "lanCollab.chatNeedGroup" })}
              </p>
              <button
                type="button"
                onClick={openLanPanel}
                className="rounded-full border border-[var(--accent-border)] bg-[var(--accent-soft)] px-3 py-1 text-[11px] font-medium text-[var(--accent-strong)]"
              >
                {intl.formatMessage({ id: "lanCollab.chatOpenSettings" })}
              </button>
            </div>
          ) : (
            <>
              {status.groups.length > 1 && (
                <div className="thin-scrollbar flex gap-1 overflow-x-auto border-b border-[var(--border-subtle)] px-2 py-1.5">
                  {status.groups.map((group) => {
                    const active = group.groupId === selectedGroupId;
                    return (
                      <button
                        key={group.groupId}
                        type="button"
                        onClick={() => handleSelectGroup(group.groupId)}
                        className={`shrink-0 rounded-full px-2.5 py-1 text-[11px] transition-colors ${
                          active
                            ? "bg-[var(--accent-soft)] font-medium text-[var(--accent-strong)]"
                            : "bg-[var(--surface-elevated)] text-[var(--text-muted)] hover:text-[var(--text-strong)]"
                        }`}
                      >
                        {group.name}
                      </button>
                    );
                  })}
                </div>
              )}

              {error && (
                <div
                  role="alert"
                  className="flex items-start gap-2 border-b border-[var(--danger)]/30 bg-[var(--danger-soft)] px-3 py-1.5 text-[11px] text-[var(--danger)]"
                >
                  <div className="min-w-0 flex-1 whitespace-pre-wrap break-words">{error}</div>
                  <button
                    type="button"
                    onClick={dismissError}
                    className="flex h-5 w-5 shrink-0 items-center justify-center rounded-[var(--radius-sm)] text-[var(--danger)]/80 transition-colors hover:bg-[var(--danger)]/10 hover:text-[var(--danger)]"
                    title={intl.formatMessage({ id: "lanCollab.dismissError" })}
                    aria-label={intl.formatMessage({ id: "lanCollab.dismissError" })}
                  >
                    <IconX size={12} stroke={2} />
                  </button>
                </div>
              )}

              <div className="thin-scrollbar min-h-0 flex-1 space-y-2 overflow-auto p-2.5">
                {messages.length === 0 ? (
                  <p className="p-2 text-[11px] text-[var(--text-muted)]">
                    {intl.formatMessage({ id: "lanCollab.noMessages" })}
                  </p>
                ) : (
                  messages.map((msg) => {
                    const mine = !!selfNodeId && msg.fromNodeId === selfNodeId;
                    return (
                      <div
                        key={msg.messageId}
                        className={`max-w-[92%] rounded-[var(--radius-sm)] border px-2.5 py-1.5 ${
                          mine
                            ? "ml-auto border-[var(--accent-border)] bg-[var(--accent-soft)]"
                            : "border-[var(--border-subtle)] bg-[var(--surface-main)]"
                        }`}
                      >
                        <div className="mb-0.5 flex items-center justify-between gap-2 text-[10px] text-[var(--text-faint)]">
                          <span className="truncate font-medium text-[var(--text-muted)]">
                            {msg.fromDisplayName}
                          </span>
                          <span className="shrink-0">{formatTime(msg.createdAt)}</span>
                        </div>
                        <div className="whitespace-pre-wrap break-words text-[12px] text-[var(--text-base)]">
                          {msg.text}
                        </div>
                      </div>
                    );
                  })
                )}
                <div ref={messagesEndRef} />
              </div>

              <div className="flex gap-2 border-t border-[var(--border-subtle)] p-2.5">
                <input
                  type="text"
                  value={messageDraft}
                  onChange={(e) => setMessageDraft(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && !e.shiftKey) {
                      e.preventDefault();
                      void handleSend();
                    }
                  }}
                  disabled={!selectedGroup || busy}
                  className="min-w-0 flex-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-main)] px-2.5 py-1.5 text-[12px] text-[var(--text-base)] outline-none focus:border-[var(--accent)] disabled:opacity-50"
                  placeholder={intl.formatMessage({ id: "lanCollab.messagePlaceholder" })}
                />
                <button
                  type="button"
                  onClick={() => void handleSend()}
                  disabled={!selectedGroup || busy || !messageDraft.trim()}
                  className="inline-flex h-8 w-8 items-center justify-center rounded-[var(--radius-sm)] bg-[var(--accent)] text-white disabled:opacity-50"
                  title={intl.formatMessage({ id: "lanCollab.send" })}
                >
                  <IconSend size={14} stroke={1.8} />
                </button>
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}
