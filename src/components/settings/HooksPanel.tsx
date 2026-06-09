import { useEffect, useState } from "react";
import { useIntl } from "react-intl";
import { hookList } from "../../api";
import { useAppStore } from "../../stores/appStore";
import type { HookListItem } from "../../types/hook";

interface HookConfig {
  id: string;
  name: string;
  command?: string;
  enabled: boolean;
  sourceName?: string;
  sourcePath?: string;
  matcher?: string | null;
}

const KNOWN_HOOKS = [
  { name: "on-agent-start", descKey: "settings.hooks.onAgentStart" },
  { name: "on-user-prompt-submit", descKey: "settings.hooks.onUserPromptSubmit" },
  { name: "on-agent-end", descKey: "settings.hooks.onAgentEnd" },
  { name: "on-file-change", descKey: "settings.hooks.onFileChange" },
  { name: "on-command-exec", descKey: "settings.hooks.onCommandExec" },
  { name: "on-post-tool-use", descKey: "settings.hooks.onPostToolUse" },
  { name: "on-subagent-stop", descKey: "settings.hooks.onSubagentStop" },
];

export function HooksPanel() {
  const intl = useIntl();
  const configPath = useAppStore((state) => state.configPath);
  const [hooks, setHooks] = useState<HookConfig[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    hookList()
      .then((runtimeHooks) => {
        setHooks(buildHookRows(runtimeHooks));
      })
      .catch(() => {
        setHooks(
          KNOWN_HOOKS.map((hook) => ({
            id: hook.name,
            name: hook.name,
            enabled: false,
          })),
        );
      })
      .finally(() => setLoading(false));
  }, []);

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
            {intl.formatMessage({ id: "settings.hooks" })}
          </h3>
          <p className="text-sm text-[var(--text-muted)]">
            {intl.formatMessage({ id: "settings.hooks.description" })}
          </p>
        </div>

        {configPath && (
          <div className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface-contrast)]/78 px-4 py-3">
            <p className="text-xs uppercase tracking-[0.16em] text-[var(--text-faint)]">
              {intl.formatMessage({ id: "settings.hooks.runtimeSource" })}
            </p>
            <p className="mt-2 break-all font-mono text-xs text-[var(--text-base)]">{configPath}</p>
          </div>
        )}

        <div className="space-y-3">
          {hooks.map((hook) => {
            const knownHook = KNOWN_HOOKS.find((item) => item.name === hook.name);
            return (
              <div
                key={hook.name}
                className="flex items-start justify-between gap-4 rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface-contrast)]/82 px-4 py-4"
              >
                <div className="min-w-0">
                  <div className="font-mono text-sm font-medium text-[var(--text-strong)]">
                    {hook.name}
                  </div>
                  {knownHook && (
                    <div className="mt-1 text-xs text-[var(--text-muted)]">
                      {intl.formatMessage({ id: knownHook.descKey })}
                    </div>
                  )}
                  <div className="mt-3 break-all font-mono text-xs text-[var(--text-base)]">
                    {hook.command ?? intl.formatMessage({ id: "settings.hooks.noCommand" })}
                  </div>
                  {hook.sourceName && (
                    <div className="mt-2 text-xs text-[var(--text-muted)]">
                      {hook.sourceName}
                      {hook.matcher ? ` · ${hook.matcher}` : ""}
                    </div>
                  )}
                  {hook.sourcePath && (
                    <div className="mt-1 break-all font-mono text-[11px] text-[var(--text-faint)]">
                      {hook.sourcePath}
                    </div>
                  )}
                </div>
                <span
                  className={`rounded-full px-2.5 py-1 text-xs ${
                    hook.enabled
                      ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                      : "bg-[var(--surface-soft)] text-[var(--text-muted)]"
                  }`}
                >
                  {hook.enabled
                    ? intl.formatMessage({ id: "settings.hooks.active" })
                    : intl.formatMessage({ id: "settings.hooks.inactive" })}
                </span>
              </div>
            );
          })}
        </div>

        <p className="text-sm text-[var(--text-muted)]">
          {intl.formatMessage({ id: "settings.hooks.hint" })}
        </p>
      </section>
    </div>
  );
}

function buildHookRows(runtimeHooks: HookListItem[]): HookConfig[] {
  const rows: HookConfig[] = runtimeHooks.map((hook) => ({
    id: hook.id,
    name: hook.event,
    command: hook.command,
    enabled: hook.enabled,
    sourceName: hook.sourceName,
    sourcePath: hook.sourcePath,
    matcher: hook.matcher,
  }));

  for (const hook of KNOWN_HOOKS) {
    if (!rows.some((row) => row.name === hook.name)) {
      rows.push({
        id: hook.name,
        name: hook.name,
        enabled: false,
      });
    }
  }

  return rows.sort((left, right) => {
    const leftKnown = KNOWN_HOOKS.findIndex((hook) => hook.name === left.name);
    const rightKnown = KNOWN_HOOKS.findIndex((hook) => hook.name === right.name);
    const leftOrder = leftKnown >= 0 ? leftKnown : KNOWN_HOOKS.length;
    const rightOrder = rightKnown >= 0 ? rightKnown : KNOWN_HOOKS.length;
    if (leftOrder !== rightOrder) return leftOrder - rightOrder;
    return left.id.localeCompare(right.id);
  });
}
