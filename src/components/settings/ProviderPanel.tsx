import { useCallback, useEffect, useMemo, useState } from "react";
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
  IconEye,
  IconEyeOff,
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
    maxOutputTokens: string;
    newModelId: string;
    newModelLabel: string;
    newModelContextLength: string;
    newModelSupportsVision: boolean;
  }>({ name: "", apiKey: "", baseUrl: "", wireApi: "", maxOutputTokens: "131072", newModelId: "", newModelLabel: "", newModelContextLength: "65535", newModelSupportsVision: false });

  const selectedProvider = useMemo(
    () => providers.find((p) => p.id === selectedId) ?? null,
    [providers, selectedId],
  );

  const selectedPreset = useMemo(
    () => PROVIDER_PRESETS.find((p) => p.type === selectedProvider?.type),
    [selectedProvider?.type],
  );

  useEffect(() => {
    const fallbackId = activeProviderId ?? providers[0]?.id ?? null;
    setSelectedId((currentId) => {
      if (currentId && providers.some((provider) => provider.id === currentId)) {
        return currentId;
      }
      return fallbackId;
    });
  }, [activeProviderId, providers]);

  useEffect(() => {
    if (!selectedProvider) return;
    setEditForm((form) => ({
      ...form,
      name: selectedProvider.name,
      apiKey: "",
      baseUrl: selectedProvider.baseUrl,
      wireApi: selectedProvider.wireApi,
      maxOutputTokens: String(selectedProvider.maxOutputTokens ?? 131072),
    }));
  }, [
    selectedProvider?.id,
    selectedProvider?.name,
    selectedProvider?.baseUrl,
    selectedProvider?.wireApi,
    selectedProvider?.maxOutputTokens,
  ]);

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
        maxOutputTokens: String(provider.maxOutputTokens ?? 131072),
        newModelId: "",
        newModelLabel: "",
        newModelContextLength: "65535",
        newModelSupportsVision: false,
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

    if (editForm.newModelId.trim()) {
      setFeedback({ kind: "error", text: intl.formatMessage({ id: "settings.provider.unsavedModel" }) });
      return;
    }

    setSaving(true);
    setFeedback(null);

    try {
      const trimmedName = editForm.name.trim() || selectedProvider.name;
      const trimmedBaseUrl = editForm.baseUrl.trim().replace(/\/+$/, "");
      const trimmedApiKey = editForm.apiKey.trim();
      const trimmedWireApi = editForm.wireApi.trim();

      const parsedMaxTokens = parseInt(editForm.maxOutputTokens, 10);
      const maxOutputTokens = Number.isFinite(parsedMaxTokens) && parsedMaxTokens > 0 ? parsedMaxTokens : 131072;

      const updates: Partial<ProviderConfig> = {
        name: trimmedName,
        baseUrl: trimmedBaseUrl,
        wireApi: trimmedWireApi || selectedProvider.wireApi,
        maxOutputTokens,
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
          { keyPath: "max_output_tokens", value: maxOutputTokens, mergeStrategy: "replace" },
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
    const parsedCtx = parseInt(editForm.newModelContextLength, 10);
    const model: ProviderModel = {
      id: editForm.newModelId.trim(),
      label: editForm.newModelLabel.trim() || editForm.newModelId.trim(),
      supportsVision: editForm.newModelSupportsVision,
      contextLength: Number.isFinite(parsedCtx) && parsedCtx > 0 ? parsedCtx : 65535,
    };
    useAppStore.getState().addProviderModel(selectedProvider.id, model);
    setEditForm((f) => ({ ...f, newModelId: "", newModelLabel: "", newModelContextLength: "65535", newModelSupportsVision: false }));
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
      <div className="flex w-60 flex-shrink-0 flex-col rounded-[var(--radius-lg)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] p-3">
        <div className="mb-3 flex items-center justify-between">
          <h3 className="text-xs font-semibold text-[var(--text-strong)]">
            {intl.formatMessage({ id: "settings.provider.type" })}
          </h3>
          <button
            type="button"
            onClick={() => setShowPresetDialog(true)}
            className="inline-flex items-center gap-1 rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--accent-soft)] px-2 py-1 text-[11px] font-medium text-[var(--accent-strong)] transition-colors hover:bg-[var(--surface-elevated)]"
          >
            <IconPlus size={12} stroke={2} />
            {intl.formatMessage({ id: "settings.provider.addCustom" })}
          </button>
        </div>

        <div className="thin-scrollbar flex-1 space-y-1 overflow-y-auto pr-1">
          {providers.length === 0 && (
            <div className="rounded-[var(--radius-md)] border border-dashed border-[var(--border-strong)] bg-[var(--surface-contrast)]/60 px-3 py-8 text-center text-xs text-[var(--text-faint)]">
              <p>{intl.formatMessage({ id: "settings.provider.description" })}</p>
              <button
                type="button"
                onClick={() => setShowPresetDialog(true)}
                className="mt-3 text-[var(--accent-strong)] underline underline-offset-4"
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
                className={`group relative flex w-full items-center gap-2 rounded-[var(--radius-md)] border px-2 py-1.5 transition-all ${
                  isSelected
                    ? "border-[var(--accent-border)] bg-[var(--accent-soft)] text-[var(--text-strong)]"
                    : "border-transparent bg-transparent text-[var(--text-base)] hover:border-[var(--border-subtle)] hover:bg-[var(--surface-soft)]"
                }`}
              >
                <button
                  type="button"
                  onClick={() => handleSelect(provider.id)}
                  aria-pressed={isSelected}
                  className="flex min-w-0 flex-1 items-center gap-2 text-left"
                >
                  {/* 激活指示器 */}
                  <span className={`h-2 w-2 flex-shrink-0 rounded-full ${isActive ? "bg-[var(--accent)]" : "bg-[var(--border-strong)]"}`} />
                  {/* 信息 */}
                  <div className="flex-1 overflow-hidden">
                    <div className="flex items-center gap-1">
                      <span className="truncate text-xs font-medium text-[var(--text-strong)]">
                        {provider.name}
                      </span>
                      {isActive && (
                        <IconCheck size={10} stroke={3} className="flex-shrink-0 text-[var(--accent-strong)]" />
                      )}
                    </div>
                    <span className="block truncate text-[11px] text-[var(--text-faint)]">
                      {provider.baseUrl || provider.type}
                    </span>
                  </div>
                </button>
                {/* 删除按钮（非激活的才能删除） */}
                {!isActive && (
                  <button
                    type="button"
                    onClick={(e) => { e.stopPropagation(); handleDelete(provider.id); }}
                    className="flex flex-shrink-0 rounded-[var(--radius-sm)] p-0.5 text-[var(--text-faint)] opacity-0 transition-opacity hover:text-[var(--danger)] group-hover:opacity-100"
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
      <div className="thin-scrollbar flex-1 overflow-y-auto">
        {selectedProvider ? (
          <div className="settings-card flex min-h-full flex-col gap-3">
            {/* 标题行 */}
            <div className="flex items-start justify-between gap-4 border-b border-[var(--border-subtle)] pb-3">
              <div className="min-w-0 space-y-2">
                <div className="flex flex-wrap items-center gap-2">
                  <h3 className="truncate text-sm font-semibold text-[var(--text-strong)]">
                    {selectedProvider.name}
                  </h3>
                  <span className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-1.5 py-0.5 text-[11px] text-[var(--text-faint)]">
                    {selectedProvider.type}
                  </span>
                  {activeProviderId === selectedProvider.id && (
                    <span className="rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--accent-soft)] px-2 py-0.5 text-[11px] font-medium text-[var(--accent-strong)]">
                      {intl.formatMessage({ id: "settings.provider.current" })}
                    </span>
                  )}
                </div>
                <p className="truncate font-mono text-[11px] text-[var(--text-faint)]">
                  {selectedProvider.baseUrl || selectedPreset?.defaultBaseUrl || selectedProvider.type}
                </p>
              </div>
              {activeProviderId !== selectedProvider.id && (
                <button
                  type="button"
                  onClick={() => handleActivate(selectedProvider.id)}
                  className="inline-flex shrink-0 items-center gap-1 rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--accent-soft)] px-2.5 py-1.5 text-[11px] font-medium text-[var(--accent-strong)] transition-colors hover:bg-[var(--surface-elevated)]"
                >
                  <IconBolt size={12} stroke={2} />
                  {intl.formatMessage({ id: "settings.provider.activateThis" })}
                </button>
              )}
            </div>

            {/* 名称 */}
            <div className="space-y-1.5">
              <label className="settings-field-label">
                {intl.formatMessage({ id: "settings.provider.name" })}
              </label>
              <input
                value={editForm.name}
                onChange={(e) => setEditForm((f) => ({ ...f, name: e.target.value }))}
                placeholder={intl.formatMessage({ id: "settings.provider.namePlaceholder" })}
                className="app-input"
              />
            </div>

            {/* API Key & Base URL */}
            <div className="grid gap-3 md:grid-cols-2">
              <div className="space-y-1.5">
                <label className="settings-field-label">
                  {intl.formatMessage({ id: "settings.provider.apiKey" })}
                  {selectedProvider.apiKey && (
                    <span className="ml-2 rounded-[var(--radius-sm)] border border-[var(--accent-border)] bg-[var(--accent-soft)] px-1.5 py-0.5 text-[11px] text-[var(--accent-strong)]">
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
                  return selectedPreset?.signupUrl ? (
                    <a
                      href={selectedPreset.signupUrl}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="inline-flex items-center gap-1 text-[11px] text-[var(--accent-strong)] hover:underline"
                    >
                      <IconExternalLink size={11} stroke={2} />
                      {intl.formatMessage({ id: "settings.provider.getApiKey" })}
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
              <p className="text-[11px] text-[var(--text-faint)]">
                {editForm.wireApi &&
                  intl.formatMessage({
                    id: `settings.provider.transportHint.${editForm.wireApi}`,
                  })}
              </p>
            </div>

            {/* 最大输出 Token */}
            <div className="space-y-1.5">
              <label className="settings-field-label">
                {intl.formatMessage({ id: "settings.provider.maxOutputTokens" })}
              </label>
              <input
                type="number"
                min={1}
                value={editForm.maxOutputTokens}
                onChange={(e) => setEditForm((f) => ({ ...f, maxOutputTokens: e.target.value }))}
                className="app-input w-56"
              />
              <p className="text-[11px] text-[var(--text-faint)]">
                {intl.formatMessage({ id: "settings.provider.maxOutputTokensHint" })}
              </p>
            </div>

            {/* 模型列表 */}
            <div className="space-y-2">
              <h4 className="text-xs font-medium text-[var(--text-strong)]">
                {intl.formatMessage({ id: "settings.provider.models" })}
              </h4>
              <div className="space-y-1">
                {selectedProvider.models.map((model) => (
                  <ModelRow
                    key={model.id}
                    model={model}
                    providerId={selectedProvider.id}
                    onRemove={() => handleRemoveModel(model.id)}
                  />
                ))}
              </div>
              {/* 添加新模型 */}
              <div className="space-y-2 rounded-[var(--radius-md)] border border-dashed border-[var(--border-strong)] bg-[var(--surface-contrast)]/40 p-3">
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
                </div>
                <div className="flex items-center gap-3">
                  <div className="flex items-center gap-1.5">
                    <label className="text-[11px] text-[var(--text-faint)]">
                      {intl.formatMessage({ id: "settings.provider.contextLength" })}
                    </label>
                    <input
                      type="number"
                      min={1}
                      value={editForm.newModelContextLength}
                      onChange={(e) => setEditForm((f) => ({ ...f, newModelContextLength: e.target.value }))}
                      className="app-input w-24 text-xs"
                      onKeyDown={(e) => { if (e.key === "Enter") handleAddModel(); }}
                    />
                  </div>
                  <label className="flex cursor-pointer items-center gap-1.5 text-[11px] text-[var(--text-faint)]">
                    <input
                      type="checkbox"
                      checked={editForm.newModelSupportsVision}
                      onChange={(e) => setEditForm((f) => ({ ...f, newModelSupportsVision: e.target.checked }))}
                      className="h-3.5 w-3.5 rounded border-[var(--border-subtle)] accent-[var(--accent)]"
                    />
                    {intl.formatMessage({ id: "settings.provider.supportsVision" })}
                  </label>
                  <div className="flex-1" />
                  <button
                    type="button"
                    onClick={handleAddModel}
                    className="flex h-7 w-7 items-center justify-center rounded-[var(--radius-sm)] bg-[var(--accent-soft)] text-[var(--accent-strong)] transition-colors hover:bg-[var(--accent)] hover:text-white"
                  >
                    <IconPlus size={12} stroke={2} />
                  </button>
                </div>
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
            <div className="-mx-5 mt-auto flex justify-end border-t border-[var(--border-subtle)] bg-[var(--surface-panel)]/95 px-5 py-3">
              <button type="button" onClick={handleSave} disabled={saving} className="primary-button">
                {saving
                  ? intl.formatMessage({ id: "common.saving" })
                  : intl.formatMessage({ id: "common.save" })}
              </button>
            </div>
          </div>
        ) : (
          <div className="flex h-full items-center justify-center text-xs text-[var(--text-faint)]">
            {providers.length === 0
              ? intl.formatMessage({ id: "settings.provider.emptyCreate" })
              : intl.formatMessage({ id: "settings.provider.emptySelect" })}
          </div>
        )}
      </div>

      {/* 从预设添加供应商的对话框 */}
      {showPresetDialog && (
        <div className="fixed bottom-0 left-0 right-0 top-8 z-50 flex items-center justify-center bg-black/45 p-4 backdrop-blur-sm" onClick={() => setShowPresetDialog(false)}>
          <div className="thin-scrollbar max-h-[80vh] w-full max-w-[520px] overflow-y-auto rounded-[var(--radius-lg)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] p-5 shadow-[var(--shadow-strong)]" onClick={(e) => e.stopPropagation()}>
            <div className="mb-4 flex items-center justify-between">
              <h3 className="text-sm font-semibold text-[var(--text-strong)]">
                {intl.formatMessage({ id: "settings.provider.addProvider" })}
              </h3>
              <button type="button" onClick={() => setShowPresetDialog(false)} className="icon-button">
                <IconX size={16} stroke={2} />
              </button>
            </div>
            <p className="mb-4 text-xs text-[var(--text-muted)]">
              {intl.formatMessage({ id: "settings.provider.addProviderHint" })}
            </p>
            {presetsByCategory.map(([category, presets]) => (
              <div key={category} className="mb-4">
                <p className="mb-2 text-[11px] font-medium uppercase tracking-[0.14em] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: `settings.provider.category.${category}` })}
                </p>
                <div className="grid grid-cols-3 gap-2">
                  {presets.map((preset) => (
                    <div
                      key={preset.type}
                      className="rounded-[var(--radius-md)] border border-[var(--border-subtle)] bg-[var(--surface-contrast)] px-3 py-2 text-left transition-colors hover:border-[var(--accent-border)] hover:bg-[var(--accent-soft)]"
                    >
                      <button
                        type="button"
                        onClick={() => handleCreateFromPreset(preset)}
                        className="w-full text-left"
                      >
                        <span className="block text-xs font-medium text-[var(--text-strong)]">{preset.name}</span>
                        <span className="block truncate text-[11px] text-[var(--text-faint)]">
                          {preset.defaultModels[0]?.label ?? preset.type}
                        </span>
                      </button>
                      {preset.signupUrl && (
                        <a
                          href={preset.signupUrl}
                          target="_blank"
                          rel="noopener noreferrer"
                          onClick={(e) => e.stopPropagation()}
                          className="mt-1 inline-flex items-center gap-0.5 text-[11px] text-[var(--accent-strong)] hover:underline"
                        >
                          <IconExternalLink size={9} stroke={2} />
                          {intl.formatMessage({ id: "settings.provider.getKeyShort" })}
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

function ModelRow({
  model,
  providerId,
  onRemove,
}: {
  model: ProviderModel;
  providerId: string;
  onRemove: () => void;
}) {
  const intl = useIntl();
  const [editingCtx, setEditingCtx] = useState(false);
  const [ctxValue, setCtxValue] = useState(String(model.contextLength ?? 65535));

  const handleToggleVision = useCallback(() => {
    useAppStore.getState().updateProviderModel(providerId, model.id, {
      supportsVision: !model.supportsVision,
    });
  }, [providerId, model.id, model.supportsVision]);

  const handleSaveContextLength = useCallback(() => {
    const parsed = parseInt(ctxValue, 10);
    if (Number.isFinite(parsed) && parsed > 0) {
      useAppStore.getState().updateProviderModel(providerId, model.id, {
        contextLength: parsed,
      });
    }
    setEditingCtx(false);
  }, [providerId, model.id, ctxValue]);

  return (
    <div className="flex items-center justify-between gap-2 rounded-[var(--radius-md)] border border-[var(--border-subtle)] bg-[var(--surface-contrast)] px-3 py-1.5">
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5">
          <span className="text-xs font-medium text-[var(--text-strong)]">{model.label}</span>
          <span className="break-all font-mono text-[11px] text-[var(--text-faint)]">{model.id}</span>
        </div>
        <div className="mt-0.5 flex items-center gap-2">
          {editingCtx ? (
            <div className="flex items-center gap-1">
              <input
                type="number"
                min={1}
                value={ctxValue}
                onChange={(e) => setCtxValue(e.target.value)}
                onBlur={handleSaveContextLength}
                onKeyDown={(e) => {
                  if (e.key === "Enter") handleSaveContextLength();
                  if (e.key === "Escape") setEditingCtx(false);
                }}
                className="app-input w-24 text-[11px]"
                autoFocus
              />
              <span className="text-[10px] text-[var(--text-faint)]">tokens</span>
            </div>
          ) : (
            <button
              type="button"
              onClick={() => {
                setCtxValue(String(model.contextLength ?? 65535));
                setEditingCtx(true);
              }}
              className="text-[11px] text-[var(--text-faint)] hover:text-[var(--accent)] transition-colors"
              title={intl.formatMessage({ id: "settings.provider.contextLengthHint" })}
            >
              {(model.contextLength ?? 65535).toLocaleString()} ctx
            </button>
          )}
          <button
            type="button"
            onClick={handleToggleVision}
            className={`flex items-center gap-0.5 rounded-[var(--radius-sm)] px-1.5 py-0.5 text-[11px] transition-colors ${
              model.supportsVision
                ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                : "text-[var(--text-faint)] hover:text-[var(--text-muted)]"
            }`}
            title={intl.formatMessage({ id: "settings.provider.supportsVisionHint" })}
          >
            {model.supportsVision ? (
              <IconEye size={11} stroke={1.8} />
            ) : (
              <IconEyeOff size={11} stroke={1.8} />
            )}
            Vision
          </button>
        </div>
      </div>
      <button
        type="button"
        onClick={onRemove}
        className="shrink-0 rounded-[var(--radius-sm)] p-1 text-[var(--text-faint)] transition-colors hover:bg-[var(--danger-soft)] hover:text-[var(--danger)]"
      >
        <IconX size={12} stroke={2} />
      </button>
    </div>
  );
}
