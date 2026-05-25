import { useState } from "react";
import { useTranslation } from "react-i18next";

import { useCaptureStore } from "../stores/captureStore";

export function HotkeyBinder() {
  const { t } = useTranslation();
  const { captureNextBinding } = useCaptureStore();
  const [capturing, setCapturing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function onBind() {
    setError(null);
    setCapturing(true);
    try {
      await captureNextBinding();
    } catch (e) {
      const msg = (e as { message?: string })?.message ?? String(e);
      // Cancelled / timed-out are not errors worth red text.
      if (msg.includes("cancelled") || msg.includes("timed out")) {
        // no-op
      } else {
        setError(msg);
      }
    } finally {
      setCapturing(false);
    }
  }

  if (capturing) {
    return (
      <span role="status" data-testid="hotkey-capturing">
        {t("settings.hotkeyCapturing")}
        <button onClick={() => setCapturing(false)} type="button">
          {t("settings.hotkeyCancel")}
        </button>
      </span>
    );
  }

  return (
    <>
      <button onClick={onBind} type="button">
        {t("settings.hotkeyBind")}
      </button>
      {error && <span className="hotkey-error">{error}</span>}
    </>
  );
}
