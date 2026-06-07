import { IconChevronDown } from "@tabler/icons-react";
import { useEffect, useState } from "react";
import { useIntl } from "react-intl";
import { standaloneConfigRead, skillList, skillRead } from "../../api";
import { useAppStore } from "../../stores/appStore";
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
  const [skills, setSkills] = useState<SkillSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [selectedSkill, setSelectedSkill] = useState<string | null>(null);
  const [skillContent, setSkillContent] = useState<string>("");

  useEffect(() => {
    const load = async () => {
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
        const list = await skillList();
        setSkills(list);
      } catch {
        setSkills([]);
      }

      setLoading(false);
    };

    void load();
  }, []);

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
