import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("../lib/tauri", () => ({
  backendApi: {
    get: vi.fn().mockResolvedValue("local"),
    set: vi.fn().mockResolvedValue(undefined),
    openaiKeyStatus: vi.fn().mockResolvedValue(false),
    openaiKeySet: vi.fn().mockResolvedValue(undefined),
    openaiKeyClear: vi.fn().mockResolvedValue(undefined),
  },
  settingsApi: {
    get: vi.fn().mockResolvedValue(null),
    set: vi.fn().mockResolvedValue(undefined),
    getJson: vi.fn().mockResolvedValue(null),
    setJson: vi.fn().mockResolvedValue(undefined),
  },
}));

const { BackendSettings } = await import("../components/BackendSettings");
const { useSettingsStore } = await import("../stores/settingsStore");

describe("BackendSettings", () => {
  beforeEach(() => {
    useSettingsStore.setState({ backend: "local", openaiKeySet: false });
  });

  it("renders both radio options and the local one selected by default", () => {
    render(<BackendSettings />);
    expect(screen.getByLabelText(/Local/i)).toBeChecked();
    expect(screen.getByLabelText(/OpenAI/i)).not.toBeChecked();
  });

  it("shows the API key input when OpenAI is selected", () => {
    useSettingsStore.setState({ backend: "openai", openaiKeySet: false });
    render(<BackendSettings />);
    expect(screen.getByPlaceholderText(/sk-/i)).toBeInTheDocument();
    expect(
      screen.getByText(/Nenhuma chave configurada|No key configured/i),
    ).toBeInTheDocument();
  });

  it("saves an entered key and shows configured state", async () => {
    useSettingsStore.setState({ backend: "openai", openaiKeySet: false });
    render(<BackendSettings />);
    const input = screen.getByPlaceholderText(/sk-/i) as HTMLInputElement;
    fireEvent.change(input, { target: { value: "sk-1234567890" } });
    fireEvent.click(screen.getByRole("button", { name: /Salvar|Save/i }));
    await waitFor(() =>
      expect(useSettingsStore.getState().openaiKeySet).toBe(true),
    );
    expect(input.value).toBe("");
  });
});
