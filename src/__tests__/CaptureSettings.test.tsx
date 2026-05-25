import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string) => {
    if (command === "capture_get_mode") return "always_on";
    if (command === "capture_set_mode") return undefined;
    if (command === "hotkey_get_binding") return null;
    if (command === "hotkey_set_binding") return undefined;
    if (command === "hotkey_clear_binding") return undefined;
    if (command === "hotkey_capture_next") return { kind: "key", code: "Space" };
    return undefined;
  }),
}));

// Import after mocks.
const { CaptureSettings } = await import("../components/CaptureSettings");
const { useCaptureStore } = await import("../stores/captureStore");

beforeEach(() => {
  useCaptureStore.setState({ captureMode: "always_on", hotkeyBinding: null });
});

describe("CaptureSettings", () => {
  it("renders the current mode and updates the store on change", async () => {
    render(<CaptureSettings />);
    const select = screen.getByLabelText(/mode/i) as HTMLSelectElement;
    expect(select.value).toBe("always_on");
    await userEvent.selectOptions(select, "ptt");
    expect(useCaptureStore.getState().captureMode).toBe("ptt");
  });

  it("shows 'None' when no hotkey is bound", () => {
    render(<CaptureSettings />);
    expect(screen.getByTestId("hotkey-current").textContent).toMatch(/none|nenhum/i);
  });

  it("shows the human label when a binding exists", () => {
    useCaptureStore.setState({
      captureMode: "ptt",
      hotkeyBinding: { kind: "key", code: "ControlRight" },
    });
    render(<CaptureSettings />);
    expect(screen.getByTestId("hotkey-current").textContent).toMatch(/right ctrl/i);
  });
});
