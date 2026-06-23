import { describe, it, expect, beforeEach, vi } from "vitest";

vi.mock("../api/app_state", () => ({
  appStateGet: vi.fn(async () => null),
  appStateSet: vi.fn(async () => undefined),
}));

import { useSettingsStore, type BaziProfile } from "../stores/settingsStore";

describe("settingsStore", () => {
  beforeEach(() => {
    useSettingsStore.setState({
      locale: "zh-CN",
      theme: "dark",
      fortuneEnabled: true,
      backgroundImagePath: null,
      baziProfile: null,
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

  it("defaults fortuneEnabled to true", () => {
    expect(useSettingsStore.getState().fortuneEnabled).toBe(true);
  });

  it("setFortuneEnabled toggles fortune", () => {
    useSettingsStore.getState().setFortuneEnabled(false);
    expect(useSettingsStore.getState().fortuneEnabled).toBe(false);
    useSettingsStore.getState().setFortuneEnabled(true);
    expect(useSettingsStore.getState().fortuneEnabled).toBe(true);
  });

  it("defaults backgroundImagePath to null", () => {
    expect(useSettingsStore.getState().backgroundImagePath).toBeNull();
  });

  it("setBackgroundImagePath stores and clears path", () => {
    const imagePath = "C:\\images\\wallpaper.jpg";
    useSettingsStore.getState().setBackgroundImagePath(imagePath);
    expect(useSettingsStore.getState().backgroundImagePath).toBe(imagePath);

    useSettingsStore.getState().setBackgroundImagePath(null);
    expect(useSettingsStore.getState().backgroundImagePath).toBeNull();
  });

  it("defaults baziProfile to null", () => {
    expect(useSettingsStore.getState().baziProfile).toBeNull();
  });

  it("setBaziProfile stores and clears profile", () => {
    const profile: BaziProfile = {
      name: "测试",
      birthDate: "1990-01-01",
      birthTime: "zi",
      gender: "male",
      lunarCalendar: false,
    };
    useSettingsStore.getState().setBaziProfile(profile);
    expect(useSettingsStore.getState().baziProfile).toEqual(profile);

    useSettingsStore.getState().setBaziProfile(null);
    expect(useSettingsStore.getState().baziProfile).toBeNull();
  });

  it("returns defaults for all fields", () => {
    useSettingsStore.setState({
      locale: "zh-CN",
      theme: "dark",
      fortuneEnabled: true,
      backgroundImagePath: null,
      baziProfile: null,
    });
    const state = useSettingsStore.getState();
    expect(state.locale).toBe("zh-CN");
    expect(state.theme).toBe("dark");
    expect(state.fortuneEnabled).toBe(true);
    expect(state.backgroundImagePath).toBeNull();
    expect(state.baziProfile).toBeNull();
  });
});
