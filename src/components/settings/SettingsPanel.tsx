import { IconX } from "@tabler/icons-react";
import { useState } from "react";
import { useIntl } from "react-intl";
import { useSettingsStore } from "../../stores/settingsStore";
import { ProviderPanel } from "./ProviderPanel";
import { IntegrationPanel } from "./IntegrationPanel";
import { PluginsPanel } from "./PluginsPanel";
import { SkillsPanel } from "./SkillsPanel";
import { HooksPanel } from "./HooksPanel";
import { UsageDashboard } from "./UsageDashboard";

interface SettingsPanelProps {
  onClose: () => void;
}

type SettingsTab = "general" | "provider" | "usage" | "integration" | "plugins" | "skills" | "hooks";

export function SettingsPanel({ onClose }: SettingsPanelProps) {
  const intl = useIntl();
  const locale = useSettingsStore((state) => state.locale);
  const theme = useSettingsStore((state) => state.theme);
  const setLocale = useSettingsStore((state) => state.setLocale);
  const setTheme = useSettingsStore((state) => state.setTheme);
  const [tab, setTab] = useState<SettingsTab>("provider");

  const tabs: Array<{ id: SettingsTab; label: string; detail: string }> = [
    {
      id: "general",
      label: intl.formatMessage({ id: "common.settings" }),
      detail: intl.formatMessage({ id: "settings.general.description" }),
    },
    {
      id: "provider",
      label: intl.formatMessage({ id: "settings.provider" }),
      detail: intl.formatMessage({ id: "settings.provider.description" }),
    },
    {
      id: "usage",
      label: intl.formatMessage({ id: "settings.usage", defaultMessage: "用量追踪" }),
      detail: intl.formatMessage({ id: "settings.usage.description", defaultMessage: "查看 Token 用量和费用统计" }),
    },
    {
      id: "integration",
      label: intl.formatMessage({ id: "settings.integration" }),
      detail: intl.formatMessage({ id: "settings.integration.description" }),
    },
    {
      id: "plugins",
      label: intl.formatMessage({ id: "settings.plugins" }),
      detail: intl.formatMessage({ id: "settings.plugins.description" }),
    },
    {
      id: "skills",
      label: intl.formatMessage({ id: "settings.skills" }),
      detail: intl.formatMessage({ id: "settings.skills.description" }),
    },
    {
      id: "hooks",
      label: intl.formatMessage({ id: "settings.hooks" }),
      detail: intl.formatMessage({ id: "settings.hooks.description" }),
    },
  ];

  const activeTab = tabs.find((item) => item.id === tab) ?? tabs[0];

  return (
    <div className="fixed bottom-0 left-0 right-0 top-8 z-40 flex items-center justify-center bg-black/50 p-4 backdrop-blur-sm">
      <div className="app-shell-panel flex max-h-[88vh] min-h-[70vh] w-full max-w-5xl overflow-hidden">
        <aside className="w-full max-w-[220px] shrink-0 border-r border-[var(--border-subtle)] bg-[var(--surface-soft)]/65 p-4">
          <div className="mb-5 space-y-0.5">
            <p className="font-mono text-[11px] uppercase tracking-[0.2em] text-[var(--text-faint)]">
              CN-Codex
            </p>
            <h2 className="text-[14px] font-semibold tracking-tight text-[var(--text-strong)]">
              {intl.formatMessage({ id: "sidebar.settings" })}
            </h2>
          </div>

          <nav className="space-y-1">
            {tabs.map((item) => (
              <button
                key={item.id}
                onClick={() => setTab(item.id)}
                className={`settings-nav-item ${tab === item.id ? "is-active" : ""}`}
              >
                <span className="text-[12px] font-medium">{item.label}</span>
              </button>
            ))}
          </nav>
        </aside>

        <section className="flex min-w-0 flex-1 flex-col bg-[var(--surface-raised)]/94">
          <header className="flex items-center justify-between border-b border-[var(--border-subtle)] px-6 py-4">
            <h3 className="text-[13px] font-semibold tracking-tight text-[var(--text-strong)]">
              {activeTab.label}
            </h3>
            <button
              onClick={onClose}
              className="icon-button"
              aria-label={intl.formatMessage({ id: "common.close" })}
            >
              <IconX size={16} stroke={1.8} />
            </button>
          </header>

          <div className="thin-scrollbar flex-1 overflow-y-auto px-6 py-5">
            {tab === "general" && (
              <div className="space-y-5">
                <section className="settings-card space-y-3">
                  <h4 className="text-[13px] font-semibold text-[var(--text-strong)]">
                    {intl.formatMessage({ id: "settings.language" })}
                  </h4>
                  <select
                    value={locale}
                    onChange={(event) => setLocale(event.target.value)}
                    className="app-select max-w-xs"
                  >
                    <option value="zh-CN">中文</option>
                    <option value="en-US">English</option>
                  </select>
                </section>

                <section className="settings-card space-y-3">
                  <h4 className="text-[13px] font-semibold text-[var(--text-strong)]">
                    {intl.formatMessage({ id: "settings.theme" })}
                  </h4>
                  <div className="grid gap-2 md:grid-cols-3">
                    {(["dark", "light", "system"] as const).map((option) => (
                      <button
                        key={option}
                        onClick={() => setTheme(option)}
                        className={`theme-option ${theme === option ? "is-active" : ""}`}
                      >
                        <span className="text-[13px] font-medium">
                          {intl.formatMessage({ id: `settings.theme.${option}` })}
                        </span>
                        <span className="text-xs text-[var(--text-faint)]">
                          {intl.formatMessage({ id: `settings.theme.${option}.description` })}
                        </span>
                      </button>
                    ))}
                  </div>
                </section>
              </div>
            )}

            {tab === "provider" && <ProviderPanel />}
            {tab === "usage" && <UsageDashboard />}
            {tab === "integration" && <IntegrationPanel />}
            {tab === "plugins" && <PluginsPanel />}
            {tab === "skills" && <SkillsPanel />}
            {tab === "hooks" && <HooksPanel />}
          </div>
        </section>
      </div>
    </div>
  );
}
