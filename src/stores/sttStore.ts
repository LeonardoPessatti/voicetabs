import { create } from "zustand";

import { sttApi, SttStatus } from "../lib/tauri";

type SttState = {
  status: SttStatus;
  pollHandle: ReturnType<typeof setInterval> | null;

  refresh: () => Promise<void>;
  startPolling: () => void;
  stopPolling: () => void;
};

const POLL_INTERVAL_MS = 1500;

export const useSttStore = create<SttState>((set, get) => ({
  status: { state: "loading", backend: "unknown" },
  pollHandle: null,

  async refresh() {
    try {
      const status = await sttApi.status();
      set({ status });
    } catch (e) {
      set({ status: { state: "error", message: String(e) } });
    }
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
