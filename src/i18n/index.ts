import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import en from "./locales/en.json";
import ptBR from "./locales/pt-BR.json";

export const SUPPORTED_LOCALES = ["pt-BR", "en"] as const;
export type SupportedLocale = (typeof SUPPORTED_LOCALES)[number];

function detectInitialLocale(): SupportedLocale {
  const fromBrowser = navigator.language || "pt-BR";
  return fromBrowser.toLowerCase().startsWith("en") ? "en" : "pt-BR";
}

export function initI18n(locale?: SupportedLocale) {
  const initial = locale ?? detectInitialLocale();
  if (!i18n.isInitialized) {
    i18n.use(initReactI18next).init({
      resources: {
        en: { translation: en },
        "pt-BR": { translation: ptBR },
      },
      lng: initial,
      fallbackLng: "pt-BR",
      interpolation: { escapeValue: false },
    });
  } else {
    void i18n.changeLanguage(initial);
  }
  return i18n;
}

export default i18n;
