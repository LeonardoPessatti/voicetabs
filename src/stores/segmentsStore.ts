import { create } from "zustand";

import { listenSegmentCreated, Segment, segmentsApi } from "../lib/tauri";

type SegmentsState = {
  segmentsByTab: Record<number, Segment[]>;
  loading: Record<number, boolean>;
  unlistenSegmentCreated: (() => void) | null;

  loadForTab: (tabId: number) => Promise<void>;
  startListening: () => Promise<void>;
  stopListening: () => void;
  updateSegmentText: (id: number, text: string) => Promise<void>;
  deleteSegment: (id: number) => Promise<void>;
  /** After re-transcribe, the backend returns the new text + model id;
   *  this method patches the row in place. */
  applyRetranscribe: (
    id: number,
    next: { text: string; model_id: string; avg_logprob: number; no_speech_prob: number },
  ) => void;
};

export const useSegmentsStore = create<SegmentsState>((set, get) => ({
  segmentsByTab: {},
  loading: {},
  unlistenSegmentCreated: null,

  async loadForTab(tabId) {
    if (get().loading[tabId]) return;
    set((s) => ({ loading: { ...s.loading, [tabId]: true } }));
    try {
      const segments = await segmentsApi.listForTab(tabId);
      set((s) => ({
        segmentsByTab: { ...s.segmentsByTab, [tabId]: segments },
        loading: { ...s.loading, [tabId]: false },
      }));
    } catch (e) {
      console.error("segmentsApi.listForTab failed", e);
      set((s) => ({ loading: { ...s.loading, [tabId]: false } }));
    }
  },

  async startListening() {
    if (get().unlistenSegmentCreated) return;
    const unlisten = await listenSegmentCreated((segment) => {
      set((s) => {
        const existing = s.segmentsByTab[segment.tab_id] ?? [];
        // The backend always inserts with position = max+1, but we sort
        // defensively in case the listener fires while a manual reload is
        // in flight. Use `id` as tiebreaker.
        const next = [...existing, segment].sort(
          (a, b) => a.position - b.position || a.id - b.id,
        );
        return { segmentsByTab: { ...s.segmentsByTab, [segment.tab_id]: next } };
      });
    });
    set({ unlistenSegmentCreated: unlisten });
  },

  stopListening() {
    const u = get().unlistenSegmentCreated;
    if (u) {
      u();
      set({ unlistenSegmentCreated: null });
    }
  },

  async updateSegmentText(id, text) {
    await segmentsApi.update(id, text);
    set((s) => {
      const next = { ...s.segmentsByTab };
      for (const [tabIdStr, list] of Object.entries(next)) {
        const tabId = Number(tabIdStr);
        next[tabId] = list.map((seg) =>
          seg.id === id ? { ...seg, text } : seg,
        );
      }
      return { segmentsByTab: next };
    });
  },

  async deleteSegment(id) {
    await segmentsApi.delete(id);
    set((s) => {
      const next = { ...s.segmentsByTab };
      for (const [tabIdStr, list] of Object.entries(next)) {
        const tabId = Number(tabIdStr);
        next[tabId] = list.filter((seg) => seg.id !== id);
      }
      return { segmentsByTab: next };
    });
  },

  applyRetranscribe(id, next) {
    set((s) => {
      const updated = { ...s.segmentsByTab };
      for (const [tabIdStr, list] of Object.entries(updated)) {
        const tabId = Number(tabIdStr);
        updated[tabId] = list.map((seg) =>
          seg.id === id
            ? {
                ...seg,
                text: next.text,
                model_id: next.model_id,
                avg_logprob: next.avg_logprob,
                no_speech_prob: next.no_speech_prob,
              }
            : seg,
        );
      }
      return { segmentsByTab: updated };
    });
  },
}));
