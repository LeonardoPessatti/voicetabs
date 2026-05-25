import { describe, expect, it, vi, beforeEach } from "vitest";

const settingsGet = vi.fn();
const settingsSet = vi.fn();
const backendGet = vi.fn();
const backendSet = vi.fn();
const openaiKeyStatus = vi.fn();
const openaiKeySet = vi.fn();
const openaiKeyClear = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args?: Record<string, unknown>) => {
    if (command === "settings_get") return settingsGet(args);
    if (command === "settings_set") return settingsSet(args);
    if (command === "backend_get") return backendGet(args);
    if (command === "backend_set") return backendSet(args);
    if (command === "openai_key_status") return openaiKeyStatus(args);
    if (command === "openai_key_set") return openaiKeySet(args);
    if (command === "openai_key_clear") return openaiKeyClear(args);
    return undefined;
  }),
}));

const { useSettingsStore } = await import("../stores/settingsStore");

beforeEach(() => {
  useSettingsStore.setState({
    uiLocale: "en",
    drawerOpen: false,
    vocabTerms: [],
    showTimestamps: true,
    backend: "local",
    openaiKeySet: false,
  });
  settingsGet.mockReset();
  settingsSet.mockReset();
  backendGet.mockReset();
  backendSet.mockReset();
  openaiKeyStatus.mockReset();
  openaiKeySet.mockReset();
  openaiKeyClear.mockReset();
  // Default backend mocks so existing load() tests don't blow up.
  backendGet.mockResolvedValue("local");
  openaiKeyStatus.mockResolvedValue(false);
});

describe("settingsStore showTimestamps", () => {
  it("defaults to true when the key is absent", async () => {
    settingsGet.mockImplementation(async ({ key }: { key: string }) => {
      if (key === "show_timestamps") return null;
      return null;
    });
    await useSettingsStore.getState().load();
    expect(useSettingsStore.getState().showTimestamps).toBe(true);
  });

  it("reads false back as false", async () => {
    settingsGet.mockImplementation(async ({ key }: { key: string }) => {
      if (key === "show_timestamps") return "false";
      return null;
    });
    await useSettingsStore.getState().load();
    expect(useSettingsStore.getState().showTimestamps).toBe(false);
  });

  it("setShowTimestamps(false) persists the string 'false' and updates state", async () => {
    await useSettingsStore.getState().setShowTimestamps(false);
    expect(settingsSet).toHaveBeenCalledWith({ key: "show_timestamps", value: "false" });
    expect(useSettingsStore.getState().showTimestamps).toBe(false);
  });

  it("setShowTimestamps(true) persists the string 'true'", async () => {
    await useSettingsStore.getState().setShowTimestamps(true);
    expect(settingsSet).toHaveBeenCalledWith({ key: "show_timestamps", value: "true" });
    expect(useSettingsStore.getState().showTimestamps).toBe(true);
  });
});

describe("settingsStore backend + openai key", () => {
  it("load() populates backend and openaiKeySet from the backend api", async () => {
    settingsGet.mockResolvedValue(null);
    backendGet.mockResolvedValue("openai");
    openaiKeyStatus.mockResolvedValue(true);
    await useSettingsStore.getState().load();
    expect(useSettingsStore.getState().backend).toBe("openai");
    expect(useSettingsStore.getState().openaiKeySet).toBe(true);
  });

  it("setBackend('openai') calls backend_set and updates state", async () => {
    backendSet.mockResolvedValue(undefined);
    await useSettingsStore.getState().setBackend("openai");
    expect(backendSet).toHaveBeenCalledWith({ kind: "openai" });
    expect(useSettingsStore.getState().backend).toBe("openai");
  });

  it("saveOpenAiKey('sk-x') flips openaiKeySet to true", async () => {
    openaiKeySet.mockResolvedValue(undefined);
    await useSettingsStore.getState().saveOpenAiKey("sk-x");
    expect(openaiKeySet).toHaveBeenCalledWith({ value: "sk-x" });
    expect(useSettingsStore.getState().openaiKeySet).toBe(true);
  });

  it("clearOpenAiKey() flips openaiKeySet false AND re-reads backend (auto-revert)", async () => {
    useSettingsStore.setState({ backend: "openai", openaiKeySet: true });
    openaiKeyClear.mockResolvedValue(undefined);
    // Simulate the server auto-reverting to "local" after the key clear.
    backendGet.mockResolvedValue("local");
    await useSettingsStore.getState().clearOpenAiKey();
    expect(openaiKeyClear).toHaveBeenCalled();
    expect(backendGet).toHaveBeenCalled();
    expect(useSettingsStore.getState().openaiKeySet).toBe(false);
    expect(useSettingsStore.getState().backend).toBe("local");
  });
});
