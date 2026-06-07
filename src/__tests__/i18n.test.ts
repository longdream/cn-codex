import { describe, it, expect } from "vitest";
import zhCN from "../i18n/zh-CN/common.json";
import enUS from "../i18n/en-US/common.json";

describe("i18n messages", () => {
  it("zh-CN has all required base keys", () => {
    expect(zhCN).toHaveProperty("app.title");
    expect(zhCN).toHaveProperty("app.description");
    expect(zhCN).toHaveProperty("sidebar.newChat");
    expect(zhCN).toHaveProperty("sidebar.history");
    expect(zhCN).toHaveProperty("settings.language");
    expect(zhCN).toHaveProperty("settings.theme");
  });

  it("en-US has all required base keys", () => {
    expect(enUS).toHaveProperty("app.title");
    expect(enUS).toHaveProperty("app.description");
    expect(enUS).toHaveProperty("sidebar.newChat");
    expect(enUS).toHaveProperty("sidebar.history");
    expect(enUS).toHaveProperty("settings.language");
    expect(enUS).toHaveProperty("settings.theme");
  });

  it("both locales have matching keys", () => {
    const zhKeys = Object.keys(zhCN).sort();
    const enKeys = Object.keys(enUS).sort();
    expect(zhKeys).toEqual(enKeys);
  });

  it("zh-CN title is CN-Codex", () => {
    expect((zhCN as Record<string, string>)["app.title"]).toBe("CN-Codex");
  });

  it("en-US title is CN-Codex", () => {
    expect((enUS as Record<string, string>)["app.title"]).toBe("CN-Codex");
  });

  it("has all provider settings keys", () => {
    for (const msgs of [zhCN, enUS] as Record<string, string>[]) {
      expect(msgs).toHaveProperty("settings.provider");
      expect(msgs).toHaveProperty("settings.provider.type");
      expect(msgs).toHaveProperty("settings.provider.model");
      expect(msgs).toHaveProperty("settings.provider.effort");
      expect(msgs).toHaveProperty("settings.provider.apiKeyPlaceholder");
    }
  });

  it("has all integration keys", () => {
    for (const msgs of [zhCN, enUS] as Record<string, string>[]) {
      expect(msgs).toHaveProperty("settings.integration");
      expect(msgs).toHaveProperty("settings.integration.mcpServers");
      expect(msgs).toHaveProperty("settings.integration.noMcp");
      expect(msgs).toHaveProperty("settings.integration.mcpHint");
      expect(msgs).toHaveProperty("settings.integration.skills");
      expect(msgs).toHaveProperty("settings.integration.noSkills");
      expect(msgs).toHaveProperty("settings.integration.skillsHint");
    }
  });

  it("has all hooks keys", () => {
    for (const msgs of [zhCN, enUS] as Record<string, string>[]) {
      expect(msgs).toHaveProperty("settings.hooks");
      expect(msgs).toHaveProperty("settings.hooks.onAgentStart");
      expect(msgs).toHaveProperty("settings.hooks.onAgentEnd");
      expect(msgs).toHaveProperty("settings.hooks.onFileChange");
      expect(msgs).toHaveProperty("settings.hooks.onCommandExec");
      expect(msgs).toHaveProperty("settings.hooks.active");
      expect(msgs).toHaveProperty("settings.hooks.inactive");
      expect(msgs).toHaveProperty("settings.hooks.hint");
    }
  });

  it("has chat copy/copied and common loading/saving keys", () => {
    for (const msgs of [zhCN, enUS] as Record<string, string>[]) {
      expect(msgs).toHaveProperty("chat.copy");
      expect(msgs).toHaveProperty("chat.copied");
      expect(msgs).toHaveProperty("common.loading");
      expect(msgs).toHaveProperty("common.saving");
      expect(msgs).toHaveProperty("common.edit");
      expect(msgs).toHaveProperty("approval.title");
      expect(msgs).toHaveProperty("status.version");
    }
  });

  it("has at least 60 keys in each locale", () => {
    expect(Object.keys(zhCN).length).toBeGreaterThanOrEqual(60);
    expect(Object.keys(enUS).length).toBeGreaterThanOrEqual(60);
  });
});
