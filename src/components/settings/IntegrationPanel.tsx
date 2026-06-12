import { IconChevronDown, IconDownload, IconExternalLink, IconFolderOpen, IconPower, IconRefresh, IconTrash } from "@tabler/icons-react";
import { useCallback, useEffect, useState } from "react";
import { useIntl } from "react-intl";
import {
  npmToolList,
  npmToolInstall,
  npmToolUninstall,
  pluginImportCodexCache,
  pluginList,
  pluginSetEnabled,
  pluginUninstall,
  standaloneConfigRead,
  standaloneConfigWrite,
  skillList,
  skillRead,
} from "../../api";
import type { NpmToolInfo } from "../../api/npmTool";
import { revealInExplorer } from "../../api/window";
import { useAppStore } from "../../stores/appStore";
import type { PluginSummary } from "../../types/plugin";
import type { SkillSummary } from "../../types/skill";

interface McpServerInfo {
  name: string;
  command?: string;
  args?: string[];
}

interface WorkspacePathCard {
  key: string;
  label: string;
  displayPath: string | null;
  openPath: string | null;
}

function normalizeWindowsVerbatimPath(raw: string | null | undefined): string | null {
  // Windows 在某些 API（尤其 canonicalize）下会返回扩展路径前缀 `\\?\`。
  // 该前缀对内部文件操作可用，但不适合直接展示给用户，也可能导致 Explorer 打开不稳定。
  if (!raw) return null;
  const trimmed = raw.trim();
  if (!trimmed) return null;
  if (trimmed.startsWith("\\\\?\\UNC\\")) {
    return `\\\\${trimmed.slice("\\\\?\\UNC\\".length)}`;
  }
  if (trimmed.startsWith("\\\\?\\")) {
    return trimmed.slice("\\\\?\\".length);
  }
  return trimmed;
}

function joinWindowsPath(base: string | null, segment: string): string | null {
  if (!base) return null;
  return `${base.replace(/[\\/]+$/, "")}\\${segment.replace(/^[\\/]+/, "")}`;
}

