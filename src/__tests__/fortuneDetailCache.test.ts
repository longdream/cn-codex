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

const SUMMARY = {
  date: "2026-06-22",
  overall: "顺势",
  direction: "东南",
  bestAction: "行动",
  environment: "有利",
  summary: "稳中有进",
};

const DETAIL = {
  qimenDetail: "奇门值符得位，宜先行动后沟通。",
  advice: "上午推进关键事项，下午做复盘。",
};

describe("fortune detail cache", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    appStateGetMock.mockResolvedValue(null);
    appStateSetMock.mockResolvedValue(undefined);
    getStateMock.mockReturnValue({});
  });

  it("returns cached detail when within ttl", async () => {
    const { getCachedFortuneDetail } = await import("../utils/fortune");
    const now = 1_717_000_000_000;
    appStateGetMock.mockResolvedValue(JSON.stringify({
      cachedAt: now - 1_000,
      detail: DETAIL,
    }));

    const result = await getCachedFortuneDetail(SUMMARY, null, now);
    expect(result).toEqual({
      qimenDetail: DETAIL.qimenDetail,
      advice: DETAIL.advice,
      ziweiDetail: undefined,
    });
  });

  it("returns null when cache is expired", async () => {
    const { FORTUNE_DETAIL_CACHE_TTL_MS, getCachedFortuneDetail } = await import("../utils/fortune");
    const now = 1_717_000_000_000;
    appStateGetMock.mockResolvedValue(JSON.stringify({
      cachedAt: now - FORTUNE_DETAIL_CACHE_TTL_MS - 1,
      detail: DETAIL,
    }));

    const result = await getCachedFortuneDetail(SUMMARY, null, now);
    expect(result).toBeNull();
  });

  it("writes detail payload with cachedAt timestamp", async () => {
    const { setCachedFortuneDetail } = await import("../utils/fortune");
    const cachedAt = 1_717_000_000_000;

    await setCachedFortuneDetail(SUMMARY, DETAIL, null, cachedAt);

    expect(appStateSetMock).toHaveBeenCalledTimes(1);
    const [cacheKey, payloadRaw] = appStateSetMock.mock.calls[0];
    expect(cacheKey).toContain(`fortune_detail_${SUMMARY.date}_`);
    const payload = JSON.parse(payloadRaw as string);
    expect(payload.cachedAt).toBe(cachedAt);
    expect(payload.detail).toEqual(DETAIL);
  });
});
