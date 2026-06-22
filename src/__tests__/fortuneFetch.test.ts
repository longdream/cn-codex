import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();
const appStateGetMock = vi.fn();
const appStateSetMock = vi.fn();
const getStateMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("../api/app_state", () => ({
  appStateGet: appStateGetMock,
  appStateSet: appStateSetMock,
}));

vi.mock("../stores/appStore", () => ({
  useAppStore: {
    getState: getStateMock,
  },
}));

function mockActiveStoreState() {
  getStateMock.mockReturnValue({
    getActiveProvider: () => ({
      id: "provider-openai",
      type: "openai",
      name: "OpenAI",
      category: "global",
      baseUrl: "https://api.example.com/v1",
      apiKey: "sk-test",
      wireApi: "chat",
      requiresOpenAIAuth: true,
      models: [
        {
          id: "test-model",
          label: "Test Model",
          supportsVision: false,
          contextLength: 128000,
          maxOutputTokens: 65535,
        },
      ],
      isCustom: false,
      createdAt: Date.now(),
    }),
    getActiveModel: () => ({
      id: "provider-openai:test-model",
      provider: "provider-openai",
      model: "test-model",
      label: "OpenAI / Test Model",
      supportsVision: false,
    }),
    currentModel: "test-model",
    activeEndpointIndex: null,
  });
}

describe("fetchDailyFortuneSummary", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockActiveStoreState();
    appStateGetMock.mockResolvedValue(null);
    appStateSetMock.mockResolvedValue(undefined);
  });

  it("calls fortune_llm_call only once when parsing fails", async () => {
    invokeMock.mockResolvedValue("这是一段推理过程，不是 JSON。");

    const { fetchDailyFortuneSummary } = await import("../utils/fortune");
    await expect(fetchDailyFortuneSummary(null, true)).rejects.toThrow(
      "Failed to parse fortune summary response from LLM",
    );

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(appStateSetMock).not.toHaveBeenCalled();
  });

  it("parses repaired json and writes cache on success", async () => {
    invokeMock.mockResolvedValue(`{
      date: '2026-06-22',
      overall: '阻滞',
      direction: '北方',
      bestAction: '等待',
      environment: '不利',
      summary: '空亡当头，宜静待时机',
    }`);

    const { fetchDailyFortuneSummary } = await import("../utils/fortune");
    const result = await fetchDailyFortuneSummary(null, true);

    expect(result.overall).toBe("阻滞");
    expect(result.direction).toBe("北方");
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(appStateSetMock).toHaveBeenCalledTimes(1);
  });
});
