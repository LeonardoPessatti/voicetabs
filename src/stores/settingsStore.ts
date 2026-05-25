import { create } from "zustand";

import { settingsApi } from "../lib/tauri";
import i18n, { SupportedLocale, SUPPORTED_LOCALES } from "../i18n";

const LOCALE_KEY = "ui_locale";
const VOCAB_KEY = "vocab_terms";
const TIMESTAMPS_KEY = "show_timestamps";

type SettingsState = {
  uiLocale: SupportedLocale;
  drawerOpen: boolean;
  vocabTerms: string[];
  showTimestamps: boolean;

  load: () => Promise<void>;
  setLocale: (locale: SupportedLocale) => Promise<void>;
  setVocabTerms: (terms: string[]) => Promise<void>;
  setShowTimestamps: (value: boolean) => Promise<void>;
  openDrawer: () => void;
  closeDrawer: () => void;
};

export const useSettingsStore = create<SettingsState>((set) => ({
  uiLocale: "pt-BR",
  drawerOpen: false,
  vocabTerms: [],
  showTimestamps: true,

  async load() {
    const stored = await settingsApi.get(LOCALE_KEY);
    let uiLocale: SupportedLocale;
    if (SUPPORTED_LOCALES.includes(stored as SupportedLocale)) {
      uiLocale = stored as SupportedLocale;
      await i18n.changeLanguage(uiLocale);
      document.documentElement.lang = uiLocale;
    } else {
      uiLocale = SUPPORTED_LOCALES.includes(i18n.language as SupportedLocale)
        ? (i18n.language as SupportedLocale)
        : (navigator.language?.toLowerCase().startsWith("en") ? "en" : "pt-BR");
      document.documentElement.lang = uiLocale;
    }

    const vocabRaw = await settingsApi.get(VOCAB_KEY);
    let vocabTerms: string[] = [];
    if (vocabRaw !== null) {
      try {
        const parsed = JSON.parse(vocabRaw);
        if (Array.isArray(parsed)) {
          vocabTerms = parsed.filter((x): x is string => typeof x === "string");
        }
      } catch {
        vocabTerms = [];
      }
    }

    const tsRaw = await settingsApi.get(TIMESTAMPS_KEY);
    const showTimestamps = tsRaw === null ? true : tsRaw !== "false";

    set({ uiLocale, vocabTerms, showTimestamps });
  },

  async setLocale(locale) {
    await settingsApi.set(LOCALE_KEY, locale);
    await i18n.changeLanguage(locale);
    document.documentElement.lang = locale;
    set({ uiLocale: locale });
  },

  async setVocabTerms(terms) {
    const cleaned = terms.map((t) => t.trim()).filter((t) => t.length > 0);
    await settingsApi.set(VOCAB_KEY, JSON.stringify(cleaned));
    set({ vocabTerms: cleaned });
  },

  async setShowTimestamps(value) {
    await settingsApi.set(TIMESTAMPS_KEY, value ? "true" : "false");
    set({ showTimestamps: value });
  },

  openDrawer() {
    set({ drawerOpen: true });
  },
  closeDrawer() {
    set({ drawerOpen: false });
  },
}));
