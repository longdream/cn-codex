import { useCallback, useEffect, useMemo, useState } from "react";
import { useIntl } from "react-intl";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  lanCollabInstallRemoteWorkflow,
  lanCollabListWorkflowShareOrigins,
  lanCollabShareWorkflow,
  lanCollabStatus,
  lanCollabUnshareWorkflow,
  type CollabGroup,
  type SharedWorkflowOffer,
  type WorkflowOriginSummary,
} from "../../api/lanCollab";
import type { WorkflowSummary } from "../../api/workflow";
import { LanShareGroupPicker, groupLabel } from "./LanShareGroupPicker";

interface LanWorkflowShareSectionProps {
  workflows: WorkflowSummary[];
  onInstalled?: () => void;
}

function updateLabel(
  intl: ReturnType<typeof useIntl>,
  origin: WorkflowOriginSummary | undefined,
  remoteHash?: string,
): string | null {
  if (!origin) return null;
  const remote = (remoteHash || "").trim();
  const installed = origin.installedContentHash;
  if (origin.localModified && remote && remote !== installed) {
    return intl.formatMessage({ id: "settings.lanShare.updateConflict" });
  }
  if (origin.localModified) {
    return intl.formatMessage({ id: "settings.lanShare.updateLocalModified" });
  }
  if (remote && remote !== installed) {
    return intl.formatMessage({ id: "settings.lanShare.updateHasUpdate" });
  }
  if (remote && remote === installed) {
    return intl.formatMessage({ id: "settings.lanShare.updateUpToDate" });
  }
  return intl.formatMessage(
    { id: "settings.lanShare.fromHost" },
    { host: origin.sourceHostDisplayName },
  );
}

