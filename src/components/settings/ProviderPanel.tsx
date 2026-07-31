import { useCallback, useEffect, useMemo, useState } from "react";
import { useIntl } from "react-intl";
import { invoke } from "@tauri-apps/api/core";
import {
  IconCheck,
  IconPlus,
  IconTrash,
  IconX,
  IconBolt,
  IconRefresh,
  IconExternalLink,
  IconChevronDown,
  IconChevronRight,
  IconEye,
  IconEyeOff,
  IconGripVertical,
  IconPencil,
  IconPlayerPlay,
  IconLoader2,
  IconSearch,
} from "@tabler/icons-react";
import {
  fetchProviderModels,
  probeModelCapabilities,
  standaloneConfigWrite,
} from "../../api";
import {
  useAppStore,
  PROVIDER_PRESETS,
  createProviderFromPreset,
  DEFAULT_MODEL_CONTEXT_LENGTH,
  DEFAULT_MODEL_MAX_OUTPUT_TOKENS,
  VISION_FALLBACK_KIND_LOCAL_OCR,
  VISION_FALLBACK_KIND_MULTIMODAL,
} from "../../stores/appStore";
import type { ProviderConfig, ProviderPreset, ProviderModel, PoolModelEndpoint } from "../../types/provider";
import type { ConfigEdit } from "../../types";
import type { RemoteProviderModel } from "../../api";
import { SettingsPagination, usePagedItems } from "./SettingsPagination";
import { filterProviderModels } from "../../utils/chatModelSelection";

const LOCAL_OCR_FALLBACK_VALUE = "__local_ocr__";

function buildMultimodalFallbackValue(providerId: string, modelId: string): string {
  return `multimodal:${providerId}:${modelId}`;
}

function parseMultimodalFallbackValue(value: string): { providerId: string; modelId: string } | null {
  if (!value.startsWith("multimodal:")) {
    return null;
  }
  const payload = value.slice("multimodal:".length);
  const delimiter = payload.indexOf(":");
  if (delimiter <= 0 || delimiter >= payload.length - 1) {
    return null;
  }
  const providerId = payload.slice(0, delimiter).trim();
  const modelId = payload.slice(delimiter + 1).trim();
  if (!providerId || !modelId) {
    return null;
  }
  return { providerId, modelId };
}

