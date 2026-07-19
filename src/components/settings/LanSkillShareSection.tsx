import { useCallback, useEffect, useMemo, useState } from "react";
import { useIntl } from "react-intl";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  lanCollabInstallRemoteSkill,
  lanCollabShareSkill,
  lanCollabStatus,
  lanCollabUnshareSkill,
  type CollabGroup,
  type SharedSkillOffer,
} from "../../api/lanCollab";
import type { SkillSummary } from "../../types/skill";
import { LanShareGroupPicker, groupLabel } from "./LanShareGroupPicker";

interface LanSkillShareSectionProps {
  skills: SkillSummary[];
  onInstalled?: () => void;
}

export function LanSkillShareSection({ skills, onInstalled }: LanSkillShareSectionProps) {
  const intl = useIntl();
  const [enabled, setEnabled] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [shareSelection, setShareSelection] = useState("");
  const [shareGroupId, setShareGroupId] = useState("");
  const [groups, setGroups] = useState<CollabGroup[]>([]);
  const [localShared, setLocalShared] = useState<SharedSkillOffer[]>([]);
  const [remoteShared, setRemoteShared] = useState<SharedSkillOffer[]>([]);

  const shareableOptions = useMemo(() => {
    // 同一 Skill 可共享到不同协作组；仅过滤“当前所选组已共享”的条目
    const sharedInSelectedGroup = new Set(
      localShared
        .filter((item) => !shareGroupId || item.groupId === shareGroupId)
        .map((item) => item.skillId),
    );
    return skills.filter((skill) => !sharedInSelectedGroup.has(skill.id));
  }, [skills, localShared, shareGroupId]);

  const refresh = useCallback(async () => {
    try {
      const status = await lanCollabStatus();
      setEnabled(status.enabled);
      setGroups(status.groups ?? []);
      setLocalShared(status.localSharedSkills ?? []);
      setRemoteShared(status.remoteSharedSkills ?? []);
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
          await listen("lan-collab-skill-share", () => {
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
        throw new Error(intl.formatMessage({ id: "settings.lanShare.skillPickRequired" }));
      }
      if (!shareGroupId) {
        throw new Error(intl.formatMessage({ id: "settings.lanShare.groupPickRequired" }));
      }
      await lanCollabShareSkill({ skillId: shareSelection, groupId: shareGroupId });
      setShareSelection("");
      setNotice(intl.formatMessage({ id: "settings.lanShare.skillShareSuccess" }));
      await refresh();
    });

  const handleUnshare = (shareId: string) =>
    void runBusy(async () => {
      setNotice(null);
      await lanCollabUnshareSkill(shareId);
      setNotice(intl.formatMessage({ id: "settings.lanShare.skillUnshareSuccess" }));
      await refresh();
    });

  const handleInstall = (offer: SharedSkillOffer) =>
    void runBusy(async () => {
      setNotice(null);
      const exists = skills.some((skill) => skill.id === offer.skillId);
      let overwrite = false;
      if (exists) {
        overwrite = window.confirm(
          intl.formatMessage(
            { id: "settings.lanShare.skillOverwriteConfirm" },
            { id: offer.skillId },
          ),
        );
        if (!overwrite) return;
      }
      // 若本机已安装共享副本且用户选择覆盖，允许 force（后端会在 localModified 时再校验）
      let forceOverwrite = false;
      if (exists && overwrite) {
        forceOverwrite = window.confirm(
          intl.formatMessage(
            { id: "settings.lanShare.skillForceOverwriteConfirm" },
            { id: offer.skillId },
          ),
        );
      }
      const installedId = await lanCollabInstallRemoteSkill({
        hostNodeId: offer.hostNodeId,
        shareId: offer.shareId,
        overwrite,
        forceOverwrite: forceOverwrite || overwrite,
      });
      setNotice(
        intl.formatMessage(
          { id: "settings.lanShare.skillInstallSuccess" },
          { id: installedId },
        ),
      );
      onInstalled?.();
    });

  return (
    <section className="settings-card space-y-3">
      <div className="space-y-1">
        <h3 className="text-[13px] font-semibold text-[var(--text-strong)]">
          {intl.formatMessage({ id: "settings.lanShare.skillSection" })}
        </h3>
        <p className="text-[12px] leading-relaxed text-[var(--text-muted)]">
          {intl.formatMessage({ id: "settings.lanShare.skillHint" })}
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
              ? intl.formatMessage({ id: "settings.lanShare.skillPickPlaceholder" })
              : intl.formatMessage({ id: "settings.lanShare.skillNoLocal" })}
          </option>
          {shareableOptions.map((skill) => (
            <option key={skill.id} value={skill.id}>
              {skill.name || skill.id}
            </option>
          ))}
        </select>
        <button
          type="button"
          onClick={handleShare}
          disabled={!enabled || busy || !shareSelection || !shareGroupId || groups.length === 0}
          className="rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--accent-soft)] px-3 text-[11px] text-[var(--accent-strong)] disabled:opacity-50"
        >
          {intl.formatMessage({ id: "settings.lanShare.skillShare" })}
        </button>
      </div>

      <div>
        <div className="mb-1 text-[11px] font-medium text-[var(--text-muted)]">
          {intl.formatMessage({ id: "settings.lanShare.skillLocalShared" })}
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
                    {offer.name || offer.skillId}
                  </div>
                  <div className="truncate text-[10px] text-[var(--text-faint)]">
                    {offer.skillId}
                    {offer.description ? ` · ${offer.description}` : ""}
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
                  {intl.formatMessage({ id: "settings.lanShare.skillUnshare" })}
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-[11px] text-[var(--text-muted)]">
            {intl.formatMessage({ id: "settings.lanShare.skillNoLocalShared" })}
          </p>
        )}
      </div>

      <div>
        <div className="mb-1 text-[11px] font-medium text-[var(--text-muted)]">
          {intl.formatMessage({ id: "settings.lanShare.skillRemoteShared" })}
        </div>
        {remoteShared.length ? (
          <ul className="space-y-1">
            {remoteShared.map((offer) => (
              <li
                key={`${offer.hostNodeId}-${offer.shareId}`}
                className="flex items-center justify-between gap-2 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] px-2 py-1.5 text-[11px]"
              >
                <div className="min-w-0">
                  <div className="truncate font-medium text-[var(--text-strong)]">
                    {offer.name || offer.skillId}
                  </div>
                  <div className="truncate text-[10px] text-[var(--text-faint)]">
                    {offer.hostDisplayName} · {offer.skillId}
                    {offer.online
                      ? ""
                      : ` · ${intl.formatMessage({ id: "lanCollab.peerOffline" })}`}
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => handleInstall(offer)}
                  disabled={busy || !offer.online}
                  className="shrink-0 rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--accent-soft)] px-2 py-1 text-[10px] text-[var(--accent-strong)] disabled:opacity-50"
                >
                  {intl.formatMessage({ id: "settings.lanShare.skillInstall" })}
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-[11px] text-[var(--text-muted)]">
            {intl.formatMessage({ id: "settings.lanShare.skillNoRemoteShared" })}
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
