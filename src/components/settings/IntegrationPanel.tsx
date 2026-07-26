import { IconDownload, IconFolderOpen, IconLoader2, IconTrash } from "@tabler/icons-react";
import { useCallback, useEffect, useState } from "react";
import { useIntl } from "react-intl";
import {
  standaloneConfigRead,
  standaloneConfigWrite,
  standaloneMcpEnablePlaywright,
} from "../../api";
import { revealInExplorer } from "../../api/window";
import { useAppStore } from "../../stores/appStore";
import { SettingsPagination, usePagedItems } from "./SettingsPagination";

interface McpServerInfo {
  name: string;
  command?: string;
  args?: string[];
  url?: string;
  type?: string;
  disabled?: boolean;
}

function looksLikeMcpSseUrl(url: string): boolean {
  const path = url.trim().toLowerCase().split("?")[0] ?? "";
  return path.endsWith("/sse") || path.includes("/sse/");
}

function normalizeMcpTransportType(raw: unknown, url?: string): string {
  const explicit =
    typeof raw === "string" ? raw.trim().toLowerCase().replace(/-/g, "_") : "";
  if (
    explicit === "http" ||
    explicit === "streamable_http" ||
    explicit === "streamablehttp"
  ) {
    return "http";
  }
  if (explicit === "sse") return "sse";
  if (explicit === "stdio" || explicit === "local") return "stdio";
  if (url && url.trim()) {
    return looksLikeMcpSseUrl(url) ? "sse" : "http";
  }
  return "stdio";
}

function extractMcpServerMap(raw: Record<string, unknown>): Record<string, Record<string, unknown>> {
  const nested = raw.mcpServers ?? raw.mcp_servers;
  if (nested && typeof nested === "object" && !Array.isArray(nested)) {
    return nested as Record<string, Record<string, unknown>>;
  }
  // Direct map form: { "name": { command/url... } }
  return raw as Record<string, Record<string, unknown>>;
}

function normalizeImportedMcpServer(
  name: string,
  value: unknown,
): Record<string, unknown> | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }
  const source = value as Record<string, unknown>;
  const next: Record<string, unknown> = { ...source };

  // Cursor/Cline style active flag → CN-Codex disabled
  if (typeof next.disabled !== "boolean") {
    if (typeof next.isActive === "boolean") {
      next.disabled = !next.isActive;
    } else if (typeof next.is_active === "boolean") {
      next.disabled = !next.is_active;
    } else if (typeof next.enabled === "boolean") {
      next.disabled = !next.enabled;
    }
  }
  delete next.isActive;
  delete next.is_active;
  delete next.enabled;
  delete next.name;
  delete next.description;

  const command = typeof next.command === "string" ? next.command.trim() : "";
  const url =
    (typeof next.url === "string" && next.url.trim()) ||
    (typeof next.server_url === "string" && next.server_url.trim()) ||
    "";
  if (!command && !url) {
    return null;
  }

  // Keep explicit transport/type when present.
  if (typeof next.type === "string") {
    next.type = normalizeMcpTransportType(next.type);
  } else if (typeof next.transport === "string") {
    next.type = normalizeMcpTransportType(next.transport);
  } else if (url) {
    next.type = looksLikeMcpSseUrl(url) ? "sse" : "http";
  }

  // Prefer canonical fields.
  if (url && !next.url) {
    next.url = url;
  }
  delete next.server_url;

  // Avoid storing empty command for remote servers.
  if (!command) {
    delete next.command;
    delete next.args;
  }

  void name;
  return next;
}

interface WorkspacePathCard {
  key: string;
  label: string;
  displayPath: string | null;
  openPath: string | null;
}

