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
    baziProfile: s.baziProfile,
  };
}

interface SettingsState {
  locale: string;
  theme: "dark" | "light" | "system";
  fortuneEnabled: boolean;
  backgroundImagePath: string | null;
  baziProfile: BaziProfile | null;
  fortuneRefreshTrigger: number;
  setLocale: (locale: string) => void;
  setTheme: (theme: "dark" | "light" | "system") => void;
  setFortuneEnabled: (enabled: boolean) => void;
  setBackgroundImagePath: (path: string | null) => void;
  setBaziProfile: (profile: BaziProfile | null) => void;
  triggerFortuneRefresh: () => void;
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
        baziProfile: parsed.baziProfile ?? null,
      });
      persist(getPersistedSnapshot(useSettingsStore.getState()));
    }
  } catch {
    // 首次使用，无数据，使用默认值
  }
}
