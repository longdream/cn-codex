import { describe, expect, it } from "vitest";

import type { ModelEntry } from "../stores/appStore";
import type { PoolModelEndpoint, ProviderConfig, ProviderModel } from "../types/provider";
import { resolveFortuneLlmConfig } from "../utils/fortune";

function createModel(
  id: string,
  endpoints?: PoolModelEndpoint[],
): ProviderModel {
  return {
    id,
    label: id,
    supportsVision: false,
    contextLength: 128000,
    maxOutputTokens: 65535,
    endpoints,
  };
}

function createProvider(overrides?: Partial<ProviderConfig>): ProviderConfig {
  return {
    id: "provider-main",
    type: "openai",
    name: "Test Provider",
    category: "global",
    baseUrl: "https://api.example.com/v1",
    apiKey: "sk-test",
    wireApi: "chat",
    requiresOpenAIAuth: true,
    models: [createModel("model-a")],
    isCustom: false,
    createdAt: 0,
    ...overrides,
  };
}

function createActiveModel(provider: string, model: string): ModelEntry {
  return {
    id: `${provider}:${model}`,
    provider,
    model,
    label: `${provider}/${model}`,
    supportsVision: false,
  };
}

describe("resolveFortuneLlmConfig", () => {
  it("supports non-local provider even when activeModel is missing", () => {
    const provider = createProvider({
      type: "openai",
      baseUrl: "https://api.non-local.example/v1",
      apiKey: "key-non-local",
      wireApi: "responses",
      models: [createModel("gpt-4o"), createModel("gpt-4.1")],
    });

    const resolved = resolveFortuneLlmConfig({
      provider,
      activeModel: null,
      currentModel: "gpt-4o",
      activeEndpointIndex: null,
    });

    expect(resolved).toEqual({
      baseUrl: "https://api.non-local.example/v1",
      apiKey: "key-non-local",
      modelName: "gpt-4o",
      wireApi: "responses",
    });
  });

  it("uses local-pool enabled endpoint and endpoint-level overrides", () => {
    const endpoints: PoolModelEndpoint[] = [
      { id: "ep-disabled", url: "http://disabled/v1", label: "disabled", enabled: false, apiKey: "k-disabled", wireApi: "chat" },
      { id: "ep-a", url: "http://10.0.0.2:8080/v1", label: "a", enabled: true, apiKey: "k-a", wireApi: "responses" },
      { id: "ep-b", url: "http://10.0.0.3:8080/v1", label: "b", enabled: true },
    ];

    const provider = createProvider({
      id: "provider-local-pool",
      type: "local-pool",
      category: "local",
      baseUrl: "",
      apiKey: "",
      wireApi: "chat",
      models: [createModel("qwen-local", endpoints)],
    });

    const resolved = resolveFortuneLlmConfig({
      provider,
      activeModel: createActiveModel(provider.id, "qwen-local"),
      currentModel: "qwen-local",
      activeEndpointIndex: 0,
    });

    expect(resolved).toEqual({
      baseUrl: "http://10.0.0.2:8080/v1",
      apiKey: "k-a",
      modelName: "qwen-local",
      wireApi: "responses",
    });
  });

  it("throws when local-pool model has no enabled endpoint", () => {
    const provider = createProvider({
      id: "provider-local-pool",
      type: "local-pool",
      category: "local",
      baseUrl: "",
      apiKey: "",
      wireApi: "chat",
      models: [
        createModel("qwen-local", [
          { id: "ep-disabled-a", url: "http://10.0.0.2:8080/v1", label: "a", enabled: false },
          { id: "ep-disabled-b", url: "http://10.0.0.3:8080/v1", label: "b", enabled: false },
        ]),
      ],
    });

    expect(() =>
      resolveFortuneLlmConfig({
        provider,
        activeModel: createActiveModel(provider.id, "qwen-local"),
        currentModel: "qwen-local",
        activeEndpointIndex: 0,
      })).toThrow("No enabled endpoint configured for local-pool model");
  });
});
