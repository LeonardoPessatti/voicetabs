import { create } from "zustand";

import { settingsApi } from "../lib/tauri";
import i18n, { SupportedLocale, SUPPORTED_LOCALES } from "../i18n";

const LOCALE_KEY = "ui_locale";

type SettingsState = {
  uiLocale: SupportedLocale;
  drawerOpen: boolean;

  load: () => Promise<void>;
  setLocale: (locale: SupportedLocale) => Promise<void>;
  openDrawer: () => void;
  closeDrawer: () => void;
};

export const useSettingsStore = create<SettingsState>((set) => ({
  uiLocale: "pt-BR",
  drawerOpen: false,

  async load() {
    const stored = await settingsApi.get(LOCALE_KEY);
    if (SUPPORTED_LOCALES.includes(stored as SupportedLocale)) {
      // A stored value exists — apply it.
      const locale = stored as SupportedLocale;
      await i18n.changeLanguage(locale);
      document.documentElement.lang = locale;
      set({ uiLocale: locale });
    } else {
      // No stored value — use whatever i18n is already set to (from initI18n),
      // or fall back to navigator language detection.
      const current = SUPPORTED_LOCALES.includes(i18n.language as SupportedLocale)
        ? (i18n.language as SupportedLocale)
        : (navigator.language?.toLowerCase().startsWith("en") ? "en" : "pt-BR");
      document.documentElement.lang = current;
      set({ uiLocale: current });
    }
  },

  async setLocale(locale) {
    await settingsApi.set(LOCALE_KEY, locale);
    await i18n.changeLanguage(locale);
    document.documentElement.lang = locale;
    set({ uiLocale: locale });
  },

  openDrawer() {
    set({ drawerOpen: true });
  },
  closeDrawer() {
    set({ drawerOpen: false });
  },
}));