export function IntegrationPanel() {
  const intl = useIntl();
  const configDir = useAppStore((state) => state.configDir);
  const configPath = useAppStore((state) => state.configPath);
  const workspaceCwd = useAppStore((state) => state.workspaceCwd);
  const [servers, setServers] = useState<McpServerInfo[]>([]);
  const [plugins, setPlugins] = useState<PluginSummary[]>([]);
  const [skills, setSkills] = useState<SkillSummary[]>([]);
  const [npmTools, setNpmTools] = useState<NpmToolInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [selectedSkill, setSelectedSkill] = useState<string | null>(null);
  const [skillContent, setSkillContent] = useState<string>("");
  const [npmActionId, setNpmActionId] = useState<string | null>(null);
  const [npmActionStatus, setNpmActionStatus] = useState<string | null>(null);
  const [npmActionError, setNpmActionError] = useState<string | null>(null);
  const [importingPlugins, setImportingPlugins] = useState(false);
  const [pluginImportStatus, setPluginImportStatus] = useState<string | null>(null);
  const [pluginImportError, setPluginImportError] = useState<string | null>(null);
  const [pluginActionId, setPluginActionId] = useState<string | null>(null);
  const [pluginActionStatus, setPluginActionStatus] = useState<string | null>(null);
  const [pluginActionError, setPluginActionError] = useState<string | null>(null);
  // MCP JSON 导入状态
  const [mcpJsonOpen, setMcpJsonOpen] = useState(false);
  const [mcpJsonText, setMcpJsonText] = useState("");
  const [mcpJsonError, setMcpJsonError] = useState<string | null>(null);
  const [mcpSaving, setMcpSaving] = useState(false);

  const displayConfigDir = normalizeWindowsVerbatimPath(configDir);
  const displayConfigPath = normalizeWindowsVerbatimPath(configPath);
  const displayWorkspaceCwd = normalizeWindowsVerbatimPath(workspaceCwd);
  const displaySkillsDir = joinWindowsPath(displayConfigDir, "skills");
  const displayPluginsDir = joinWindowsPath(displayConfigDir, "plugins");

  const workspacePathCards: WorkspacePathCard[] = [
    {
      key: "config-path",
      label: intl.formatMessage({ id: "settings.integration.configPath" }),
      displayPath: displayConfigPath,
      // 优先打开配置目录，确保配置文件未创建时也能进入对应位置。
      openPath: displayConfigDir ?? displayConfigPath,
    },
    {
      key: "skills-dir",
      label: intl.formatMessage({ id: "settings.integration.skillsDir" }),
      displayPath: displaySkillsDir,
      openPath: displaySkillsDir,
    },
    {
      key: "plugins-dir",
      label: intl.formatMessage({ id: "settings.integration.pluginsDir" }),
      displayPath: displayPluginsDir,
      openPath: displayPluginsDir,
    },
    {
      key: "config-dir",
      label: intl.formatMessage({ id: "settings.integration.configDir" }),
      displayPath: displayConfigDir,
      openPath: displayConfigDir,
    },
    {
      key: "workspace-cwd",
      label: intl.formatMessage({ id: "settings.integration.cwd" }),
      displayPath: displayWorkspaceCwd,
      openPath: displayWorkspaceCwd,
    },
  ];

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

    try {
      const list = await npmToolList();
      setNpmTools(list);
    } catch {
      setNpmTools([]);
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

  const handleNpmInstall = async (tool: NpmToolInfo) => {
    if (npmActionId) return;
    setNpmActionId(tool.id);
    setNpmActionStatus(null);
    setNpmActionError(null);
    try {
      const result = await npmToolInstall(tool.id);
      if (result.success) {
        setNpmActionStatus(
          intl.formatMessage(
            { id: "settings.integration.npmToolInstallSuccess" },
            { name: tool.displayName },
          ),
        );
      } else {
        setNpmActionError(
          intl.formatMessage(
            { id: "settings.integration.npmToolInstallFailed" },
            { name: tool.displayName, error: result.output },
          ),
        );
      }
      await load();
    } catch (err) {
      setNpmActionError(err instanceof Error ? err.message : String(err));
    } finally {
      setNpmActionId(null);
    }
  };

  const handleNpmUninstall = async (tool: NpmToolInfo) => {
    if (npmActionId) return;
    const confirmed = window.confirm(
      intl.formatMessage(
        { id: "settings.integration.npmToolUninstallConfirm" },
        { name: tool.displayName },
      ),
    );
    if (!confirmed) return;

    setNpmActionId(tool.id);
    setNpmActionStatus(null);
    setNpmActionError(null);
    try {
      const result = await npmToolUninstall(tool.id);
      if (result.success) {
        setNpmActionStatus(
          intl.formatMessage(
            { id: "settings.integration.npmToolUninstallSuccess" },
            { name: tool.displayName },
          ),
        );
      } else {
        setNpmActionError(
          intl.formatMessage(
            { id: "settings.integration.npmToolUninstallFailed" },
            { name: tool.displayName, error: result.output },
          ),
        );
      }
      await load();
    } catch (err) {
      setNpmActionError(err instanceof Error ? err.message : String(err));
    } finally {
      setNpmActionId(null);
    }
  };

  const handleNpmRefresh = async () => {
    if (npmActionId) return;
    try {
      const list = await npmToolList();
      setNpmTools(list);
    } catch {
      // ignore
    }
  };

  const handleMcpJsonImport = async () => {
    setMcpJsonError(null);
    const text = mcpJsonText.trim();
    if (!text) return;

    let parsed: Record<string, unknown>;
    try {
      parsed = JSON.parse(text);
    } catch {
      setMcpJsonError(intl.formatMessage({ id: "settings.integration.mcpJsonInvalid" }));
      return;
    }

    if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
      setMcpJsonError(intl.formatMessage({ id: "settings.integration.mcpJsonInvalid" }));
      return;
    }

    setMcpSaving(true);
    try {
      const edits = Object.entries(parsed).map(([name, value]) => ({
        keyPath: `mcp_servers.${name}`,
        value,
      }));
      await standaloneConfigWrite(edits);
      setMcpJsonText("");
      setMcpJsonOpen(false);
      await load();
    } catch (err) {
      setMcpJsonError(err instanceof Error ? err.message : String(err));
    } finally {
      setMcpSaving(false);
    }
  };

  const handleMcpDelete = async (name: string) => {
    const confirmed = window.confirm(
      intl.formatMessage(
        { id: "settings.integration.mcpDeleteConfirm" },
        { name },
      ),
    );
    if (!confirmed) return;
    try {
      await standaloneConfigWrite([
        { keyPath: `mcp_servers.${name}`, value: null },
      ]);
      await load();
    } catch (err) {
      console.error("Failed to delete MCP server:", err);
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
        <div className="space-y-1">
          <h3 className="text-[13px] font-semibold text-[var(--text-strong)]">
            {intl.formatMessage({ id: "settings.integration.workspaceConfig" })}
          </h3>
          <p className="text-[13px] text-[var(--text-muted)]">
            {intl.formatMessage({ id: "settings.integration.workspaceHint" })}
          </p>
        </div>

        <div className="grid gap-3 md:grid-cols-2">
          {workspacePathCards.map((card) => (
            <div
              key={card.key}
              className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface-contrast)]/78 px-4 py-3"
            >
              <div className="flex items-center justify-between gap-2">
                <p className="text-xs uppercase tracking-[0.16em] text-[var(--text-faint)]">
                  {card.label}
                </p>
                <button
                  type="button"
                  className="icon-button h-7 w-7"
                  disabled={!card.openPath}
                  onClick={() => {
                    if (!card.openPath) return;
                    void revealInExplorer(card.openPath);
                  }}
                  title={intl.formatMessage({ id: "contextMenu.openInExplorer" })}
                  aria-label={intl.formatMessage({ id: "contextMenu.openInExplorer" })}
                >
                  <IconFolderOpen size={14} stroke={1.8} />
                </button>
              </div>
              <p className="mt-2 break-all font-mono text-xs text-[var(--text-base)]">
                {card.displayPath ?? "-"}
              </p>
            </div>
          ))}
        </div>
      </section>

      <section className="settings-card space-y-4">
        <div className="flex items-end justify-between gap-4">
          <div className="space-y-1">
            <h3 className="text-[13px] font-semibold text-[var(--text-strong)]">
              {intl.formatMessage({ id: "settings.integration.mcpServers" })}
            </h3>
            <p className="text-[13px] text-[var(--text-muted)]">
              {intl.formatMessage({ id: "settings.integration.mcpHint" })}
            </p>
          </div>
          <button
            onClick={() => { setMcpJsonOpen(!mcpJsonOpen); setMcpJsonError(null); }}
            className="flex items-center gap-1.5 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-contrast)] px-3 py-1.5 text-xs text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
          >
            <IconDownload size={13} stroke={1.8} />
            {intl.formatMessage({ id: "settings.integration.mcpImport" })}
          </button>
        </div>

        {mcpJsonOpen && (
          <div className="rounded-2xl border border-[var(--accent-border)] bg-[var(--surface-contrast)]/82 px-4 py-4 space-y-3">
            <p className="text-xs text-[var(--text-muted)]">
              {intl.formatMessage({ id: "settings.integration.mcpJsonHint" })}
            </p>
            <textarea
              value={mcpJsonText}
              onChange={(e) => { setMcpJsonText(e.target.value); setMcpJsonError(null); }}
              placeholder={'{\n  "server-name": {\n    "command": "npx",\n    "args": ["-y", "@modelcontextprotocol/server-xxx"]\n  }\n}'}
              className="app-input w-full min-h-[120px] resize-y font-mono text-xs"
              spellCheck={false}
            />
            {mcpJsonError && (
              <p className="text-xs text-[var(--danger)]">{mcpJsonError}</p>
            )}
            <div className="flex items-center gap-2">
              <button
                onClick={handleMcpJsonImport}
                disabled={mcpSaving || !mcpJsonText.trim()}
                className="rounded-[var(--radius-sm)] bg-[var(--accent)] px-3 py-1.5 text-xs font-medium text-white transition-opacity hover:opacity-90 disabled:opacity-50"
              >
                {intl.formatMessage({ id: "settings.integration.mcpImportBtn" })}
              </button>
              <button
                onClick={() => { setMcpJsonOpen(false); setMcpJsonText(""); setMcpJsonError(null); }}
                className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] px-3 py-1.5 text-xs text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-elevated)]"
              >
                {intl.formatMessage({ id: "common.cancel" })}
              </button>
            </div>
          </div>
        )}

        {servers.length === 0 && !mcpJsonOpen ? (
          <div className="rounded-2xl border border-dashed border-[var(--border-strong)] bg-[var(--surface-contrast)]/56 px-4 py-5 text-center">
            <p className="text-[13px] text-[var(--text-muted)]">
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
                  <span className="text-[13px] font-semibold text-[var(--text-strong)]">
                    {server.name}
                  </span>
                  <div className="flex items-center gap-2">
                    <span className="rounded-full bg-[var(--accent-soft)] px-2 py-1 text-[11px] text-[var(--accent-strong)]">
                      MCP
                    </span>
                    <button
                      onClick={() => handleMcpDelete(server.name)}
                      className="flex h-6 w-6 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-red-500/10 hover:text-red-400"
                      title={intl.formatMessage({ id: "common.delete" })}
                    >
                      <IconTrash size={13} stroke={1.8} />
                    </button>
                  </div>
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

      <section className="settings-card space-y-4">
        <div className="flex items-end justify-between gap-4">
          <div className="space-y-1">
            <h3 className="text-[13px] font-semibold text-[var(--text-strong)]">
              {intl.formatMessage({ id: "settings.integration.npmTools" })}
            </h3>
            <p className="text-[13px] text-[var(--text-muted)]">
              {intl.formatMessage({ id: "settings.integration.npmToolsHint" })}
            </p>
          </div>
          <button
            onClick={handleNpmRefresh}
            disabled={Boolean(npmActionId)}
            className="flex items-center gap-1.5 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-contrast)] px-3 py-1.5 text-xs text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)] disabled:opacity-50"
            title={intl.formatMessage({ id: "settings.integration.npmToolRefresh" })}
          >
            <IconRefresh size={13} stroke={1.8} />
            {intl.formatMessage({ id: "settings.integration.npmToolRefresh" })}
          </button>
        </div>

        {npmActionStatus && (
          <p className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface-contrast)]/72 px-3 py-2 text-xs text-[var(--text-muted)]">
            {npmActionStatus}
          </p>
        )}
        {npmActionError && (
          <p className="break-words rounded-2xl border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-300">
            {npmActionError}
          </p>
        )}

        {npmTools.length === 0 ? (
          <div className="rounded-2xl border border-dashed border-[var(--border-strong)] bg-[var(--surface-contrast)]/56 px-4 py-5 text-center">
            <p className="text-[13px] text-[var(--text-muted)]">
              {intl.formatMessage({ id: "settings.integration.noNpmTools" })}
            </p>
          </div>
        ) : (
          <div className="space-y-3">
            {npmTools.map((tool) => {
              const toolBusy = npmActionId === tool.id;
              const canInstall = tool.nodeAvailable && !tool.installed;
              return (
                <div
                  key={tool.id}
                  className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface-contrast)]/82 px-4 py-4"
                >
                  <div className="flex flex-wrap items-center justify-between gap-3">
                    <div className="min-w-0 space-y-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <p className="text-[13px] font-semibold text-[var(--text-strong)]">
                          {tool.displayName}
                        </p>
                        {tool.installedVersion && (
                          <span className="rounded-full bg-[var(--surface-soft)] px-2 py-1 text-[11px] text-[var(--text-muted)]">
                            v{tool.installedVersion}
                          </span>
                        )}
                      </div>
                      <p className="text-xs text-[var(--text-muted)]">{tool.description}</p>
                    </div>
                    <div className="flex flex-wrap items-center gap-2">
                      <span
                        className={`rounded-full px-2 py-1 text-[11px] ${
                          tool.installed
                            ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                            : "bg-[var(--surface-soft)] text-[var(--text-faint)]"
                        }`}
                      >
                        {intl.formatMessage({
                          id: tool.installed
                            ? "settings.integration.npmToolInstalled"
                            : "settings.integration.npmToolNotInstalled",
                        })}
                      </span>

                      {tool.installed ? (
                        <button
                          onClick={() => handleNpmUninstall(tool)}
                          disabled={toolBusy || Boolean(npmActionId)}
                          className="flex h-7 w-7 items-center justify-center rounded-[var(--radius-sm)] border border-red-500/25 bg-red-500/10 text-red-300 transition-colors hover:bg-red-500/20 disabled:opacity-50"
                          title={intl.formatMessage({ id: "settings.integration.npmToolUninstall" })}
                        >
                          <IconTrash size={13} stroke={1.8} />
                        </button>
                      ) : (
                        <button
                          onClick={() => handleNpmInstall(tool)}
                          disabled={!canInstall || toolBusy || Boolean(npmActionId)}
                          className="flex items-center gap-1.5 rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--accent)] px-3 py-1.5 text-xs font-medium text-white transition-opacity hover:opacity-90 disabled:opacity-50"
                        >
                          <IconDownload size={13} stroke={1.8} />
                          {intl.formatMessage({
                            id: toolBusy
                              ? "settings.integration.npmToolInstalling"
                              : "settings.integration.npmToolInstall",
                          })}
                        </button>
                      )}

                      <a
                        href={tool.homepage}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="flex h-7 w-7 items-center justify-center rounded-[var(--radius-sm)] border border-[var(--border-subtle)] text-[var(--text-faint)] transition-colors hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
                        title={intl.formatMessage({ id: "settings.integration.npmToolDocs" })}
                      >
                        <IconExternalLink size={13} stroke={1.8} />
                      </a>
                    </div>
                  </div>

                  {tool.missingDeps.length > 0 && (
                    <div className="mt-3 flex flex-wrap items-center gap-2">
                      <span className="text-[11px] text-[var(--text-faint)]">
                        {intl.formatMessage({ id: "settings.integration.npmToolDeps" })}:
                      </span>
                      {tool.missingDeps.map((dep) => (
                        <span
                          key={dep}
                          className="rounded-full bg-red-500/10 px-2 py-1 text-[11px] text-red-300"
                        >
                          {dep}
                        </span>
                      ))}
                    </div>
                  )}

                  {tool.missingDeps.length === 0 && !tool.installed && (
                    <div className="mt-3 flex flex-wrap items-center gap-2">
                      <span className="text-[11px] text-[var(--text-faint)]">
                        {intl.formatMessage({ id: "settings.integration.npmToolDeps" })}:
                      </span>
                      <span className="rounded-full bg-[var(--accent-soft)] px-2 py-1 text-[11px] text-[var(--accent-strong)]">
                        Node.js {tool.nodeVersion ? `v${tool.nodeVersion}` : ""} ✓
                      </span>
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
            <h3 className="text-[13px] font-semibold text-[var(--text-strong)]">
              {intl.formatMessage({ id: "settings.integration.skills" })}
            </h3>
            <p className="text-[13px] text-[var(--text-muted)]">
              {intl.formatMessage({ id: "settings.integration.skillsHint" })}
            </p>
          </div>
          <span className="rounded-full border border-[var(--border-subtle)] bg-[var(--surface-contrast)] px-3 py-1 text-xs text-[var(--text-muted)]">
            {skills.length}
          </span>
        </div>

        {skills.length === 0 ? (
          <div className="rounded-2xl border border-dashed border-[var(--border-strong)] bg-[var(--surface-contrast)]/56 px-4 py-5 text-center">
            <p className="text-[13px] text-[var(--text-muted)]">
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
                      <p className="text-[13px] font-semibold text-[var(--text-strong)]">{skill.name}</p>
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
