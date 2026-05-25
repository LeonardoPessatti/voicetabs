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

const { useSettingsStore } = await import("../stores/settingsStore");

beforeEach(() => {
  useSettingsStore.setState({
    uiLocale: "en",
    drawerOpen: false,
    vocabTerms: [],
    showTimestamps: true,
  });
  settingsGet.mockReset();
  settingsSet.mockReset();
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
