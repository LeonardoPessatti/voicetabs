import { create } from "zustand";

import { captureApi, CaptureStatus } from "../lib/tauri";

type CaptureState = {
  status: CaptureStatus;
  /** Polling timer handle for jsdom-friendly teardown in tests. */
  pollHandle: ReturnType<typeof setInterval> | null;

  refresh: () => Promise<void>;
  startCapture: () => Promise<void>;
  stopCapture: () => Promise<void>;
  startPolling: () => void;
  stopPolling: () => void;
};

const POLL_INTERVAL_MS = 1000;

export const useCaptureStore = create<CaptureState>((set, get) => ({
  status: { state: "idle" },
  pollHandle: null,

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
}));
