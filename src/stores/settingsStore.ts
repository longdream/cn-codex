import { create } from "zustand";
import { appStateGet, appStateSet } from "../api/app_state";

const SETTINGS_KEY = "settings";

function persist(state: { locale: string; theme: string }) {
  void appStateSet(SETTINGS_KEY, JSON.stringify(state));
}

interface SettingsState {
  locale: string;
  theme: "dark" | "light" | "system";
  setLocale: (locale: string) => void;
  setTheme: (theme: "dark" | "light" | "system") => void;
}

export const useSettingsStore = create<SettingsState>((set, get) => ({
  locale: "zh-CN",
  theme: "dark",
  setLocale: (locale) => {
    set({ locale });
    persist({ locale, theme: get().theme });
  },
  setTheme: (theme) => {
    set({ theme });
    persist({ locale: get().locale, theme });
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
      });
    }
  } catch {
    // 首次使用，无数据，使用默认值
  }
}
