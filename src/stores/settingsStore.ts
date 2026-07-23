import { create } from "zustand";
import { appStateGet, appStateSet } from "../api/app_state";

const SETTINGS_KEY = "settings";
const SETTINGS_SNAPSHOT_STORAGE_KEY = "cn-codex:settings-snapshot";

export interface BaziProfile {
  name: string;
  birthDate: string;
  birthTime: string;
  gender: "male" | "female";
  lunarCalendar: boolean;
  occupation?: string;
  industry?: string;
}

interface PersistedSettings {
  locale: string;
  theme: string;
  fortuneEnabled: boolean;
  backgroundImagePath: string | null;
  backgroundBlur: number;
  backgroundBrightness: number;
  backgroundOverlay: number;
  backgroundScale: number;
  baziProfile: BaziProfile | null;
}

function normalizeThemeMode(value: unknown): "dark" | "light" | "system" {
  if (value === "light" || value === "system") {
    return value;
  }
  return "dark";
}

function readSettingsSnapshotFromStorage(): Partial<PersistedSettings> | null {
  if (typeof window === "undefined") {
    return null;
  }
  try {
    const raw = window.localStorage.getItem(SETTINGS_SNAPSHOT_STORAGE_KEY);
    if (!raw) {
      return null;
    }
    const parsed = JSON.parse(raw) as Partial<PersistedSettings>;
    return parsed && typeof parsed === "object" ? parsed : null;
  } catch {
    return null;
  }
}

function writeSettingsSnapshotToStorage(state: PersistedSettings): void {
  if (typeof window === "undefined") {
    return;
  }
  try {
    window.localStorage.setItem(
      SETTINGS_SNAPSHOT_STORAGE_KEY,
      JSON.stringify(state),
    );
  } catch {
    // Ignore localStorage failures and keep SQLite as source of truth.
  }
}

function persist(state: PersistedSettings) {
  void appStateSet(SETTINGS_KEY, JSON.stringify(state));
  writeSettingsSnapshotToStorage(state);
}

function getPersistedSnapshot(s: SettingsState): PersistedSettings {
  return {
    locale: s.locale,
    theme: s.theme,
    fortuneEnabled: s.fortuneEnabled,
    backgroundImagePath: s.backgroundImagePath,
    backgroundBlur: s.backgroundBlur,
    backgroundBrightness: s.backgroundBrightness,
    backgroundOverlay: s.backgroundOverlay,
    backgroundScale: s.backgroundScale,
    baziProfile: s.baziProfile,
  };
}

interface SettingsState {
  locale: string;
  theme: "dark" | "light" | "system";
  fortuneEnabled: boolean;
  backgroundImagePath: string | null;
  backgroundBlur: number;
  backgroundBrightness: number;
  backgroundOverlay: number;
  backgroundScale: number;
  baziProfile: BaziProfile | null;
  fortuneRefreshTrigger: number;
  setLocale: (locale: string) => void;
  setTheme: (theme: "dark" | "light" | "system") => void;
  setFortuneEnabled: (enabled: boolean) => void;
  setBackgroundImagePath: (path: string | null) => void;
  setBackgroundBlur: (value: number) => void;
  setBackgroundBrightness: (value: number) => void;
  setBackgroundOverlay: (value: number) => void;
  setBackgroundScale: (value: number) => void;
  setBaziProfile: (profile: BaziProfile | null) => void;
  triggerFortuneRefresh: () => void;
}

export const DEFAULT_BACKGROUND_BLUR = 18;
export const DEFAULT_BACKGROUND_BRIGHTNESS = 0.72;
export const DEFAULT_BACKGROUND_OVERLAY = 0.45;
export const DEFAULT_BACKGROUND_SCALE = 1.06;

function clampNumber(value: unknown, min: number, max: number, fallback: number): number {
  const n = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(n)) return fallback;
  return Math.min(max, Math.max(min, n));
}

const initialSettingsSnapshot = readSettingsSnapshotFromStorage();

