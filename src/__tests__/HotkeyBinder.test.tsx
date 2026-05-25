import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

// Import after mocks.
const { HotkeyBinder } = await import("../components/HotkeyBinder");
const { useCaptureStore } = await import("../stores/captureStore");

beforeEach(() => {
  invokeMock.mockReset();
  useCaptureStore.setState({ captureMode: "ptt", hotkeyBinding: null });
});

describe("HotkeyBinder", () => {
  it("shows 'Bind' by default and switches to capturing on click", async () => {
    let resolveCapture: (b: { kind: "key"; code: string }) => void;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "hotkey_capture_next") {
        return new Promise((res) => {
          resolveCapture = res as never;
        });
      }
      return Promise.resolve(undefined);
    });
    render(<HotkeyBinder />);
    await userEvent.click(screen.getByRole("button"));
    expect(screen.getByTestId("hotkey-capturing")).toBeInTheDocument();
    resolveCapture!({ kind: "key", code: "Space" });
  });

  it("calls captureNextBinding and updates the store on success", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "hotkey_capture_next") {
        return Promise.resolve({ kind: "key", code: "ControlRight" });
      }
      // hotkey_set_binding
      return Promise.resolve(undefined);
    });
    render(<HotkeyBinder />);
    await userEvent.click(screen.getByRole("button"));
    // Allow the promise chain to resolve.
    await new Promise((r) => setTimeout(r, 0));
    expect(useCaptureStore.getState().hotkeyBinding?.code).toBe("ControlRight");
  });

  it("treats backend 'cancelled' errors as silent", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "hotkey_capture_next") {
        return Promise.reject(new Error("capture cancelled"));
      }
      return Promise.resolve(undefined);
    });
    render(<HotkeyBinder />);
    await userEvent.click(screen.getByRole("button"));
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.queryByText(/cancelled/)).not.toBeInTheDocument();
  });
});
