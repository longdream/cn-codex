import { useEffect, useState } from "react";
import { useIntl } from "react-intl";
import { standaloneConfigRead } from "../../api";
import { useAppStore } from "../../stores/appStore";

interface HookConfig {
  name: string;
  command?: string;
  enabled: boolean;
}

const KNOWN_HOOKS = [
  { name: "on-agent-start", descKey: "settings.hooks.onAgentStart" },
  { name: "on-agent-end", descKey: "settings.hooks.onAgentEnd" },
  { name: "on-file-change", descKey: "settings.hooks.onFileChange" },
  { name: "on-command-exec", descKey: "settings.hooks.onCommandExec" },
];

export function HooksPanel() {
  const intl = useIntl();
  const configPath = useAppStore((state) => state.configPath);
  const [hooks, setHooks] = useState<HookConfig[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    standaloneConfigRead()
      .then((resp) => {
        const cfg = (resp?.config ?? {}) as Record<string, unknown>;
        const hooksConfig = (cfg.hooks ?? {}) as Record<string, Record<string, unknown>>;

        const parsed: HookConfig[] = KNOWN_HOOKS.map((hook) => {
          const hookCfg = hooksConfig[hook.name];
          return {
            name: hook.name,
            command: hookCfg?.command as string | undefined,
            enabled: !!hookCfg?.command,
          };
        });

        const customHooks = Object.keys(hooksConfig)
          .filter((key) => !KNOWN_HOOKS.some((hook) => hook.name === key))
          .map((name) => ({
            name,
            command: hooksConfig[name]?.command as string | undefined,
            enabled: !!hooksConfig[name]?.command,
          }));

        setHooks([...parsed, ...customHooks]);
      })
      .catch(() => {
        setHooks(
          KNOWN_HOOKS.map((hook) => ({
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