export const useSettingsStore = create<SettingsState>((set, get) => ({
  locale:
    typeof initialSettingsSnapshot?.locale === "string"
      ? initialSettingsSnapshot.locale
      : "zh-CN",
  theme: normalizeThemeMode(initialSettingsSnapshot?.theme),
  fortuneEnabled: initialSettingsSnapshot?.fortuneEnabled ?? false,
  backgroundImagePath:
    typeof initialSettingsSnapshot?.backgroundImagePath === "string"
      ? initialSettingsSnapshot.backgroundImagePath
      : null,
  backgroundBlur: clampNumber(
    initialSettingsSnapshot?.backgroundBlur,
    0,
    40,
    DEFAULT_BACKGROUND_BLUR,
  ),
  backgroundBrightness: clampNumber(
    initialSettingsSnapshot?.backgroundBrightness,
    0.3,
    1.4,
    DEFAULT_BACKGROUND_BRIGHTNESS,
  ),
  backgroundOverlay: clampNumber(
    initialSettingsSnapshot?.backgroundOverlay,
    0,
    0.9,
    DEFAULT_BACKGROUND_OVERLAY,
  ),
  backgroundScale: clampNumber(
    initialSettingsSnapshot?.backgroundScale,
    1,
    1.3,
    DEFAULT_BACKGROUND_SCALE,
  ),
  baziProfile: initialSettingsSnapshot?.baziProfile ?? null,
  fortuneRefreshTrigger: 0,
  setLocale: (locale) => {
    set({ locale });
    persist(getPersistedSnapshot({ ...get(), locale }));
  },
  setTheme: (theme) => {
    set({ theme });
    persist(getPersistedSnapshot({ ...get(), theme }));
  },
  setFortuneEnabled: (fortuneEnabled) => {
    set({ fortuneEnabled });
    persist(getPersistedSnapshot({ ...get(), fortuneEnabled }));
  },
  setBackgroundImagePath: (backgroundImagePath) => {
    set({ backgroundImagePath });
    persist(getPersistedSnapshot({ ...get(), backgroundImagePath }));
  },
  setBackgroundBlur: (value) => {
    const backgroundBlur = clampNumber(value, 0, 40, DEFAULT_BACKGROUND_BLUR);
    set({ backgroundBlur });
    persist(getPersistedSnapshot({ ...get(), backgroundBlur }));
  },
  setBackgroundBrightness: (value) => {
    const backgroundBrightness = clampNumber(
      value,
      0.3,
      1.4,
      DEFAULT_BACKGROUND_BRIGHTNESS,
    );
    set({ backgroundBrightness });
    persist(getPersistedSnapshot({ ...get(), backgroundBrightness }));
  },
  setBackgroundOverlay: (value) => {
    const backgroundOverlay = clampNumber(
      value,
      0,
      0.9,
      DEFAULT_BACKGROUND_OVERLAY,
    );
    set({ backgroundOverlay });
    persist(getPersistedSnapshot({ ...get(), backgroundOverlay }));
  },
  setBackgroundScale: (value) => {
    const backgroundScale = clampNumber(value, 1, 1.3, DEFAULT_BACKGROUND_SCALE);
    set({ backgroundScale });
    persist(getPersistedSnapshot({ ...get(), backgroundScale }));
  },
  setBaziProfile: (baziProfile) => {
    set({ baziProfile });
    persist(getPersistedSnapshot({ ...get(), baziProfile }));
  },
  triggerFortuneRefresh: () => {
    set((s) => ({ fortuneRefreshTrigger: s.fortuneRefreshTrigger + 1 }));
  },
}));

/**
 * 从 SQLite 加载 settings 到 store。
 * 应在 App 挂载时调用一次。
 */
export async function initSettingsFromDb(): Promise<void> {
  try {
    const raw = await appStateGet(SETTINGS_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      useSettingsStore.setState({
        locale: parsed.locale ?? "zh-CN",
        theme: normalizeThemeMode(parsed.theme),
        fortuneEnabled: parsed.fortuneEnabled ?? false,
        backgroundImagePath:
          typeof parsed.backgroundImagePath === "string"
            ? parsed.backgroundImagePath
            : null,
        backgroundBlur: clampNumber(
          parsed.backgroundBlur,
          0,
          40,
          DEFAULT_BACKGROUND_BLUR,
        ),
        backgroundBrightness: clampNumber(
          parsed.backgroundBrightness,
          0.3,
          1.4,
          DEFAULT_BACKGROUND_BRIGHTNESS,
        ),
        backgroundOverlay: clampNumber(
          parsed.backgroundOverlay,
          0,
          0.9,
          DEFAULT_BACKGROUND_OVERLAY,
        ),
        backgroundScale: clampNumber(
          parsed.backgroundScale,
          1,
          1.3,
          DEFAULT_BACKGROUND_SCALE,
        ),
        baziProfile: parsed.baziProfile ?? null,
      });
      persist(getPersistedSnapshot(useSettingsStore.getState()));
    }
  } catch {
    // 首次使用，无数据，使用默认值
  }
}
