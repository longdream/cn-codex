import type { ProviderConfig } from "../types/provider";
import type { StandaloneChatProviderOverride } from "../api/standalone";

export const SKILL_LAB_PREFERENCES_STORAGE_KEY = "codey.skillLab.runtimePreferences.v1";

export interface SkillLabStagePreference {
  providerId: string;
  modelId: string;
  smartbrainEnabled: boolean;
}

export interface SkillLabPreferences {
  generation: SkillLabStagePreference;
  evolution: SkillLabStagePreference;
}

export interface SkillLabRuntimeConfig {
  provider: StandaloneChatProviderOverride;
  smartbrainEnabled: boolean;
}

export function buildSkillLabRuntimeConfig(
  preference: SkillLabStagePreference,
  buildProviderSnapshot: (
    providerId: string,
    modelId: string,
  ) => StandaloneChatProviderOverride | null,
): SkillLabRuntimeConfig | null {
  const providerId = preference.providerId.trim();
  const modelId = preference.modelId.trim();
  if (!providerId || !modelId) {
    return null;
  }
  const provider = buildProviderSnapshot(providerId, modelId);
  if (!provider) {
    return null;
  }
  return {
    provider: { ...provider },
    smartbrainEnabled: preference.smartbrainEnabled === true,
  };
}

export function selectProviderDefaultModel(
  providers: ProviderConfig[],
  providerId: string,
): { providerId: string; modelId: string } {
  const provider = providers.find((item) => item.id === providerId);
  return {
    providerId,
    modelId: provider?.models[0]?.id ?? "",
  };
}

function findValidSelection(
  providers: ProviderConfig[],
  providerId: string | null | undefined,
  modelId: string | null | undefined,
): { providerId: string; modelId: string } | null {
  const provider = providers.find((item) => item.id === providerId);
  if (!provider) {
    return null;
  }
  const model = provider.models.find((item) => item.id === modelId);
  return model ? { providerId: provider.id, modelId: model.id } : null;
}

function fallbackSelection(
  providers: ProviderConfig[],
  activeProviderId: string | null | undefined,
  activeModelId: string | null | undefined,
): { providerId: string; modelId: string } {
  const active = findValidSelection(providers, activeProviderId, activeModelId);
  if (active) {
    return active;
  }

  const activeProvider = providers.find((item) => item.id === activeProviderId);
  if (activeProvider?.models[0]) {
    return {
      providerId: activeProvider.id,
      modelId: activeProvider.models[0].id,
    };
  }

  const firstProvider = providers.find((item) => item.models.length > 0);
  return {
    providerId: firstProvider?.id ?? "",
    modelId: firstProvider?.models[0]?.id ?? "",
  };
}

export function resolveSkillLabStagePreference(
  preference: Partial<SkillLabStagePreference> | null | undefined,
  providers: ProviderConfig[],
  activeProviderId: string | null | undefined,
  activeModelId: string | null | undefined,
): SkillLabStagePreference {
  const saved = findValidSelection(
    providers,
    preference?.providerId,
    preference?.modelId,
  );
  const selection = saved ?? fallbackSelection(providers, activeProviderId, activeModelId);
  return {
    ...selection,
    smartbrainEnabled: preference?.smartbrainEnabled === true,
  };
}

function parseStoredPreferences(): Partial<SkillLabPreferences> | null {
  try {
    const raw = window.localStorage.getItem(SKILL_LAB_PREFERENCES_STORAGE_KEY);
    if (!raw) {
      return null;
    }
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === "object"
      ? parsed as Partial<SkillLabPreferences>
      : null;
  } catch {
    return null;
  }
}

export function loadSkillLabPreferences(
  providers: ProviderConfig[],
  activeProviderId: string | null | undefined,
  activeModelId: string | null | undefined,
): SkillLabPreferences {
  const stored = parseStoredPreferences();
  return {
    generation: resolveSkillLabStagePreference(
      stored?.generation,
      providers,
      activeProviderId,
      activeModelId,
    ),
    evolution: resolveSkillLabStagePreference(
      stored?.evolution,
      providers,
      activeProviderId,
      activeModelId,
    ),
  };
}

function sanitizePreference(
  preference: SkillLabStagePreference,
): SkillLabStagePreference {
  return {
    providerId: preference.providerId,
    modelId: preference.modelId,
    smartbrainEnabled: preference.smartbrainEnabled === true,
  };
}

export function saveSkillLabPreferences(preferences: SkillLabPreferences): void {
  try {
    window.localStorage.setItem(
      SKILL_LAB_PREFERENCES_STORAGE_KEY,
      JSON.stringify({
        generation: sanitizePreference(preferences.generation),
        evolution: sanitizePreference(preferences.evolution),
      }),
    );
  } catch {
    // 偏好保存失败不应阻止 Skill 实验室运行。
  }
}
