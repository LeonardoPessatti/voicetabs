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
  it("renders the persisted vocab terms in the textarea", () => {
    useSettingsStore.setState({ vocabTerms: ["foo", "bar"] });
    render(<VocabSettings />);
    expect(screen.getByRole("textbox")).toHaveValue("foo\nbar");
  });

  it("persists trimmed non-empty lines on blur", () => {
    render(<VocabSettings />);
    const textarea = screen.getByRole("textbox");
    fireEvent.change(textarea, { target: { value: "  alpha\n\n beta \n" } });
    fireEvent.blur(textarea);
    expect(settingsSet).toHaveBeenCalledWith({
      key: "vocab_terms",
      value: JSON.stringify(["alpha", "beta"]),
    });
  });

  it("does not call set if the value is unchanged after blur", () => {
    useSettingsStore.setState({ vocabTerms: ["x"] });
    render(<VocabSettings />);
    const textarea = screen.getByRole("textbox");
    fireEvent.blur(textarea); // no edit
    expect(settingsSet).not.toHaveBeenCalled();
  });
});
