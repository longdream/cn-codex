import { IconDownload, IconPower, IconTrash } from "@tabler/icons-react";
import { useCallback, useEffect, useState } from "react";
import { useIntl } from "react-intl";
import {
  pluginImportCodexCache,
  pluginList,
  pluginSetEnabled,
  pluginUninstall,
} from "../../api";
import type { PluginSummary } from "../../types/plugin";

export function PluginsPanel() {
  const intl = useIntl();
  const [plugins, setPlugins] = useState<PluginSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [importingPlugins, setImportingPlugins] = useState(false);
  const [pluginImportStatus, setPluginImportStatus] = useState<string | null>(null);
  const [pluginImportError, setPluginImportError] = useState<string | null>(null);
  const [pluginActionId, setPluginActionId] = useState<string | null>(null);
  const [pluginActionStatus, setPluginActionStatus] = useState<string | null>(null);
  const [pluginActionError, setPluginActionError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const list = await pluginList();
      setPlugins(list);
    } catch {
      setPlugins([]);
    }
    setLoading(false);
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const handleImportPlugins = async () => {
    if (importingPlugins) return;
    setImportingPlugins(true);
    setPluginImportStatus(null);
    setPluginImportError(null);
    setPluginActionStatus(null);
    setPluginActionError(null);
    try {
      const result = await pluginImportCodexCache();
      const imported = result.imported.length;
      const errors = result.errors.length;
      setPluginImportStatus(
        errors > 0
          ? intl.formatMessage(
              { id: "settings.integration.pluginImportPartial" },
              { imported, errors },
            )
          : intl.formatMessage(
              { id: "settings.integration.pluginImportSuccess" },
              { imported },
            ),
      );
      await load();
    } catch (err) {
      setPluginImportError(err instanceof Error ? err.message : String(err));
    } finally {
      setImportingPlugins(false);
    }
  };

  const handleSetPluginEnabled = async (plugin: PluginSummary, enabled: boolean) => {
    if (pluginActionId) return;
    setPluginActionId(plugin.id);
    setPluginActionStatus(null);
    setPluginActionError(null);
    try {
      const updated = await pluginSetEnabled(plugin.id, enabled);
      setPluginActionStatus(
        intl.formatMessage(
          {
            id: enabled
              ? "settings.integration.pluginActionEnabled"
              : "settings.integration.pluginActionDisabled",
          },
          { name: updated.displayName },
        ),
      );
      await load();
    } catch (err) {
      setPluginActionError(err instanceof Error ? err.message : String(err));
    } finally {
      setPluginActionId(null);
    }
  };

  const handleUninstallPlugin = async (plugin: PluginSummary) => {
    if (pluginActionId) return;
    const confirmed = window.confirm(
      intl.formatMessage(
        { id: "settings.integration.pluginUninstallConfirm" },
        { name: plugin.displayName },
      ),
    );
    if (!confirmed) return;

    setPluginActionId(plugin.id);
    setPluginActionStatus(null);
    setPluginActionError(null);
    try {
      await pluginUninstall(plugin.id);
      setPluginActionStatus(
        intl.formatMessage(
          { id: "settings.integration.pluginActionUninstalled" },
          { name: plugin.displayName },
        ),
      );
      await load();
    } catch (err) {
      setPluginActionError(err instanceof Error ? err.message : String(err));
    } finally {
      setPluginActionId(null);
    }
  };

  if (loading) {
    return (
      <div className="text-[13px] text-[var(--text-muted)]">
        {intl.formatMessage({ id: "common.loading" })}
      </div>
    );
  }

  return (
    <div className="space-y-5">
      <section className="settings-card space-y-4">
        <div className="flex items-end justify-between gap-4">
          <div className="space-y-1">
            <h3 className="text-[13px] font-semibold text-[var(--text-strong)]">
              {intl.formatMessage({ id: "settings.integration.plugins" })}
            </h3>
            <p className="text-[13px] text-[var(--text-muted)]">
              {intl.formatMessage({ id: "settings.integration.pluginsHint" })}
            </p>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            <button
              onClick={handleImportPlugins}
              disabled={importingPlugins}
              className="flex items-center gap-1.5 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-contrast)] px-3 py-1.5 text-xs text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)] disabled:opacity-50"
              title={intl.formatMessage({ id: "settings.integration.pluginImportTitle" })}
            >
              <IconDownload size={13} stroke={1.8} />
              {intl.formatMessage({
                id: importingPlugins
                  ? "settings.integration.pluginImporting"
                  : "settings.integration.pluginImport",
              })}
            </button>
            <span className="rounded-full border border-[var(--border-subtle)] bg-[var(--surface-contrast)] px-3 py-1 text-xs text-[var(--text-muted)]">
              {plugins.length}
            </span>
          </div>
        </div>

        {pluginImportStatus && (
          <p className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface-contrast)]/72 px-3 py-2 text-xs text-[var(--text-muted)]">
            {pluginImportStatus}
          </p>
        )}
        {pluginImportError && (
          <p className="break-words rounded-2xl border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-300">
            {pluginImportError}
          </p>
        )}
        {pluginActionStatus && (
          <p className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface-contrast)]/72 px-3 py-2 text-xs text-[var(--text-muted)]">
            {pluginActionStatus}
          </p>
        )}
        {pluginActionError && (
          <p className="break-words rounded-2xl border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-300">
            {pluginActionError}
          </p>
        )}

        {plugins.length === 0 ? (
          <div className="rounded-2xl border border-dashed border-[var(--border-strong)] bg-[var(--surface-contrast)]/56 px-4 py-5 text-center">
            <p className="text-[13px] text-[var(--text-muted)]">
              {intl.formatMessage({ id: "settings.integration.noPlugins" })}
            </p>
          </div>
        ) : (
          <div className="space-y-3">
            {plugins.map((plugin) => {
              const capabilityBadges = [
                plugin.skillsCount > 0
                  ? intl.formatMessage(
                      { id: "settings.integration.pluginSkills" },
                      { count: plugin.skillsCount },
                    )
                  : null,
                plugin.hasMcpServers ? "MCP" : null,
                plugin.appsCount > 0 ? `Apps (${plugin.appsCount})` : plugin.hasApps ? "Apps" : null,
                plugin.hasHooks ? "Hooks" : null,
              ].filter((label): label is string => Boolean(label));
              const summary =
                plugin.interface?.shortDescription ?? plugin.description ?? plugin.manifestPath;
              const pluginBusy = pluginActionId === plugin.id;

              return (
                <div
                  key={plugin.id}
                  className={`rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface-contrast)]/82 px-4 py-4 ${
                    plugin.enabled ? "" : "opacity-70"
                  }`}
                >
                  <div className="flex flex-wrap items-center justify-between gap-3">
                    <div className="min-w-0 space-y-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <p className="text-[13px] font-semibold text-[var(--text-strong)]">
                          {plugin.displayName}
                        </p>
                        {plugin.version && (
                          <span className="rounded-full bg-[var(--surface-soft)] px-2 py-1 text-[11px] text-[var(--text-muted)]">
                            v{plugin.version}
                          </span>
                        )}
                      </div>
                      <p className="break-words text-xs text-[var(--text-muted)]">{summary}</p>
                    </div>
                    <div className="flex flex-wrap items-center gap-2">
                      <span
                        className={`rounded-full px-2 py-1 text-[11px] ${
                          plugin.enabled
                            ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                            : "bg-[var(--surface-soft)] text-[var(--text-faint)]"
                        }`}
                      >
                        {intl.formatMessage({
                          id: plugin.enabled
                            ? "settings.integration.pluginEnabled"
                            : "settings.integration.pluginDisabled",
                        })}
                      </span>
                      <button
                        onClick={() => handleSetPluginEnabled(plugin, !plugin.enabled)}
                        disabled={pluginBusy || Boolean(pluginActionId)}
                        className="flex items-center gap-1.5 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-main)] px-2.5 py-1.5 text-xs text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)] disabled:opacity-50"
                        title={intl.formatMessage({
                          id: plugin.enabled
                            ? "settings.integration.pluginDisableTitle"
                            : "settings.integration.pluginEnableTitle",
                        })}
                      >
                        <IconPower size={13} stroke={1.8} />
                        {intl.formatMessage({
                          id: plugin.enabled
                            ? "settings.integration.pluginDisable"
                            : "settings.integration.pluginEnable",
                        })}
                      </button>
                      <button
                        onClick={() => handleUninstallPlugin(plugin)}
                        disabled={pluginBusy || Boolean(pluginActionId)}
                        className="flex h-7 w-7 items-center justify-center rounded-[var(--radius-sm)] border border-red-500/25 bg-red-500/10 text-red-300 transition-colors hover:bg-red-500/20 disabled:opacity-50"
                        title={intl.formatMessage({ id: "settings.integration.pluginUninstallTitle" })}
                      >
                        <IconTrash size={13} stroke={1.8} />
                      </button>
                    </div>
                  </div>

                  {capabilityBadges.length > 0 && (
                    <div className="mt-3 flex flex-wrap gap-2">
                      {capabilityBadges.map((label) => (
                        <span
                          key={label}
                          className="rounded-full bg-[var(--surface-soft)] px-2 py-1 text-[11px] text-[var(--text-muted)]"
                        >
                          {label}
                        </span>
                      ))}
                    </div>
                  )}

                  {plugin.error ? (
                    <p className="mt-3 break-words rounded-2xl border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-300">
                      {plugin.error}
                    </p>
                  ) : (
                    <p className="mt-3 break-all font-mono text-xs text-[var(--text-muted)]">
                      {plugin.path}
                    </p>
                  )}

                  {plugin.apps.length > 0 && (
                    <div className="mt-3 space-y-2 rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface-main)]/50 px-3 py-3">
                      {plugin.apps.map((app) => (
                        <div
                          key={`${app.key}:${app.connectorId}`}
                          className="flex flex-wrap items-center gap-2 text-xs"
                        >
                          <span className="font-medium text-[var(--text-strong)]">{app.key}</span>
                          <span className="font-mono text-[var(--text-muted)]">
                            {app.connectorId}
                          </span>
                        </div>
                      ))}
                    </div>
                  )}

                  {plugin.warnings.length > 0 && (
                    <div className="mt-3 space-y-1">
                      {plugin.warnings.map((warning) => (
                        <p
                          key={warning}
                          className="break-words text-xs text-[var(--text-faint)]"
                        >
                          {warning}
                        </p>
                      ))}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </section>
    </div>
  );
}
