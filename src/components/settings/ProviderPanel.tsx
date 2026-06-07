import { useCallback, useEffect, useMemo, useState } from "react";
import { useIntl } from "react-intl";
import {
  standaloneConfigRead,
  standaloneConfigWrite,
} from "../../api";
import { useAppStore } from "../../stores/appStore";
import type { ConfigEdit } from "../../types";

type ReasoningEffort = "low" | "medium" | "high" | "xhigh";
type WebSearchMode = "live" | "cached" | "disabled";
type ApprovalPolicy = "untrusted" | "on-failure" | "on-request" | "never";

interface ProviderPreset {
  label: string;
  descriptionId: string;
  defaultModel: string;
  defaultBaseUrl: string;
  wireApi: string;
  requiresOpenAIAuth: boolean;
  providerKind: "builtin" | "custom";
}

interface ProviderFormState {
  modelProvider: string;
  model: string;
  baseUrl: string;
  apiKey: string;
  reasoningEffort: ReasoningEffort;
  webSearch: WebSearchMode;
  approvalPolicy: ApprovalPolicy;
  wireApi: string;
  requiresOpenAIAuth: boolean;
}

type ProviderConfig = Record<string, unknown>;
type ProviderConfigMap = Record<string, ProviderConfig>;

interface ProviderCategory {
  label: string;
  providers: string[];
}

const PROVIDER_CATEGORIES: ProviderCategory[] = [
  { label: "Global", providers: ["openai", "anthropic", "google"] },
  { label: "China", providers: ["deepseek", "volcengine", "qwen", "zhipu", "moonshot", "siliconflow", "baichuan"] },
  { label: "Local", providers: ["ollama", "lmstudio"] },
  { label: "Other", providers: ["custom"] },
];

const PROVIDER_PRESETS: Record<string, ProviderPreset> = {
  openai: {
    label: "OpenAI",
    descriptionId: "settings.provider.preset.openaiDescription",
    defaultModel: "gpt-5",
    defaultBaseUrl: "",
    wireApi: "",
    requiresOpenAIAuth: true,
    providerKind: "builtin",
  },
  anthropic: {
    label: "Anthropic",
    descriptionId: "settings.provider.preset.anthropicDescription",
    defaultModel: "claude-sonnet-4-20250514",
    defaultBaseUrl: "https://api.anthropic.com/v1",
    wireApi: "chat",
    requiresOpenAIAuth: false,
    providerKind: "custom",
  },
  google: {
    label: "Google Gemini",
    descriptionId: "settings.provider.preset.googleDescription",
    defaultModel: "gemini-2.5-pro",
    defaultBaseUrl: "https://generativelanguage.googleapis.com/v1beta/openai",
    wireApi: "chat",
    requiresOpenAIAuth: false,
    providerKind: "custom",
  },
  deepseek: {
    label: "DeepSeek",
    descriptionId: "settings.provider.preset.deepseekDescription",
    defaultModel: "deepseek-chat",
    defaultBaseUrl: "https://api.deepseek.com/v1",
    wireApi: "chat",
    requiresOpenAIAuth: false,
    providerKind: "custom",
  },
  volcengine: {
    label: "Volcengine Ark",
    descriptionId: "settings.provider.preset.volcengineDescription",
    defaultModel: "deepseek-v4-pro-260425",
    defaultBaseUrl: "https://ark.cn-beijing.volces.com/api/v3",
    wireApi: "chat",
    requiresOpenAIAuth: false,
    providerKind: "custom",
  },
  qwen: {
    label: "Qwen (Tongyi)",
    descriptionId: "settings.provider.preset.qwenDescription",
    defaultModel: "qwen-max",
    defaultBaseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    wireApi: "chat",
    requiresOpenAIAuth: false,
    providerKind: "custom",
  },
  zhipu: {
    label: "Zhipu AI",
    descriptionId: "settings.provider.preset.zhipuDescription",
    defaultModel: "glm-4-plus",
    defaultBaseUrl: "https://open.bigmodel.cn/api/paas/v4",
    wireApi: "chat",
    requiresOpenAIAuth: false,
    providerKind: "custom",
  },
  moonshot: {
    label: "Moonshot AI",
    descriptionId: "settings.provider.preset.moonshotDescription",
    defaultModel: "moonshot-v1-128k",
    defaultBaseUrl: "https://api.moonshot.cn/v1",
    wireApi: "chat",
    requiresOpenAIAuth: false,
    providerKind: "custom",
  },
  siliconflow: {
    label: "SiliconFlow",
    descriptionId: "settings.provider.preset.siliconflowDescription",
    defaultModel: "deepseek-ai/DeepSeek-V3",
    defaultBaseUrl: "https://api.siliconflow.cn/v1",
    wireApi: "chat",
    requiresOpenAIAuth: false,
    providerKind: "custom",
  },
  baichuan: {
    label: "Baichuan",
    descriptionId: "settings.provider.preset.baichuanDescription",
    defaultModel: "Baichuan4",
    defaultBaseUrl: "https://api.baichuan-ai.com/v1",
    wireApi: "chat",
    requiresOpenAIAuth: false,
    providerKind: "custom",
  },
  ollama: {
    label: "Ollama",
    descriptionId: "settings.provider.preset.ollamaDescription",
    defaultModel: "qwen2.5-coder:7b",
    defaultBaseUrl: "",
    wireApi: "",
    requiresOpenAIAuth: false,
    providerKind: "builtin",
  },
  lmstudio: {
    label: "LM Studio",
    descriptionId: "settings.provider.preset.lmstudioDescription",
    defaultModel: "local-model",
    defaultBaseUrl: "",
    wireApi: "",
    requiresOpenAIAuth: false,
    providerKind: "builtin",
  },
  custom: {
    label: "Custom",
    descriptionId: "settings.provider.preset.customDescription",
    defaultModel: "",
    defaultBaseUrl: "",
    wireApi: "chat",
    requiresOpenAIAuth: false,
    providerKind: "custom",
  },
};

