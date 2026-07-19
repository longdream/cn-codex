import {
  IconLink,
  IconNetwork,
  IconPlus,
  IconPlugConnected,
  IconRefresh,
  IconUsersGroup,
  IconX,
} from "@tabler/icons-react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import { useIntl } from "react-intl";
import {
  lanCollabConnectPeer,
  lanCollabCreateGroup,
  lanCollabJoinGroup,
  lanCollabRefreshScan,
  lanCollabSetDisplayName,
  lanCollabSetEnabled,
  lanCollabStatus,
  parseHostPort,
  type CollabGroup,
  type DiscoveredGroupSummary,
  type LanCollabStatus,
  type NearbyPeer,
} from "../../api/lanCollab";

function formatTime(ts: number): string {
  if (!ts) return "";
  try {
    return new Date(ts * 1000).toLocaleString();
  } catch {
    return String(ts);
  }
}

export function LanCollabPanel() {
  const intl = useIntl();
  const [status, setStatus] = useState<LanCollabStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [displayNameDraft, setDisplayNameDraft] = useState("");
  const [groupNameDraft, setGroupNameDraft] = useState("");
  const [inviteDraft, setInviteDraft] = useState("");
  const [connectDraft, setConnectDraft] = useState("");
  const [selectedGroupId, setSelectedGroupId] = useState<string | null>(null);
  const errorClearTimerRef = useRef<number | null>(null);

  const discoveredGroups = status?.discoveredGroups ?? [];
  const discovery = status?.discovery;

  const clearErrorSoon = useCallback((delayMs = 12000) => {
    if (errorClearTimerRef.current != null) {
      window.clearTimeout(errorClearTimerRef.current);
    }
    errorClearTimerRef.current = window.setTimeout(() => {
      setError(null);
      errorClearTimerRef.current = null;
    }, delayMs);
  }, []);

  const showError = useCallback(
    (message: string) => {
      setError(message);
      // 错误提示放顶部后仍需足够阅读时间；用户主动操作会立即覆盖/清除。
      clearErrorSoon(12000);
    },
    [clearErrorSoon],
  );

  const dismissError = useCallback(() => {
    if (errorClearTimerRef.current != null) {
      window.clearTimeout(errorClearTimerRef.current);
      errorClearTimerRef.current = null;
    }
    setError(null);
  }, []);

  const refresh = useCallback(async (options?: { clearError?: boolean }) => {
    // 后台轮询 / 事件刷新不要清错误，否则提示会“闪一下就没了”。
    if (options?.clearError) {
      dismissError();
    }
    try {
      const next = await lanCollabStatus();
      setStatus(next);
      setDisplayNameDraft(next.identity.displayName);
      if (selectedGroupId && !next.groups.some((g) => g.groupId === selectedGroupId)) {
        setSelectedGroupId(next.groups[0]?.groupId ?? null);
      } else if (!selectedGroupId && next.groups.length > 0) {
        setSelectedGroupId(next.groups[0].groupId);
      }
    } catch (err) {
      showError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, [dismissError, selectedGroupId, showError]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    return () => {
      if (errorClearTimerRef.current != null) {
        window.clearTimeout(errorClearTimerRef.current);
      }
    };
  }, []);

  // 后端推送：对端 / 组 / 发现变更
  useEffect(() => {
    let disposed = false;
    const unlisteners: UnlistenFn[] = [];

    const setup = async () => {
      try {
        unlisteners.push(
          await listen<NearbyPeer>("lan-collab-peer", () => {
            if (!disposed) void refresh();
          }),
        );
        unlisteners.push(
          await listen<CollabGroup>("lan-collab-group", () => {
            if (!disposed) void refresh();
          }),
        );
        unlisteners.push(
          await listen<DiscoveredGroupSummary[]>("lan-collab-discovery", () => {
            if (!disposed) void refresh();
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
  }, [refresh, showError]);

  // 轮询兜底（事件丢失时仍能看到连接/组状态）
  useEffect(() => {
    if (!status?.enabled) return;
    const timer = window.setInterval(() => {
      void refresh();
    }, 4000);
    return () => window.clearInterval(timer);
  }, [status?.enabled, refresh]);

  const runBusy = async (fn: () => Promise<void>) => {
    setBusy(true);
    dismissError();
    try {
      await fn();
    } catch (err) {
      showError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const handleToggle = () =>
    void runBusy(async () => {
      const next = await lanCollabSetEnabled(!(status?.enabled ?? false));
      setStatus(next);
    });

  const handleSaveName = () =>
    void runBusy(async () => {
      await lanCollabSetDisplayName(displayNameDraft);
      await refresh();
    });

  const handleConnect = () =>
    void runBusy(async () => {
      const parsed = parseHostPort(connectDraft);
      if (!parsed) {
        throw new Error(intl.formatMessage({ id: "lanCollab.connectInvalid" }));
      }
      await lanCollabConnectPeer(parsed.host, parsed.port);
      setConnectDraft("");
      await refresh();
    });

  const handleScanNow = () =>
    void runBusy(async () => {
      const next = await lanCollabRefreshScan();
      setStatus(next);
    });

  const handleCreateGroup = () =>
    void runBusy(async () => {
      const group = await lanCollabCreateGroup(groupNameDraft);
      setGroupNameDraft("");
      setSelectedGroupId(group.groupId);
      await refresh();
    });

  const handleJoinGroup = () =>
    void runBusy(async () => {
      const group = await lanCollabJoinGroup(inviteDraft);
      setInviteDraft("");
      setSelectedGroupId(group.groupId);
      await refresh();
    });

  if (loading && !status) {
    return (
      <div className="flex flex-1 items-center justify-center p-4 text-xs text-[var(--text-muted)]">
        {intl.formatMessage({ id: "lanCollab.loading" })}
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <div className="flex items-center gap-2 border-b border-[var(--border-subtle)] px-3 py-2">
        <IconNetwork size={14} stroke={1.8} className="text-[var(--accent)]" />
        <span className="min-w-0 flex-1 truncate text-xs font-semibold text-[var(--text-strong)]">
          {intl.formatMessage({ id: "lanCollab.title" })}
        </span>
        {status?.enabled && (
          <span className="rounded-full bg-[var(--accent-soft)] px-2 py-0.5 text-[10px] text-[var(--accent-strong)]">
            {intl.formatMessage(
              { id: "lanCollab.connectedCount" },
              { count: status.connectedPeerCount ?? 0 },
            )}
          </span>
        )}
        <button
          type="button"
          onClick={() => void refresh({ clearError: true })}
          disabled={busy}
          className="flex h-6 w-6 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)] disabled:opacity-50"
          title={intl.formatMessage({ id: "lanCollab.refresh" })}
        >
          <IconRefresh size={14} stroke={1.8} />
        </button>
      </div>

      {error && (
        <div
          role="alert"
          className="z-20 flex shrink-0 items-start gap-2 border-b border-[var(--danger)]/30 bg-[var(--danger-soft)] px-3 py-2 text-[11px] text-[var(--danger)] shadow-[0_4px_12px_rgba(0,0,0,0.08)]"
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

      <div className="thin-scrollbar flex min-h-0 flex-1 flex-col gap-3 overflow-auto p-3">
        <section className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-main)] p-3">
          <div className="mb-2 flex items-start justify-between gap-3">
            <div className="min-w-0 flex-1">
              <div className="text-[12px] font-semibold text-[var(--text-strong)]">
                {intl.formatMessage({ id: "lanCollab.nodeSection" })}
              </div>
              <div
                className="mt-0.5 truncate font-mono text-[10px] text-[var(--text-faint)]"
                title={status?.identity.nodeId}
              >
                {status?.identity.nodeId}
              </div>
            </div>
            <button
              type="button"
              onClick={handleToggle}
              disabled={busy}
              className={`inline-flex h-7 min-w-[4.75rem] flex-none items-center justify-center whitespace-nowrap rounded-full px-3.5 text-[11px] font-medium leading-none transition-colors disabled:opacity-50 ${
                status?.enabled
                  ? "bg-[var(--accent)] text-white"
                  : "border border-[var(--border-subtle)] bg-[var(--surface-panel)] text-[var(--text-base)] hover:bg-[var(--surface-elevated)]"
              }`}
            >
              {status?.enabled
                ? intl.formatMessage({ id: "lanCollab.enabled" })
                : intl.formatMessage({ id: "lanCollab.disabled" })}
            </button>
          </div>

          <div className="mb-2 flex gap-2">
            <input
              type="text"
              value={displayNameDraft}
              onChange={(e) => setDisplayNameDraft(e.target.value)}
              className="min-w-0 flex-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] px-2 py-1.5 text-[12px] text-[var(--text-base)] outline-none focus:border-[var(--accent)]"
              placeholder={intl.formatMessage({ id: "lanCollab.displayNamePlaceholder" })}
            />
            <button
              type="button"
              onClick={handleSaveName}
              disabled={busy}
              className="rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--accent-soft)] px-2.5 text-[11px] text-[var(--accent-strong)] disabled:opacity-50"
            >
              {intl.formatMessage({ id: "lanCollab.saveName" })}
            </button>
          </div>

          <p className="text-[11px] leading-relaxed text-[var(--text-muted)]">{status?.note}</p>
          {status?.localAddress && (
            <p className="mt-1 font-mono text-[10px] text-[var(--text-faint)]">
              {intl.formatMessage(
                { id: "lanCollab.localAddress" },
                { address: status.localAddress },
              )}
            </p>
          )}
          <p className="mt-1 text-[10px] text-[var(--text-faint)]">
            {intl.formatMessage(
              { id: "lanCollab.architecture" },
              { mode: status?.architecture ?? "weak-center-owner-plus-p2p" },
            )}
          </p>
        </section>

        <section className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-main)] p-3">
          <div className="mb-2 flex items-center justify-between gap-2">
            <div className="flex items-center gap-1.5 text-[12px] font-semibold text-[var(--text-strong)]">
              <IconUsersGroup size={14} stroke={1.8} className="text-[var(--accent)]" />
              {intl.formatMessage({ id: "lanCollab.discoverSection" })}
            </div>
            <button
              type="button"
              onClick={handleScanNow}
              disabled={!status?.enabled || busy}
              className="inline-flex items-center gap-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] px-2 py-1 text-[11px] text-[var(--text-base)] disabled:opacity-50"
            >
              <IconRefresh size={12} stroke={1.8} />
              {intl.formatMessage({ id: "lanCollab.scanNow" })}
            </button>
          </div>
          <p className="mb-2 text-[11px] leading-relaxed text-[var(--text-muted)]">
            {intl.formatMessage({ id: "lanCollab.discoverHint" })}
          </p>
          {discovery?.scanning && (
            <p className="mb-2 text-[10px] text-[var(--accent-strong)]">
              {intl.formatMessage({ id: "lanCollab.discoverScanning" })}
            </p>
          )}
          {discovery?.lastScanAt ? (
            <p className="mb-2 text-[10px] text-[var(--text-faint)]">
              {intl.formatMessage(
                { id: "lanCollab.discoverLastScan" },
                { time: formatTime(discovery.lastScanAt) },
              )}
            </p>
          ) : null}

          <div className="mb-2 flex gap-2">
            <input
              type="text"
              value={inviteDraft}
              onChange={(e) => setInviteDraft(e.target.value)}
              disabled={!status?.enabled || busy}
              className="min-w-0 flex-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] px-2 py-1.5 font-mono text-[12px] outline-none focus:border-[var(--accent)] disabled:opacity-50"
              placeholder={intl.formatMessage({ id: "lanCollab.invitePlaceholder" })}
            />
            <button
              type="button"
              onClick={handleJoinGroup}
              disabled={!status?.enabled || busy || !inviteDraft.trim()}
              className="inline-flex items-center gap-1 rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--accent-soft)] px-2.5 text-[11px] text-[var(--accent-strong)] disabled:opacity-50"
            >
              <IconLink size={12} stroke={1.8} />
              {intl.formatMessage({ id: "lanCollab.joinGroup" })}
            </button>
          </div>

          {discoveredGroups.length > 0 ? (
            <ul className="space-y-1">
              {discoveredGroups.map((group) => (
                <li
                  key={`${group.groupId}-${group.ownerNodeId}`}
                  className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] px-2.5 py-2 text-[11px]"
                >
                  <div className="flex items-center justify-between gap-2">
                    <span className="truncate font-medium text-[var(--text-strong)]">
                      {group.name}
                    </span>
                    <span className="text-[10px] text-[var(--text-faint)]">
                      {intl.formatMessage(
                        { id: "lanCollab.memberCount" },
                        { count: group.memberCount },
                      )}
                    </span>
                  </div>
                  <div className="mt-0.5 truncate text-[10px] text-[var(--text-faint)]">
                    {group.ownerDisplayName} · {group.ownerAddress}:{group.ownerPort}
                  </div>
                </li>
              ))}
            </ul>
          ) : (
            <p className="text-[11px] text-[var(--text-muted)]">
              {intl.formatMessage({ id: "lanCollab.discoverEmpty" })}
            </p>
          )}
        </section>

        <section className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-main)] p-3">
          <div className="mb-1 flex items-center gap-1.5 text-[12px] font-semibold text-[var(--text-strong)]">
            <IconPlugConnected size={14} stroke={1.8} className="text-[var(--accent)]" />
            {intl.formatMessage({ id: "lanCollab.connectSection" })}
          </div>
          <p className="mb-2 text-[11px] leading-relaxed text-[var(--text-muted)]">
            {intl.formatMessage({ id: "lanCollab.connectHint" })}
          </p>
          <div className="flex gap-2">
            <input
              type="text"
              value={connectDraft}
              onChange={(e) => setConnectDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  handleConnect();
                }
              }}
              disabled={!status?.enabled || busy}
              className="min-w-0 flex-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] px-2 py-1.5 font-mono text-[12px] outline-none focus:border-[var(--accent)] disabled:opacity-50"
              placeholder={intl.formatMessage({ id: "lanCollab.connectPlaceholder" })}
            />
            <button
              type="button"
              onClick={handleConnect}
              disabled={!status?.enabled || busy || !connectDraft.trim()}
              className="inline-flex items-center gap-1 rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--accent-soft)] px-2.5 text-[11px] text-[var(--accent-strong)] disabled:opacity-50"
            >
              <IconPlugConnected size={12} stroke={1.8} />
              {intl.formatMessage({ id: "lanCollab.connect" })}
            </button>
          </div>
        </section>

        <section className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-main)] p-3">
          <div className="mb-2 flex items-center gap-1.5 text-[12px] font-semibold text-[var(--text-strong)]">
            <IconUsersGroup size={14} stroke={1.8} className="text-[var(--accent)]" />
            {intl.formatMessage({ id: "lanCollab.groupsSection" })}
          </div>
          <div className="mb-2 flex gap-2">
            <input
              type="text"
              value={groupNameDraft}
              onChange={(e) => setGroupNameDraft(e.target.value)}
              disabled={!status?.enabled || busy}
              className="min-w-0 flex-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] px-2 py-1.5 text-[12px] outline-none focus:border-[var(--accent)] disabled:opacity-50"
              placeholder={intl.formatMessage({ id: "lanCollab.groupNamePlaceholder" })}
            />
            <button
              type="button"
              onClick={handleCreateGroup}
              disabled={!status?.enabled || busy || !groupNameDraft.trim()}
              className="inline-flex items-center gap-1 rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--accent-soft)] px-2.5 text-[11px] text-[var(--accent-strong)] disabled:opacity-50"
            >
              <IconPlus size={12} stroke={1.8} />
              {intl.formatMessage({ id: "lanCollab.createGroup" })}
            </button>
          </div>

          {status?.groups.length ? (
            <div className="space-y-1.5">
              {status.groups.map((group) => {
                const active = group.groupId === selectedGroupId;
                return (
                  <button
                    key={group.groupId}
                    type="button"
                    onClick={() => setSelectedGroupId(group.groupId)}
                    className={`w-full rounded-[var(--radius-sm)] border px-2.5 py-2 text-left transition-colors ${
                      active
                        ? "border-[var(--accent-border)] bg-[var(--accent-soft)]"
                        : "border-[var(--border-subtle)] bg-[var(--surface-panel)] hover:bg-[var(--surface-elevated)]"
                    }`}
                  >
                    <div className="flex items-center justify-between gap-2">
                      <span className="truncate text-[12px] font-medium text-[var(--text-strong)]">
                        {group.name}
                      </span>
                      <span className="text-[10px] text-[var(--text-faint)]">
                        {group.isOwner
                          ? intl.formatMessage({ id: "lanCollab.roleOwner" })
                          : intl.formatMessage({ id: "lanCollab.roleMember" })}
                      </span>
                    </div>
                    <div className="mt-0.5 flex flex-wrap gap-x-2 gap-y-0.5 text-[10px] text-[var(--text-faint)]">
                      <span>
                        {intl.formatMessage(
                          { id: "lanCollab.memberCount" },
                          { count: group.members.length },
                        )}
                      </span>
                      {group.inviteCode && (
                        <span className="font-mono">
                          {intl.formatMessage(
                            { id: "lanCollab.inviteCode" },
                            { code: group.inviteCode },
                          )}
                        </span>
                      )}
                      {!group.ownerOnline && (
                        <span className="text-[var(--warning)]">
                          {intl.formatMessage({ id: "lanCollab.ownerOffline" })}
                        </span>
                      )}
                    </div>
                  </button>
                );
              })}
            </div>
          ) : (
            <p className="text-[11px] text-[var(--text-muted)]">
              {intl.formatMessage({ id: "lanCollab.noGroups" })}
            </p>
          )}
        </section>

        <section className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-main)] p-3">
          <div className="mb-1 text-[12px] font-semibold text-[var(--text-strong)]">
            {intl.formatMessage({ id: "lanCollab.peersSection" })}
          </div>
          {status?.peers.length ? (
            <ul className="space-y-1">
              {status.peers.map((peer) => (
                <li
                  key={`${peer.nodeId}:${peer.port}`}
                  className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] px-2 py-1.5 text-[11px]"
                >
                  <div className="flex items-center justify-between gap-2">
                    <div className="min-w-0 truncate font-medium text-[var(--text-strong)]">
                      {peer.displayName}
                    </div>
                    <span
                      className={`text-[10px] ${
                        peer.connected ? "text-[var(--accent-strong)]" : "text-[var(--text-faint)]"
                      }`}
                    >
                      {peer.connected
                        ? intl.formatMessage({ id: "lanCollab.peerConnected" })
                        : intl.formatMessage({ id: "lanCollab.peerOffline" })}
                    </span>
                  </div>
                  <div className="font-mono text-[10px] text-[var(--text-faint)]">
                    {peer.address}:{peer.port}
                  </div>
                </li>
              ))}
            </ul>
          ) : (
            <p className="text-[11px] text-[var(--text-muted)]">
              {intl.formatMessage({ id: "lanCollab.noPeers" })}
            </p>
          )}
        </section>
      </div>
    </div>
  );
}
