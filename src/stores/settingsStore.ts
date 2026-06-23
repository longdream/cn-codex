import { create } from "zustand";
import { appStateGet, appStateSet } from "../api/app_state";

const SETTINGS_KEY = "settings";

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

function persist(state: PersistedSettings) {
  void appStateSet(SETTINGS_KEY, JSON.stringify(state));
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

export const useSettingsStore = create<SettingsState>((set, get) => ({
  locale: "zh-CN",
  theme: "dark",
  fortuneEnabled: true,
  backgroundImagePath: null,
  baziProfile: null,
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
        theme: parsed.theme ?? "dark",
        fortuneEnabled: parsed.fortuneEnabled ?? true,
        backgroundImagePath:
          typeof parsed.backgroundImagePath === "string"
            ? parsed.backgroundImagePath
            : null,
        baziProfile: parsed.baziProfile ?? null,
      });
    }
  } catch {
    // 首次使用，无数据，使用默认值
  }
}
