import { describe, it, expect, beforeEach, vi } from "vitest";

vi.mock("../api/app_state", () => ({
  appStateGet: vi.fn(async () => null),
  appStateSet: vi.fn(async () => undefined),
}));

import {
  useSettingsStore,
  type BaziProfile,
  DEFAULT_BACKGROUND_BLUR,
  DEFAULT_BACKGROUND_BRIGHTNESS,
  DEFAULT_BACKGROUND_OVERLAY,
  DEFAULT_BACKGROUND_SCALE,
} from "../stores/settingsStore";

describe("settingsStore", () => {
  beforeEach(() => {
    useSettingsStore.setState({
      locale: "zh-CN",
      theme: "dark",
      fortuneEnabled: false,
      backgroundImagePath: null,
      backgroundBlur: DEFAULT_BACKGROUND_BLUR,
      backgroundBrightness: DEFAULT_BACKGROUND_BRIGHTNESS,
      backgroundOverlay: DEFAULT_BACKGROUND_OVERLAY,
      backgroundScale: DEFAULT_BACKGROUND_SCALE,
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

  it("defaults fortuneEnabled to false", () => {
    expect(useSettingsStore.getState().fortuneEnabled).toBe(false);
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

  it("defaults background adjust params", () => {
    const state = useSettingsStore.getState();
    expect(state.backgroundBlur).toBe(DEFAULT_BACKGROUND_BLUR);
    expect(state.backgroundBrightness).toBe(DEFAULT_BACKGROUND_BRIGHTNESS);
    expect(state.backgroundOverlay).toBe(DEFAULT_BACKGROUND_OVERLAY);
    expect(state.backgroundScale).toBe(DEFAULT_BACKGROUND_SCALE);
  });

  it("setBackgroundBlur clamps values", () => {
    useSettingsStore.getState().setBackgroundBlur(24);
    expect(useSettingsStore.getState().backgroundBlur).toBe(24);
    useSettingsStore.getState().setBackgroundBlur(999);
    expect(useSettingsStore.getState().backgroundBlur).toBe(40);
    useSettingsStore.getState().setBackgroundBlur(-3);
    expect(useSettingsStore.getState().backgroundBlur).toBe(0);
  });

  it("setBackgroundBrightness clamps values", () => {
    useSettingsStore.getState().setBackgroundBrightness(1.1);
    expect(useSettingsStore.getState().backgroundBrightness).toBe(1.1);
    useSettingsStore.getState().setBackgroundBrightness(3);
    expect(useSettingsStore.getState().backgroundBrightness).toBe(1.4);
    useSettingsStore.getState().setBackgroundBrightness(0.1);
    expect(useSettingsStore.getState().backgroundBrightness).toBe(0.3);
  });

  it("setBackgroundOverlay clamps values", () => {
    useSettingsStore.getState().setBackgroundOverlay(0.2);
    expect(useSettingsStore.getState().backgroundOverlay).toBe(0.2);
    useSettingsStore.getState().setBackgroundOverlay(2);
    expect(useSettingsStore.getState().backgroundOverlay).toBe(0.9);
    useSettingsStore.getState().setBackgroundOverlay(-1);
    expect(useSettingsStore.getState().backgroundOverlay).toBe(0);
  });

  it("setBackgroundScale clamps values", () => {
    useSettingsStore.getState().setBackgroundScale(1.2);
    expect(useSettingsStore.getState().backgroundScale).toBe(1.2);
    useSettingsStore.getState().setBackgroundScale(2);
    expect(useSettingsStore.getState().backgroundScale).toBe(1.3);
    useSettingsStore.getState().setBackgroundScale(0.5);
    expect(useSettingsStore.getState().backgroundScale).toBe(1);
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
      fortuneEnabled: false,
      backgroundImagePath: null,
      backgroundBlur: DEFAULT_BACKGROUND_BLUR,
      backgroundBrightness: DEFAULT_BACKGROUND_BRIGHTNESS,
      backgroundOverlay: DEFAULT_BACKGROUND_OVERLAY,
      backgroundScale: DEFAULT_BACKGROUND_SCALE,
      baziProfile: null,
    });
    const state = useSettingsStore.getState();
    expect(state.locale).toBe("zh-CN");
    expect(state.theme).toBe("dark");
    expect(state.fortuneEnabled).toBe(false);
    expect(state.backgroundImagePath).toBeNull();
    expect(state.backgroundBlur).toBe(DEFAULT_BACKGROUND_BLUR);
    expect(state.backgroundBrightness).toBe(DEFAULT_BACKGROUND_BRIGHTNESS);
    expect(state.backgroundOverlay).toBe(DEFAULT_BACKGROUND_OVERLAY);
    expect(state.backgroundScale).toBe(DEFAULT_BACKGROUND_SCALE);
    expect(state.baziProfile).toBeNull();
  });
});
