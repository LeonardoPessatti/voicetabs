import { useTranslation } from "react-i18next";

import { useSettingsStore } from "../stores/settingsStore";
import { SupportedLocale } from "../i18n";

export function SettingsDrawer() {
  const { t } = useTranslation();
  const { drawerOpen, closeDrawer, uiLocale, setLocale } = useSettingsStore();

  if (!drawerOpen) return null;

  return (
    <div className="drawer-backdrop" onClick={closeDrawer}>
      <aside className="drawer" role="dialog" aria-label={t("settings.open")} onClick={(e) => e.stopPropagation()}>
        <header className="drawer__header">
          <h2>{t("settings.open")}</h2>
          <button onClick={closeDrawer} aria-label={t("settings.close")}>×</button>
        </header>

        <section className="drawer__section">
          <label htmlFor="locale-select">{t("settings.language")}</label>
          <select
            id="locale-select"
            value={uiLocale}
            onChange={(e) => void setLocale(e.target.value as SupportedLocale)}
          >
            <option value="pt-BR">{t("settings.languagePtBr")}</option>
            <option value="en">{t("settings.languageEn")}</option>
          </select>
        </section>
      </aside>
    </div>
  );
}
