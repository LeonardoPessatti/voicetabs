import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { useSettingsStore } from "../stores/settingsStore";

export function VocabSettings() {
  const { t } = useTranslation();
  const vocabTerms = useSettingsStore((s) => s.vocabTerms);
  const setVocabTerms = useSettingsStore((s) => s.setVocabTerms);
  const [draft, setDraft] = useState<string>(vocabTerms.join("\n"));

  // Re-sync when the store updates externally (e.g. after a `load()`).
  useEffect(() => {
    setDraft(vocabTerms.join("\n"));
  }, [vocabTerms]);

  function commit() {
    const parsed = draft
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line.length > 0);
    // Only persist if the canonicalised list actually differs, to avoid a
    // settings-set on every blur with no real change.
    if (
      parsed.length === vocabTerms.length &&
      parsed.every((v, i) => v === vocabTerms[i])
    ) {
      return;
    }
    void setVocabTerms(parsed);
  }

  return (
    <section className="drawer__section">
      <label htmlFor="vocab-textarea">{t("settings.vocabHeading")}</label>
      <textarea
        id="vocab-textarea"
        className="drawer__textarea"
        value={draft}
        placeholder={t("settings.vocabPlaceholder")}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        rows={6}
      />
      <small className="drawer__hint">{t("settings.vocabHint")}</small>
    </section>
  );
}
