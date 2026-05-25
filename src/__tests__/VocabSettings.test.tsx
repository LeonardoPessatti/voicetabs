import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";

const settingsGet = vi.fn();
const settingsSet = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args?: Record<string, unknown>) => {
    if (command === "settings_get") return settingsGet(args);
    if (command === "settings_set") return settingsSet(args);
    return undefined;
  }),
}));

// Import after mocks.
const { VocabSettings } = await import("../components/VocabSettings");
const { useSettingsStore } = await import("../stores/settingsStore");

beforeEach(() => {
  useSettingsStore.setState({
    uiLocale: "en",
    drawerOpen: false,
    vocabTerms: [],
  });
  settingsGet.mockReset();
  settingsSet.mockReset();
});

describe("VocabSettings", () => {
  it("renders existing terms as chips", () => {
    useSettingsStore.setState({ vocabTerms: ["alpha", "beta"] });
    render(<VocabSettings />);
    expect(screen.getByText("alpha")).toBeInTheDocument();
    expect(screen.getByText("beta")).toBeInTheDocument();
  });

  it("adds a new chip on Enter and persists", () => {
    render(<VocabSettings />);
    const input = screen.getByRole("textbox");
    fireEvent.change(input, { target: { value: "gamma" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(settingsSet).toHaveBeenCalledWith({
      key: "vocab_terms",
      value: JSON.stringify(["gamma"]),
    });
  });

  it("adds a new chip on comma and persists", () => {
    render(<VocabSettings />);
    const input = screen.getByRole("textbox");
    fireEvent.change(input, { target: { value: "delta," } });
    // Implementations may split on comma in onChange OR onKeyDown; the
    // assertion only requires that the chip ultimately lands.
    expect(settingsSet).toHaveBeenCalledWith({
      key: "vocab_terms",
      value: JSON.stringify(["delta"]),
    });
  });

  it("removes a chip on its × button", () => {
    useSettingsStore.setState({ vocabTerms: ["alpha", "beta"] });
    render(<VocabSettings />);
    fireEvent.click(
      screen.getByRole("button", { name: /remove alpha|remover alpha/i }),
    );
    expect(settingsSet).toHaveBeenCalledWith({
      key: "vocab_terms",
      value: JSON.stringify(["beta"]),
    });
  });

  it("Backspace on empty input removes the last chip", () => {
    useSettingsStore.setState({ vocabTerms: ["alpha", "beta"] });
    render(<VocabSettings />);
    const input = screen.getByRole("textbox");
    fireEvent.keyDown(input, { key: "Backspace" });
    expect(settingsSet).toHaveBeenCalledWith({
      key: "vocab_terms",
      value: JSON.stringify(["alpha"]),
    });
  });
});