const DEFAULT_FORM: ProviderFormState = {
  modelProvider: "openai",
  model: "gpt-5",
  baseUrl: "",
  apiKey: "",
  reasoningEffort: "medium",
  webSearch: "live",
  approvalPolicy: "on-request",
  wireApi: "",
  requiresOpenAIAuth: true,
};

const PROVIDER_BASE_URL_KEYS = [
  "base_url",
  "baseUrl",
  "endpoint",
  "url",
  "api_base_url",
  "apiBaseUrl",
] as const;
const PROVIDER_TOKEN_KEYS = [
  "experimental_bearer_token",
  "experimentalBearerToken",
  "api_key",
  "apiKey",
  "bearer_token",
  "bearerToken",
  "key",
  "token",
] as const;
const PROVIDER_WIRE_API_KEYS = [
  "wire_api",
  "wireApi",
  "api_format",
  "apiFormat",
  "api",
] as const;
const PROVIDER_AUTH_KEYS = ["requires_openai_auth", "requiresOpenaiAuth"] as const;

function getPreset(provider: string): ProviderPreset {
  return (
    PROVIDER_PRESETS[provider] ?? {
      label: provider,
      descriptionId: "settings.provider.preset.configDescription",
      defaultModel: "",
      defaultBaseUrl: "",
      wireApi: "responses",
      requiresOpenAIAuth: false,
      providerKind: "custom",
    }
  );
}

function readString(...values: unknown[]): string {
  for (const value of values) {
    if (typeof value === "string") {
      return value;
    }
  }
  return "";
}

function readBoolean(fallback: boolean, ...values: unknown[]): boolean {
  for (const value of values) {
    if (typeof value === "boolean") {
      return value;
    }
  }
  return fallback;
}

function normalizeReasoning(value: unknown): ReasoningEffort {
  return value === "low" || value === "medium" || value === "high" || value === "xhigh"
    ? value
    : "medium";
}

function normalizeWebSearch(value: unknown): WebSearchMode {
  return value === "live" || value === "cached" || value === "disabled" ? value : "live";
}

function normalizeApproval(value: unknown): ApprovalPolicy {
  return value === "untrusted" ||
    value === "on-failure" ||
    value === "on-request" ||
    value === "never"
    ? value
    : "on-request";
}

function adoptPreset(current: string, previousDefault: string, nextDefault: string): string {
  const trimmed = current.trim();
  if (!trimmed || trimmed === previousDefault) {
    return nextDefault;
  }
  return current;
}

function normalizeProviderConfigs(value: unknown): ProviderConfigMap {
  if (!value || typeof value !== "object") {
    return {};
  }

  return Object.fromEntries(
    Object.entries(value as Record<string, unknown>).map(([key, config]) => [
      key,
      typeof config === "object" && config !== null ? (config as ProviderConfig) : {},
    ]),
  );
}