export function LanWorkflowShareSection({
  workflows,
  onInstalled,
}: LanWorkflowShareSectionProps) {
  const intl = useIntl();
  const [enabled, setEnabled] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [shareSelection, setShareSelection] = useState("");
  const [shareGroupId, setShareGroupId] = useState("");
  const [groups, setGroups] = useState<CollabGroup[]>([]);
  const [localShared, setLocalShared] = useState<SharedWorkflowOffer[]>([]);
  const [remoteShared, setRemoteShared] = useState<SharedWorkflowOffer[]>([]);
  const [origins, setOrigins] = useState<WorkflowOriginSummary[]>([]);

  const originByName = useMemo(() => {
    const map = new Map<string, WorkflowOriginSummary>();
    for (const item of origins) {
      map.set(item.workflowName, item);
    }
    return map;
  }, [origins]);

  const shareableOptions = useMemo(() => {
    // 同一 Workflow 可共享到不同协作组
    const sharedInSelectedGroup = new Set(
      localShared
        .filter((item) => !shareGroupId || item.groupId === shareGroupId)
        .map((item) => item.workflowName),
    );
    return workflows.filter((wf) => !sharedInSelectedGroup.has(wf.name));
  }, [workflows, localShared, shareGroupId]);

  const refresh = useCallback(async () => {
    try {
      const [status, originList] = await Promise.all([
        lanCollabStatus(),
        lanCollabListWorkflowShareOrigins().catch(() => [] as WorkflowOriginSummary[]),
      ]);
      setEnabled(status.enabled);
      setGroups(status.groups ?? []);
      setLocalShared(status.localSharedWorkflows ?? []);
      setRemoteShared(status.remoteSharedWorkflows ?? []);
      setOrigins(originList);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    let disposed = false;
    const unlisteners: UnlistenFn[] = [];
    const setup = async () => {
      try {
        unlisteners.push(
          await listen("lan-collab-workflow-share", () => {
            if (!disposed) void refresh();
          }),
        );
        unlisteners.push(
          await listen("lan-collab-peer", () => {
            if (!disposed) void refresh();
          }),
        );
      } catch {
        // ignore
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
  }, [refresh]);

  const runBusy = async (fn: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await fn();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const handleShare = () =>
    void runBusy(async () => {
      setNotice(null);
      if (!shareSelection) {
        throw new Error(intl.formatMessage({ id: "settings.lanShare.workflowPickRequired" }));
      }
      if (!shareGroupId) {
        throw new Error(intl.formatMessage({ id: "settings.lanShare.groupPickRequired" }));
      }
      await lanCollabShareWorkflow({
        workflowName: shareSelection,
        groupId: shareGroupId,
      });
      setShareSelection("");
      setNotice(intl.formatMessage({ id: "settings.lanShare.workflowShareSuccess" }));
      await refresh();
    });

  const handleUnshare = (shareId: string) =>
    void runBusy(async () => {
      setNotice(null);
      await lanCollabUnshareWorkflow(shareId);
      setNotice(intl.formatMessage({ id: "settings.lanShare.workflowUnshareSuccess" }));
      await refresh();
    });

  const handleInstall = (offer: SharedWorkflowOffer) =>
    void runBusy(async () => {
      setNotice(null);
      const exists = workflows.some((wf) => wf.name === offer.workflowName);
      const origin = originByName.get(offer.workflowName);
      let overwrite = false;
      let forceOverwrite = false;

      if (exists) {
        if (origin?.localModified) {
          const ok = window.confirm(
            intl.formatMessage(
              { id: "settings.lanShare.workflowForceOverwriteConfirm" },
              { id: offer.workflowName },
            ),
          );
          if (!ok) return;
          overwrite = true;
          forceOverwrite = true;
        } else {
          overwrite = window.confirm(
            intl.formatMessage(
              { id: "settings.lanShare.workflowOverwriteConfirm" },
              { id: offer.workflowName },
            ),
          );
          if (!overwrite) return;
        }
      }

      const installedName = await lanCollabInstallRemoteWorkflow({
        hostNodeId: offer.hostNodeId,
        shareId: offer.shareId,
        overwrite,
        forceOverwrite,
      });
      setNotice(
        intl.formatMessage(
          { id: "settings.lanShare.workflowInstallSuccess" },
          { id: installedName },
        ),
      );
      onInstalled?.();
      await refresh();
    });

  return (
    <section className="settings-card space-y-3">
      <div className="space-y-1">
        <h3 className="text-[13px] font-semibold text-[var(--text-strong)]">
          {intl.formatMessage({ id: "settings.lanShare.workflowSection" })}
        </h3>
        <p className="text-[12px] leading-relaxed text-[var(--text-muted)]">
          {intl.formatMessage({ id: "settings.lanShare.workflowHint" })}
        </p>
        {!enabled && (
          <p className="text-[11px] text-[var(--warning)]">
            {intl.formatMessage({ id: "settings.lanShare.needEnable" })}
          </p>
        )}
      </div>

      <LanShareGroupPicker
        groups={groups}
        value={shareGroupId}
        onChange={setShareGroupId}
        disabled={!enabled || busy}
        emptyLabel={intl.formatMessage({ id: "settings.lanShare.groupEmpty" })}
        placeholder={intl.formatMessage({ id: "settings.lanShare.groupPickPlaceholder" })}
      />
      <p className="text-[11px] text-[var(--text-faint)]">
        {intl.formatMessage({ id: "settings.lanShare.groupHint" })}
      </p>

      <div className="flex flex-wrap gap-2">
        <select
          value={shareSelection}
          onChange={(e) => setShareSelection(e.target.value)}
          disabled={!enabled || busy || shareableOptions.length === 0}
          className="min-w-0 flex-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] px-2 py-1.5 text-[12px] outline-none focus:border-[var(--accent)] disabled:opacity-50"
        >
          <option value="">
            {shareableOptions.length
              ? intl.formatMessage({ id: "settings.lanShare.workflowPickPlaceholder" })
              : intl.formatMessage({ id: "settings.lanShare.workflowNoLocal" })}
          </option>
          {shareableOptions.map((wf) => (
            <option key={wf.name} value={wf.name}>
              {wf.title || wf.name}
            </option>
          ))}
        </select>
        <button
          type="button"
          onClick={handleShare}
          disabled={!enabled || busy || !shareSelection || !shareGroupId || groups.length === 0}
          className="rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--accent-soft)] px-3 text-[11px] text-[var(--accent-strong)] disabled:opacity-50"
        >
          {intl.formatMessage({ id: "settings.lanShare.workflowShare" })}
        </button>
      </div>

      <div>
        <div className="mb-1 text-[11px] font-medium text-[var(--text-muted)]">
          {intl.formatMessage({ id: "settings.lanShare.workflowLocalShared" })}
        </div>
        {localShared.length ? (
          <ul className="space-y-1">
            {localShared.map((offer) => (
              <li
                key={offer.shareId}
                className="flex items-center justify-between gap-2 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] px-2 py-1.5 text-[11px]"
              >
                <div className="min-w-0">
                  <div className="truncate font-medium text-[var(--text-strong)]">
                    {offer.title || offer.workflowName}
                  </div>
                  <div className="truncate text-[10px] text-[var(--text-faint)]">
                    {offer.workflowName}
                    {offer.contentHash ? ` · ${offer.contentHash.slice(0, 18)}…` : ""}
                    {` · ${intl.formatMessage(
                      { id: "settings.lanShare.sharedToGroup" },
                      { group: groupLabel(groups, offer.groupId) },
                    )}`}
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => handleUnshare(offer.shareId)}
                  disabled={busy}
                  className="shrink-0 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] px-2 py-1 text-[10px] text-[var(--text-muted)] hover:bg-[var(--surface-elevated)] disabled:opacity-50"
                >
                  {intl.formatMessage({ id: "settings.lanShare.workflowUnshare" })}
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-[11px] text-[var(--text-muted)]">
            {intl.formatMessage({ id: "settings.lanShare.workflowNoLocalShared" })}
          </p>
        )}
      </div>

      <div>
        <div className="mb-1 text-[11px] font-medium text-[var(--text-muted)]">
          {intl.formatMessage({ id: "settings.lanShare.workflowRemoteShared" })}
        </div>
        {remoteShared.length ? (
          <ul className="space-y-1">
            {remoteShared.map((offer) => {
              const origin = originByName.get(offer.workflowName);
              const badge = updateLabel(intl, origin, offer.contentHash);
              const isInstalled = Boolean(origin);
              return (
                <li
                  key={`${offer.hostNodeId}-${offer.shareId}`}
                  className="flex items-center justify-between gap-2 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] px-2 py-1.5 text-[11px]"
                >
                  <div className="min-w-0">
                    <div className="truncate font-medium text-[var(--text-strong)]">
                      {offer.title || offer.workflowName}
                    </div>
                    <div className="truncate text-[10px] text-[var(--text-faint)]">
                      {offer.hostDisplayName} · {offer.workflowName}
                      {offer.online
                        ? ""
                        : ` · ${intl.formatMessage({ id: "lanCollab.peerOffline" })}`}
                      {badge ? ` · ${badge}` : ""}
                    </div>
                  </div>
                  <button
                    type="button"
                    onClick={() => handleInstall(offer)}
                    disabled={busy || !offer.online}
                    className="shrink-0 rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--accent-soft)] px-2 py-1 text-[10px] text-[var(--accent-strong)] disabled:opacity-50"
                  >
                    {isInstalled
                      ? intl.formatMessage({ id: "settings.lanShare.workflowUpdate" })
                      : intl.formatMessage({ id: "settings.lanShare.workflowInstall" })}
                  </button>
                </li>
              );
            })}
          </ul>
        ) : (
          <p className="text-[11px] text-[var(--text-muted)]">
            {intl.formatMessage({ id: "settings.lanShare.workflowNoRemoteShared" })}
          </p>
        )}
      </div>

      {notice && (
        <div className="rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--accent-soft)] px-2.5 py-1.5 text-[11px] text-[var(--accent-strong)]">
          {notice}
        </div>
      )}
      {error && (
        <div className="rounded-[var(--radius-sm)] border border-[var(--danger)]/30 bg-[var(--danger-soft)] px-2.5 py-1.5 text-[11px] text-[var(--danger)]">
          {error}
        </div>
      )}
    </section>
  );
}
