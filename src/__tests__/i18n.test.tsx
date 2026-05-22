import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import App from "../App";
import { initI18n } from "../i18n";

// Mock the Tauri bridge so `invoke` returns sane defaults instead of failing
// in jsdom (no Tauri host). One persisted tab named "Test" is returned for the
// initial list; subsequent calls (set, create, rename, etc.) are no-ops.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args?: Record<string, unknown>) => {
    if (command === "tabs_list") {
      return [
        {
          id: 1,
          title: "Test",
          order_idx: 0,
          created_at: 0,
          updated_at: 0,
        },
      ];
    }
    if (command === "tabs_create") {
      const title = (args?.title as string) ?? "New";
      return { id: 2, title, order_idx: 1, created_at: 0, updated_at: 0 };
    }
    if (command === "settings_get") {
      return null;
    }
    if (command === "capture_status") {
      return { state: "idle" };
    }
    if (command === "capture_start" || command === "capture_stop") {
      return undefined;
    }
    if (command === "stt_status") {
      return { state: "ready", backend: "cpu", model_id: "stub" };
    }
    // All other commands resolve to undefined / no-op.
    return undefined;
  }),
}));

describe("App locale", () => {
  it("renders with PT-BR locale and shows the new-tab button labeled in PT-BR", async () => {
    initI18n("pt-BR");
    render(<App />);
    await waitFor(() => {
      expect(screen.getByRole("button", { name: /nova aba/i })).toBeInTheDocument();
    });
  });

  it("renders with EN locale and shows the new-tab button labeled in English", async () => {
    initI18n("en");
    render(<App />);
    await waitFor(() => {
      expect(screen.getByRole("button", { name: /new tab/i })).toBeInTheDocument();
    });
  });

  it("includes a tab rendered by the mocked invoke list response", async () => {
    initI18n("en");
    render(<App />);
    await waitFor(() => {
      expect(screen.getByRole("tab", { name: /Test/ })).toBeInTheDocument();
    });
  });
});

// Keep a static reference to ensure vi.mock doesn't get tree-shaken
const _keep = fireEvent;
void _keep;