function mergeFetchedModels(
  currentModels: ProviderModel[],
  fetchedModels: RemoteProviderModel[],
): { models: ProviderModel[]; addedCount: number } {
  const fetchedById = new Map(fetchedModels.map((model) => [model.id, model]));
  const nextModels = currentModels.map((model) => {
    const fetched = fetchedById.get(model.id);
    if (!fetched) {
      return model;
    }

    const shouldReplaceLabel = !model.label.trim() || model.label === model.id;
    const shouldReplaceContextLength = !model.contextLength || model.contextLength === DEFAULT_MODEL_CONTEXT_LENGTH;
    const shouldReplaceMaxOutputTokens =
      !model.maxOutputTokens || model.maxOutputTokens === DEFAULT_MODEL_MAX_OUTPUT_TOKENS;

    return {
      ...model,
      label: shouldReplaceLabel ? fetched.label || model.label : model.label,
      supportsVision: model.supportsVision || fetched.supportsVision,
      contextLength:
        shouldReplaceContextLength && fetched.contextLength
          ? fetched.contextLength
          : model.contextLength,
      maxOutputTokens:
        shouldReplaceMaxOutputTokens && fetched.maxOutputTokens
          ? fetched.maxOutputTokens
          : model.maxOutputTokens,
    };
  });

  const existingIds = new Set(currentModels.map((model) => model.id));
  let addedCount = 0;
  for (const fetched of fetchedModels) {
    if (existingIds.has(fetched.id)) {
      continue;
    }
    nextModels.push({
      id: fetched.id,
      label: fetched.label || fetched.id,
      supportsVision: fetched.supportsVision,
      contextLength: fetched.contextLength ?? DEFAULT_MODEL_CONTEXT_LENGTH,
      maxOutputTokens: fetched.maxOutputTokens ?? DEFAULT_MODEL_MAX_OUTPUT_TOKENS,
    });
    existingIds.add(fetched.id);
    addedCount += 1;
  }

  return { models: nextModels, addedCount };
}

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
  const [fetchingModels, setFetchingModels] = useState(false);
  const [feedback, setFeedback] = useState<{ kind: "success" | "error"; text: string } | null>(null);
  const [showPresetDialog, setShowPresetDialog] = useState(false);
  const [modelSearchQuery, setModelSearchQuery] = useState("");

  // 编辑表单
  const [editForm, setEditForm] = useState<{
    name: string;
    apiKey: string;
    baseUrl: string;
    wireApi: string;
    newModelId: string;
    newModelLabel: string;
    newModelContextLength: string;
    newModelMaxOutputTokens: string;
    newModelSupportsVision: boolean;
  }>({
    name: "",
    apiKey: "",
    baseUrl: "",
    wireApi: "",
    newModelId: "",
    newModelLabel: "",
    newModelContextLength: String(DEFAULT_MODEL_CONTEXT_LENGTH),
    newModelMaxOutputTokens: String(DEFAULT_MODEL_MAX_OUTPUT_TOKENS),
    newModelSupportsVision: false,
  });

  const selectedProvider = useMemo(
    () => providers.find((p) => p.id === selectedId) ?? null,
    [providers, selectedId],
  );

  const selectedPreset = useMemo(
    () => PROVIDER_PRESETS.find((p) => p.type === selectedProvider?.type),
    [selectedProvider?.type],
  );
  const {
    page: providersPage,
    setPage: setProvidersPage,
    pageSize: providersPageSize,
    totalItems: totalProviders,
    totalPages: totalProviderPages,
    pagedItems: pagedProviders,
  } = usePagedItems(providers);
  const filteredModels = useMemo(
    () => filterProviderModels(selectedProvider?.models ?? [], modelSearchQuery),
    [modelSearchQuery, selectedProvider?.models],
  );
  const {
    page: modelsPage,
    setPage: setModelsPage,
    pageSize: modelsPageSize,
    totalItems: totalModels,
    totalPages: totalModelPages,
    pagedItems: pagedModels,
  } = usePagedItems(filteredModels);

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
    setModelSearchQuery("");
  }, [selectedProvider?.id]);

  useEffect(() => {
    if (!selectedProvider) return;
    setEditForm((form) => ({
      ...form,
      name: selectedProvider.name,
      apiKey: "",
      baseUrl: selectedProvider.baseUrl,
      wireApi: selectedProvider.wireApi,
    }));
  }, [
    selectedProvider?.id,
    selectedProvider?.name,
    selectedProvider?.baseUrl,
    selectedProvider?.wireApi,
  ]);

  // 选中实例时加载配置到编辑表单
  const handleSelect = useCallback((id: string) => {
    setSelectedId(id);
    setFeedback(null);
    setModelSearchQuery("");
    const provider = useAppStore.getState().providers.find((p) => p.id === id);
    if (provider) {
      setEditForm({
        name: provider.name,
        apiKey: "",
        baseUrl: provider.baseUrl,
        wireApi: provider.wireApi,
        newModelId: "",
        newModelLabel: "",
        newModelContextLength: String(DEFAULT_MODEL_CONTEXT_LENGTH),
        newModelMaxOutputTokens: String(DEFAULT_MODEL_MAX_OUTPUT_TOKENS),
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
    const defaultName = intl.formatMessage({ id: preset.name, defaultMessage: preset.name });
    const instance = createProviderFromPreset(preset, { name: defaultName });
    useAppStore.getState().addCustomProvider(instance);
    setShowPresetDialog(false);
    handleSelect(instance.id);
  }, [handleSelect, intl]);

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
    if (selectedProvider.type === "local-pool") {
      const enabledEndpoints = selectedProvider.models.flatMap((model) =>
        (model.endpoints ?? []).filter((endpoint) => endpoint.enabled),
      );
      const hasEnabledEndpoint = enabledEndpoints.length > 0;
      const hasInvalidEnabledEndpoint = enabledEndpoints.some((endpoint) =>
        endpoint.url.trim().length === 0 || endpoint.model.trim().length === 0,
      );
      if (!hasEnabledEndpoint || hasInvalidEnabledEndpoint) {
        setFeedback({ kind: "error", text: intl.formatMessage({ id: "settings.pool.endpointRequired" }) });
        return;
      }
    }

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
      const store = useAppStore.getState();
      store.updateProvider(selectedProvider.id, updates);

      // 如果是激活的实例，同步写入 config.toml
      if (selectedProvider.id === store.activeProviderId) {
        const providerKey = selectedProvider.type || "custom";
        const providerOverride: Record<string, unknown> = {};
        if (trimmedBaseUrl) providerOverride.base_url = trimmedBaseUrl;
        if (trimmedWireApi) providerOverride.wire_api = trimmedWireApi;
        const persistedKey = trimmedApiKey || selectedProvider.apiKey;
        if (persistedKey) providerOverride.experimental_bearer_token = persistedKey;
        providerOverride.requires_openai_auth = selectedProvider.requiresOpenAIAuth;

        const activeModelEntry = store.getActiveModel();
        const preferredModelId = activeModelEntry?.provider === selectedProvider.id
          ? activeModelEntry.model
          : undefined;
        const targetModel = selectedProvider.models.find((model) => model.id === preferredModelId)
          ?? selectedProvider.models[0];
        const targetModelId = targetModel?.id ?? "";
        const targetModelContextLength = targetModel?.contextLength ?? DEFAULT_MODEL_CONTEXT_LENGTH;
        const targetModelMaxTokens = targetModel?.maxOutputTokens ?? DEFAULT_MODEL_MAX_OUTPUT_TOKENS;
        const modelSupportsVision = Boolean(targetModel?.supportsVision);
        const fallbackProviderId = targetModel?.visionFallbackProviderId?.trim() ?? "";
        const fallbackModelId = targetModel?.visionFallbackModelId?.trim() ?? "";
        let fallbackKindValue: string | null = null;
        let fallbackProviderKey: string | null = null;
        let fallbackModelValue: string | null = null;
        if (!modelSupportsVision) {
          if (targetModel?.visionFallbackKind === VISION_FALLBACK_KIND_LOCAL_OCR) {
            fallbackKindValue = VISION_FALLBACK_KIND_LOCAL_OCR;
          } else if (fallbackProviderId && fallbackModelId) {
            const fallbackProvider = store.providers.find((provider) =>
              provider.id === fallbackProviderId || provider.type === fallbackProviderId
            );
            const fallbackModel = fallbackProvider?.models.find((candidate) =>
              candidate.id === fallbackModelId && candidate.supportsVision
            );
            if (fallbackProvider && fallbackModel) {
              fallbackKindValue = VISION_FALLBACK_KIND_MULTIMODAL;
              fallbackProviderKey = fallbackProvider.type || fallbackProvider.id;
              fallbackModelValue = fallbackModel.id;
            }
          }
        }
        const modelEndpoints = selectedProvider.type === "local-pool" && targetModel?.endpoints?.length
          ? targetModel.endpoints
              .filter((endpoint) => endpoint.enabled)
              .map((endpoint) => ({
                url: endpoint.url.trim(),
                label: endpoint.label.trim() || undefined,
                model: endpoint.model.trim() || targetModel.id,
                api_key: endpoint.apiKey?.trim() || undefined,
                wire_api: endpoint.wireApi?.trim() || undefined,
              }))
              .filter((endpoint) => endpoint.url.length > 0 && endpoint.model.length > 0)
          : [];
        const edits: ConfigEdit[] = [
          { keyPath: "model_provider", value: providerKey, mergeStrategy: "replace" },
          { keyPath: "model", value: targetModelId, mergeStrategy: "replace" },
          { keyPath: `model_providers.${providerKey}`, value: providerOverride, mergeStrategy: "replace" },
          { keyPath: "model_context_window", value: targetModelContextLength, mergeStrategy: "replace" },
          { keyPath: "max_output_tokens", value: targetModelMaxTokens, mergeStrategy: "replace" },
          { keyPath: "model_supports_vision", value: modelSupportsVision, mergeStrategy: "replace" },
          { keyPath: "vision_fallback_kind", value: fallbackKindValue, mergeStrategy: "replace" },
          { keyPath: "vision_fallback_provider", value: fallbackProviderKey, mergeStrategy: "replace" },
          { keyPath: "vision_fallback_model", value: fallbackModelValue, mergeStrategy: "replace" },
          { keyPath: "model_endpoints", value: modelEndpoints, mergeStrategy: "replace" },
          {
            keyPath: "active_endpoint_index",
            value: modelEndpoints.length > 0 ? 0 : null,
            mergeStrategy: "replace",
          },
        ];
        await standaloneConfigWrite(edits);
        if (selectedProvider.type === "local-pool") {
          useAppStore.setState({ activeEndpointIndex: modelEndpoints.length > 0 ? 0 : null });
        }
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

  const handleFetchModels = useCallback(async () => {
    if (!selectedProvider || selectedProvider.type === "local-pool") return;

    const baseUrl = editForm.baseUrl.trim() || selectedProvider.baseUrl.trim();
    const apiKey = editForm.apiKey.trim() || selectedProvider.apiKey.trim();
    const wireApi = editForm.wireApi.trim() || selectedProvider.wireApi.trim() || "chat";

    if (!baseUrl) {
      setFeedback({ kind: "error", text: intl.formatMessage({ id: "settings.provider.baseUrlRequired" }) });
      return;
    }

    setFetchingModels(true);
    setFeedback(null);

    try {
      const result = await fetchProviderModels({ baseUrl, apiKey, wireApi });
      if (!result.supported) {
        setFeedback({
          kind: "error",
          text: intl.formatMessage({ id: "settings.provider.fetchModelsUnsupported" }),
        });
        return;
      }
      if (result.models.length === 0) {
        setFeedback({
          kind: "error",
          text: intl.formatMessage({ id: "settings.provider.fetchModelsEmpty" }),
        });
        return;
      }

      const store = useAppStore.getState();
      const currentProvider = store.providers.find((provider) => provider.id === selectedProvider.id) ?? selectedProvider;
      const merged = mergeFetchedModels(currentProvider.models, result.models);
      store.updateProvider(selectedProvider.id, { models: merged.models });

      setFeedback({
        kind: "success",
        text: intl.formatMessage(
          { id: "settings.provider.fetchModelsSuccess" },
          { count: result.models.length, added: merged.addedCount },
        ),
      });
    } catch (error) {
      console.error("Failed to fetch provider models:", error);
      const message = error instanceof Error ? error.message : String(error);
      setFeedback({
        kind: "error",
        text: intl.formatMessage(
          { id: "settings.provider.fetchModelsFailed" },
          { error: message },
        ),
      });
    } finally {
      setFetchingModels(false);
    }
  }, [editForm.apiKey, editForm.baseUrl, editForm.wireApi, intl, selectedProvider]);

  // 添加模型
  const handleAddModel = useCallback(() => {
    if (!selectedProvider || !editForm.newModelId.trim()) return;
    const parsedContextLength = parseInt(editForm.newModelContextLength, 10);
    const parsedMaxTokens = parseInt(editForm.newModelMaxOutputTokens, 10);
    const model: ProviderModel = {
      id: editForm.newModelId.trim(),
      label: editForm.newModelLabel.trim() || editForm.newModelId.trim(),
      supportsVision: editForm.newModelSupportsVision,
      contextLength: Number.isFinite(parsedContextLength) && parsedContextLength > 0
        ? parsedContextLength
        : DEFAULT_MODEL_CONTEXT_LENGTH,
      maxOutputTokens: Number.isFinite(parsedMaxTokens) && parsedMaxTokens > 0
        ? parsedMaxTokens
        : DEFAULT_MODEL_MAX_OUTPUT_TOKENS,
    };
    useAppStore.getState().addProviderModel(selectedProvider.id, model);
    setEditForm((f) => ({
      ...f,
      newModelId: "",
      newModelLabel: "",
      newModelContextLength: String(DEFAULT_MODEL_CONTEXT_LENGTH),
      newModelMaxOutputTokens: String(DEFAULT_MODEL_MAX_OUTPUT_TOKENS),
      newModelSupportsVision: false,
    }));
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
  const {
    page: presetCategoryPage,
    setPage: setPresetCategoryPage,
    pageSize: presetCategoryPageSize,
    totalItems: totalPresetCategories,
    totalPages: totalPresetCategoryPages,
    pagedItems: pagedPresetCategories,
  } = usePagedItems(presetsByCategory);

  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      <div className="flex min-h-0 flex-1 gap-4">
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
          {pagedProviders.map((provider) => {
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
          <SettingsPagination
            page={providersPage}
            onPageChange={setProvidersPage}
            pageSize={providersPageSize}
            totalItems={totalProviders}
            totalPages={totalProviderPages}
            className="flex items-center justify-between pt-2"
          />
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

            {/* API Key & Base URL（local-pool 不需要顶级配置） */}
            {selectedProvider.type !== "local-pool" && (
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
            )}
            {selectedProvider.type === "local-pool" && (
              <div className="rounded-[var(--radius-md)] border border-[var(--accent-border)] bg-[var(--accent-soft)]/30 px-4 py-3">
                <p className="text-xs text-[var(--text-muted)]">
                  {intl.formatMessage({ id: "settings.pool.localPoolDesc" })}
                </p>
              </div>
            )}

            {/* Wire API 格式选择 */}
            <div className="space-y-1.5">
              <label className="settings-field-label">
                {intl.formatMessage({ id: "settings.provider.transport" })}
              </label>
              <select
                value={editForm.wireApi}
                onChange={(e) => setEditForm((f) => ({ ...f, wireApi: e.target.value }))}
                className="app-select w-56"
              >
                <option value="chat">{intl.formatMessage({ id: "provider.transport.chat" })}</option>
                <option value="responses">{intl.formatMessage({ id: "provider.transport.responses" })}</option>
                <option value="anthropic">{intl.formatMessage({ id: "provider.transport.anthropic" })}</option>
                <option value="gemini">{intl.formatMessage({ id: "provider.transport.gemini" })}</option>
              </select>
              <p className="text-[11px] text-[var(--text-faint)]">
                {editForm.wireApi &&
                  intl.formatMessage({
                    id: `settings.provider.transportHint.${editForm.wireApi}`,
                  })}
              </p>
            </div>

            {/* 模型列表 */}
            <div className="space-y-2">
              <div className="flex items-center justify-between gap-2">
                <h4 className="text-xs font-medium text-[var(--text-strong)]">
                  {intl.formatMessage({ id: "settings.provider.models" })}
                </h4>
                {selectedProvider.type !== "local-pool" && (
                  <button
                    type="button"
                    onClick={handleFetchModels}
                    disabled={fetchingModels}
                    className="inline-flex h-7 w-7 items-center justify-center rounded-[var(--radius-sm)] border border-[var(--border-subtle)] text-[var(--text-faint)] transition-colors hover:border-[var(--accent-border)] hover:bg-[var(--accent-soft)] hover:text-[var(--accent-strong)] disabled:cursor-not-allowed disabled:opacity-50"
                    title={intl.formatMessage({ id: "settings.provider.fetchModels" })}
                  >
                    {fetchingModels ? (
                      <IconLoader2 size={12} stroke={2} className="animate-spin" />
                    ) : (
                      <IconRefresh size={12} stroke={2} />
                    )}
                  </button>
                )}
              </div>
              {(selectedProvider.models.length > 0 || modelSearchQuery.trim()) && (
                <div className="relative">
                  <IconSearch
                    size={12}
                    stroke={1.8}
                    className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-[var(--text-faint)]"
                  />
                  <input
                    type="text"
                    value={modelSearchQuery}
                    onChange={(event) => setModelSearchQuery(event.target.value)}
                    placeholder={intl.formatMessage({ id: "settings.provider.modelSearchPlaceholder" })}
                    autoComplete="off"
                    className="app-input w-full pl-8 text-xs"
                  />
                </div>
              )}
              <div className="space-y-1">
                {pagedModels.length > 0 ? (
                  pagedModels.map((model) => (
                    <ModelRow
                      key={model.id}
                      model={model}
                      providerId={selectedProvider.id}
                      isPoolProvider={selectedProvider.type === "local-pool"}
                      onRemove={() => handleRemoveModel(model.id)}
                    />
                  ))
                ) : (
                  <div className="rounded-[var(--radius-sm)] border border-dashed border-[var(--border-subtle)] px-3 py-4 text-center text-[11px] text-[var(--text-faint)]">
                    {selectedProvider.models.length > 0
                      ? intl.formatMessage({ id: "settings.provider.modelSearchEmpty" })
                      : intl.formatMessage({ id: "settings.provider.modelsEmpty" })}
                  </div>
                )}
              </div>
              <SettingsPagination
                page={modelsPage}
                onPageChange={setModelsPage}
                pageSize={modelsPageSize}
                totalItems={totalModels}
                totalPages={totalModelPages}
              />
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
                      title={intl.formatMessage({ id: "settings.provider.contextLengthHint" })}
                      onKeyDown={(e) => { if (e.key === "Enter") handleAddModel(); }}
                    />
                  </div>
                  <div className="flex items-center gap-1.5">
                    <label className="text-[11px] text-[var(--text-faint)]">
                      {intl.formatMessage({ id: "settings.provider.maxOutputTokens" })}
                    </label>
                    <input
                      type="number"
                      min={1}
                      value={editForm.newModelMaxOutputTokens}
                      onChange={(e) => setEditForm((f) => ({ ...f, newModelMaxOutputTokens: e.target.value }))}
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
            {pagedPresetCategories.map(([category, presets]) => (
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
                        <span className="block text-xs font-medium text-[var(--text-strong)]">
                          {intl.formatMessage({ id: preset.name, defaultMessage: preset.name })}
                        </span>
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
            <SettingsPagination
              page={presetCategoryPage}
              onPageChange={setPresetCategoryPage}
              pageSize={presetCategoryPageSize}
              totalItems={totalPresetCategories}
              totalPages={totalPresetCategoryPages}
            />

          </div>
        </div>
      )}
    </div>
  );
}

function ModelRow({
  model,
  providerId,
  isPoolProvider,
  onRemove,
}: {
  model: ProviderModel;
  providerId: string;
  isPoolProvider?: boolean;
  onRemove: () => void;
}) {
  const intl = useIntl();
  const [editingContextLength, setEditingContextLength] = useState(false);
  const [contextLengthValue, setContextLengthValue] = useState(
    String(model.contextLength ?? DEFAULT_MODEL_CONTEXT_LENGTH),
  );
  const [editingMaxTokens, setEditingMaxTokens] = useState(false);
  const [maxTokensValue, setMaxTokensValue] = useState(
    String(model.maxOutputTokens ?? DEFAULT_MODEL_MAX_OUTPUT_TOKENS),
  );
  const [showEndpoints, setShowEndpoints] = useState(false);
  const [newEndpointUrl, setNewEndpointUrl] = useState("");
  const [newEndpointLabel, setNewEndpointLabel] = useState("");
  const [newEndpointModel, setNewEndpointModel] = useState(model.id);
  const [newEndpointApiKey, setNewEndpointApiKey] = useState("");
  const [newEndpointWireApi, setNewEndpointWireApi] = useState("");
  const [editingEpId, setEditingEpId] = useState<string | null>(null);
  const [editEpForm, setEditEpForm] = useState({
    url: "",
    label: "",
    model: "",
    apiKey: "",
    wireApi: "",
  });
  const activeEndpointIndex = useAppStore((s) => s.activeEndpointIndex);
  const providers = useAppStore((s) => s.providers);

  const visionFallbackOptions = useMemo(() => {
    const options: Array<{ value: string; label: string }> = [
      {
        value: LOCAL_OCR_FALLBACK_VALUE,
        label: intl.formatMessage({ id: "settings.provider.visionFallbackLocalOcr" }),
      },
    ];
    providers.forEach((provider) => {
      provider.models.forEach((candidate) => {
        if (!candidate.supportsVision) {
          return;
        }
        options.push({
          value: buildMultimodalFallbackValue(provider.id, candidate.id),
          label: `${provider.name} / ${candidate.label}`,
        });
      });
    });
    return options;
  }, [providers, intl]);

  const selectedVisionFallbackValue = useMemo(() => {
    if (model.visionFallbackKind === VISION_FALLBACK_KIND_LOCAL_OCR) {
      return LOCAL_OCR_FALLBACK_VALUE;
    }
    const fallbackProviderId = model.visionFallbackProviderId?.trim();
    const fallbackModelId = model.visionFallbackModelId?.trim();
    if (!fallbackProviderId || !fallbackModelId) {
      return "";
    }
    const fallbackProvider = providers.find((provider) =>
      provider.id === fallbackProviderId || provider.type === fallbackProviderId
    );
    const fallbackModel = fallbackProvider?.models.find((candidate) =>
      candidate.id === fallbackModelId && candidate.supportsVision
    );
    if (!fallbackProvider || !fallbackModel) {
      return "";
    }
    return buildMultimodalFallbackValue(fallbackProvider.id, fallbackModel.id);
  }, [
    model.visionFallbackKind,
    model.visionFallbackProviderId,
    model.visionFallbackModelId,
    providers,
  ]);

  const handleToggleVision = useCallback(() => {
    useAppStore.getState().updateProviderModel(providerId, model.id, {
      supportsVision: !model.supportsVision,
    });
  }, [providerId, model.id, model.supportsVision]);

  const clearVisionFallback = useCallback(() => {
    useAppStore.getState().updateProviderModel(providerId, model.id, {
      visionFallbackKind: undefined,
      visionFallbackProviderId: undefined,
      visionFallbackModelId: undefined,
    });
  }, [providerId, model.id]);

  const handleVisionFallbackChange = useCallback((value: string) => {
    if (!value) {
      clearVisionFallback();
      return;
    }
    if (value === LOCAL_OCR_FALLBACK_VALUE) {
      useAppStore.getState().updateProviderModel(providerId, model.id, {
        visionFallbackKind: VISION_FALLBACK_KIND_LOCAL_OCR,
        visionFallbackProviderId: undefined,
        visionFallbackModelId: undefined,
      });
      return;
    }
    const parsed = parseMultimodalFallbackValue(value);
    if (!parsed) {
      clearVisionFallback();
      return;
    }
    const fallbackProvider = providers.find((provider) =>
      provider.id === parsed.providerId || provider.type === parsed.providerId
    );
    const fallbackModel = fallbackProvider?.models.find((candidate) =>
      candidate.id === parsed.modelId && candidate.supportsVision
    );
    if (!fallbackProvider || !fallbackModel) {
      clearVisionFallback();
      return;
    }
    useAppStore.getState().updateProviderModel(providerId, model.id, {
      visionFallbackKind: VISION_FALLBACK_KIND_MULTIMODAL,
      visionFallbackProviderId: fallbackProvider.id,
      visionFallbackModelId: fallbackModel.id,
    });
  }, [clearVisionFallback, providerId, model.id, providers]);

  const handleSaveMaxOutputTokens = useCallback(() => {
    const parsed = parseInt(maxTokensValue, 10);
    useAppStore.getState().updateProviderModel(providerId, model.id, {
      maxOutputTokens: Number.isFinite(parsed) && parsed > 0
        ? parsed
        : DEFAULT_MODEL_MAX_OUTPUT_TOKENS,
    });
    setEditingMaxTokens(false);
  }, [providerId, model.id, maxTokensValue]);

  const handleSaveContextLength = useCallback(() => {
    const parsed = parseInt(contextLengthValue, 10);
    useAppStore.getState().updateProviderModel(providerId, model.id, {
      contextLength: Number.isFinite(parsed) && parsed > 0
        ? parsed
        : DEFAULT_MODEL_CONTEXT_LENGTH,
    });
    setEditingContextLength(false);
  }, [providerId, model.id, contextLengthValue]);

  const endpoints = model.endpoints ?? [];
  const {
    page: endpointsPage,
    setPage: setEndpointsPage,
    pageSize: endpointsPageSize,
    totalItems: totalEndpoints,
    totalPages: totalEndpointPages,
    pagedItems: pagedEndpoints,
  } = usePagedItems(endpoints, 6);

  const handleAddEndpoint = useCallback(() => {
    const url = newEndpointUrl.trim();
    const modelName = newEndpointModel.trim();
    if (!url || !modelName) return;
    const ep: PoolModelEndpoint = {
      id: crypto.randomUUID(),
      url,
      model: modelName,
      label: newEndpointLabel.trim(),
      enabled: true,
      ...(newEndpointApiKey.trim() ? { apiKey: newEndpointApiKey.trim() } : {}),
      ...(newEndpointWireApi ? { wireApi: newEndpointWireApi } : {}),
    };
    const updated = [...endpoints, ep];
    useAppStore.getState().updateProviderModel(providerId, model.id, { endpoints: updated });
    setNewEndpointUrl("");
    setNewEndpointLabel("");
    setNewEndpointModel(model.id);
    setNewEndpointApiKey("");
    setNewEndpointWireApi("");
  }, [
    providerId,
    model.id,
    endpoints,
    newEndpointUrl,
    newEndpointLabel,
    newEndpointModel,
    newEndpointApiKey,
    newEndpointWireApi,
  ]);

  const handleRemoveEndpoint = useCallback((epId: string) => {
    const updated = endpoints.filter((ep) => ep.id !== epId);
    useAppStore.getState().updateProviderModel(providerId, model.id, { endpoints: updated });
  }, [providerId, model.id, endpoints]);

  const handleToggleEndpoint = useCallback((epId: string) => {
    const updated = endpoints.map((ep) =>
      ep.id === epId ? { ...ep, enabled: !ep.enabled } : ep,
    );
    useAppStore.getState().updateProviderModel(providerId, model.id, { endpoints: updated });
  }, [providerId, model.id, endpoints]);

  const handleMoveEndpoint = useCallback((epId: string, direction: -1 | 1) => {
    const idx = endpoints.findIndex((ep) => ep.id === epId);
    if (idx < 0) return;
    const targetIdx = idx + direction;
    if (targetIdx < 0 || targetIdx >= endpoints.length) return;
    const updated = [...endpoints];
    [updated[idx], updated[targetIdx]] = [updated[targetIdx], updated[idx]];
    useAppStore.getState().updateProviderModel(providerId, model.id, { endpoints: updated });
  }, [providerId, model.id, endpoints]);

  const handleStartEditEndpoint = useCallback((ep: PoolModelEndpoint) => {
    setEditingEpId(ep.id);
    setEditEpForm({
      url: ep.url,
      label: ep.label,
      model: ep.model,
      apiKey: ep.apiKey ?? "",
      wireApi: ep.wireApi ?? "",
    });
  }, []);

  const handleSaveEditEndpoint = useCallback(() => {
    if (!editingEpId) return;
    const updated = endpoints.map((ep) =>
      ep.id === editingEpId
        ? {
          ...ep,
          url: editEpForm.url.trim(),
          label: editEpForm.label.trim(),
          model: editEpForm.model.trim(),
          apiKey: editEpForm.apiKey.trim() || undefined,
          wireApi: editEpForm.wireApi.trim() || undefined,
        }
        : ep,
    );
    useAppStore.getState().updateProviderModel(providerId, model.id, { endpoints: updated });
    setEditingEpId(null);
  }, [providerId, model.id, endpoints, editingEpId, editEpForm]);

  const handleCancelEditEndpoint = useCallback(() => {
    setEditingEpId(null);
  }, []);

  // 模型连接测试
  const [testState, setTestState] = useState<{
    status: "idle" | "testing" | "done";
    result?: {
      success: boolean;
      statusCode?: number;
      latencyMs?: number;
      outputTokens?: number;
      tokensPerSec?: number;
      error?: string;
    };
  }>({ status: "idle" });
  const [probeState, setProbeState] = useState<"idle" | "probing">("idle");

  const handleTestModel = useCallback(async () => {
    const provider = providers.find((p) => p.id === providerId);
    if (!provider) return;

    // local-pool 模式下，取第一个启用的端点作为测试目标
    const ep = isPoolProvider
      ? model.endpoints?.find((e) => e.enabled)
      : null;
    const baseUrl = ep?.url || provider.baseUrl;
    const apiKey = ep?.apiKey || provider.apiKey;
    const wireApi = ep?.wireApi || provider.wireApi || "chat";
    const modelName = ep?.model || model.id;

    setTestState({ status: "testing" });
    try {
      const result = await invoke<{
        success: boolean;
        statusCode?: number;
        latencyMs?: number;
        outputTokens?: number;
        tokensPerSec?: number;
        error?: string;
      }>("test_model_connection", {
        baseUrl,
        apiKey,
        model: modelName,
        wireApi,
      });
      setTestState({ status: "done", result });
    } catch (err) {
      setTestState({
        status: "done",
        result: {
          success: false,
          error: err instanceof Error ? err.message : String(err),
        },
      });
    }
  }, [providers, providerId, model, isPoolProvider]);

  const handleProbeCapabilities = useCallback(async () => {
    const provider = providers.find((p) => p.id === providerId);
    if (!provider) return;
    const ep = isPoolProvider ? model.endpoints?.find((e) => e.enabled) : null;
    const baseUrl = ep?.url || provider.baseUrl;
    const apiKey = ep?.apiKey || provider.apiKey;
    const wireApi = ep?.wireApi || provider.wireApi || "chat";
    const modelName = ep?.model || model.id;
    setProbeState("probing");
    try {
      const result = await probeModelCapabilities({
        baseUrl,
        apiKey,
        model: modelName,
        wireApi,
        providerKey: provider.id,
        forceRefresh: true,
      });
      if (result.success) {
        useAppStore.getState().updateProviderModel(providerId, model.id, {
          capabilities: {
            ...result.capabilities,
            probedAt: result.probedAt,
            fingerprint: result.fingerprint,
            wireApi,
          },
        });
      }
    } catch (error) {
      console.error("Failed to probe model capabilities:", error);
    } finally {
      setProbeState("idle");
    }
  }, [providers, providerId, model, isPoolProvider]);

  return (
    <div className="rounded-[var(--radius-md)] border border-[var(--border-subtle)] bg-[var(--surface-contrast)]">
      <div className="flex items-center justify-between gap-2 px-3 py-1.5">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5">
            {isPoolProvider && (
              <button
                type="button"
                onClick={() => setShowEndpoints(!showEndpoints)}
                className="shrink-0 text-[var(--text-faint)] transition-colors hover:text-[var(--text-muted)]"
              >
                {showEndpoints ? <IconChevronDown size={12} stroke={2} /> : <IconChevronRight size={12} stroke={2} />}
              </button>
            )}
            <span className="text-xs font-medium text-[var(--text-strong)]">{model.label}</span>
            <span className="break-all font-mono text-[11px] text-[var(--text-faint)]">{model.id}</span>
            {isPoolProvider && (
              <span className="text-[10px] text-[var(--text-faint)]">
                ({endpoints.filter((ep) => ep.enabled).length}/{endpoints.length} {intl.formatMessage({ id: "settings.pool.endpoints" })})
              </span>
            )}
          </div>
          <div className="mt-0.5 flex items-center gap-2">
            {editingContextLength ? (
              <div className="flex items-center gap-1">
                <input
                  type="number"
                  min={1}
                  value={contextLengthValue}
                  onChange={(e) => setContextLengthValue(e.target.value)}
                  onBlur={handleSaveContextLength}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") handleSaveContextLength();
                    if (e.key === "Escape") setEditingContextLength(false);
                  }}
                  className="app-input w-24 text-[11px]"
                  autoFocus
                />
                <span className="text-[10px] text-[var(--text-faint)]">ctx</span>
              </div>
            ) : (
              <button
                type="button"
                onClick={() => {
                  setContextLengthValue(String(model.contextLength ?? DEFAULT_MODEL_CONTEXT_LENGTH));
                  setEditingContextLength(true);
                  setEditingMaxTokens(false);
                }}
                className="text-[11px] text-[var(--text-faint)] transition-colors hover:text-[var(--accent)]"
                title={intl.formatMessage({ id: "settings.provider.contextLengthHint" })}
              >
                {(model.contextLength ?? DEFAULT_MODEL_CONTEXT_LENGTH).toLocaleString()} ctx
              </button>
            )}
            {editingMaxTokens ? (
              <div className="flex items-center gap-1">
                <input
                  type="number"
                  min={1}
                  value={maxTokensValue}
                  onChange={(e) => setMaxTokensValue(e.target.value)}
                  onBlur={handleSaveMaxOutputTokens}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") handleSaveMaxOutputTokens();
                    if (e.key === "Escape") setEditingMaxTokens(false);
                  }}
                  className="app-input w-24 text-[11px]"
                  autoFocus
                />
                <span className="text-[10px] text-[var(--text-faint)]">max</span>
              </div>
            ) : (
              <button
                type="button"
                onClick={() => {
                  setMaxTokensValue(String(model.maxOutputTokens ?? DEFAULT_MODEL_MAX_OUTPUT_TOKENS));
                  setEditingMaxTokens(true);
                  setEditingContextLength(false);
                }}
                className="text-[11px] text-[var(--text-faint)] transition-colors hover:text-[var(--accent)]"
                title={intl.formatMessage({ id: "settings.provider.maxOutputTokensHint" })}
              >
                {(model.maxOutputTokens ?? DEFAULT_MODEL_MAX_OUTPUT_TOKENS).toLocaleString()} max
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
          {!model.supportsVision && (
            <div className="mt-1.5 flex flex-wrap items-center gap-1.5">
              <span className="text-[10px] text-[var(--text-faint)]">
                {intl.formatMessage({ id: "settings.provider.visionFallback" })}
              </span>
              <select
                value={selectedVisionFallbackValue}
                onChange={(event) => handleVisionFallbackChange(event.target.value)}
                className="app-select min-w-56 px-2 py-1 text-[12px] text-[var(--text-strong)]"
              >
                <option value="">
                  {intl.formatMessage({ id: "settings.provider.visionFallbackNone" })}
                </option>
                {visionFallbackOptions.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </div>
          )}
        </div>
        {/* 测试按钮 */}
        <button
          type="button"
          onClick={handleTestModel}
          disabled={testState.status === "testing"}
          className="shrink-0 rounded-[var(--radius-sm)] p-1 text-[var(--text-faint)] transition-colors hover:bg-[var(--accent-soft)] hover:text-[var(--accent-strong)] disabled:opacity-40"
          title={intl.formatMessage({ id: "settings.provider.testModel" })}
        >
          {testState.status === "testing" ? (
            <IconLoader2 size={12} stroke={2} className="animate-spin" />
          ) : (
            <IconPlayerPlay size={12} stroke={2} />
          )}
        </button>
        <button
          type="button"
          onClick={handleProbeCapabilities}
          disabled={probeState === "probing"}
          className="shrink-0 rounded-[var(--radius-sm)] p-1 text-[var(--text-faint)] transition-colors hover:bg-[var(--accent-soft)] hover:text-[var(--accent-strong)] disabled:opacity-40"
          title={intl.formatMessage({ id: "settings.provider.probeCapabilities", defaultMessage: "Probe model capabilities" })}
        >
          {probeState === "probing" ? (
            <IconLoader2 size={12} stroke={2} className="animate-spin" />
          ) : (
            <IconRefresh size={12} stroke={2} />
          )}
        </button>
        <button
          type="button"
          onClick={onRemove}
          className="shrink-0 rounded-[var(--radius-sm)] p-1 text-[var(--text-faint)] transition-colors hover:bg-[var(--danger-soft)] hover:text-[var(--danger)]"
        >
          <IconX size={12} stroke={2} />
        </button>
      </div>

      {/* 测试结果 */}
      {model.capabilities && (
        <div className="mx-3 mb-2 flex flex-wrap items-center gap-1 text-[10px] text-[var(--text-faint)]">
          {([
            ["tools", model.capabilities.structuredTools],
            ["stream", model.capabilities.streaming],
            ["reasoning", model.capabilities.reasoning],
            ["usage", model.capabilities.usage],
            ["parallel", model.capabilities.parallelToolCalls],
          ] as const).map(([label, supported]) => (
            <span
              key={label}
              className={`rounded px-1.5 py-0.5 ${supported === true
                ? "bg-green-500/10 text-green-600 dark:text-green-400"
                : supported === false
                  ? "bg-red-500/10 text-red-600 dark:text-red-400"
                  : "bg-[var(--surface-muted)] text-[var(--text-faint)]"}`}
            >
              {label} {supported === true ? "ok" : supported === false ? "no" : "?"}
            </span>
          ))}
          {model.capabilities.recommendedWireApi && model.capabilities.recommendedWireApi !== model.capabilities.wireApi && (
            <span className="ml-1 text-[var(--accent-strong)]">
              {intl.formatMessage({ id: "settings.provider.recommendedWireApi", defaultMessage: "recommended" })}: {model.capabilities.recommendedWireApi}
            </span>
          )}
          <span className="ml-1">{new Date(model.capabilities.probedAt).toLocaleString()}</span>
        </div>
      )}

      {testState.status === "done" && testState.result && (
        <div className={`mx-3 mb-2 flex items-center gap-2 rounded-[var(--radius-sm)] px-2 py-1 text-[11px] ${
          testState.result.success
            ? "bg-green-500/10 text-green-600 dark:text-green-400"
            : "bg-red-500/10 text-red-600 dark:text-red-400"
        }`}>
          {testState.result.success ? (
            <>
              <IconCheck size={12} stroke={2} />
              <span>
                {intl.formatMessage({ id: "settings.provider.testSuccess" })}
                {" · "}
                {testState.result.latencyMs}ms
                {testState.result.tokensPerSec ? ` · ${testState.result.tokensPerSec} t/s` : ""}
                {testState.result.outputTokens ? ` (${testState.result.outputTokens} tokens)` : ""}
              </span>
            </>
          ) : (
            <>
              <IconX size={12} stroke={2} />
              <span className="min-w-0 truncate">
                {intl.formatMessage({ id: "settings.provider.testFailed" })}
                {testState.result.statusCode ? ` [${testState.result.statusCode}]` : ""}
                {testState.result.error ? ` — ${testState.result.error.slice(0, 120)}` : ""}
              </span>
            </>
          )}
          <button
            type="button"
            onClick={() => setTestState({ status: "idle" })}
            className="ml-auto shrink-0 opacity-60 hover:opacity-100"
          >
            <IconX size={10} stroke={2} />
          </button>
        </div>
      )}

      {/* 端点列表（仅 local-pool） */}
      {isPoolProvider && showEndpoints && (
        <div className="border-t border-[var(--border-subtle)] px-3 py-2">
          <div className="space-y-1">
            {endpoints.length === 0 && (
              <p className="text-[11px] text-[var(--text-faint)]">
                {intl.formatMessage({ id: "settings.pool.noEndpoints" })}
              </p>
            )}
            {pagedEndpoints.map((ep, idx) => {
              const globalIdx = endpointsPage * endpointsPageSize + idx;
              const isActive = activeEndpointIndex === globalIdx;
              const isEditing = editingEpId === ep.id;
              return (
                <div
                  key={ep.id}
                  className={`rounded-[var(--radius-sm)] border text-[11px] ${
                    isActive
                      ? "border-green-500/50 bg-[var(--surface-panel)]"
                      : ep.enabled
                        ? "border-[var(--border-subtle)] bg-[var(--surface-panel)]"
                        : "border-[var(--border-subtle)] bg-[var(--surface-contrast)]/50 opacity-60"
                  }`}
                >
                  <div className="flex items-center gap-1.5 px-2 py-1">
                    {/* 绿点指示器 */}
                    <span
                      className={`h-2 w-2 shrink-0 rounded-full transition-colors ${
                        isActive ? "bg-green-500 shadow-[0_0_4px_rgba(34,197,94,0.6)]" : "bg-transparent"
                      }`}
                      title={isActive ? intl.formatMessage({ id: "settings.pool.activeEndpoint" }) : ""}
                    />
                    <IconGripVertical size={10} stroke={1.5} className="shrink-0 cursor-grab text-[var(--text-faint)]" />
                    <span className="shrink-0 w-4 text-center font-mono text-[10px] text-[var(--text-faint)]">{globalIdx + 1}</span>
                    <span className="min-w-0 flex-1 truncate font-mono text-[var(--text-base)]" title={ep.url}>{ep.url}</span>
                    <span
                      className="shrink-0 rounded bg-[var(--surface-contrast)] px-1 py-0.5 font-mono text-[10px] text-[var(--text-faint)]"
                      title={ep.model}
                    >
                      {ep.model}
                    </span>
                    {ep.label && (
                      <span className="shrink-0 text-[var(--text-faint)]">{ep.label}</span>
                    )}
                    {ep.wireApi && (
                      <span className="shrink-0 rounded bg-[var(--surface-contrast)] px-1 py-0.5 font-mono text-[10px] text-[var(--text-faint)]">
                        {ep.wireApi}
                      </span>
                    )}
                    {ep.apiKey && (
                      <span className="shrink-0 text-[10px] text-[var(--accent-strong)]">Key</span>
                    )}
                    <button
                      type="button"
                      onClick={() => handleToggleEndpoint(ep.id)}
                      className={`shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium transition-colors ${
                        ep.enabled
                          ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                          : "bg-[var(--surface-contrast)] text-[var(--text-faint)]"
                      }`}
                    >
                      {ep.enabled
                        ? intl.formatMessage({ id: "settings.pool.endpointEnabled" })
                        : intl.formatMessage({ id: "settings.pool.endpointDisabled" })}
                    </button>
                    <button
                      type="button"
                      onClick={() => handleStartEditEndpoint(ep)}
                      className="shrink-0 rounded p-0.5 text-[var(--text-faint)] transition-colors hover:text-[var(--accent)]"
                      title={intl.formatMessage({ id: "settings.pool.editEndpoint" })}
                    >
                      <IconPencil size={10} stroke={2} />
                    </button>
                    <div className="flex shrink-0 gap-0.5">
                      <button
                        type="button"
                        onClick={() => handleMoveEndpoint(ep.id, -1)}
                        disabled={globalIdx === 0}
                        className="rounded p-0.5 text-[var(--text-faint)] transition-colors hover:text-[var(--text-muted)] disabled:opacity-30"
                        title="Move up"
                      >
                        <svg width="10" height="10" viewBox="0 0 10 10" fill="currentColor"><path d="M5 2L1 7h8z" /></svg>
                      </button>
                      <button
                        type="button"
                        onClick={() => handleMoveEndpoint(ep.id, 1)}
                        disabled={globalIdx === endpoints.length - 1}
                        className="rounded p-0.5 text-[var(--text-faint)] transition-colors hover:text-[var(--text-muted)] disabled:opacity-30"
                        title="Move down"
                      >
                        <svg width="10" height="10" viewBox="0 0 10 10" fill="currentColor"><path d="M5 8L1 3h8z" /></svg>
                      </button>
                    </div>
                    <button
                      type="button"
                      onClick={() => handleRemoveEndpoint(ep.id)}
                      className="shrink-0 rounded p-0.5 text-[var(--text-faint)] transition-colors hover:text-[var(--danger)]"
                    >
                      <IconX size={10} stroke={2} />
                    </button>
                  </div>
                  {/* 编辑模式 */}
                  {isEditing && (
                    <div className="border-t border-[var(--border-subtle)] px-2 py-2">
                      <div className="grid grid-cols-[1fr_auto] gap-x-1.5 gap-y-1.5">
                        <div className="space-y-0.5">
                          <label className="text-[10px] text-[var(--text-faint)]">
                            {intl.formatMessage({ id: "settings.pool.endpointUrl" })}
                          </label>
                          <input
                            value={editEpForm.url}
                            onChange={(e) => setEditEpForm((f) => ({ ...f, url: e.target.value }))}
                            className="app-input w-full text-[11px]"
                          />
                        </div>
                        <div className="space-y-0.5">
                          <label className="text-[10px] text-[var(--text-faint)]">
                            {intl.formatMessage({ id: "settings.pool.endpointLabel" })}
                          </label>
                          <input
                            value={editEpForm.label}
                            onChange={(e) => setEditEpForm((f) => ({ ...f, label: e.target.value }))}
                            className="app-input w-24 text-[11px]"
                          />
                        </div>
                        <div className="space-y-0.5">
                          <label className="text-[10px] text-[var(--text-faint)]">
                            {intl.formatMessage({ id: "settings.pool.endpointModel" })}
                          </label>
                          <input
                            value={editEpForm.model}
                            onChange={(e) => setEditEpForm((f) => ({ ...f, model: e.target.value }))}
                            className="app-input w-full text-[11px]"
                          />
                        </div>
                        <div className="space-y-0.5">
                          <label className="text-[10px] text-[var(--text-faint)]">
                            {intl.formatMessage({ id: "settings.pool.endpointApiKey" })}
                          </label>
                          <input
                            type="password"
                            autoComplete="off"
                            value={editEpForm.apiKey}
                            onChange={(e) => setEditEpForm((f) => ({ ...f, apiKey: e.target.value }))}
                            className="app-input w-full text-[11px]"
                          />
                        </div>
                        <div className="space-y-0.5">
                          <label className="text-[10px] text-[var(--text-faint)]">Wire API</label>
                          <select
                            value={editEpForm.wireApi}
                            onChange={(e) => setEditEpForm((f) => ({ ...f, wireApi: e.target.value }))}
                            className="app-input w-24 text-[11px]"
                          >
                            <option value="">{intl.formatMessage({ id: "settings.pool.wireApiDefault" })}</option>
                            <option value="chat">Chat</option>
                            <option value="responses">Responses</option>
                            <option value="anthropic">Anthropic</option>
                            <option value="gemini">Gemini</option>
                          </select>
                        </div>
                      </div>
                      <div className="mt-2 flex justify-end gap-1.5">
                        <button
                          type="button"
                          onClick={handleCancelEditEndpoint}
                          className="rounded-[var(--radius-sm)] px-2 py-0.5 text-[11px] text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-contrast)]"
                        >
                          {intl.formatMessage({ id: "settings.pool.cancelEdit" })}
                        </button>
                        <button
                          type="button"
                          onClick={handleSaveEditEndpoint}
                          disabled={!editEpForm.url.trim() || !editEpForm.model.trim()}
                          className="rounded-[var(--radius-sm)] bg-[var(--accent-soft)] px-2 py-0.5 text-[11px] font-medium text-[var(--accent-strong)] transition-colors hover:bg-[var(--accent)] hover:text-white disabled:opacity-40"
                        >
                          {intl.formatMessage({ id: "settings.pool.saveEndpoint" })}
                        </button>
                      </div>
                    </div>
                  )}
                </div>
              );
            })}
            <SettingsPagination
              page={endpointsPage}
              onPageChange={setEndpointsPage}
              pageSize={endpointsPageSize}
              totalItems={totalEndpoints}
              totalPages={totalEndpointPages}
            />
          </div>
          <div className="mt-3 rounded-[var(--radius-sm)] border border-dashed border-[var(--border-subtle)] p-2.5">
            <p className="mb-2 text-[11px] font-medium text-[var(--text-muted)]">
              {intl.formatMessage({ id: "settings.pool.addEndpoint" })}
            </p>
            <div className="grid grid-cols-[1fr_auto] gap-x-1.5 gap-y-1.5">
              <div className="space-y-0.5">
                <label className="text-[10px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "settings.pool.endpointUrl" })}
                </label>
                <input
                  value={newEndpointUrl}
                  onChange={(e) => setNewEndpointUrl(e.target.value)}
                  placeholder={intl.formatMessage({ id: "settings.pool.endpointUrlPlaceholder" })}
                  className="app-input w-full text-[11px]"
                  onKeyDown={(e) => { if (e.key === "Enter") handleAddEndpoint(); }}
                />
              </div>
              <div className="space-y-0.5">
                <label className="text-[10px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "settings.pool.endpointLabel" })}
                </label>
                <input
                  value={newEndpointLabel}
                  onChange={(e) => setNewEndpointLabel(e.target.value)}
                  placeholder={intl.formatMessage({ id: "settings.pool.endpointLabelPlaceholder" })}
                  className="app-input w-24 text-[11px]"
                  onKeyDown={(e) => { if (e.key === "Enter") handleAddEndpoint(); }}
                />
              </div>
              <div className="space-y-0.5">
                <label className="text-[10px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "settings.pool.endpointModel" })}
                </label>
                <input
                  value={newEndpointModel}
                  onChange={(e) => setNewEndpointModel(e.target.value)}
                  placeholder={intl.formatMessage({ id: "settings.pool.endpointModelPlaceholder" })}
                  className="app-input w-full text-[11px]"
                  onKeyDown={(e) => { if (e.key === "Enter") handleAddEndpoint(); }}
                />
              </div>
              <div className="space-y-0.5">
                <label className="text-[10px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "settings.pool.endpointApiKey" })}
                </label>
                <input
                  type="password"
                  autoComplete="off"
                  value={newEndpointApiKey}
                  onChange={(e) => setNewEndpointApiKey(e.target.value)}
                  placeholder={intl.formatMessage({ id: "settings.pool.endpointApiKeyPlaceholder" })}
                  className="app-input w-full text-[11px]"
                  onKeyDown={(e) => { if (e.key === "Enter") handleAddEndpoint(); }}
                />
              </div>
              <div className="space-y-0.5">
                <label className="text-[10px] text-[var(--text-faint)]">
                  Wire API
                </label>
                <select
                  value={newEndpointWireApi}
                  onChange={(e) => setNewEndpointWireApi(e.target.value)}
                  className="app-input w-24 text-[11px]"
                >
                  <option value="">{intl.formatMessage({ id: "settings.pool.wireApiDefault" })}</option>
                  <option value="chat">Chat</option>
                  <option value="responses">Responses</option>
                  <option value="anthropic">Anthropic</option>
                  <option value="gemini">Gemini</option>
                </select>
              </div>
            </div>
            <div className="mt-2 flex justify-end">
              <button
                type="button"
                onClick={handleAddEndpoint}
                disabled={!newEndpointUrl.trim() || !newEndpointModel.trim()}
                className="flex items-center gap-1 rounded-[var(--radius-sm)] bg-[var(--accent-soft)] px-2.5 py-1 text-[11px] font-medium text-[var(--accent-strong)] transition-colors hover:bg-[var(--accent)] hover:text-white disabled:opacity-40"
              >
                <IconPlus size={11} stroke={2} />
                {intl.formatMessage({ id: "settings.pool.addEndpoint" })}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
