import { beforeEach, describe, expect, it } from "vitest";

import type { ProviderConfig } from "../types/provider";
import {
  SKILL_LAB_PREFERENCES_STORAGE_KEY,
  buildSkillLabRuntimeConfig,
  loadSkillLabPreferences,
  resolveSkillLabStagePreference,
  selectProviderDefaultModel,
  saveSkillLabPreferences,
  type SkillLabPreferences,
} from "../utils/skillLabRuntime";
import { filterProviderModels } from "../utils/chatModelSelection";

function createProvider(
  id: string,
  models: Array<{ id: string; label: string }>,
  apiKey = `${id}-secret`,
): ProviderConfig {
  return {
    id,
    type: id,
    name: id,
    category: "other",
    baseUrl: `https://${id}.example.com/v1`,
    apiKey,
    wireApi: "chat",
    requiresOpenAIAuth: false,
    models: models.map((model) => ({
      ...model,
      supportsVision: false,
      contextLength: 128000,
      maxOutputTokens: 65535,
    })),
    isCustom: true,
    createdAt: 1,
  };
}

const providers = [
  createProvider("quality", [
    { id: "quality-pro", label: "Quality Pro" },
    { id: "quality-fast", label: "Quality Fast" },
  ]),
  createProvider("evaluator", [
    { id: "eval-model", label: "Eval Model" },
  ]),
];

describe("Skill Lab runtime preferences", () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it("defaults both stages to the current provider and model with knowledge disabled", () => {
    const preferences = loadSkillLabPreferences(
      providers,
      "quality",
      "quality-fast",
    );

    expect(preferences).toEqual({
      generation: {
        providerId: "quality",
        modelId: "quality-fast",
        smartbrainEnabled: false,
      },
      evolution: {
        providerId: "quality",
        modelId: "quality-fast",
        smartbrainEnabled: false,
      },
    });
  });

  it("keeps generation and evolution selections independent", () => {
    const preferences: SkillLabPreferences = {
      generation: {
        providerId: "quality",
        modelId: "quality-pro",
        smartbrainEnabled: true,
      },
      evolution: {
        providerId: "evaluator",
        modelId: "eval-model",
        smartbrainEnabled: false,
      },
    };

    saveSkillLabPreferences(preferences);

    expect(loadSkillLabPreferences(providers, "quality", "quality-fast"))
      .toEqual(preferences);
  });

  it("falls back to the current valid selection when a saved model was removed", () => {
    expect(resolveSkillLabStagePreference(
      {
        providerId: "quality",
        modelId: "removed-model",
        smartbrainEnabled: true,
      },
      providers,
      "evaluator",
      "eval-model",
    )).toEqual({
      providerId: "evaluator",
      modelId: "eval-model",
      smartbrainEnabled: true,
    });
  });

  it("persists only ids and switches without provider secrets", () => {
    saveSkillLabPreferences({
      generation: {
        providerId: "quality",
        modelId: "quality-pro",
        smartbrainEnabled: true,
      },
      evolution: {
        providerId: "evaluator",
        modelId: "eval-model",
        smartbrainEnabled: false,
      },
    });

    const raw = window.localStorage.getItem(SKILL_LAB_PREFERENCES_STORAGE_KEY);
    expect(raw).not.toBeNull();
    expect(raw).not.toContain("quality-secret");
    expect(raw).not.toContain("evaluator-secret");
    expect(JSON.parse(raw ?? "{}")).toEqual({
      generation: {
        providerId: "quality",
        modelId: "quality-pro",
        smartbrainEnabled: true,
      },
      evolution: {
        providerId: "evaluator",
        modelId: "eval-model",
        smartbrainEnabled: false,
      },
    });
  });
});

describe("Skill Lab provider and model selection", () => {
  it("selects the first model when switching providers", () => {
    expect(selectProviderDefaultModel(providers, "evaluator")).toEqual({
      providerId: "evaluator",
      modelId: "eval-model",
    });
  });

  it("returns an invalid empty selection for a missing or empty provider", () => {
    expect(selectProviderDefaultModel(providers, "missing")).toEqual({
      providerId: "missing",
      modelId: "",
    });
    expect(selectProviderDefaultModel([], "quality")).toEqual({
      providerId: "quality",
      modelId: "",
    });
  });

  it("filters models by label and id without case sensitivity", () => {
    expect(filterProviderModels(providers[0].models, "PRO").map((model) => model.id))
      .toEqual(["quality-pro"]);
    expect(filterProviderModels(providers[0].models, "fast").map((model) => model.id))
      .toEqual(["quality-fast"]);
  });
});

describe("Skill Lab command runtime snapshots", () => {
  it("builds independent generation and evolution runtime configs", () => {
    const preferences: SkillLabPreferences = {
      generation: {
        providerId: "quality",
        modelId: "quality-pro",
        smartbrainEnabled: true,
      },
      evolution: {
        providerId: "evaluator",
        modelId: "eval-model",
        smartbrainEnabled: false,
      },
    };
    const buildProviderSnapshot = (providerId: string, modelId: string) => ({
      providerKey: providerId,
      modelId,
      apiKey: `${providerId}-runtime-secret`,
    });

    const generation = buildSkillLabRuntimeConfig(
      preferences.generation,
      buildProviderSnapshot,
    );
    const evolution = buildSkillLabRuntimeConfig(
      preferences.evolution,
      buildProviderSnapshot,
    );

    expect(generation).toEqual({
      provider: {
        providerKey: "quality",
        modelId: "quality-pro",
        apiKey: "quality-runtime-secret",
      },
      smartbrainEnabled: true,
    });
    expect(evolution).toEqual({
      provider: {
        providerKey: "evaluator",
        modelId: "eval-model",
        apiKey: "evaluator-runtime-secret",
      },
      smartbrainEnabled: false,
    });
    expect(generation).not.toHaveProperty("skillId");
    expect(generation).not.toHaveProperty("name");
    expect(generation).not.toHaveProperty("goal");
    expect(evolution).not.toEqual(generation);
  });

  it("captures the stage values at command construction time", () => {
    const preference = {
      providerId: "quality",
      modelId: "quality-pro",
      smartbrainEnabled: true,
    };
    const runtimeConfig = buildSkillLabRuntimeConfig(
      preference,
      (providerId, modelId) => ({ providerKey: providerId, modelId }),
    );

    preference.providerId = "evaluator";
    preference.modelId = "eval-model";
    preference.smartbrainEnabled = false;

    expect(runtimeConfig).toEqual({
      provider: { providerKey: "quality", modelId: "quality-pro" },
      smartbrainEnabled: true,
    });
  });

  it("rejects an invalid provider or model selection", () => {
    expect(buildSkillLabRuntimeConfig(
      { providerId: "", modelId: "quality-pro", smartbrainEnabled: false },
      () => ({ providerKey: "quality", modelId: "quality-pro" }),
    )).toBeNull();
    expect(buildSkillLabRuntimeConfig(
      { providerId: "quality", modelId: "", smartbrainEnabled: false },
      () => ({ providerKey: "quality", modelId: "quality-pro" }),
    )).toBeNull();
    expect(buildSkillLabRuntimeConfig(
      { providerId: "quality", modelId: "quality-pro", smartbrainEnabled: false },
      () => null,
    )).toBeNull();
  });
});
