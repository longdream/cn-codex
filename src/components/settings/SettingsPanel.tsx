import { IconExternalLink, IconX } from "@tabler/icons-react";
import { useCallback, useEffect, useState } from "react";
import { useIntl } from "react-intl";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useSettingsStore, type BaziProfile } from "../../stores/settingsStore";
import { useAppVersion } from "../../hooks/useAppVersion";
import { useAppStore } from "../../stores/appStore";
import { ProviderPanel } from "./ProviderPanel";
import { IntegrationPanel } from "./IntegrationPanel";
import { PluginsPanel } from "./PluginsPanel";
import { SkillsPanel } from "./SkillsPanel";
import { SkillLabPanel } from "./SkillLabPanel";
import { RobotsPanel } from "./RobotsPanel";
import { WorkflowsPanel } from "./WorkflowsPanel";
import { UsageDashboard } from "./UsageDashboard";
import { ImageGenerationPanel } from "./ImageGenerationPanel";
import { SmartbrainPanel } from "./SmartbrainPanel";
import { VoiceSettingsPanel } from "./VoiceSettingsPanel";
interface SettingsPanelProps {
  onClose: () => void;
}

type SettingsTab = "provider" | "image" | "usage" | "integration" | "plugins" | "skills" | "skill-lab" | "robots" | "workflows" | "smartbrain" | "voice" | "rules" | "general";

function displayFileName(path: string): string {
  const normalized = path.replace(/\\/g, "/");
  const parts = normalized.split("/");
  return parts[parts.length - 1] || path;
}

