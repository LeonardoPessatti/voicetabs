import { useTranslation } from "react-i18next";

export function EmptyTabHint() {
  const { t } = useTranslation();
  return (
    <div className="empty-tab-hint" role="status" aria-live="polite">
      <div className="empty-tab-hint__icon" aria-hidden>🎤</div>
      <p className="empty-tab-hint__text">{t("segments.emptyHint")}</p>
    </div>
  );
}
