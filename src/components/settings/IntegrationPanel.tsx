import { IconChevronDown, IconDownload, IconPower, IconTrash } from "@tabler/icons-react";
import { useCallback, useEffect, useState } from "react";
import { useIntl } from "react-intl";
import {
  pluginImportCodexCache,
  pluginList,
  pluginSetEnabled,
  pluginUninstall,
  standaloneConfigRead,
  skillList,
  skillRead,
} from "../../api";
import { useAppStore } from "../../stores/appStore";
import type { PluginSummary } from "../../types/plugin";
import type { SkillSummary } from "../../types/skill";

interface McpServerInfo {
  name: string;
  command?: string;
  args?: string[];
}

export function IntegrationPanel() {
  const intl = useIntl();
  const configDir = useAppStore((state) => state.configDir);
  const configPath = useAppStore((state) => state.configPath);
  const workspaceCwd = useAppStore((state) => state.workspaceCwd);
  const [servers, setServers] = useState<McpServerInfo[]>([]);
  const [plugins, setPlugins] = useState<PluginSummary[]>([]);
  const [skills, setSkills] = useState<SkillSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [selectedSkill, setSelectedSkill] = useState<string | null>(null);
  const [skillContent, setSkillContent] = useState<string>("");
  const [importingPlugins, setImportingPlugins] = useState(false);
  const [pluginImportStatus, setPluginImportStatus] = useState<string | null>(null);
  const [pluginImportError, setPluginImportError] = useState<string | null>(null);
  const [pluginActionId, setPluginActionId] = useState<string | null>(null);
  const [pluginActionStatus, setPluginActionStatus] = useState<string | null>(null);
  const [pluginActionError, setPluginActionError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const resp = await standaloneConfigRead();
      const cfg = (resp?.config ?? {}) as Record<string, unknown>;
      const mcpServers = (cfg.mcp_servers ?? cfg.mcpServers ?? {}) as Record<
        string,
        Record<string, unknown>
      >;
      const parsed: McpServerInfo[] = Object.entries(mcpServers).map(([name, val]) => ({
        name,
        command: (val.command as string) ?? "",
        args: (val.args as string[]) ?? [],
      }));
      setServers(parsed);
    } catch {
      setServers([]);
    }

    try {
      const list = await pluginList();
      setPlugins(list);
    } catch {
      setPlugins([]);
    }

    try {
      const list = await skillList();
      setSkills(list);
    } catch {
      setSkills([]);
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

  const handleViewSkill = async (id: string) => {
    if (selectedSkill === id) {
      setSelectedSkill(null);
      setSkillContent("");
      return;
    }

    try {
      const detail = await skillRead(id);
      setSelectedSkill(id);
      setSkillContent(detail.content);
    } catch (err) {
      console.error("Failed to read skill:", err);
    }
  };

  if (loading) {
    return (
      <div className="text-sm text-[var(--text-muted)]">
        {intl.formatMessage({ id: "common.loading" })}
      </div>
    );
  }

  return (
    <div className="space-y-5">
      <section className="settings-card space-y-4">
        <div className="space-y-1">
          <h3 className="text-sm font-semibold text-[var(--text-strong)]">
            {intl.formatMessage({ id: "settings.integration.workspaceConfig" })}
          </h3>
          <p className="text-sm text-[var(--text-muted)]">
            {intl.formatMessage({ id: "settings.integration.workspaceHint" })}
          </p>
        </div>

        <div className="grid gap-3 md:grid-cols-2">
          <div className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface-contrast)]/78 px-4 py-3">
            <p className="text-xs uppercase tracking-[0.16em] text-[var(--text-faint)]">
              {intl.formatMessage({ id: "settings.integration.configPath" })}
            </p>
            <p className="mt-2 break-all font-mono text-xs text-[var(--text-base)]">
              {configPath ?? "-"}
            </p>
          </div>

          <div className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface-contrast)]/78 px-4 py-3">
            <p className="text-xs uppercase tracking-[0.16em] text-[var(--text-faint)]">
              {intl.formatMessage({ id: "settings.integration.skillsDir" })}
            </p>
            <p className="mt-2 break-all font-mono text-xs text-[var(--text-base)]">
              {configDir ? `${configDir}\\skills` : "-"}
            </p>
          </div>

          <div className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface-contrast)]/78 px-4 py-3">
            <p className="text-xs uppercase tracking-[0.16em] text-[var(--text-faint)]">
              {intl.formatMessage({ id: "settings.integration.pluginsDir" })}
            </p>
            <p className="mt-2 break-all font-mono text-xs text-[var(--text-base)]">
              {configDir ? `${configDir}\\plugins` : "-"}
            </p>
          </div>

          <div className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface-contrast)]/78 px-4 py-3">
            <p className="text-xs uppercase tracking-[0.16em] text-[var(--text-faint)]">
              {intl.formatMessage({ id: "settings.integration.configDir" })}
            </p>
            <p className="mt-2 break-all font-mono text-xs text-[var(--text-base)]">
              {configDir ?? "-"}
            </p>
          </div>

          <div className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface-contrast)]/78 px-4 py-3">
            <p className="text-xs uppercase tracking-[0.16em] text-[var(--text-faint)]">
              {intl.formatMessage({ id: "settings.integration.cwd" })}
            </p>
            <p className="mt-2 break-all font-mono text-xs text-[var(--text-base)]">
              {workspaceCwd ?? "-"}
            </p>
          </div>
        </div>
      </section>

      <section className="settings-card space-y-4">
        <div className="space-y-1">
          <h3 className="text-sm font-semibold text-[var(--text-strong)]">
            {intl.formatMessage({ id: "settings.integration.mcpServers" })}
          </h3>
          <p className="text-sm text-[var(--text-muted)]">
            {intl.formatMessage({ id: "settings.integration.mcpHint" })}
          </p>
        </div>

        {servers.length === 0 ? (
          <div className="rounded-2xl border border-dashed border-[var(--border-strong)] bg-[var(--surface-contrast)]/56 px-4 py-5 text-center">
            <p className="text-sm text-[var(--text-muted)]">
              {intl.formatMessage({ id: "settings.integration.noMcp" })}
            </p>
          </div>
        ) : (
          <div className="space-y-3">
            {servers.map((server) => (
              <div
                key={server.name}
                className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface-contrast)]/82 px-4 py-4"
              >
                <div className="flex items-center justify-between gap-3">
                  <span className="text-sm font-semibold text-[var(--text-strong)]">
                    {server.name}
                  </span>
                  <span className="rounded-full bg-[var(--accent-soft)] px-2 py-1 text-[11px] text-[var(--accent-strong)]">
                    MCP
                  </span>
                </div>
                <p className="mt-2 break-all font-mono text-xs text-[var(--text-muted)]">
                  {server.command ? `${server.command} ${server.args?.join(" ") ?? ""}`.trim() : "-"}
                </p>
              </div>
            ))}
          </div>
        )}
      </section>

      <section className="settings-card space-y-4">
        <div className="flex items-end justify-between gap-4">
          <div className="space-y-1">
            <h3 className="text-sm font-semibold text-[var(--text-strong)]">
              {intl.formatMessage({ id: "settings.integration.plugins" })}
            </h3>
            <p className="text-sm text-[var(--text-muted)]">
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
            <p className="text-sm text-[var(--text-muted)]">
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
                        <p className="text-sm font-semibold text-[var(--text-strong)]">
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

      <section className="settings-card space-y-4">
        <div className="flex items-end justify-between gap-4">
          <div className="space-y-1">
            <h3 className="text-sm font-semibold text-[var(--text-strong)]">
              {intl.formatMessage({ id: "settings.integration.skills" })}
            </h3>
            <p className="text-sm text-[var(--text-muted)]">
              {intl.formatMessage({ id: "settings.integration.skillsHint" })}
            </p>
          </div>
          <span className="rounded-full border border-[var(--border-subtle)] bg-[var(--surface-contrast)] px-3 py-1 text-xs text-[var(--text-muted)]">
            {skills.length}
          </span>
        </div>

        {skills.length === 0 ? (
          <div className="rounded-2xl border border-dashed border-[var(--border-strong)] bg-[var(--surface-contrast)]/56 px-4 py-5 text-center">
            <p className="text-sm text-[var(--text-muted)]">
              {intl.formatMessage({ id: "settings.integration.noSkills" })}
            </p>
          </div>
        ) : (
          <div className="space-y-3">
            {skills.map((skill) => (
              <div
                key={skill.id}
                className="overflow-hidden rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface-contrast)]/82"
              >
                <button
                  onClick={() => handleViewSkill(skill.id)}
                  className="w-full px-4 py-4 text-left transition-colors hover:bg-[var(--surface-elevated)]"
                >
                  <div className="flex items-center justify-between gap-4">
                    <div className="space-y-1">
                      <p className="text-sm font-semibold text-[var(--text-strong)]">{skill.name}</p>
                      {skill.description && (
                        <p className="text-xs text-[var(--text-muted)]">{skill.description}</p>
                      )}
                    </div>
                    <IconChevronDown
                      size={16}
                      stroke={1.8}
                      className={`shrink-0 text-[var(--text-faint)] transition-transform ${
                        selectedSkill === skill.id ? "rotate-180" : ""
                      }`}
                    />
                  </div>
                  {skill.tags.length > 0 && (
                    <div className="mt-3 flex flex-wrap gap-2">
                      {skill.tags.map((tag) => (
                        <span
                          key={tag}
                          className="rounded-full bg-[var(--surface-soft)] px-2 py-1 text-[11px] text-[var(--text-muted)]"
                        >
                          {tag}
                        </span>
                      ))}
                    </div>
                  )}
                </button>
                {selectedSkill === skill.id && skillContent && (
                  <div className="border-t border-[var(--border-subtle)] px-4 py-4">
                    <p className="mb-3 text-xs uppercase tracking-[0.16em] text-[var(--text-faint)]">
                      {intl.formatMessage({ id: "settings.integration.skillPreview" })}
                    </p>
                    <pre className="thin-scrollbar max-h-72 overflow-y-auto whitespace-pre-wrap rounded-2xl bg-[var(--surface-main)]/72 p-4 text-xs text-[var(--text-base)]">
                      {skillContent}
                    </pre>
                  </div>
                )}
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
