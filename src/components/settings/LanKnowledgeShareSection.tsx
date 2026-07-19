import { useCallback, useEffect, useState } from "react";
import { useIntl } from "react-intl";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  lanCollabFetchRemoteKnowledge,
  lanCollabListShareableKnowledgeDocs,
  lanCollabSearchRemoteKnowledge,
  lanCollabShareKnowledge,
  lanCollabStatus,
  lanCollabUnshareKnowledge,
  type RemoteKnowledgeDoc,
  type RemoteKnowledgeHit,
  type SharedKnowledgeDocMeta,
  type SharedKnowledgeOffer,
  type CollabGroup,
} from "../../api/lanCollab";
import { LanShareGroupPicker, groupLabel } from "./LanShareGroupPicker";

export function LanKnowledgeShareSection() {
  const intl = useIntl();
  const [enabled, setEnabled] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [titleDraft, setTitleDraft] = useState("");
  const [shareGroupId, setShareGroupId] = useState("");
  const [groups, setGroups] = useState<CollabGroup[]>([]);
  const [shareableDocs, setShareableDocs] = useState<SharedKnowledgeDocMeta[]>([]);
  const [selectedDocIds, setSelectedDocIds] = useState<string[]>([]);
  const [localShared, setLocalShared] = useState<SharedKnowledgeOffer[]>([]);
  const [remoteShared, setRemoteShared] = useState<SharedKnowledgeOffer[]>([]);
  const [searchTarget, setSearchTarget] = useState("");
  const [searchDraft, setSearchDraft] = useState("");
  const [hits, setHits] = useState<RemoteKnowledgeHit[]>([]);
  const [preview, setPreview] = useState<RemoteKnowledgeDoc | null>(null);

  const refresh = useCallback(async () => {
    try {
      const status = await lanCollabStatus();
      setEnabled(status.enabled);
      setGroups(status.groups ?? []);
      setLocalShared(status.localSharedKnowledge ?? []);
      setRemoteShared(status.remoteSharedKnowledge ?? []);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  const refreshShareableDocs = useCallback(async () => {
    try {
      const docs = await lanCollabListShareableKnowledgeDocs();
      setShareableDocs(docs);
    } catch {
      setShareableDocs([]);
    }
  }, []);

  useEffect(() => {
    void refresh();
    void refreshShareableDocs();
  }, [refresh, refreshShareableDocs]);

  useEffect(() => {
    let disposed = false;
    const unlisteners: UnlistenFn[] = [];
    const setup = async () => {
      try {
        unlisteners.push(
          await listen("lan-collab-knowledge-share", () => {
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

  const toggleDocSelection = (docId: string) => {
    setSelectedDocIds((prev) =>
      prev.includes(docId) ? prev.filter((id) => id !== docId) : [...prev, docId],
    );
  };

  const handleShare = () =>
    void runBusy(async () => {
      setNotice(null);
      if (!shareGroupId) {
        throw new Error(intl.formatMessage({ id: "settings.lanShare.groupPickRequired" }));
      }
      if (selectedDocIds.length === 0) {
        throw new Error(intl.formatMessage({ id: "lanCollab.kbDocsRequired" }));
      }
      const title =
        titleDraft.trim() || intl.formatMessage({ id: "lanCollab.kbDefaultTitle" });
      await lanCollabShareKnowledge({
        title,
        groupId: shareGroupId,
        docIds: selectedDocIds,
      });
      setTitleDraft("");
      setSelectedDocIds([]);
      setNotice(intl.formatMessage({ id: "lanCollab.kbShareSuccess" }));
      await refresh();
      await refreshShareableDocs();
    });

  const handleUnshare = (shareId: string) =>
    void runBusy(async () => {
      setNotice(null);
      await lanCollabUnshareKnowledge(shareId);
      setNotice(intl.formatMessage({ id: "lanCollab.kbUnshareSuccess" }));
      await refresh();
    });

  const handleSearch = () =>
    void runBusy(async () => {
      setNotice(null);
      setPreview(null);
      const target = remoteShared.find(
        (offer) => `${offer.hostNodeId}::${offer.shareId}` === searchTarget,
      );
      if (!target) {
        throw new Error(intl.formatMessage({ id: "lanCollab.kbSearchPickRequired" }));
      }
      if (!searchDraft.trim()) {
        throw new Error(intl.formatMessage({ id: "lanCollab.kbSearchQueryRequired" }));
      }
      const nextHits = await lanCollabSearchRemoteKnowledge({
        hostNodeId: target.hostNodeId,
        shareId: target.shareId,
        query: searchDraft.trim(),
        topK: 8,
      });
      setHits(nextHits);
      setNotice(
        intl.formatMessage({ id: "lanCollab.kbSearchDone" }, { count: nextHits.length }),
      );
    });

  const handleFetch = (hit: RemoteKnowledgeHit) =>
    void runBusy(async () => {
      setNotice(null);
      const doc = await lanCollabFetchRemoteKnowledge({
        hostNodeId: hit.hostNodeId,
        shareId: hit.shareId,
        docId: hit.docId,
      });
      setPreview(doc);
      setNotice(intl.formatMessage({ id: "lanCollab.kbFetchSuccess" }, { title: doc.title }));
    });

  return (
    <section className="settings-card space-y-3">
      <div className="space-y-1">
        <h3 className="text-[13px] font-semibold text-[var(--text-strong)]">
          {intl.formatMessage({ id: "lanCollab.kbShareSection" })}
        </h3>
        <p className="text-[12px] leading-relaxed text-[var(--text-muted)]">
          {intl.formatMessage({ id: "lanCollab.kbShareHint" })}
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
        <input
          type="text"
          value={titleDraft}
          onChange={(e) => setTitleDraft(e.target.value)}
          disabled={!enabled || busy}
          className="min-w-0 flex-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] px-2 py-1.5 text-[12px] outline-none focus:border-[var(--accent)] disabled:opacity-50"
          placeholder={intl.formatMessage({ id: "lanCollab.kbTitlePlaceholder" })}
        />
        <button
          type="button"
          onClick={handleShare}
          disabled={
            !enabled || busy || shareableDocs.length === 0 || !shareGroupId || groups.length === 0 || selectedDocIds.length === 0
          }
          className="rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--accent-soft)] px-3 text-[11px] text-[var(--accent-strong)] disabled:opacity-50"
        >
          {intl.formatMessage({ id: "lanCollab.shareKnowledge" })}
        </button>
      </div>

      {shareableDocs.length === 0 ? (
        <p className="text-[11px] text-[var(--text-muted)]">
          {intl.formatMessage({ id: "lanCollab.kbNoLocalDocs" })}
        </p>
      ) : (
        <div className="space-y-1">
          <p className="text-[11px] text-[var(--text-faint)]">
            {intl.formatMessage({ id: "lanCollab.kbShareSelectHint" })}
          </p>
          <div className="thin-scrollbar max-h-36 space-y-1 overflow-auto rounded-[var(--radius-sm)] border border-[var(--border-subtle)] p-2">
            {shareableDocs.slice(0, 40).map((doc) => (
              <label
                key={doc.docId}
                className="flex cursor-pointer items-center gap-2 rounded px-1 py-0.5 text-[11px] hover:bg-[var(--surface-elevated)]"
              >
                <input
                  type="checkbox"
                  checked={selectedDocIds.includes(doc.docId)}
                  onChange={() => toggleDocSelection(doc.docId)}
                  disabled={!enabled || busy}
                />
                <span className="min-w-0 truncate text-[var(--text-base)]">{doc.title}</span>
              </label>
            ))}
          </div>
        </div>
      )}

      <div>
        <div className="mb-1 text-[11px] font-medium text-[var(--text-muted)]">
          {intl.formatMessage({ id: "lanCollab.kbLocalShared" })}
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
                    {offer.title}
                  </div>
                  <div className="text-[10px] text-[var(--text-faint)]">
                    {intl.formatMessage({ id: "lanCollab.kbDocCount" }, { count: offer.docCount })}
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
                  {intl.formatMessage({ id: "lanCollab.unshareKnowledge" })}
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-[11px] text-[var(--text-muted)]">
            {intl.formatMessage({ id: "lanCollab.kbNoLocalShared" })}
          </p>
        )}
      </div>

      <div>
        <div className="mb-1 text-[11px] font-medium text-[var(--text-muted)]">
          {intl.formatMessage({ id: "lanCollab.kbRemoteShared" })}
        </div>
        {remoteShared.length ? (
          <ul className="mb-2 space-y-1">
            {remoteShared.map((offer) => (
              <li
                key={`${offer.hostNodeId}-${offer.shareId}`}
                className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] px-2 py-1.5 text-[11px]"
              >
                <div className="truncate font-medium text-[var(--text-strong)]">{offer.title}</div>
                <div className="truncate text-[10px] text-[var(--text-faint)]">
                  {offer.hostDisplayName} ·{" "}
                  {intl.formatMessage({ id: "lanCollab.kbDocCount" }, { count: offer.docCount })}
                  {offer.online
                    ? ""
                    : ` · ${intl.formatMessage({ id: "lanCollab.peerOffline" })}`}
                </div>
              </li>
            ))}
          </ul>
        ) : (
          <p className="mb-2 text-[11px] text-[var(--text-muted)]">
            {intl.formatMessage({ id: "lanCollab.kbNoRemoteShared" })}
          </p>
        )}

        <div className="flex flex-col gap-2">
          <select
            value={searchTarget}
            onChange={(e) => setSearchTarget(e.target.value)}
            disabled={!enabled || busy || remoteShared.length === 0}
            className="w-full rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] px-2 py-1.5 text-[12px] outline-none focus:border-[var(--accent)] disabled:opacity-50"
          >
            <option value="">
              {intl.formatMessage({ id: "lanCollab.kbSearchPickPlaceholder" })}
            </option>
            {remoteShared.map((offer) => (
              <option
                key={`${offer.hostNodeId}-${offer.shareId}`}
                value={`${offer.hostNodeId}::${offer.shareId}`}
              >
                {offer.hostDisplayName} / {offer.title}
              </option>
            ))}
          </select>
          <div className="flex gap-2">
            <input
              type="text"
              value={searchDraft}
              onChange={(e) => setSearchDraft(e.target.value)}
              disabled={!enabled || busy || !searchTarget}
              className="min-w-0 flex-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] px-2 py-1.5 text-[12px] outline-none focus:border-[var(--accent)] disabled:opacity-50"
              placeholder={intl.formatMessage({ id: "lanCollab.kbSearchPlaceholder" })}
            />
            <button
              type="button"
              onClick={handleSearch}
              disabled={!enabled || busy || !searchTarget || !searchDraft.trim()}
              className="rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--accent-soft)] px-2.5 text-[11px] text-[var(--accent-strong)] disabled:opacity-50"
            >
              {intl.formatMessage({ id: "lanCollab.kbSearch" })}
            </button>
          </div>
        </div>

        {hits.length > 0 && (
          <ul className="mt-2 space-y-1">
            {hits.map((hit) => (
              <li
                key={`${hit.shareId}-${hit.docId}-${hit.chunkIndex ?? "doc"}`}
                className="flex items-center justify-between gap-2 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] px-2 py-1.5 text-[11px]"
              >
                <div className="min-w-0">
                  <div className="truncate font-medium text-[var(--text-strong)]">{hit.title}</div>
                  <div className="text-[10px] text-[var(--text-faint)]">
                    score {hit.score.toFixed(2)}
                    {hit.domain ? ` · ${hit.domain}` : ""}
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => handleFetch(hit)}
                  disabled={busy}
                  className="shrink-0 rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--accent-soft)] px-2 py-1 text-[10px] text-[var(--accent-strong)] disabled:opacity-50"
                >
                  {intl.formatMessage({ id: "lanCollab.kbFetch" })}
                </button>
              </li>
            ))}
          </ul>
        )}

        {preview && (
          <div className="mt-2 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] p-2">
            <div className="mb-1 text-[11px] font-semibold text-[var(--text-strong)]">
              {preview.title}
              <span className="ml-2 font-normal text-[10px] text-[var(--text-faint)]">
                {preview.hostDisplayName}
              </span>
            </div>
            <pre className="thin-scrollbar max-h-40 overflow-auto whitespace-pre-wrap break-words text-[11px] text-[var(--text-base)]">
              {preview.content}
            </pre>
          </div>
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
