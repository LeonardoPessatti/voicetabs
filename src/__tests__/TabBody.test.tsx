import { fireEvent, render, screen, act } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => []),
  convertFileSrc: (p: string) => `asset://localhost/${encodeURIComponent(p)}`,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));
vi.mock("../lib/clipboard", () => ({
  writeText: vi.fn(async () => {}),
}));

// Import AFTER the mocks so the modules under test see them.
const { TabBody } = await import("../components/TabBody");
const { useSegmentsStore } = await import("../stores/segmentsStore");

function seg(id: number, text = `s${id}`) {
  return {
    id,
    tab_id: 1,
    position: id,
    text,
    original_text: text,
    audio_path: `${id}.wav`,
    started_at: id * 1_000,
    ended_at: id * 1_000 + 500,
    duration_ms: 500,
    vocab_snapshot: "[]",
    avg_logprob: -0.3,
    no_speech_prob: 0.02,
    model_id: "m",
  };
}

beforeEach(() => {
  useSegmentsStore.setState({
    segmentsByTab: {},
    loading: {},
    unlistenSegmentCreated: null,
  });
});

describe("TabBody", () => {
  it("renders the empty-state hint when not loading AND segments.length === 0", async () => {
    useSegmentsStore.setState({ segmentsByTab: { 1: [] }, loading: { 1: false } });
    render(<TabBody activeTabId={1} />);
    // loadForTab fires inside an effect and resolves asynchronously
    // (mocked invoke returns []); wait until the empty hint appears.
    expect(
      await screen.findByText(/comece a falar|start speaking/i),
    ).toBeInTheDocument();
  });

  it("does NOT render the empty-state hint while loading is true", async () => {
    useSegmentsStore.setState({ segmentsByTab: {}, loading: { 1: false } });
    render(<TabBody activeTabId={1} />);
    // After the initial loadForTab fires, force the store back to loading=true
    // to simulate the in-flight state we're asserting about.
    act(() => {
      useSegmentsStore.setState({ segmentsByTab: {}, loading: { 1: true } });
    });
    expect(screen.queryByText(/comece a falar|start speaking/i)).toBeNull();
  });

  it("renders the Copy tab button and writes joined segments to clipboard", async () => {
    const { writeText } = await import("../lib/clipboard");
    useSegmentsStore.setState({
      segmentsByTab: { 1: [seg(1, "first"), seg(2, "second")] },
      loading: { 1: false },
    });
    render(<TabBody activeTabId={1} />);
    fireEvent.click(
      screen.getByRole("button", { name: /copy tab|copiar aba/i }),
    );
    expect(writeText).toHaveBeenCalledWith("first\n\nsecond");
  });

  it("shows the 'new transcription' pill when scrolled up and a segment arrives", async () => {
    useSegmentsStore.setState({
      segmentsByTab: { 1: [seg(1)] },
      loading: { 1: false },
    });
    const { rerender } = render(<TabBody activeTabId={1} />);
    // Force the scroll container into "not near bottom" state.
    const list = document.querySelector(".tab-body__list") as HTMLDivElement;
    Object.defineProperty(list, "scrollTop", { value: 0, writable: true });
    Object.defineProperty(list, "scrollHeight", { value: 5000, writable: true });
    Object.defineProperty(list, "clientHeight", { value: 200, writable: true });
    act(() => {
      list.dispatchEvent(new Event("scroll"));
    });
    // New segment lands.
    act(() => {
      useSegmentsStore.setState({
        segmentsByTab: { 1: [seg(1), seg(2)] },
        loading: { 1: false },
      });
    });
    rerender(<TabBody activeTabId={1} />);
    expect(
      screen.getByRole("button", {
        name: /new transcription|nova transcrição/i,
      }),
    ).toBeInTheDocument();
  });
});
