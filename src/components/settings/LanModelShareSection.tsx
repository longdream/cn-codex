import { useCallback, useEffect, useMemo, useState } from "react";
import { useIntl } from "react-intl";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  lanCollabShareModel,
  lanCollabStatus,
  lanCollabUnshareModel,
  sharedModelBaseUrl,
  type SharedModelOffer,
} from "../../api/lanCollab";
import { useAppStore } from "../../stores/appStore";
import type { ProviderConfig, ProviderModel } from "../../types/provider";

export function LanModelShareSection() {
  const intl = useIntl();
  const providers = useAppStore((s) => s.providers);
  const activeProviderId = useAppStore((s) => s.activeProviderId);
  const addCustomProvider = useAppStore((s) => s.addCustomProvider);
  const updateProvider = useAppStore((s) => s.updateProvider);
  const activateProvider = useAppStore((s) => s.activateProvider);

  const [enabled, setEnabled] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [shareSelection, setShareSelection] = useState("");
  const [localShared, setLocalShared] = useState<SharedModelOffer[]>([]);
  const [remoteShared, setRemoteShared] = useState<SharedModelOffer[]>([]);

  const shareableOptions = useMemo(() => {
    const options: Array<{
      key: string;
      provider: ProviderConfig;
      model: ProviderModel;
    }> = [];
    for (const provider of providers) {
      if (provider.id.startsWith("lan-share-")) continue;
      for (const model of provider.models) {
        options.push({
          key: `${provider.id}::${model.id}`,
          provider,
          model,
        });
      }
    }
    return options;
  }, [providers]);

  const refresh = useCallback(async () => {
    try {
      const status = await lanCollabStatus();
      setEnabled(status.enabled);
      setLocalShared(status.localSharedModels ?? []);
      setRemoteShared(status.remoteSharedModels ?? []);
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
          await listen("lan-collab-model-share", () => {
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

  const handleShareSelected = () =>
    void runBusy(async () => {
      setNotice(null);
      const option = shareableOptions.find((item) => item.key === shareSelection);
      if (!option) {
        throw new Error(intl.formatMessage({ id: "lanCollab.sharePickRequired" }));
      }
      if (!option.provider.baseUrl?.trim()) {
        throw new Error(intl.formatMessage({ id: "lanCollab.shareNeedBaseUrl" }));
      }
      if (option.provider.requiresOpenAIAuth && !option.provider.apiKey?.trim()) {
        throw new Error(intl.formatMessage({ id: "lanCollab.shareNeedApiKey" }));
      }
      await lanCollabShareModel({
        modelId: option.model.id,
        displayName: `${option.provider.name} / ${option.model.label || option.model.id}`,
        providerId: option.provider.type || option.provider.id,
        upstreamModel: option.model.id,
        upstreamBaseUrl: option.provider.baseUrl,
        upstreamApiKey: option.provider.apiKey || null,
      });
      setShareSelection("");
      setNotice(intl.formatMessage({ id: "lanCollab.shareSuccess" }));
      await refresh();
    });

  const handleUnshare = (shareId: string) =>
    void runBusy(async () => {
      setNotice(null);
      await lanCollabUnshareModel(shareId);
      setNotice(intl.formatMessage({ id: "lanCollab.unshareSuccess" }));
      await refresh();
    });

  const handleUseRemoteModel = (offer: SharedModelOffer) =>
    void runBusy(async () => {
      setNotice(null);
      const providerId = `lan-share-${offer.hostNodeId}-${offer.shareId}`;
      const baseUrl = sharedModelBaseUrl(offer);
      const model: ProviderModel = {
        id: offer.shareId,
        label: offer.displayName || offer.upstreamModel || offer.modelId,
        supportsVision: false,
        contextLength: 128000,
        maxOutputTokens: 65535,
      };
      const existing = useAppStore.getState().providers.find((p) => p.id === providerId);
      if (existing) {
        updateProvider(providerId, {
          name: `${offer.hostDisplayName} · ${offer.displayName}`,
          baseUrl,
          apiKey: offer.accessToken,
          models: [model],
          requiresOpenAIAuth: true,
          wireApi: "chat",
        });
      } else {
        const provider: ProviderConfig = {
          id: providerId,
          type: "custom",
          name: `${offer.hostDisplayName} · ${offer.displayName}`,
          category: "local",
          baseUrl,
          apiKey: offer.accessToken,
          wireApi: "chat",
          requiresOpenAIAuth: true,
          models: [model],
          isCustom: true,
          createdAt: Date.now(),
        };
        addCustomProvider(provider);
      }
      activateProvider(providerId);
      setNotice(
        intl.formatMessage(
          { id: "lanCollab.useRemoteSuccess" },
          { name: offer.displayName },
        ),
      );
    });

  return (
    <section className="settings-card space-y-3">
      <div className="space-y-1">
        <h3 className="text-[13px] font-semibold text-[var(--text-strong)]">
          {intl.formatMessage({ id: "lanCollab.modelShareSection" })}
        </h3>
        <p className="text-[12px] leading-relaxed text-[var(--text-muted)]">
          {intl.formatMessage({ id: "lanCollab.modelShareHint" })}
        </p>
        {!enabled && (
          <p className="text-[11px] text-[var(--warning)]">
            {intl.formatMessage({ id: "settings.lanShare.needEnable" })}
          </p>
        )}
      </div>

      <div className="flex flex-wrap gap-2">
        <select
          value={shareSelection}
          onChange={(e) => setShareSelection(e.target.value)}
          disabled={!enabled || busy || shareableOptions.length === 0}
          className="min-w-0 flex-1 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] px-2 py-1.5 text-[12px] outline-none focus:border-[var(--accent)] disabled:opacity-50"
        >
          <option value="">
            {shareableOptions.length
              ? intl.formatMessage({ id: "lanCollab.sharePickPlaceholder" })
              : intl.formatMessage({ id: "lanCollab.shareNoLocalModels" })}
          </option>
          {shareableOptions.map((item) => (
            <option key={item.key} value={item.key}>
              {item.provider.name} / {item.model.label || item.model.id}
              {item.provider.id === activeProviderId ? " ★" : ""}
            </option>
          ))}
        </select>
        <button
          type="button"
          onClick={handleShareSelected}
          disabled={!enabled || busy || !shareSelection}
          className="rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--accent-soft)] px-3 text-[11px] text-[var(--accent-strong)] disabled:opacity-50"
        >
          {intl.formatMessage({ id: "lanCollab.shareModel" })}
        </button>
      </div>

      <div>
        <div className="mb-1 text-[11px] font-medium text-[var(--text-muted)]">
          {intl.formatMessage({ id: "lanCollab.localShared" })}
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
                    {offer.displayName}
                  </div>
                  <div className="font-mono text-[10px] text-[var(--text-faint)]">
                    :{offer.proxyPort} · {offer.upstreamModel}
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => handleUnshare(offer.shareId)}
                  disabled={busy}
                  className="shrink-0 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] px-2 py-1 text-[10px] text-[var(--text-muted)] hover:bg-[var(--surface-elevated)] disabled:opacity-50"
                >
                  {intl.formatMessage({ id: "lanCollab.unshareModel" })}
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-[11px] text-[var(--text-muted)]">
            {intl.formatMessage({ id: "lanCollab.noLocalShared" })}
          </p>
        )}
      </div>

      <div>
        <div className="mb-1 text-[11px] font-medium text-[var(--text-muted)]">
          {intl.formatMessage({ id: "lanCollab.remoteShared" })}
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
                    {offer.displayName}
                  </div>
                  <div className="truncate text-[10px] text-[var(--text-faint)]">
                    {offer.hostDisplayName} · {offer.hostAddress}:{offer.proxyPort}
                    {offer.online
                      ? ""
                      : ` · ${intl.formatMessage({ id: "lanCollab.peerOffline" })}`}
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => handleUseRemoteModel(offer)}
                  disabled={busy || !offer.online}
                  className="shrink-0 rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--accent-soft)] px-2 py-1 text-[10px] text-[var(--accent-strong)] disabled:opacity-50"
                >
                  {intl.formatMessage({ id: "lanCollab.useRemoteModel" })}
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-[11px] text-[var(--text-muted)]">
            {intl.formatMessage({ id: "lanCollab.noRemoteShared" })}
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
