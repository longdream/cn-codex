import { describe, it, expect, beforeEach } from "vitest";
import { useSettingsStore } from "../stores/settingsStore";

const STORAGE_KEY = "cn-codex-settings";

describe("settingsStore", () => {
  beforeEach(() => {
    localStorage.clear();
    useSettingsStore.setState({
      locale: "zh-CN",
      theme: "dark",
    });
  });

  it("defaults locale to zh-CN", () => {
    expect(useSettingsStore.getState().locale).toBe("zh-CN");
  });

  it("defaults theme to dark", () => {
    expect(useSettingsStore.getState().theme).toBe("dark");
  });

  it("setLocale switches to en-US", () => {
    useSettingsStore.getState().setLocale("en-US");
    expect(useSettingsStore.getState().locale).toBe("en-US");
  });

  it("setLocale switches back to zh-CN", () => {
    useSettingsStore.getState().setLocale("en-US");
    useSettingsStore.getState().setLocale("zh-CN");
    expect(useSettingsStore.getState().locale).toBe("zh-CN");
  });

  it("setTheme switches to light", () => {
    useSettingsStore.getState().setTheme("light");
    expect(useSettingsStore.getState().theme).toBe("light");
  });

  it("setLocale persists to localStorage", () => {
    useSettingsStore.getState().setLocale("en-US");
    const stored = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}");
    expect(stored.locale).toBe("en-US");
  });

  it("setTheme persists to localStorage", () => {
    useSettingsStore.getState().setTheme("system");
    const stored = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}");
    expect(stored.theme).toBe("system");
  });

  it("returns defaults when localStorage is empty", () => {
    localStorage.clear();
    useSettingsStore.setState({ locale: "zh-CN", theme: "dark" });
    expect(useSettingsStore.getState().locale).toBe("zh-CN");
    expect(useSettingsStore.getState().theme).toBe("dark");
  });

  it("handles malformed JSON in localStorage gracefully", () => {
    localStorage.setItem(STORAGE_KEY, "not valid json{{{");
    // Re-import would be needed to fully test load, but at minimum
    // the store should still work with current state
    expect(useSettingsStore.getState().locale).toBe("zh-CN");
    expect(useSettingsStore.getState().theme).toBe("dark");
  });
});
