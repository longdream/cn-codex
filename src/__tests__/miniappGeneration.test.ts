import { describe, expect, it } from "vitest";

import {
  buildMiniAppGeneratePrompt,
} from "../components/settings/MiniAppSettingsPanel";
import type { MiniAppRecord } from "../api/miniapp";

function createMiniApp(overrides: Partial<MiniAppRecord> = {}): MiniAppRecord {
  return {
    id: "miniapp-focus-timer",
    name: "专注计时器",
    slug: "focus-timer",
    description: "带界面的计时器",
    databaseId: "",
    databaseName: "",
    status: "generated",
    rootPath: "D:/workspace/codey/miniapps/focus-timer",
    pages: [{ id: "home", title: "首页", path: "/" }],
    tools: [],
    createdAt: 1,
    updatedAt: 1,
    ...overrides,
  };
}

describe("buildMiniAppGeneratePrompt", () => {
  it("requires an interactive UI without assuming a database", () => {
    const prompt = buildMiniAppGeneratePrompt({
      app: createMiniApp(),
      requirement: "做一个支持暂停和历史记录的专注计时器",
      allowWrite: false,
      mode: "generate",
    });

    expect(prompt).toContain("必须交付可用界面");
    expect(prompt).toContain("功能类型不设限");
    expect(prompt).toContain("本小程序未选择数据库");
    expect(prompt).toContain("任务完成后宿主会自动启动小程序");
  });

  it("adds database guidance only when a database is selected", () => {
    const prompt = buildMiniAppGeneratePrompt({
      app: createMiniApp({ databaseId: "db-1", databaseName: "工作库" }),
      requirement: "做一个库存看板",
      allowWrite: true,
      mode: "patch",
    });

    expect(prompt).toContain("db-1");
    expect(prompt).toContain("允许按需求执行授权范围内的写操作");
    expect(prompt).not.toContain("本小程序未选择数据库");
  });
});
