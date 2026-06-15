import { IconX } from "@tabler/icons-react";
import { useCallback, useEffect, useState } from "react";
import { useIntl } from "react-intl";
import { invoke } from "@tauri-apps/api/core";
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
  const [webServerEnabled, setWebServerEnabled] = useState(false);
  const [webServerUrl, setWebServerUrl] = useState<string | null>(null);
  const [webServerLoading, setWebServerLoading] = useState(false);
  const [relayServerUrl, setRelayServerUrl] = useState("");
  const [relaySaving, setRelaySaving] = useState(false);

  // relay 地址规范化（去空白与末尾 `/`），避免配置被保存成 `http://host:8080/`
  // 后续在拼接 `/m/...` 时出现 `//m/...`。
  const normalizeRelayServerUrl = useCallback((value: string): string => {
    return value.trim().replace(/\/+$/, "");
  }, []);

  useEffect(() => {
    invoke<boolean>("get_mobile_server_status").then((running) => {
      setWebServerEnabled(running);
      if (running) {
        invoke<string>("get_mobile_server_url").then(setWebServerUrl).catch(() => {});
      }
    }).catch(() => {});
    // 加载 relay 服务器配置
    invoke<{ config?: { relay_server_url?: string } }>("standalone_config_read").then((result) => {
      if (result?.config?.relay_server_url) {
        setRelayServerUrl(normalizeRelayServerUrl(result.config.relay_server_url));
      }
    }).catch(() => {});
  }, [normalizeRelayServerUrl]);

  const handleWebServerToggle = useCallback(async () => {
    if (webServerLoading) return;
    setWebServerLoading(true);
    try {
      if (!webServerEnabled) {
        const url = await invoke<string>("start_mobile_server");
        setWebServerEnabled(true);
        setWebServerUrl(url);
      } else {
        await invoke("stop_mobile_server");
        setWebServerEnabled(false);
        setWebServerUrl(null);
      }
    } catch (err) {
      console.error("Web server toggle failed:", err);
    } finally {
      setWebServerLoading(false);
    }
  }, [webServerEnabled, webServerLoading]);

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

                <section className="settings-card space-y-3">
                  <h4 className="text-[13px] font-semibold text-[var(--text-strong)]">
                    Web 服务（手机同步）
                  </h4>
                  <p className="text-xs text-[var(--text-faint)]">
                    开启后可通过手机扫码实时查看 PC 端对话内容
                  </p>
                  <div className="flex items-center gap-3">
                    <button
                      onClick={handleWebServerToggle}
                      disabled={webServerLoading}
                      className={`relative inline-flex h-5 w-9 shrink-0 cursor-pointer items-center rounded-full transition-colors ${
                        webServerEnabled ? "bg-[var(--accent)]" : "bg-[var(--border-subtle)]"
                      } ${webServerLoading ? "opacity-50" : ""}`}
                    >
                      <span
                        className={`inline-block h-3.5 w-3.5 rounded-full bg-white shadow-sm transition-transform ${
                          webServerEnabled ? "translate-x-[18px]" : "translate-x-[3px]"
                        }`}
                      />
                    </button>
                    <span className="text-xs text-[var(--text-muted)]">
                      {webServerLoading ? "正在操作..." : webServerEnabled ? "已开启" : "已关闭"}
                    </span>
                  </div>
                  {webServerEnabled && webServerUrl && (
                    <p className="font-mono text-[11px] text-[var(--text-muted)]">
                      {webServerUrl}
                    </p>
                  )}
                </section>

                <section className="settings-card space-y-3">
                  <h4 className="text-[13px] font-semibold text-[var(--text-strong)]">
                    中转服务器（公网访问）
                  </h4>
                  <p className="text-xs text-[var(--text-faint)]">
                    配置后二维码将指向中转服务器地址，手机无需与 PC 在同一局域网
                  </p>
                  <div className="flex items-center gap-2">
                    <input
                      type="text"
                      value={relayServerUrl}
                      onChange={(e) => setRelayServerUrl(e.target.value)}
                      placeholder="http://your-server.com:8080"
                      className="app-input flex-1 max-w-sm"
                    />
                    <button
                      disabled={relaySaving}
                      onClick={async () => {
                        setRelaySaving(true);
                        try {
                          // 保存前统一规范化，确保后端与二维码使用的都是无尾斜杠地址。
                          const normalizedRelayServerUrl = normalizeRelayServerUrl(relayServerUrl);
                          await invoke("standalone_config_write", {
                            edits: [{ keyPath: "relay_server_url", value: normalizedRelayServerUrl || "" }],
                          });
                          setRelayServerUrl(normalizedRelayServerUrl);
                        } catch (err) {
                          console.error("Save relay url failed:", err);
                        } finally {
                          setRelaySaving(false);
                        }
                      }}
                      className="app-button-secondary text-xs"
                    >
                      {relaySaving ? "保存中..." : "保存"}
                    </button>
                  </div>
                  <p className="text-[11px] text-[var(--text-faint)]">
                    留空则使用局域网直连模式。保存后需重新开启 Web 服务生效。
                  </p>
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
