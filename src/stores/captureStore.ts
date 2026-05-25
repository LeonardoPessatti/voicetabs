import { create } from "zustand";

import {
  Binding,
  captureApi,
  captureModeApi,
  CaptureMode,
  CaptureStatus,
  hotkeyApi,
} from "../lib/tauri";

type CaptureState = {
  status: CaptureStatus;
  /** Polling timer handle for jsdom-friendly teardown in tests. */
  pollHandle: ReturnType<typeof setInterval> | null;

  captureMode: CaptureMode;
  hotkeyBinding: Binding | null;

  refresh: () => Promise<void>;
  startCapture: () => Promise<void>;
  stopCapture: () => Promise<void>;
  startPolling: () => void;
  stopPolling: () => void;

  loadMode: () => Promise<void>;
  setMode: (m: CaptureMode) => Promise<void>;

  loadBinding: () => Promise<void>;
  setBinding: (b: Binding) => Promise<void>;
  clearBinding: () => Promise<void>;
  captureNextBinding: () => Promise<Binding>;
};

const POLL_INTERVAL_MS = 1000;

export const useCaptureStore = create<CaptureState>((set, get) => ({
  status: { state: "idle" },
  pollHandle: null,

  captureMode: "always_on",
  hotkeyBinding: null,

  async refresh() {
    try {
      const status = await captureApi.status();
      set({ status });
    } catch (e) {
      set({ status: { state: "error", message: String(e) } });
    }
  },

  async startCapture() {
    await captureApi.start();
    await get().refresh();
  },

  async stopCapture() {
    await captureApi.stop();
    await get().refresh();
  },

  startPolling() {
    if (get().pollHandle !== null) return;
    const handle = setInterval(() => {
      void get().refresh();
    }, POLL_INTERVAL_MS);
    set({ pollHandle: handle });
  },

  stopPolling() {
    const h = get().pollHandle;
    if (h !== null) {
      clearInterval(h);
      set({ pollHandle: null });
    }
  },

  async loadMode() {
    const m = await captureModeApi.get();
    set({ captureMode: m });
  },
  async setMode(m) {
    await captureModeApi.set(m);
    set({ captureMode: m });
  },

  async loadBinding() {
    const b = await hotkeyApi.get();
    set({ hotkeyBinding: b });
  },
  async setBinding(b) {
    await hotkeyApi.set(b);
    set({ hotkeyBinding: b });
  },
  async clearBinding() {
    await hotkeyApi.clear();
    set({ hotkeyBinding: null });
  },
  async captureNextBinding() {
    const b = await hotkeyApi.captureNext();
    await hotkeyApi.set(b);
    set({ hotkeyBinding: b });
    return b;
  },
}));
