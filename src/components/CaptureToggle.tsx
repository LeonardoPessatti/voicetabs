import { useTranslation } from "react-i18next";

import { CaptureStatus } from "../lib/tauri";

type Props = {
  status: CaptureStatus;
  onStart: () => void;
  onStop: () => void;
};

export function CaptureToggle({ status, onStart, onStop }: Props) {
  const { t } = useTranslation();

  if (status.state === "error") {
    return (
      <button
        className="footer-button capture-toggle capture-toggle--error"
        onClick={onStart}
        title={status.message}
      >
        {t("capture.errorPrefix")} {status.message}
      </button>
    );
  }

  const isOn = status.state === "capturing";
  return (
    <button
      className={`footer-button capture-toggle${isOn ? " capture-toggle--on" : ""}`}
      onClick={isOn ? onStop : onStart}
    >
      {isOn ? t("capture.toggleOn") : t("capture.toggleOff")}
    </button>
  );
}
