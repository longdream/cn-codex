import type { ProviderConfig, ProviderModel } from "../types/provider";

export interface ResolveProviderModelOptions {
  overrideModelId?: string | null;
  currentModel?: string | null;
  legacyModelId?: string | null;
}

/**
 * Resolve a model only within the selected provider.
 * Legacy global model state is intentionally the last compatible fallback.
 */
export function resolveProviderModelId(
  provider: ProviderConfig | null | undefined,
  options: ResolveProviderModelOptions,
): string | null {
  if (!provider || provider.models.length === 0) {
    return null;
  }

  const candidates = [
    options.overrideModelId,
    options.currentModel,
    options.legacyModelId,
  ];
  for (const candidate of candidates) {
    const modelId = candidate?.trim();
    if (modelId && provider.models.some((model) => model.id === modelId)) {
      return modelId;
    }
  }

  return provider.models[0]?.id ?? null;
}

export function filterProviderModels(
  models: ProviderModel[],
  query: string,
): ProviderModel[] {
  const normalizedQuery = query.trim().toLocaleLowerCase();
  if (!normalizedQuery) {
    return models;
  }

  return models.filter((model) =>
    model.id.toLocaleLowerCase().includes(normalizedQuery)
    || model.label.toLocaleLowerCase().includes(normalizedQuery),
  );
}

export function resolveModelPickerProviderId(
  providerIds: string[],
  effectiveProviderId: string | null | undefined,
  activeProviderId: string | null | undefined,
): string | null {
  const availableProviderIds = new Set(providerIds);
  if (effectiveProviderId && availableProviderIds.has(effectiveProviderId)) {
    return effectiveProviderId;
  }
  if (activeProviderId && availableProviderIds.has(activeProviderId)) {
    return activeProviderId;
  }
  return providerIds[0] ?? null;
}
