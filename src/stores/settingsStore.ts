import { create } from "zustand";

const STORAGE_KEY = "cn-codex-settings";

function loadPersistedSettings(): { locale: string; theme: "dark" | "light" | "system" } {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      return {
        locale: parsed.locale ?? "zh-CN",
        theme: parsed.theme ?? "dark",
      };
    }
  } catch {
    // Ignore parse errors
  }
  return { locale: "zh-CN", theme: "dark" };
}

function persist(state: { locale: string; theme: string }) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    // Ignore storage errors
  }
}

interface SettingsState {
  locale: string;
  theme: "dark" | "light" | "system";
  setLocale: (locale: string) => void;
  setTheme: (theme: "dark" | "light" | "system") => void;
}

const initial = loadPersistedSettings();

export const useSettingsStore = create<SettingsState>((set, get) => ({
  locale: initial.locale,
  theme: initial.theme,
  setLocale: (locale) => {
    set({ locale });
    persist({ locale, theme: get().theme });
  },
  setTheme: (theme) => {
    set({ theme });
    persist({ locale: get().locale, theme });
  },
}));
