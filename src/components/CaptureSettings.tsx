import { useTranslation } from "react-i18next";

import { useCaptureStore } from "../stores/captureStore";
import { CaptureMode } from "../lib/tauri";
import { HotkeyBinder } from "./HotkeyBinder";

export function CaptureSettings() {
  const { t } = useTranslation();
  const { captureMode, setMode, hotkeyBinding } = useCaptureStore();

  return (
    <section className="drawer__section">
      <h3>{t("settings.captureHeading")}</h3>
      <label htmlFor="capture-mode-select">{t("settings.captureMode")}</label>
      <select
        id="capture-mode-select"
        value={captureMode}
        onChange={(e) => void setMode(e.target.value as CaptureMode)}
      >
        <option value="always_on">{t("settings.captureModeAlwaysOn")}</option>
        <option value="ptt">{t("settings.captureModePtt")}</option>
      </select>

      <div className="hotkey-row">
        <span className="hotkey-row__label">{t("settings.hotkey")}</span>
        <span className="hotkey-row__current" data-testid="hotkey-current">
          {hotkeyBinding ? humanLabel(hotkeyBinding) : t("settings.hotkeyNone")}
        </span>
        <HotkeyBinder />
      </div>
    </section>
  );
}

function humanLabel(b: { kind: string; code: string }): string {
  if (b.kind === "mouse") {
    if (b.code === "MouseButton4") return "Mouse Button 4";
    if (b.code === "MouseButton5") return "Mouse Button 5";
    return b.code;
  }
  // Keyboard: a tiny in-frontend lookup so we don't roundtrip to Rust.
  const map: Record<string, string> = {
    ControlLeft: "Left Ctrl",
    ControlRight: "Right Ctrl",
    ShiftLeft: "Left Shift",
    ShiftRight: "Right Shift",
    AltLeft: "Left Alt",
    AltRight: "Right Alt",
    Space: "Space",
  };
  return map[b.code] ?? b.code;
}
