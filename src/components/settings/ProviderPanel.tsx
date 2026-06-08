import { useCallback, useMemo, useState } from "react";
import { useIntl } from "react-intl";
import {
  IconCheck,
  IconPlus,
  IconTrash,
  IconX,
  IconBolt,
  IconExternalLink,
} from "@tabler/icons-react";
import {
  standaloneConfigWrite,
} from "../../api";
import { useAppStore, PROVIDER_PRESETS, createProviderFromPreset } from "../../stores/appStore";
import type { ProviderConfig, ProviderPreset, ProviderModel } from "../../types/provider";
import type { ConfigEdit } from "../../types";

/**
 * 供应商管理面板 (cc-switch 风格)
 * 左侧：实例列表（平铺），支持添加/激活/删除
 * 右侧：选中实例的配置表单
 */
export function ProviderPanel() {
  const intl = useIntl();
  const providers = useAppStore((s) => s.providers);
  const activeProviderId = useAppStore((s) => s.activeProviderId);

  const [selectedId, setSelectedId] = useState<string | null>(activeProviderId ?? providers[0]?.id ?? null);
  const [saving, setSaving] = useState(false);
  const [feedback, setFeedback] = useState<{ kind: "success" | "error"; text: string } | null>(null);
  const [showPresetDialog, setShowPresetDialog] = useState(false);

  // 编辑表单
  const [editForm, setEditForm] = useState<{
    name: string;
    apiKey: string;
    baseUrl: string;
    wireApi: string;
    newModelId: string;
    newModelLabel: string;
  }>({ name: "", apiKey: "", baseUrl: "", wireApi: "", newModelId: "", newModelLabel: "" });

  const selectedProvider = useMemo(
    () => providers.find((p) => p.id === selectedId) ?? null,
    [providers, selectedId],
  );

  // 选中实例时加载配置到编辑表单
  const handleSelect = useCallback((id: string) => {
    setSelectedId(id);
    setFeedback(null);
    const provider = useAppStore.getState().providers.find((p) => p.id === id);
    if (provider) {
      setEditForm({
        name: provider.name,
        apiKey: "",
        baseUrl: provider.baseUrl,
        wireApi: provider.wireApi,
        newModelId: "",
        newModelLabel: "",
      });
    }
  }, []);

  // 激活实例
  const handleActivate = useCallback((id: string) => {
    useAppStore.getState().activateProvider(id);
    setFeedback(null);
  }, []);

  // 从预设创建新实例
  const handleCreateFromPreset = useCallback((preset: ProviderPreset) => {
    const instance = createProviderFromPreset(preset);
    useAppStore.getState().addCustomProvider(instance);
    setShowPresetDialog(false);
    handleSelect(instance.id);
  }, [handleSelect]);

  // 删除实例
  const handleDelete = useCallback((providerId: string) => {
    // 不能删除当前激活的实例
    if (providerId === useAppStore.getState().activeProviderId) return;
    useAppStore.getState().removeCustomProvider(providerId);
    if (selectedId === providerId) {
      const remaining = useAppStore.getState().providers;
      setSelectedId(remaining[0]?.id ?? null);
    }
  }, [selectedId]);

  // 保存配置
  const handleSave = useCallback(async () => {
    if (!selectedProvider) return;
    setSaving(true);
    setFeedback(null);

    try {
      const trimmedName = editForm.name.trim() || selectedProvider.name;
      const trimmedBaseUrl = editForm.baseUrl.trim().replace(/\/+$/, "");
      const trimmedApiKey = editForm.apiKey.trim();
      const trimmedWireApi = editForm.wireApi.trim();

      const updates: Partial<ProviderConfig> = {
        name: trimmedName,
        baseUrl: trimmedBaseUrl,
        wireApi: trimmedWireApi || selectedProvider.wireApi,
      };
      if (trimmedApiKey) {
        updates.apiKey = trimmedApiKey;
      }
      useAppStore.getState().updateProvider(selectedProvider.id, updates);

      // 如果是激活的实例，同步写入 config.toml
      if (selectedProvider.id === useAppStore.getState().activeProviderId) {
        const providerKey = selectedProvider.type || "custom";
        const providerOverride: Record<string, unknown> = {};
        if (trimmedBaseUrl) providerOverride.base_url = trimmedBaseUrl;
        if (trimmedWireApi) providerOverride.wire_api = trimmedWireApi;
        const persistedKey = trimmedApiKey || selectedProvider.apiKey;
        if (persistedKey) providerOverride.experimental_bearer_token = persistedKey;
        providerOverride.requires_openai_auth = selectedProvider.requiresOpenAIAuth;

        const defaultModel = selectedProvider.models[0]?.id ?? "";
        const edits: ConfigEdit[] = [
          { keyPath: "model_provider", value: providerKey, mergeStrategy: "replace" },
          { keyPath: "model", value: defaultModel, mergeStrategy: "replace" },
          { keyPath: `model_providers.${providerKey}`, value: providerOverride, mergeStrategy: "replace" },
        ];
        await standaloneConfigWrite(edits);
      }

      setEditForm((f) => ({ ...f, apiKey: "" }));
      setFeedback({ kind: "success", text: intl.formatMessage({ id: "settings.provider.saveSuccess" }) });
    } catch (error) {
      console.error("Failed to save provider:", error);
      setFeedback({ kind: "error", text: intl.formatMessage({ id: "settings.provider.saveError" }) });
    } finally {
      setSaving(false);
    }
  }, [selectedProvider, editForm, intl]);

  // 添加模型
  const handleAddModel = useCallback(() => {
    if (!selectedProvider || !editForm.newModelId.trim()) return;
    const model: ProviderModel = {
      id: editForm.newModelId.trim(),
      label: editForm.newModelLabel.trim() || editForm.newModelId.trim(),
      supportsVision: false,
    };
    useAppStore.getState().addProviderModel(selectedProvider.id, model);
    setEditForm((f) => ({ ...f, newModelId: "", newModelLabel: "" }));
  }, [selectedProvider, editForm]);

  // 删除模型
  const handleRemoveModel = useCallback((modelId: string) => {
    if (!selectedProvider) return;
    useAppStore.getState().removeProviderModel(selectedProvider.id, modelId);
  }, [selectedProvider]);

  // 预设按分类分组（用于对话框）
  const presetsByCategory = useMemo(() => {
    const map: Record<string, ProviderPreset[]> = { global: [], china: [], local: [], other: [] };
    for (const p of PROVIDER_PRESETS) {
      (map[p.category] ?? map.other).push(p);
    }
    return Object.entries(map).filter(([, items]) => items.length > 0);
  }, []);

  return (
    <div className="flex h-full gap-4">
      {/* 左侧：实例列表 */}
      <div className="flex w-56 flex-shrink-0 flex-col">
        <div className="mb-3 flex items-center justify-between">
          <h3 className="text-xs font-semibold text-[var(--text-strong)]">
            {intl.formatMessage({ id: "settings.provider.type" })}
          </h3>
          <button
            onClick={() => setShowPresetDialog(true)}
            className="flex items-center gap-1 rounded-md px-2 py-1 text-[11px] text-[var(--accent)] hover:bg-[var(--accent-soft)] transition-colors"
          >
            <IconPlus size={12} stroke={2} />
            {intl.formatMessage({ id: "settings.provider.addCustom" })}
          </button>
        </div>

        <div className="flex-1 space-y-1 overflow-y-auto pr-1">
          {providers.length === 0 && (
            <div className="py-8 text-center text-xs text-[var(--text-faint)]">
              {intl.formatMessage({ id: "settings.provider.description" })}
              <br />
              <button
                onClick={() => setShowPresetDialog(true)}
                className="mt-2 text-[var(--accent)] underline"
              >
                {intl.formatMessage({ id: "settings.provider.addCustom" })}
              </button>
            </div>
          )}
          {providers.map((provider) => {
            const isActive = activeProviderId === provider.id;
            const isSelected = selectedId === provider.id;
            return (
              <div
                key={provider.id}
                onClick={() => handleSelect(provider.id)}
                className={`group relative flex cursor-pointer items-center gap-2 rounded-lg px-3 py-2 transition-colors ${
                  isSelected
                    ? "bg-[var(--surface-elevated)] ring-1 ring-[var(--accent-border)]"
                    : "hover:bg-[var(--surface-soft)]"
                }`}
              >
                {/* 激活指示器 */}
                <span className={`h-2 w-2 flex-shrink-0 rounded-full ${isActive ? "bg-green-500" : "bg-[var(--border-subtle)]"}`} />
                {/* 信息 */}
                <div className="flex-1 overflow-hidden">
                  <div className="flex items-center gap-1">
                    <span className="truncate text-xs font-medium text-[var(--text-strong)]">
                      {provider.name}
                    </span>
                    {isActive && (
                      <IconCheck size={10} stroke={3} className="flex-shrink-0 text-green-500" />
                    )}
                  </div>
                  <span className="block truncate text-[10px] text-[var(--text-faint)]">
                    {provider.baseUrl || provider.type}
                  </span>
                </div>
                {/* 删除按钮（非激活的才能删除） */}
                {!isActive && (
                  <button
                    onClick={(e) => { e.stopPropagation(); handleDelete(provider.id); }}
                    className="hidden flex-shrink-0 rounded p-0.5 text-[var(--text-faint)] hover:text-[var(--danger)] group-hover:block"
                  >
                    <IconTrash size={12} stroke={2} />
                  </button>
                )}
              </div>
            );
          })}
        </div>
      </div>

      {/* 右侧：配置表单 */}
      <div className="flex-1 overflow-y-auto">
        {selectedProvider ? (
          <div className="settings-card space-y-4">
            {/* 标题行 */}
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <span className="rounded bg-[var(--surface-soft)] px-1.5 py-0.5 text-[10px] text-[var(--text-faint)]">
                  {selectedProvider.type}
                </span>
                {activeProviderId === selectedProvider.id && (
                  <span className="rounded-full bg-green-500/15 px-2 py-0.5 text-[10px] font-medium text-green-600">
                    {intl.formatMessage({ id: "settings.provider.current" })}
                  </span>
                )}
              </div>
              {activeProviderId !== selectedProvider.id && (
                <button
                  onClick={() => handleActivate(selectedProvider.id)}
                  className="flex items-center gap-1 rounded-md border border-[var(--accent-border)] px-2 py-1 text-[11px] text-[var(--accent)] hover:bg-[var(--accent-soft)] transition-colors"
                >
                  <IconBolt size={12} stroke={2} />
                  {intl.formatMessage({ id: "settings.provider.activateThis" })}
                </button>
              )}
            </div>

            {/* 名称 */}
            <div className="space-y-1.5">
              <label className="settings-field-label">
                名称
              </label>
              <input
                value={editForm.name}
                onChange={(e) => setEditForm((f) => ({ ...f, name: e.target.value }))}
                placeholder="供应商实例名称"
                className="app-input"
              />
            </div>

            {/* API Key & Base URL */}
            <div className="grid gap-3 md:grid-cols-2">
              <div className="space-y-1.5">
                <label className="settings-field-label">
                  {intl.formatMessage({ id: "settings.provider.apiKey" })}
                  {selectedProvider.apiKey && (
                    <span className="ml-2 rounded-full bg-[var(--accent-soft)] px-1.5 py-0.5 text-[10px] text-[var(--accent-strong)]">
                      {intl.formatMessage({ id: "settings.provider.apiKeySaved" })}
                    </span>
                  )}
                </label>
                <input
                  type="password"
                  value={editForm.apiKey}
                  onChange={(e) => setEditForm((f) => ({ ...f, apiKey: e.target.value }))}
                  placeholder={intl.formatMessage({ id: "settings.provider.apiKeyPlaceholder" })}
                  className="app-input"
                />
                {/* 获取 API Key 链接 */}
                {(() => {
                  const preset = PROVIDER_PRESETS.find((p) => p.type === selectedProvider.type);
                  return preset?.signupUrl ? (
                    <a
                      href={preset.signupUrl}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="inline-flex items-center gap-1 text-[11px] text-[var(--accent)] hover:underline"
                    >
                      <IconExternalLink size={11} stroke={2} />
                      {intl.formatMessage({ id: "settings.provider.getApiKey", defaultMessage: "前往获取 API Key" })}
                    </a>
                  ) : null;
                })()}
              </div>
              <div className="space-y-1.5">
                <label className="settings-field-label">
                  {intl.formatMessage({ id: "settings.provider.baseUrl" })}
                </label>
                <input
                  value={editForm.baseUrl}
                  onChange={(e) => setEditForm((f) => ({ ...f, baseUrl: e.target.value }))}
                  placeholder={intl.formatMessage({ id: "settings.provider.baseUrlPlaceholder" })}
                  className="app-input"
                />
              </div>
            </div>

            {/* Wire API 格式选择 */}
            <div className="space-y-1.5">
              <label className="settings-field-label">
                {intl.formatMessage({ id: "settings.provider.transport", defaultMessage: "API 格式" })}
              </label>
              <select
                value={editForm.wireApi}
                onChange={(e) => setEditForm((f) => ({ ...f, wireApi: e.target.value }))}
                className="app-select w-56"
              >
                <option value="chat">OpenAI Chat Completions</option>
                <option value="responses">OpenAI Responses API</option>
                <option value="anthropic">Anthropic Messages API</option>
                <option value="gemini">Google Gemini API</option>
              </select>
              <p className="text-[10px] text-[var(--text-faint)]">
                {editForm.wireApi === "chat" && "适用于大多数 OpenAI 兼容供应商（DeepSeek、通义千问等）"}
                {editForm.wireApi === "responses" && "适用于 OpenAI 官方 Responses API 和部分中转站"}
                {editForm.wireApi === "anthropic" && "适用于 Anthropic Claude 原生 API"}
                {editForm.wireApi === "gemini" && "适用于 Google Gemini 原生 API（非 OpenAI 兼容端点）"}
              </p>
            </div>

            {/* 模型列表 */}
            <div className="space-y-2">
              <h4 className="text-xs font-medium text-[var(--text-strong)]">
                {intl.formatMessage({ id: "settings.provider.models" })}
              </h4>
              <div className="space-y-1">
                {selectedProvider.models.map((model) => (
                  <div key={model.id} className="flex items-center justify-between rounded-md bg-[var(--surface-soft)] px-3 py-1.5">
                    <div>
                      <span className="text-xs text-[var(--text-strong)]">{model.label}</span>
                      <span className="ml-2 text-[11px] text-[var(--text-faint)]">{model.id}</span>
                      {model.supportsVision && (
                        <span className="ml-1 text-[10px] text-[var(--accent)]">Vision</span>
                      )}
                    </div>
                    <button
                      onClick={() => handleRemoveModel(model.id)}
                      className="text-[var(--text-faint)] hover:text-[var(--danger)] transition-colors"
                    >
                      <IconX size={12} stroke={2} />
                    </button>
                  </div>
                ))}
              </div>
              {/* 添加新模型 */}
              <div className="flex items-center gap-2">
                <input
                  value={editForm.newModelId}
                  onChange={(e) => setEditForm((f) => ({ ...f, newModelId: e.target.value }))}
                  placeholder={intl.formatMessage({ id: "settings.provider.modelPlaceholder" })}
                  className="app-input flex-1 text-xs"
                  onKeyDown={(e) => { if (e.key === "Enter") handleAddModel(); }}
                />
                <input
                  value={editForm.newModelLabel}
                  onChange={(e) => setEditForm((f) => ({ ...f, newModelLabel: e.target.value }))}
                  placeholder={intl.formatMessage({ id: "settings.provider.modelLabelPlaceholder" })}
                  className="app-input flex-1 text-xs"
                  onKeyDown={(e) => { if (e.key === "Enter") handleAddModel(); }}
                />
                <button onClick={handleAddModel} className="flex h-7 w-7 items-center justify-center rounded-md bg-[var(--accent-soft)] text-[var(--accent)] hover:bg-[var(--accent)] hover:text-white transition-colors">
                  <IconPlus size={12} stroke={2} />
                </button>
              </div>
            </div>

            {/* 反馈 */}
            {feedback && (
              <div className={`rounded-lg border px-4 py-3 text-sm ${
                feedback.kind === "success"
                  ? "border-[var(--accent-border)] bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                  : "border-[rgba(220,92,92,0.35)] bg-[rgba(220,92,92,0.12)] text-[var(--danger)]"
              }`}>
                {feedback.text}
              </div>
            )}

            {/* 保存按钮 */}
            <div className="flex justify-end">
              <button onClick={handleSave} disabled={saving} className="primary-button">
                {saving
                  ? intl.formatMessage({ id: "common.saving" })
                  : intl.formatMessage({ id: "common.save" })}
              </button>
            </div>
          </div>
        ) : (
          <div className="flex h-full items-center justify-center text-xs text-[var(--text-faint)]">
            {providers.length === 0
              ? "点击左上角「添加」按钮创建你的第一个供应商"
              : "选择左侧的供应商实例查看配置"}
          </div>
        )}
      </div>

      {/* 从预设添加供应商的对话框 */}
      {showPresetDialog && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50" onClick={() => setShowPresetDialog(false)}>
          <div className="w-[480px] max-h-[80vh] overflow-y-auto rounded-xl bg-[var(--surface-base)] p-5 shadow-2xl" onClick={(e) => e.stopPropagation()}>
            <div className="mb-4 flex items-center justify-between">
              <h3 className="text-sm font-semibold text-[var(--text-strong)]">
                添加供应商
              </h3>
              <button onClick={() => setShowPresetDialog(false)} className="rounded p-1 text-[var(--text-faint)] hover:bg-[var(--surface-soft)]">
                <IconX size={16} stroke={2} />
              </button>
            </div>
            <p className="mb-4 text-xs text-[var(--text-muted)]">
              选择一个预设模板，将创建新的供应商实例。同一类型可以创建多个。
            </p>
            {presetsByCategory.map(([category, presets]) => (
              <div key={category} className="mb-4">
                <p className="mb-2 text-[11px] font-medium uppercase tracking-[0.14em] text-[var(--text-faint)]">
                  {category === "global" && "国际"}
                  {category === "china" && "国内"}
                  {category === "local" && "本地"}
                  {category === "other" && "其他"}
                </p>
                <div className="grid grid-cols-3 gap-2">
                  {presets.map((preset) => (
                    <div
                      key={preset.type}
                      className="rounded-lg border border-[var(--border-subtle)] px-3 py-2 text-left transition-colors hover:border-[var(--accent-border)] hover:bg-[var(--accent-soft)]"
                    >
                      <button
                        onClick={() => handleCreateFromPreset(preset)}
                        className="w-full text-left"
                      >
                        <span className="block text-xs font-medium text-[var(--text-strong)]">{preset.name}</span>
                        <span className="block truncate text-[10px] text-[var(--text-faint)]">
                          {preset.defaultModels[0]?.label ?? preset.type}
                        </span>
                      </button>
                      {preset.signupUrl && (
                        <a
                          href={preset.signupUrl}
                          target="_blank"
                          rel="noopener noreferrer"
                          onClick={(e) => e.stopPropagation()}
                          className="mt-1 inline-flex items-center gap-0.5 text-[10px] text-[var(--accent)] hover:underline"
                        >
                          <IconExternalLink size={9} stroke={2} />
                          获取 Key
                        </a>
                      )}
                    </div>
                  ))}
                </div>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