function readProviderString(config: ProviderConfig, keys: readonly string[]): string {
  return readString(...keys.map((key) => config[key]));
}

function readProviderBoolean(
  config: ProviderConfig,
  fallback: boolean,
  keys: readonly string[],
): boolean {
  return readBoolean(
    fallback,
    ...keys.map((key) => config[key]),
  );
}

function findFirstExistingKey(
  config: ProviderConfig,
  keys: readonly string[],
): string | null {
  for (const key of keys) {
    if (Object.prototype.hasOwnProperty.call(config, key)) {
      return key;
    }
  }

  return null;
}

function clearKeys(target: ProviderConfig, keys: readonly string[]) {
  for (const key of keys) {
    delete target[key];
  }
}

function setPreferredKey(
  target: ProviderConfig,
  existingConfig: ProviderConfig,
  keys: readonly string[],
  fallbackKey: string,
  value: unknown,
) {
  const preferredKey = findFirstExistingKey(existingConfig, keys) ?? fallbackKey;
  target[preferredKey] = value;
}

function normalizeProviderBaseUrl(value: string): string {
  const trimmed = value.trim().replace(/\/+$/, "");
  if (!trimmed) {
    return "";
  }

  if (trimmed.endsWith("/responses")) {
    return trimmed.slice(0, -"/responses".length);
  }

  return trimmed;
}

