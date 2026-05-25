import { useState } from "react";
import { useTranslation } from "react-i18next";

import { BackendKind } from "../lib/tauri";
import { useSettingsStore } from "../stores/settingsStore";

export function BackendSettings() {
  const { t } = useTranslation();
  const {
    backend,
    openaiKeySet,
    setBackend,
    saveOpenAiKey,
    clearOpenAiKey,
  } = useSettingsStore();
  const [keyDraft, setKeyDraft] = useState("");
  const [error, setError] = useState<string | null>(null);

  async function pickBackend(kind: BackendKind) {
    setError(null);
    try {
      await setBackend(kind);
    } catch (e: unknown) {
      const msg = (e as { message?: string })?.message ?? String(e);
      setError(
        msg.includes("OPENAI_NO_KEY") ? t("settings.backend.errorNoKey") : msg,
      );
    }
  }

  async function onSaveKey() {
    if (!keyDraft.trim()) return;
    await saveOpenAiKey(keyDraft.trim());
    setKeyDraft("");
  }

  return (
    <section className="drawer__section" data-testid="backend-settings">
      <h3>{t("settings.backend.heading")}</h3>
      <label>
        <input
          type="radio"
          name="backend"
          value="local"
          checked={backend === "local"}
          onChange={() => void pickBackend("local")}
        />
        {t("settings.backend.local")}
      </label>
      <label>
        <input
          type="radio"
          name="backend"
          value="openai"
          checked={backend === "openai"}
          onChange={() => void pickBackend("openai")}
        />
        {t("settings.backend.openai")}
      </label>

      {backend === "openai" && (
        <div className="backend__openai">
          <label htmlFor="openai-key">{t("settings.backend.apiKey")}</label>
          <input
            id="openai-key"
            type="password"
            placeholder={t("settings.backend.apiKeyPlaceholder")}
            value={keyDraft}
            onChange={(e) => setKeyDraft(e.target.value)}
            autoComplete="off"
          />
          <div className="backend__key-actions">
            <button type="button" onClick={() => void onSaveKey()}>
              {t("settings.backend.save")}
            </button>
            <button type="button" onClick={() => void clearOpenAiKey()}>
              {t("settings.backend.clear")}
            </button>
          </div>
          <p
            className={
              openaiKeySet ? "backend__status--ok" : "backend__status--missing"
            }
          >
            {openaiKeySet
              ? t("settings.backend.configured")
              : t("settings.backend.notConfigured")}
          </p>
          <p className="backend__disclaimer">
            {t("settings.backend.disclaimer")}
          </p>
        </div>
      )}
      {error && (
        <p role="alert" className="backend__error">
          {error}
        </p>
      )}
    </section>
  );
}
