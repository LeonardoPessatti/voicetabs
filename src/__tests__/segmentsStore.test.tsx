import { describe, expect, it, vi, beforeEach } from "vitest";

let lastHandler: ((event: { payload: unknown }) => void) | null = null;

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string) => {
    if (command === "segments_list_for_tab") return [];
    return undefined;
  }),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (_name: string, handler: (e: { payload: unknown }) => void) => {
    lastHandler = handler;
    return () => {
      lastHandler = null;
    };
  }),
}));

// Import AFTER the mocks so the store sees the mocked modules.
const { useSegmentsStore } = await import("../stores/segmentsStore");

const segment = (id: number, tab_id: number, position: number) => ({
  id,
  tab_id,
  position,
  text: `seg ${id}`,
  original_text: `seg ${id}`,
  audio_path: `${id}.wav`,
  started_at: 0,
  ended_at: 100,
  duration_ms: 100,
  vocab_snapshot: "[]",
  avg_logprob: -0.3,
  no_speech_prob: 0.02,
  model_id: "test",
});

describe("useSegmentsStore segment-created listener", () => {
  beforeEach(() => {
    useSegmentsStore.setState({
      segmentsByTab: {},
      loading: {},
      unlistenSegmentCreated: null,
    });
    lastHandler = null;
  });

  it("appends a segment to the matching tab on segment-created", async () => {
    await useSegmentsStore.getState().startListening();
    expect(lastHandler).not.toBeNull();
    lastHandler!({ payload: segment(1, 7, 0) });
    expect(useSegmentsStore.getState().segmentsByTab[7]).toHaveLength(1);
    expect(useSegmentsStore.getState().segmentsByTab[7][0].id).toBe(1);
  });

  it("keeps segments sorted by position", async () => {
    await useSegmentsStore.getState().startListening();
    lastHandler!({ payload: segment(2, 7, 1) });
    lastHandler!({ payload: segment(1, 7, 0) });
    const ids = useSegmentsStore.getState().segmentsByTab[7].map((s) => s.id);
    expect(ids).toEqual([1, 2]);
  });

  it("does not affect other tabs", async () => {
    await useSegmentsStore.getState().startListening();
    lastHandler!({ payload: segment(1, 7, 0) });
    lastHandler!({ payload: segment(2, 8, 0) });
    expect(useSegmentsStore.getState().segmentsByTab[7]).toHaveLength(1);
    expect(useSegmentsStore.getState().segmentsByTab[8]).toHaveLength(1);
  });

  it("stopListening clears the unlisten handle", async () => {
    await useSegmentsStore.getState().startListening();
    useSegmentsStore.getState().stopListening();
    expect(useSegmentsStore.getState().unlistenSegmentCreated).toBeNull();
    expect(lastHandler).toBeNull();
  });
});