function normalizeWindowsVerbatimPath(raw: string | null | undefined): string | null {
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
  const [loading, setLoading] = useState(true);
  const [mcpJsonOpen, setMcpJsonOpen] = useState(false);
  const [mcpJsonText, setMcpJsonText] = useState("");
  const [mcpJsonError, setMcpJsonError] = useState<string | null>(null);
  const [mcpSaving, setMcpSaving] = useState(false);
  const [mcpDeleteTarget, setMcpDeleteTarget] = useState<string | null>(null);
  const [mcpDeleting, setMcpDeleting] = useState(false);
  const [mcpToggling, setMcpToggling] = useState<string | null>(null);
  const [playwrightEnabling, setPlaywrightEnabling] = useState(false);
  const [playwrightStatus, setPlaywrightStatus] = useState<string | null>(null);
  const [playwrightError, setPlaywrightError] = useState<string | null>(null);
  const {
    page: serversPage,
    setPage: setServersPage,
    pageSize: serversPageSize,
    totalItems: totalServers,
    totalPages: totalServerPages,
    pagedItems: pagedServers,
  } = usePagedItems(servers);

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

  const playwrightServer = servers.find((server) => server.name === "playwright");
  const playwrightEnabled = Boolean(playwrightServer && !playwrightServer.disabled);

  const load = useCallback(async () => {
    try {
      const resp = await standaloneConfigRead();
      const cfg = (resp?.config ?? {}) as Record<string, unknown>;
      const mcpServers = (cfg.mcp_servers ?? cfg.mcpServers ?? {}) as Record<
        string,
        Record<string, unknown>
      >;
      const parsed: McpServerInfo[] = Object.entries(mcpServers)
        .map(([name, val]) => ({
          name,
          command: (val.command as string) ?? "",
          args: (val.args as string[]) ?? [],
          url: typeof val.url === "string" ? val.url : undefined,
          type: normalizeMcpTransportType(
            typeof val.type === "string"
              ? val.type
              : typeof val.transport === "string"
                ? val.transport
                : "",
            typeof val.url === "string" ? val.url : undefined,
          ),
          disabled:
            typeof val.disabled === "boolean"
              ? val.disabled
              : typeof val.isActive === "boolean"
                ? !val.isActive
                : typeof val.is_active === "boolean"
                  ? !val.is_active
                  : typeof val.enabled === "boolean"
                    ? !val.enabled
                    : false,
        }))
        .sort((a, b) => a.name.localeCompare(b.name));
      setServers(parsed);
    } catch {
      setServers([]);
    }
    setLoading(false);
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    const handler = () => {
      void load();
    };
    window.addEventListener("mcp-servers-changed", handler);
    return () => window.removeEventListener("mcp-servers-changed", handler);
  }, [load]);

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
      const serverMap = extractMcpServerMap(parsed);
      const edits = Object.entries(serverMap)
        .map(([name, value]) => {
          const normalized = normalizeImportedMcpServer(name, value);
          if (!normalized) return null;
          return {
            keyPath: `mcp_servers.${name}`,
            value: normalized,
          };
        })
        .filter((item): item is { keyPath: string; value: Record<string, unknown> } => item !== null);

      if (edits.length === 0) {
        setMcpJsonError(intl.formatMessage({ id: "settings.integration.mcpJsonInvalid" }));
        setMcpSaving(false);
        return;
      }

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

  const handleMcpDelete = (name: string) => {
    setMcpDeleteTarget(name);
    setMcpJsonError(null);
  };

  const handleToggleMcpEnabled = async (server: McpServerInfo) => {
    if (mcpToggling) return;
    setMcpToggling(server.name);
    setMcpJsonError(null);
    try {
      const resp = await standaloneConfigRead();
      const cfg = (resp?.config ?? {}) as Record<string, unknown>;
      const mcpServers = (cfg.mcp_servers ?? cfg.mcpServers ?? {}) as Record<
        string,
        Record<string, unknown>
      >;
      const existing = mcpServers[server.name];
      if (!existing || typeof existing !== "object" || Array.isArray(existing)) {
        throw new Error(`MCP server '${server.name}' not found in config`);
      }

      const next: Record<string, unknown> = { ...existing };
      delete next.isActive;
      delete next.is_active;
      delete next.enabled;
      next.disabled = !server.disabled;

      // Keep transport type explicit for remote SSE/HTTP servers after toggle.
      if (typeof next.type !== "string" && typeof next.transport !== "string") {
        const url =
          (typeof next.url === "string" && next.url) ||
          (typeof next.server_url === "string" && next.server_url) ||
          server.url ||
          "";
        if (url) {
          next.type = looksLikeMcpSseUrl(url) ? "sse" : "http";
        }
      } else if (typeof next.type === "string") {
        next.type = normalizeMcpTransportType(next.type);
      } else if (typeof next.transport === "string") {
        next.type = normalizeMcpTransportType(next.transport);
      }

      await standaloneConfigWrite([
        { keyPath: `mcp_servers.${server.name}`, value: next },
      ]);
      await load();
    } catch (err) {
      console.error("Failed to toggle MCP server:", err);
      setMcpJsonError(err instanceof Error ? err.message : String(err));
    } finally {
      setMcpToggling(null);
    }
  };

  const handleConfirmMcpDelete = async () => {
    if (!mcpDeleteTarget || mcpDeleting) return;
    setMcpDeleting(true);
    try {
      await standaloneConfigWrite([
        { keyPath: `mcp_servers.${mcpDeleteTarget}`, value: null },
      ]);
      setMcpDeleteTarget(null);
      await load();
    } catch (err) {
      console.error("Failed to delete MCP server:", err);
      setMcpJsonError(err instanceof Error ? err.message : String(err));
    } finally {
      setMcpDeleting(false);
    }
  };

  const handleEnablePlaywright = async () => {
    if (playwrightEnabling) return;
    setPlaywrightEnabling(true);
    setPlaywrightStatus(null);
    setPlaywrightError(null);
    setMcpJsonError(null);
    try {
      const result = await standaloneMcpEnablePlaywright();
      await load();
      if (result.installStatus === "succeeded") {
        setPlaywrightStatus(
          intl.formatMessage({ id: "settings.integration.playwright.enableSuccess" }),
        );
      } else {
        setPlaywrightStatus(
          intl.formatMessage({ id: "settings.integration.playwright.enablePartial" }),
        );
        setPlaywrightError(result.error ?? null);
      }
    } catch (err) {
      setPlaywrightError(err instanceof Error ? err.message : String(err));
    } finally {
      setPlaywrightEnabling(false);
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

        <div className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface-contrast)]/82 px-4 py-4">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="min-w-0 space-y-1">
              <p className="text-[13px] font-semibold text-[var(--text-strong)]">
                {intl.formatMessage({ id: "settings.integration.playwright.title" })}
              </p>
              <p className="text-xs text-[var(--text-muted)]">
                {intl.formatMessage({ id: "settings.integration.playwright.description" })}
              </p>
              <p className="break-all font-mono text-[11px] text-[var(--text-faint)]">
                npx -y @playwright/mcp@latest
              </p>
            </div>
            <button
              type="button"
              onClick={handleEnablePlaywright}
              disabled={playwrightEnabling || playwrightEnabled}
              className="inline-flex items-center gap-1.5 rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--accent-soft)] px-3 py-1.5 text-xs font-medium text-[var(--accent-strong)] transition-colors hover:bg-[var(--surface-elevated)] disabled:opacity-60"
            >
              {playwrightEnabling && <IconLoader2 size={13} stroke={1.8} className="animate-spin" />}
              {intl.formatMessage({
                id: playwrightEnabled
                  ? "settings.integration.playwright.enabled"
                  : playwrightEnabling
                    ? "settings.integration.playwright.enabling"
                    : "settings.integration.playwright.enable",
              })}
            </button>
          </div>
        </div>

        {playwrightStatus && (
          <p className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface-contrast)]/72 px-3 py-2 text-xs text-[var(--text-muted)]">
            {playwrightStatus}
          </p>
        )}
        {playwrightError && (
          <p className="break-words rounded-2xl border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-300">
            {playwrightError}
          </p>
        )}

        {mcpJsonOpen && (
          <div className="rounded-2xl border border-[var(--accent-border)] bg-[var(--surface-contrast)]/82 px-4 py-4 space-y-3">
            <p className="text-xs text-[var(--text-muted)]">
              {intl.formatMessage({ id: "settings.integration.mcpJsonHint" })}
            </p>
            <textarea
              value={mcpJsonText}
              onChange={(e) => { setMcpJsonText(e.target.value); setMcpJsonError(null); }}
              placeholder={
                '{\n  "mcpServers": {\n    "stdio-demo": {\n      "command": "npx",\n      "args": ["-y", "@modelcontextprotocol/server-xxx"]\n    },\n    "sse-demo": {\n      "type": "sse",\n      "url": "http://127.0.0.1:3000/sse",\n      "isActive": true\n    }\n  }\n}'
              }
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
            {pagedServers.map((server) => (
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
                      {(server.type || (server.url ? "http" : "stdio")).toUpperCase()}
                    </span>
                    <button
                      type="button"
                      onClick={() => void handleToggleMcpEnabled(server)}
                      disabled={mcpToggling === server.name || mcpDeleting}
                      className={`rounded-full px-2 py-1 text-[11px] transition-colors disabled:opacity-60 ${
                        server.disabled
                          ? "bg-[var(--surface-soft)] text-[var(--text-faint)] hover:bg-[var(--surface-elevated)] hover:text-[var(--text-muted)]"
                          : "bg-[var(--accent-soft)] text-[var(--accent-strong)] hover:bg-[var(--surface-elevated)]"
                      }`}
                      title={intl.formatMessage({
                        id: server.disabled
                          ? "settings.integration.mcpEnable"
                          : "settings.integration.mcpDisable",
                      })}
                    >
                      {mcpToggling === server.name
                        ? intl.formatMessage({ id: "common.loading" })
                        : server.disabled
                          ? intl.formatMessage({ id: "settings.integration.mcpDisabled" })
                          : intl.formatMessage({ id: "settings.integration.mcpEnabled" })}
                    </button>
                    <button
                      onClick={() => handleMcpDelete(server.name)}
                      disabled={mcpDeleting}
                      className="flex h-6 w-6 items-center justify-center rounded-[var(--radius-sm)] text-[var(--text-faint)] transition-colors hover:bg-red-500/10 hover:text-red-400"
                      title={intl.formatMessage({ id: "common.delete" })}
                    >
                      <IconTrash size={13} stroke={1.8} />
                    </button>
                  </div>
                </div>
                <p className="mt-2 break-all font-mono text-xs text-[var(--text-muted)]">
                  {server.url
                    ? server.url
                    : server.command
                      ? `${server.command} ${server.args?.join(" ") ?? ""}`.trim()
                      : "-"}
                </p>
              </div>
            ))}
            <SettingsPagination
              page={serversPage}
              onPageChange={setServersPage}
              pageSize={serversPageSize}
              totalItems={totalServers}
              totalPages={totalServerPages}
            />
          </div>
        )}
      </section>

      {mcpDeleteTarget && (
        <div
          className="fixed bottom-0 left-0 right-0 top-8 z-50 flex items-center justify-center bg-black/45 p-4 backdrop-blur-sm"
          onClick={() => {
            if (mcpDeleting) return;
            setMcpDeleteTarget(null);
          }}
        >
          <div
            className="w-full max-w-[460px] rounded-[var(--radius-lg)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] p-5 shadow-[var(--shadow-strong)]"
            onClick={(event) => event.stopPropagation()}
          >
            <h3 className="text-sm font-semibold text-[var(--text-strong)]">
              {intl.formatMessage({ id: "settings.integration.mcpDeleteTitle" })}
            </h3>
            <p className="mt-2 text-xs text-[var(--text-muted)]">
              {intl.formatMessage(
                { id: "settings.integration.mcpDeleteConfirm" },
                { name: mcpDeleteTarget },
              )}
            </p>
            <div className="mt-4 flex justify-end gap-2">
              <button
                type="button"
                className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] px-3 py-1.5 text-xs text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-elevated)]"
                onClick={() => setMcpDeleteTarget(null)}
                disabled={mcpDeleting}
              >
                {intl.formatMessage({ id: "common.cancel" })}
              </button>
              <button
                type="button"
                className="rounded-[var(--radius-sm)] border border-red-500/30 bg-red-500/10 px-3 py-1.5 text-xs font-medium text-red-300 transition-colors hover:bg-red-500/20 disabled:opacity-50"
                onClick={() => void handleConfirmMcpDelete()}
                disabled={mcpDeleting}
              >
                {mcpDeleting
                  ? intl.formatMessage({ id: "settings.integration.mcpDeleting" })
                  : intl.formatMessage({ id: "common.confirm" })}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
