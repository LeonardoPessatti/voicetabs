import { useState } from "react";
import { useTranslation } from "react-i18next";

import { useSettingsStore } from "../stores/settingsStore";

export function VocabSettings() {
  const { t } = useTranslation();
  const vocabTerms = useSettingsStore((s) => s.vocabTerms);
  const setVocabTerms = useSettingsStore((s) => s.setVocabTerms);
  const [draft, setDraft] = useState("");

  function commit(newTerms: string[]) {
    const cleaned = newTerms.map((x) => x.trim()).filter((x) => x.length > 0);
    void setVocabTerms(cleaned);
  }

  function addFromDraft(raw: string) {
    const parts = raw.split(",").map((s) => s.trim()).filter(Boolean);
    if (parts.length === 0) {
      setDraft("");
      return;
    }
    const next = [...vocabTerms];
    for (const p of parts) if (!next.includes(p)) next.push(p);
    commit(next);
    setDraft("");
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Enter" || e.key === ",") {
      e.preventDefault();
      addFromDraft(draft);
    } else if (
      e.key === "Backspace" &&
      draft.length === 0 &&
      vocabTerms.length > 0
    ) {
      e.preventDefault();
      commit(vocabTerms.slice(0, -1));
    }
  }

  function onChange(e: React.ChangeEvent<HTMLInputElement>) {
    const v = e.target.value;
    if (v.includes(",")) {
      addFromDraft(v);
    } else {
      setDraft(v);
    }
  }

  function removeChip(term: string) {
    commit(vocabTerms.filter((x) => x !== term));
  }

  return (
    <section className="drawer__section">
      <label htmlFor="vocab-input">{t("settings.vocabHeading")}</label>
      <div className="chip-editor">
        {vocabTerms.map((term) => (
          <span key={term} className="chip">
            {term}
            <button
              type="button"
              className="chip__remove"
              aria-label={t("settings.vocabRemoveChip", { term })}
              onClick={() => removeChip(term)}
            >
              ×
            </button>
          </span>
        ))}
        <input
          id="vocab-input"
          className="chip-editor__input"
          value={draft}
          onChange={onChange}
          onKeyDown={onKeyDown}
          placeholder={t("settings.vocabPlaceholder")}
        />
      </div>
      <small className="drawer__hint">{t("settings.vocabAddChipHint")}</small>
    </section>
  );
}
