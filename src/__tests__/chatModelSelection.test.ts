import { describe, expect, it } from "vitest";

import {
  filterProviderModels,
  resolveModelPickerProviderId,
  resolveProviderModelId,
} from "../utils/chatModelSelection";
import type { ProviderConfig } from "../types/provider";

function createProvider(
  id: string,
  models: Array<{ id: string; label: string }>,
): ProviderConfig {
  return {
    id,
    type: id,
    name: id,
    category: "other",
    baseUrl: `https://${id}.example.com/v1`,
    apiKey: "",
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

const zApi = createProvider("z-api", [
  { id: "glm-4.5-air", label: "GLM 4.5 Air" },
  { id: "qwen3-coder", label: "Qwen3 Coder" },
]);

describe("resolveProviderModelId", () => {
  it("ignores a legacy Grok model when Z-API is the active provider", () => {
    expect(resolveProviderModelId(zApi, {
      overrideModelId: null,
      currentModel: "glm-4.5-air",
      legacyModelId: "grok-4",
    })).toBe("glm-4.5-air");
  });

  it("uses only overrides that belong to the selected provider", () => {
    expect(resolveProviderModelId(zApi, {
      overrideModelId: "grok-4",
      currentModel: "qwen3-coder",
      legacyModelId: null,
    })).toBe("qwen3-coder");
  });

  it("falls back to the selected provider first model", () => {
    expect(resolveProviderModelId(zApi, {
      overrideModelId: "missing-model",
      currentModel: "another-missing-model",
      legacyModelId: "grok-4",
    })).toBe("glm-4.5-air");
  });
});

describe("filterProviderModels", () => {
  it("matches model labels and ids without case sensitivity", () => {
    expect(filterProviderModels(zApi.models, "glm").map((model) => model.id))
      .toEqual(["glm-4.5-air"]);
    expect(filterProviderModels(zApi.models, "CODER").map((model) => model.id))
      .toEqual(["qwen3-coder"]);
  });

  it("returns every model for a blank query and none for an unmatched query", () => {
    expect(filterProviderModels(zApi.models, "  ")).toEqual(zApi.models);
    expect(filterProviderModels(zApi.models, "deepseek")).toEqual([]);
  });
});

describe("resolveModelPickerProviderId", () => {
  it("initializes each menu opening from the effective or global provider", () => {
    const providerIds = ["z-api", "grok"];

    expect(resolveModelPickerProviderId(providerIds, "z-api", "grok")).toBe("z-api");
    expect(resolveModelPickerProviderId(providerIds, "missing", "grok")).toBe("grok");
    expect(resolveModelPickerProviderId(providerIds, null, "missing")).toBe("z-api");
  });
});