export function SettingsPanel({ onClose }: SettingsPanelProps) {
  const intl = useIntl();
  const settingsInitialTab = useAppStore((state) => state.settingsInitialTab);
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
  const [rulesContent, setRulesContent] = useState("");
  const [rulesLoaded, setRulesLoaded] = useState(false);
  const [rulesSaving, setRulesSaving] = useState(false);
  const [rulesSaved, setRulesSaved] = useState(false);
  const appVersion = useAppVersion();

  const fortuneEnabled = useSettingsStore((state) => state.fortuneEnabled);
  const setFortuneEnabled = useSettingsStore((state) => state.setFortuneEnabled);
  const backgroundImagePath = useSettingsStore((state) => state.backgroundImagePath);
  const setBackgroundImagePath = useSettingsStore((state) => state.setBackgroundImagePath);
  const backgroundBlur = useSettingsStore((state) => state.backgroundBlur);
  const backgroundBrightness = useSettingsStore((state) => state.backgroundBrightness);
  const backgroundOverlay = useSettingsStore((state) => state.backgroundOverlay);
  const backgroundScale = useSettingsStore((state) => state.backgroundScale);
  const setBackgroundBlur = useSettingsStore((state) => state.setBackgroundBlur);
  const setBackgroundBrightness = useSettingsStore((state) => state.setBackgroundBrightness);
  const setBackgroundOverlay = useSettingsStore((state) => state.setBackgroundOverlay);
  const setBackgroundScale = useSettingsStore((state) => state.setBackgroundScale);
  const baziProfile = useSettingsStore((state) => state.baziProfile);
  const setBaziProfile = useSettingsStore((state) => state.setBaziProfile);
  const triggerFortuneRefresh = useSettingsStore((state) => state.triggerFortuneRefresh);
  const [baziDraft, setBaziDraft] = useState<BaziProfile>({
    name: "",
    birthDate: "",
    birthTime: "",
    gender: "male" as const,
    lunarCalendar: false,
    occupation: "",
    industry: "",
  });
  const [baziSaved, setBaziSaved] = useState(false);

  useEffect(() => {
    if (baziProfile) {
      setBaziDraft({ ...baziProfile });
    }
  }, [baziProfile]);

  useEffect(() => {
    if (!settingsInitialTab) {
      return;
    }
    const allowed: SettingsTab[] = [
      "provider",
      "image",
      "usage",
      "integration",
      "plugins",
      "skills",
      "skill-lab",
      "robots",
      "workflows",
      "smartbrain",
      "rules",
      "general",
    ];
    if (allowed.includes(settingsInitialTab as SettingsTab)) {
      setTab(settingsInitialTab as SettingsTab);
    }
  }, [settingsInitialTab]);

  const shichenOptions = [
    "zi", "chou", "yin", "mao", "chen", "si",
    "wu", "wei", "shen", "you", "xu", "hai",
  ] as const;

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
    invoke<{ config?: { relay_server_url?: string; smartbrain?: { enabled?: boolean } } }>("standalone_config_read").then((result) => {
      if (result?.config?.relay_server_url) {
        setRelayServerUrl(normalizeRelayServerUrl(result.config.relay_server_url));
      }
    }).catch(() => {});
  }, [normalizeRelayServerUrl]);

  const [webServerError, setWebServerError] = useState<string | null>(null);

  const handleWebServerToggle = useCallback(async () => {
    if (webServerLoading) return;
    setWebServerLoading(true);
    setWebServerError(null);
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
      const msg = typeof err === "string" ? err : (err as Error)?.message ?? String(err);
      setWebServerError(msg);
      setWebServerEnabled(false);
      setWebServerUrl(null);
    } finally {
      setWebServerLoading(false);
    }
  }, [webServerEnabled, webServerLoading]);

  useEffect(() => {
    if (tab === "rules" && !rulesLoaded) {
      invoke<string>("rules_read").then((content) => {
        setRulesContent(content);
        setRulesLoaded(true);
      }).catch(() => setRulesLoaded(true));
    }
  }, [tab, rulesLoaded]);

  const handleRulesSave = useCallback(async () => {
    setRulesSaving(true);
    setRulesSaved(false);
    try {
      await invoke("rules_write", { content: rulesContent });
      setRulesSaved(true);
      setTimeout(() => setRulesSaved(false), 2000);
    } catch (err) {
      console.error("Rules save failed:", err);
    } finally {
      setRulesSaving(false);
    }
  }, [rulesContent]);

  const handleBackgroundSelect = useCallback(async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [
          {
            name: intl.formatMessage({ id: "settings.background.fileFilter" }),
            extensions: ["png", "jpg", "jpeg", "webp", "gif"],
          },
        ],
      });
      if (typeof selected === "string") {
        const trimmed = selected.trim();
        if (trimmed) {
          setBackgroundImagePath(trimmed);
        }
      }
    } catch (error) {
      console.error("Select background image failed:", error);
    }
  }, [intl, setBackgroundImagePath]);

  const tabs: Array<{ id: SettingsTab; label: string; detail: string }> = [
    {
      id: "provider",
      label: intl.formatMessage({ id: "settings.provider" }),
      detail: intl.formatMessage({ id: "settings.provider.description" }),
    },
    {
      id: "image",
      label: intl.formatMessage({ id: "settings.image" }),
      detail: intl.formatMessage({ id: "settings.image.description" }),
    },
    {
      id: "usage",
      label: intl.formatMessage({ id: "settings.usage" }),
      detail: intl.formatMessage({ id: "settings.usage.description" }),
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
      id: "skill-lab",
      label: intl.formatMessage({ id: "settings.skillLab" }),
      detail: intl.formatMessage({ id: "settings.skillLab.description" }),
    },
    {
      id: "robots",
      label: intl.formatMessage({ id: "settings.robots" }),
      detail: intl.formatMessage({ id: "settings.robots.description" }),
    },
    {
      id: "workflows",
      label: intl.formatMessage({ id: "settings.workflows.title" }),
      detail: intl.formatMessage({ id: "settings.workflows.description" }),
    },
    {
      id: "smartbrain",
      label: intl.formatMessage({ id: "settings.smartbrain" }),
      detail: intl.formatMessage({ id: "settings.smartbrain.description" }),
    },
    {
      id: "voice",
      label: intl.formatMessage({ id: "voice.settings.persona" }),
      detail: intl.formatMessage({ id: "voice.settings.persona.description" }),
    },
    {
      id: "rules",
      label: intl.formatMessage({ id: "settings.rules" }),
      detail: intl.formatMessage({ id: "settings.rules.description" }),
    },
    {
      id: "general",
      label: intl.formatMessage({ id: "common.settings" }),
      detail: intl.formatMessage({ id: "settings.general.description" }),
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

          <div className="settings-panel-body thin-scrollbar flex-1 overflow-y-auto px-6 py-5">
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
                    {intl.formatMessage({ id: "settings.background" })}
                  </h4>
                  <p className="text-xs text-[var(--text-faint)]">
                    {intl.formatMessage({ id: "settings.background.description" })}
                  </p>
                  <p className="text-xs text-[var(--text-muted)]">
                    {intl.formatMessage({ id: "settings.background.current" })}{" "}
                    <span className="font-mono text-[11px] text-[var(--text-base)]">
                      {backgroundImagePath
                        ? displayFileName(backgroundImagePath)
                        : intl.formatMessage({ id: "settings.background.none" })}
                    </span>
                  </p>
                  <div className="flex items-center gap-2">
                    <button
                      type="button"
                      onClick={() => void handleBackgroundSelect()}
                      className="app-button-secondary text-xs"
                    >
                      {backgroundImagePath
                        ? intl.formatMessage({ id: "settings.background.change" })
                        : intl.formatMessage({ id: "settings.background.upload" })}
                    </button>
                    <button
                      type="button"
                      onClick={() => setBackgroundImagePath(null)}
                      disabled={!backgroundImagePath}
                      className="app-button-secondary text-xs disabled:opacity-50"
                    >
                      {intl.formatMessage({ id: "settings.background.clear" })}
                    </button>
                  </div>
                  {backgroundImagePath ? (
                    <div className="space-y-3 border-t border-[var(--border-subtle)] pt-3">
                      <p className="text-xs text-[var(--text-faint)]">
                        {intl.formatMessage({ id: "settings.background.adjust" })}
                      </p>
                      <label className="block space-y-1.5">
                        <div className="flex items-center justify-between text-xs text-[var(--text-muted)]">
                          <span>{intl.formatMessage({ id: "settings.background.blur" })}</span>
                          <span className="font-mono text-[11px]">{Math.round(backgroundBlur)}px</span>
                        </div>
                        <input
                          type="range"
                          min={0}
                          max={40}
                          step={1}
                          value={backgroundBlur}
                          onChange={(e) => setBackgroundBlur(Number(e.target.value))}
                          className="w-full accent-[var(--accent)]"
                        />
                      </label>
                      <label className="block space-y-1.5">
                        <div className="flex items-center justify-between text-xs text-[var(--text-muted)]">
                          <span>{intl.formatMessage({ id: "settings.background.brightness" })}</span>
                          <span className="font-mono text-[11px]">
                            {Math.round(backgroundBrightness * 100)}%
                          </span>
                        </div>
                        <input
                          type="range"
                          min={30}
                          max={140}
                          step={1}
                          value={Math.round(backgroundBrightness * 100)}
                          onChange={(e) => setBackgroundBrightness(Number(e.target.value) / 100)}
                          className="w-full accent-[var(--accent)]"
                        />
                      </label>
                      <label className="block space-y-1.5">
                        <div className="flex items-center justify-between text-xs text-[var(--text-muted)]">
                          <span>{intl.formatMessage({ id: "settings.background.overlay" })}</span>
                          <span className="font-mono text-[11px]">
                            {Math.round(backgroundOverlay * 100)}%
                          </span>
                        </div>
                        <input
                          type="range"
                          min={0}
                          max={90}
                          step={1}
                          value={Math.round(backgroundOverlay * 100)}
                          onChange={(e) => setBackgroundOverlay(Number(e.target.value) / 100)}
                          className="w-full accent-[var(--accent)]"
                        />
                      </label>
                      <label className="block space-y-1.5">
                        <div className="flex items-center justify-between text-xs text-[var(--text-muted)]">
                          <span>{intl.formatMessage({ id: "settings.background.scale" })}</span>
                          <span className="font-mono text-[11px]">
                            {Math.round(backgroundScale * 100)}%
                          </span>
                        </div>
                        <input
                          type="range"
                          min={100}
                          max={130}
                          step={1}
                          value={Math.round(backgroundScale * 100)}
                          onChange={(e) => setBackgroundScale(Number(e.target.value) / 100)}
                          className="w-full accent-[var(--accent)]"
                        />
                      </label>
                    </div>
                  ) : null}
                </section>

                <section className="settings-card space-y-3">
                  <h4 className="text-[13px] font-semibold text-[var(--text-strong)]">
                    {intl.formatMessage({ id: "settings.webServer" })}
                  </h4>
                  <p className="text-xs text-[var(--text-faint)]">
                    {intl.formatMessage({ id: "settings.webServer.description" })}
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
                      {webServerLoading
                        ? intl.formatMessage({ id: "settings.webServer.loading" })
                        : webServerEnabled
                          ? intl.formatMessage({ id: "settings.webServer.enabled" })
                          : intl.formatMessage({ id: "settings.webServer.disabled" })}
                    </span>
                  </div>
                  {webServerEnabled && webServerUrl && (
                    <p className="font-mono text-[11px] text-[var(--text-muted)]">
                      {webServerUrl}
                    </p>
                  )}
                  {webServerError && (
                    <p className="text-xs text-red-500">
                      {intl.formatMessage({ id: "settings.webServer.error" })}: {webServerError}
                    </p>
                  )}
                </section>

                <section className="settings-card space-y-3">
                  <h4 className="text-[13px] font-semibold text-[var(--text-strong)]">
                    {intl.formatMessage({ id: "settings.logs" })}
                  </h4>
                  <p className="text-xs text-[var(--text-faint)]">
                    {intl.formatMessage({ id: "settings.logs.description" })}
                  </p>
                  <div className="flex items-center gap-2">
                    <button
                      onClick={async () => {
                        try {
                          const dir = await invoke<string>("get_log_dir");
                          const { openPath } = await import("@tauri-apps/plugin-opener");
                          await openPath(dir);
                        } catch (err) {
                          console.error("Failed to open log dir:", err);
                        }
                      }}
                      className="app-button-secondary text-xs"
                    >
                      {intl.formatMessage({ id: "settings.logs.openDir" })}
                    </button>
                  </div>
                </section>

                <section className="settings-card space-y-3">
                  <h4 className="text-[13px] font-semibold text-[var(--text-strong)]">
                    {intl.formatMessage({ id: "settings.relayServer" })}
                  </h4>
                  <p className="text-xs text-[var(--text-faint)]">
                    {intl.formatMessage({ id: "settings.relayServer.description" })}
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
                      {relaySaving
                        ? intl.formatMessage({ id: "settings.relayServer.savingBtn" })
                        : intl.formatMessage({ id: "settings.relayServer.saveBtn" })}
                    </button>
                  </div>
                  <p className="text-[11px] text-[var(--text-faint)]">
                    {intl.formatMessage({ id: "settings.relayServer.hint" })}
                  </p>
                </section>

                <section className="settings-card space-y-3">
                  <div className="flex items-center justify-between">
                    <h4 className="text-[13px] font-semibold text-[var(--text-strong)]">
                      {intl.formatMessage({ id: "settings.smartbrain" })}
                    </h4>
                    <span className="rounded-full bg-[var(--surface-soft)] px-2 py-1 text-[10px] text-[var(--text-faint)]">
                      {intl.formatMessage({ id: "settings.smartbrain.dialogScoped" })}
                    </span>
                  </div>
                  <p className="text-xs text-[var(--text-faint)]">
                    {intl.formatMessage({ id: "settings.smartbrain.toggle.dialogOnly" })}
                  </p>
                </section>

                <section className="settings-card space-y-3">
                  <div className="flex items-center justify-between">
                    <h4 className="text-[13px] font-semibold text-[var(--text-strong)]">
                      {intl.formatMessage({ id: "settings.fortune" })}
                    </h4>
                    <div className="flex items-center gap-3">
                      <button
                        onClick={() => setFortuneEnabled(!fortuneEnabled)}
                        className={`relative inline-flex h-5 w-9 shrink-0 cursor-pointer items-center rounded-full transition-colors ${
                          fortuneEnabled ? "bg-[var(--accent)]" : "bg-[var(--border-subtle)]"
                        }`}
                      >
                        <span
                          className={`inline-block h-3.5 w-3.5 rounded-full bg-white shadow-sm transition-transform ${
                            fortuneEnabled ? "translate-x-[18px]" : "translate-x-[3px]"
                          }`}
                        />
                      </button>
                      <span className="text-xs text-[var(--text-muted)]">
                        {fortuneEnabled
                          ? intl.formatMessage({ id: "settings.fortune.enabled" })
                          : intl.formatMessage({ id: "settings.fortune.disabled" })}
                      </span>
                    </div>
                  </div>
                  <p className="text-xs text-[var(--text-faint)]">
                    {intl.formatMessage({ id: "settings.fortune.description" })}
                  </p>
                  {fortuneEnabled && (
                    <button
                      onClick={triggerFortuneRefresh}
                      className="flex items-center gap-1.5 rounded-lg bg-white/5 px-3 py-1.5 text-xs font-medium text-[var(--accent)] transition-colors hover:bg-white/10"
                    >
                      {intl.formatMessage({ id: "settings.fortune.refresh" })}
                    </button>
                  )}
                </section>

                {fortuneEnabled && (
                  <section className="settings-card space-y-3">
                    <h4 className="text-[13px] font-semibold text-[var(--text-strong)]">
                      {intl.formatMessage({ id: "settings.fortune.bazi" })}
                    </h4>
                    <p className="text-xs text-[var(--text-faint)]">
                      {intl.formatMessage({ id: "settings.fortune.baziHint" })}
                    </p>
                    <div className="grid gap-3 md:grid-cols-2">
                      <div className="space-y-1">
                        <label className="text-[11px] text-[var(--text-faint)]">
                          {intl.formatMessage({ id: "settings.fortune.name" })}
                        </label>
                        <input
                          type="text"
                          value={baziDraft.name}
                          onChange={(e) => setBaziDraft((d) => ({ ...d, name: e.target.value }))}
                          placeholder={intl.formatMessage({ id: "settings.fortune.namePlaceholder" })}
                          className="app-input w-full"
                        />
                      </div>
                      <div className="space-y-1">
                        <label className="text-[11px] text-[var(--text-faint)]">
                          {intl.formatMessage({ id: "settings.fortune.birthDate" })}
                        </label>
                        <input
                          type="date"
                          value={baziDraft.birthDate}
                          onChange={(e) => setBaziDraft((d) => ({ ...d, birthDate: e.target.value }))}
                          className="app-input w-full"
                        />
                      </div>
                      <div className="space-y-1">
                        <label className="text-[11px] text-[var(--text-faint)]">
                          {intl.formatMessage({ id: "settings.fortune.birthTime" })}
                        </label>
                        <select
                          value={baziDraft.birthTime}
                          onChange={(e) => setBaziDraft((d) => ({ ...d, birthTime: e.target.value }))}
                          className="app-select w-full"
                        >
                          <option value="">{intl.formatMessage({ id: "settings.fortune.birthTimePlaceholder" })}</option>
                          {shichenOptions.map((sc) => (
                            <option key={sc} value={sc}>
                              {intl.formatMessage({ id: `settings.fortune.shichen.${sc}` })}
                            </option>
                          ))}
                        </select>
                      </div>
                      <div className="space-y-1">
                        <label className="text-[11px] text-[var(--text-faint)]">
                          {intl.formatMessage({ id: "settings.fortune.gender" })}
                        </label>
                        <div className="flex items-center gap-4 pt-1">
                          <label className="flex items-center gap-1.5 cursor-pointer">
                            <input
                              type="radio"
                              name="bazi-gender"
                              checked={baziDraft.gender === "male"}
                              onChange={() => setBaziDraft((d) => ({ ...d, gender: "male" }))}
                              className="accent-[var(--accent)]"
                            />
                            <span className="text-xs text-[var(--text-muted)]">
                              {intl.formatMessage({ id: "settings.fortune.gender.male" })}
                            </span>
                          </label>
                          <label className="flex items-center gap-1.5 cursor-pointer">
                            <input
                              type="radio"
                              name="bazi-gender"
                              checked={baziDraft.gender === "female"}
                              onChange={() => setBaziDraft((d) => ({ ...d, gender: "female" }))}
                              className="accent-[var(--accent)]"
                            />
                            <span className="text-xs text-[var(--text-muted)]">
                              {intl.formatMessage({ id: "settings.fortune.gender.female" })}
                            </span>
                          </label>
                        </div>
                      </div>
                    </div>
                    <div className="grid gap-3 md:grid-cols-2">
                      <div className="space-y-1">
                        <label className="text-[11px] text-[var(--text-faint)]">
                          {intl.formatMessage({ id: "settings.fortune.occupation" })}
                        </label>
                        <input
                          type="text"
                          value={baziDraft.occupation ?? ""}
                          onChange={(e) => setBaziDraft((d) => ({ ...d, occupation: e.target.value }))}
                          placeholder={intl.formatMessage({ id: "settings.fortune.occupationPlaceholder" })}
                          className="app-input w-full"
                        />
                      </div>
                      <div className="space-y-1">
                        <label className="text-[11px] text-[var(--text-faint)]">
                          {intl.formatMessage({ id: "settings.fortune.industry" })}
                        </label>
                        <input
                          type="text"
                          value={baziDraft.industry ?? ""}
                          onChange={(e) => setBaziDraft((d) => ({ ...d, industry: e.target.value }))}
                          placeholder={intl.formatMessage({ id: "settings.fortune.industryPlaceholder" })}
                          className="app-input w-full"
                        />
                      </div>
                    </div>
                    <div className="flex items-center gap-3">
                      <label className="flex items-center gap-2 cursor-pointer">
                        <input
                          type="checkbox"
                          checked={baziDraft.lunarCalendar}
                          onChange={(e) => setBaziDraft((d) => ({ ...d, lunarCalendar: e.target.checked }))}
                          className="accent-[var(--accent)]"
                        />
                        <span className="text-xs text-[var(--text-muted)]">
                          {intl.formatMessage({ id: "settings.fortune.lunarCalendar" })}
                        </span>
                      </label>
                      <span className="text-[11px] text-[var(--text-faint)]">
                        {intl.formatMessage({ id: "settings.fortune.lunarHint" })}
                      </span>
                    </div>
                    <div className="flex items-center gap-3">
                      <button
                        onClick={() => {
                          setBaziProfile({ ...baziDraft });
                          setBaziSaved(true);
                          setTimeout(() => setBaziSaved(false), 2000);
                        }}
                        disabled={!baziDraft.name && !baziDraft.birthDate && !baziDraft.birthTime && !baziDraft.occupation?.trim() && !baziDraft.industry?.trim()}
                        className="rounded-lg bg-[var(--accent-strong)] px-4 py-1.5 text-xs font-medium text-white transition-colors hover:opacity-90 disabled:opacity-50"
                      >
                        {intl.formatMessage({ id: "settings.fortune.save" })}
                      </button>
                      {baziProfile && (
                        <button
                          onClick={() => {
                            setBaziProfile(null);
                            setBaziDraft({ name: "", birthDate: "", birthTime: "", gender: "male", lunarCalendar: false, occupation: "", industry: "" });
                          }}
                          className="app-button-secondary text-xs"
                        >
                          {intl.formatMessage({ id: "settings.fortune.clear" })}
                        </button>
                      )}
                      {baziSaved && (
                        <span className="text-xs text-green-500">
                          {intl.formatMessage({ id: "settings.fortune.saved" })}
                        </span>
                      )}
                    </div>
                  </section>
                )}

                <section className="settings-card space-y-3">
                  <h4 className="text-[13px] font-semibold text-[var(--text-strong)]">
                    {intl.formatMessage({ id: "settings.about" })}
                  </h4>
                  <p className="text-xs leading-relaxed text-[var(--text-muted)]">
                    {intl.formatMessage({ id: "settings.about.description" })}
                  </p>
                  <div className="space-y-1">
                    <p className="text-xs text-[var(--text-faint)]">
                      {intl.formatMessage({ id: "settings.about.personalLabel" })}
                    </p>
                    <p className="text-[13px] font-medium text-[var(--text-strong)]">
                      {intl.formatMessage({ id: "settings.about.personalIntro" })}
                    </p>
                  </div>
                  <div className="space-y-1">
                    <p className="text-xs text-[var(--text-faint)]">
                      {intl.formatMessage({ id: "settings.about.version" })}
                    </p>
                    <p className="text-[13px] font-medium text-[var(--text-strong)]">
                      {appVersion}
                    </p>
                  </div>
                  <a
                    href="https://github.com/longdream/cn-codex"
                    target="_blank"
                    rel="noopener noreferrer"
                    className="inline-flex items-center gap-1 text-[11px] text-[var(--accent-strong)] hover:underline"
                  >
                    <IconExternalLink size={11} stroke={2} />
                    https://github.com/longdream/cn-codex
                  </a>
                </section>
              </div>
            )}

            {tab === "provider" && <ProviderPanel />}
            {tab === "image" && <ImageGenerationPanel />}
            {tab === "usage" && <UsageDashboard />}
            {tab === "integration" && <IntegrationPanel />}
            {tab === "plugins" && <PluginsPanel />}
            {tab === "skills" && <SkillsPanel />}
            {tab === "skill-lab" && <SkillLabPanel />}
            {tab === "robots" && <RobotsPanel />}
            {tab === "workflows" && <WorkflowsPanel />}
            {tab === "smartbrain" && <SmartbrainPanel />}
            {tab === "voice" && <VoiceSettingsPanel />}
            {tab === "rules" && (
              <div className="space-y-5">
                <section className="settings-card space-y-3">
                  <h4 className="text-[13px] font-semibold text-[var(--text-strong)]">
                    {intl.formatMessage({ id: "settings.rules" })}
                  </h4>
                  <p className="text-xs text-[var(--text-muted)]">
                    {intl.formatMessage({ id: "settings.rules.hint" })}
                  </p>
                  <textarea
                    value={rulesContent}
                    onChange={(e) => setRulesContent(e.target.value)}
                    placeholder={intl.formatMessage({ id: "settings.rules.placeholder" })}
                    className="w-full min-h-[300px] rounded-xl border border-[var(--border-subtle)] bg-[var(--surface-main)] p-3 font-mono text-xs text-[var(--text-base)] placeholder:text-[var(--text-faint)] focus:border-[var(--accent-strong)] focus:outline-none resize-y"
                  />
                  <div className="flex items-center gap-3">
                    <button
                      onClick={handleRulesSave}
                      disabled={rulesSaving}
                      className="rounded-lg bg-[var(--accent-strong)] px-4 py-1.5 text-xs font-medium text-white transition-colors hover:opacity-90 disabled:opacity-50"
                    >
                      {rulesSaving
                        ? intl.formatMessage({ id: "settings.rules.saving" })
                        : intl.formatMessage({ id: "settings.rules.save" })}
                    </button>
                    {rulesSaved && (
                      <span className="text-xs text-green-500">
                        {intl.formatMessage({ id: "settings.rules.saved" })}
                      </span>
                    )}
                  </div>
                </section>
              </div>
            )}
          </div>
        </section>
      </div>
    </div>
  );
}