export function ProviderPanel() {
  const intl = useIntl();
  const configPath = useAppStore((state) => state.configPath);
  const [form, setForm] = useState<ProviderFormState>(DEFAULT_FORM);
  const [providerConfigs, setProviderConfigs] = useState<ProviderConfigMap>({});
  const [loaded, setLoaded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [feedback, setFeedback] = useState<{ kind: "success" | "error"; text: string } | null>(
    null,
  );

  const currentPreset = useMemo(() => getPreset(form.modelProvider), [form.modelProvider]);

  const categorizedProviders = useMemo(() => {
    const categories = PROVIDER_CATEGORIES.map((cat) => ({
      ...cat,
      items: cat.providers
        .filter((id) => PROVIDER_PRESETS[id])
        .map((id) => ({ id, ...PROVIDER_PRESETS[id] })),
    }));

    if (!PROVIDER_PRESETS[form.modelProvider]) {
      categories[0] = {
        ...categories[0],
        items: [
          { id: form.modelProvider, ...getPreset(form.modelProvider) },
          ...categories[0].items,
        ],
      };
    }

    return categories;
  }, [form.modelProvider]);

  const currentProviderConfig = providerConfigs[form.modelProvider] ?? {};
  const hasStoredApiKey = Boolean(readProviderString(currentProviderConfig, PROVIDER_TOKEN_KEYS));

  useEffect(() => {
    const load = async () => {
      try {
        const response = await standaloneConfigRead();
        const config = (response?.config ?? {}) as Record<string, unknown>;
        const configs = normalizeProviderConfigs(
          config.model_providers ?? config.modelProviders ?? config.providers,
        );
        const modelProvider =
          readString(config.model_provider, config.modelProvider, config.provider) || "openai";
        const providerConfig = configs[modelProvider] ?? {};
        const preset = getPreset(modelProvider);

        setProviderConfigs(configs);
        setForm({
          modelProvider,
          model:
            readString(config.model) ||
            readString(providerConfig.model) ||
            preset.defaultModel ||
            DEFAULT_FORM.model,
          baseUrl:
            normalizeProviderBaseUrl(readProviderString(providerConfig, PROVIDER_BASE_URL_KEYS)) ||
            preset.defaultBaseUrl,
          apiKey: "",
          reasoningEffort: normalizeReasoning(
            config.model_reasoning_effort ?? config.modelReasoningEffort,
          ),
          webSearch: normalizeWebSearch(config.web_search ?? config.webSearch),
          approvalPolicy: normalizeApproval(config.approval_policy ?? config.approvalPolicy),
          wireApi: readProviderString(providerConfig, PROVIDER_WIRE_API_KEYS) || preset.wireApi,
          requiresOpenAIAuth: readProviderBoolean(
            providerConfig,
            preset.requiresOpenAIAuth,
            PROVIDER_AUTH_KEYS,
          ),
        });
      } catch (error) {
        console.error("Failed to load provider config:", error);
        setFeedback({
          kind: "error",
          text: intl.formatMessage({ id: "settings.provider.loadError" }),
        });
      } finally {
        setLoaded(true);
      }
    };

    void load();
  }, [intl]);

  const handleProviderChange = useCallback(
    (nextProvider: string) => {
      setFeedback(null);
      setForm((current) => {
        const previousPreset = getPreset(current.modelProvider);
        const nextPreset = getPreset(nextProvider);
        const storedProviderConfig = providerConfigs[nextProvider] ?? {};

        return {
          ...current,
          modelProvider: nextProvider,
          model:
            readString(storedProviderConfig.model) ||
            adoptPreset(current.model, previousPreset.defaultModel, nextPreset.defaultModel),
          baseUrl:
            normalizeProviderBaseUrl(
              readProviderString(storedProviderConfig, PROVIDER_BASE_URL_KEYS),
            ) ||
            adoptPreset(
              current.baseUrl,
              previousPreset.defaultBaseUrl,
              nextPreset.defaultBaseUrl,
            ),
          wireApi:
            readProviderString(storedProviderConfig, PROVIDER_WIRE_API_KEYS) || nextPreset.wireApi,
          requiresOpenAIAuth: readProviderBoolean(
            storedProviderConfig,
            nextPreset.requiresOpenAIAuth,
            PROVIDER_AUTH_KEYS,
          ),
          apiKey: "",
        };
      });
    },
    [providerConfigs],
  );

  const handleSave = useCallback(async () => {
    const trimmedModel = form.model.trim();
    const trimmedBaseUrl = normalizeProviderBaseUrl(form.baseUrl);
    const trimmedApiKey = form.apiKey.trim();
    const trimmedWireApi = form.wireApi.trim() || currentPreset.wireApi;

    if (!trimmedModel) {
      setFeedback({
        kind: "error",
        text: intl.formatMessage({ id: "settings.provider.modelRequired" }),
      });
      return;
    }

    if (currentPreset.providerKind === "custom" && !trimmedBaseUrl) {
      setFeedback({
        kind: "error",
        text: intl.formatMessage({ id: "settings.provider.baseUrlRequired" }),
      });
      return;
    }

    setSaving(true);
    setFeedback(null);

    try {
      const existingConfig = providerConfigs[form.modelProvider] ?? {};
      const persistedToken =
        trimmedApiKey || readProviderString(existingConfig, PROVIDER_TOKEN_KEYS);

      const providerOverride: ProviderConfig = { ...existingConfig };
      clearKeys(providerOverride, PROVIDER_BASE_URL_KEYS);
      clearKeys(providerOverride, PROVIDER_TOKEN_KEYS);
      clearKeys(providerOverride, PROVIDER_WIRE_API_KEYS);
      clearKeys(providerOverride, PROVIDER_AUTH_KEYS);

      if (trimmedBaseUrl) {
        setPreferredKey(
          providerOverride,
          existingConfig,
          PROVIDER_BASE_URL_KEYS,
          "base_url",
          trimmedBaseUrl,
        );
      }

      if (trimmedWireApi) {
        setPreferredKey(
          providerOverride,
          existingConfig,
          PROVIDER_WIRE_API_KEYS,
          "wire_api",
          trimmedWireApi,
        );
      }

      setPreferredKey(
        providerOverride,
        existingConfig,
        PROVIDER_AUTH_KEYS,
        "requires_openai_auth",
        form.requiresOpenAIAuth,
      );

      if (persistedToken) {
        setPreferredKey(
          providerOverride,
          existingConfig,
          PROVIDER_TOKEN_KEYS,
          "experimental_bearer_token",
          persistedToken,
        );
      }

      if (currentPreset.providerKind === "custom") {
        providerOverride.name = providerOverride.name || currentPreset.label || form.modelProvider;
      }

      const edits: ConfigEdit[] = [
        { keyPath: "model_provider", value: form.modelProvider, mergeStrategy: "replace" },
        { keyPath: "model", value: trimmedModel, mergeStrategy: "replace" },
        {
          keyPath: "model_reasoning_effort",
          value: form.reasoningEffort,
          mergeStrategy: "replace",
        },
        { keyPath: "web_search", value: form.webSearch, mergeStrategy: "replace" },
        { keyPath: "approval_policy", value: form.approvalPolicy, mergeStrategy: "replace" },
      ];

      if (currentPreset.providerKind === "custom") {
        edits.push({
          keyPath: `model_providers.${form.modelProvider}`,
          value: providerOverride,
          mergeStrategy: "replace",
        });
      }

      await standaloneConfigWrite(edits);

      setProviderConfigs((current) => ({
        ...current,
        [form.modelProvider]: providerOverride,
      }));
      setForm((current) => ({ ...current, apiKey: "" }));
      useAppStore.getState().setCurrentModel(trimmedModel);
      setFeedback({
        kind: "success",
        text: intl.formatMessage({ id: "settings.provider.saveSuccess" }),
      });
    } catch (error) {
      console.error("Failed to save provider config:", error);
      const detail = typeof error === "string" ? error : (error as Error)?.message ?? "";
      setFeedback({
        kind: "error",
        text: detail
          ? `${intl.formatMessage({ id: "settings.provider.saveError" })} ${detail}`
          : intl.formatMessage({ id: "settings.provider.saveError" }),
      });
    } finally {
      setSaving(false);
    }
  }, [currentPreset, form, intl, providerConfigs]);

  if (!loaded) {
    return (
      <div className="text-sm text-[var(--text-muted)]">
        {intl.formatMessage({ id: "common.loading" })}
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* Provider selection grid */}
      <section className="space-y-4">
        <div className="space-y-1">
          <h3 className="text-sm font-semibold tracking-tight text-[var(--text-strong)]">
            {intl.formatMessage({ id: "settings.provider.type" })}
          </h3>
          <p className="text-sm leading-relaxed text-[var(--text-muted)]">
            {intl.formatMessage({ id: currentPreset.descriptionId })}
          </p>
        </div>

        {categorizedProviders.map((category) => (
          <div key={category.label} className="space-y-2">
            <p className="text-[11px] font-medium uppercase tracking-[0.14em] text-[var(--text-faint)]">
              {intl.formatMessage({ id: `settings.provider.category.${category.label.toLowerCase()}` })}
            </p>
            <div className="grid grid-cols-2 gap-2 sm:grid-cols-3 lg:grid-cols-4">
              {category.items.map((item) => {
                const isActive = form.modelProvider === item.id;
                return (
                  <button
                    key={item.id}
                    onClick={() => handleProviderChange(item.id)}
                    className={`provider-card ${isActive ? "is-active" : ""}`}
                  >
                    <span className="provider-card-label">{item.label}</span>
                    <span className="provider-card-model">{item.defaultModel || "custom"}</span>
                  </button>
                );
              })}
            </div>
          </div>
        ))}
      </section>

      {/* Configuration form */}
      <section className="settings-card space-y-4">
        <div className="flex items-center justify-between">
          <div className="space-y-0.5">
            <h3 className="text-sm font-semibold text-[var(--text-strong)]">
              {currentPreset.label}
            </h3>
            <p className="text-xs text-[var(--text-muted)]">
              {intl.formatMessage({ id: currentPreset.descriptionId })}
            </p>
          </div>
          {configPath && (
            <span className="max-w-[200px] truncate rounded-md bg-[var(--surface-contrast)] px-2 py-1 font-mono text-[10px] text-[var(--text-faint)]">
              {configPath.split(/[/\\]/).pop()}
            </span>
          )}
        </div>

        <div className="grid gap-3 md:grid-cols-2">
          <div className="space-y-1.5">
            <label className="settings-field-label">
              {intl.formatMessage({ id: "settings.provider.model" })}
            </label>
            <input
              value={form.model}
              onChange={(event) =>
                setForm((current) => ({ ...current, model: event.target.value }))
              }
              placeholder={
                currentPreset.defaultModel ||
                intl.formatMessage({ id: "settings.provider.modelPlaceholder" })
              }
              className="app-input"
            />
          </div>

          <div className="space-y-1.5">
            <div className="flex items-center gap-2">
              <label className="settings-field-label">
                {intl.formatMessage({ id: "settings.provider.apiKey" })}
              </label>
              {hasStoredApiKey && (
                <span className="rounded-full bg-[var(--accent-soft)] px-1.5 py-0.5 text-[10px] text-[var(--accent-strong)]">
                  {intl.formatMessage({ id: "settings.provider.apiKeySaved" })}
                </span>
              )}
            </div>
            <input
              type="password"
              value={form.apiKey}
              onChange={(event) =>
                setForm((current) => ({ ...current, apiKey: event.target.value }))
              }
              placeholder={intl.formatMessage({ id: "settings.provider.apiKeyPlaceholder" })}
              className="app-input"
            />
          </div>

          <div className="space-y-1.5 md:col-span-2">
            <label className="settings-field-label">
              {intl.formatMessage({ id: "settings.provider.baseUrl" })}
            </label>
            <input
              value={form.baseUrl}
              onChange={(event) =>
                setForm((current) => ({ ...current, baseUrl: event.target.value }))
              }
              placeholder={
                currentPreset.defaultBaseUrl ||
                intl.formatMessage({ id: "settings.provider.baseUrlPlaceholder" })
              }
              className="app-input"
            />
          </div>
        </div>
      </section>

      {/* Runtime options */}
      <section className="settings-card space-y-4">
        <div className="space-y-0.5">
          <h3 className="text-sm font-semibold text-[var(--text-strong)]">
            {intl.formatMessage({ id: "settings.provider.runtime" })}
          </h3>
          <p className="text-xs text-[var(--text-muted)]">
            {intl.formatMessage({ id: "settings.provider.runtimeHint" })}
          </p>
        </div>

        <div className="grid gap-3 md:grid-cols-2">
          <div className="space-y-1.5">
            <label className="settings-field-label">
              {intl.formatMessage({ id: "settings.provider.effort" })}
            </label>
            <select
              value={form.reasoningEffort}
              onChange={(event) =>
                setForm((current) => ({
                  ...current,
                  reasoningEffort: normalizeReasoning(event.target.value),
                }))
              }
              className="app-select"
            >
              <option value="low">{intl.formatMessage({ id: "settings.provider.reasoning.low" })}</option>
              <option value="medium">{intl.formatMessage({ id: "settings.provider.reasoning.medium" })}</option>
              <option value="high">{intl.formatMessage({ id: "settings.provider.reasoning.high" })}</option>
              <option value="xhigh">{intl.formatMessage({ id: "settings.provider.reasoning.xhigh" })}</option>
            </select>
          </div>

          <div className="space-y-1.5">
            <label className="settings-field-label">
              {intl.formatMessage({ id: "settings.provider.webSearch" })}
            </label>
            <select
              value={form.webSearch}
              onChange={(event) =>
                setForm((current) => ({
                  ...current,
                  webSearch: normalizeWebSearch(event.target.value),
                }))
              }
              className="app-select"
            >
              <option value="live">{intl.formatMessage({ id: "settings.webSearch.live" })}</option>
              <option value="cached">{intl.formatMessage({ id: "settings.webSearch.cached" })}</option>
              <option value="disabled">{intl.formatMessage({ id: "settings.webSearch.disabled" })}</option>
            </select>
          </div>

          <div className="space-y-1.5">
            <label className="settings-field-label">
              {intl.formatMessage({ id: "settings.approvalMode" })}
            </label>
            <select
              value={form.approvalPolicy}
              onChange={(event) =>
                setForm((current) => ({
                  ...current,
                  approvalPolicy: normalizeApproval(event.target.value),
                }))
              }
              className="app-select"
            >
              <option value="on-request">{intl.formatMessage({ id: "settings.approvalMode.onRequest" })}</option>
              <option value="on-failure">{intl.formatMessage({ id: "settings.approvalMode.onFailure" })}</option>
              <option value="untrusted">{intl.formatMessage({ id: "settings.approvalMode.untrusted" })}</option>
              <option value="never">{intl.formatMessage({ id: "settings.approvalMode.never" })}</option>
            </select>
          </div>

          <div className="space-y-1.5">
            <label className="settings-field-label">
              {intl.formatMessage({ id: "settings.provider.transport" })}
            </label>
            <input
              value={form.wireApi}
              onChange={(event) =>
                setForm((current) => ({ ...current, wireApi: event.target.value }))
              }
              placeholder={intl.formatMessage({ id: "settings.provider.wireApiPlaceholder" })}
              className="app-input"
            />
          </div>
        </div>
      </section>

      {/* Feedback + Save */}
      {feedback && (
        <div
          className={`rounded-lg border px-4 py-3 text-sm ${
            feedback.kind === "success"
              ? "border-[var(--accent-border)] bg-[var(--accent-soft)] text-[var(--accent-strong)]"
              : "border-[rgba(220,92,92,0.35)] bg-[rgba(220,92,92,0.12)] text-[var(--danger)]"
          }`}
        >
          {feedback.text}
        </div>
      )}

      <div className="flex items-center justify-end">
        <button onClick={handleSave} disabled={saving} className="primary-button">
          {saving
            ? intl.formatMessage({ id: "common.saving" })
            : intl.formatMessage({ id: "common.save" })}
        </button>
      </div>
    </div>
  );
}
