import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (p: string) => `asset://localhost/${encodeURIComponent(p)}`,
}));

vi.mock("../lib/clipboard", () => ({
  writeText: vi.fn(async () => {}),
}));

import { SegmentCard } from "../components/SegmentCard";
import { Segment } from "../lib/tauri";
import { writeText as clipboardWriteText } from "../lib/clipboard";

function mkSegment(text = "Hello world", id = 1): Segment {
  return {
    id,
    tab_id: 1,
    position: 0,
    text,
    original_text: text,
    audio_path: `${id}.wav`,
    started_at: 0,
    ended_at: 1_000,
    duration_ms: 1_000,
    vocab_snapshot: "[]",
    avg_logprob: -0.3,
    no_speech_prob: 0.02,
    model_id: "ggml-small-q5_0",
  };
}

describe("SegmentCard", () => {
  const noop = () => {};

  it("renders the segment text", () => {
    render(
      <SegmentCard
        segment={mkSegment("the rain in spain")}
        onEdit={noop}
        onDelete={noop}
        onRetranscribe={noop}
      />,
    );
    expect(screen.getByText("the rain in spain")).toBeInTheDocument();
  });

  it("exposes a Play button (audio replay)", () => {
    render(
      <SegmentCard
        segment={mkSegment()}
        onEdit={noop}
        onDelete={noop}
        onRetranscribe={noop}
      />,
    );
    expect(screen.getByRole("button", { name: /play|reproduzir/i })).toBeInTheDocument();
  });

  it("clicking Edit swaps in a textarea and Save calls onEdit", () => {
    const onEdit = vi.fn();
    render(
      <SegmentCard
        segment={mkSegment("orig")}
        onEdit={onEdit}
        onDelete={noop}
        onRetranscribe={noop}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /edit|editar/i }));
    const textarea = screen.getByRole("textbox");
    fireEvent.change(textarea, { target: { value: "edited" } });
    fireEvent.click(screen.getByRole("button", { name: /save|salvar/i }));
    expect(onEdit).toHaveBeenCalledWith(1, "edited");
  });

  it("Esc cancels edit without firing onEdit", () => {
    const onEdit = vi.fn();
    render(
      <SegmentCard
        segment={mkSegment("orig")}
        onEdit={onEdit}
        onDelete={noop}
        onRetranscribe={noop}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /edit|editar/i }));
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Escape" });
    expect(onEdit).not.toHaveBeenCalled();
    // Back to read mode — text is still visible as a paragraph.
    expect(screen.getByText("orig")).toBeInTheDocument();
  });

  it("calls onDelete without confirm for short text", () => {
    const onDelete = vi.fn();
    // Spy on confirm to make sure it's not called.
    const confirmSpy = vi.spyOn(window, "confirm");
    render(
      <SegmentCard
        segment={mkSegment("short")}
        onEdit={noop}
        onDelete={onDelete}
        onRetranscribe={noop}
      />,
    );
    // Open the overflow menu and click Delete.
    fireEvent.click(screen.getByRole("button", { name: /more|mais/i }));
    fireEvent.click(screen.getByRole("menuitem", { name: /delete|excluir/i }));
    expect(confirmSpy).not.toHaveBeenCalled();
    expect(onDelete).toHaveBeenCalledWith(1);
    confirmSpy.mockRestore();
  });

  it("confirms before deleting non-trivial content (>20 chars)", () => {
    const onDelete = vi.fn();
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(
      <SegmentCard
        segment={mkSegment("a much longer sentence that exceeds twenty chars by some margin")}
        onEdit={noop}
        onDelete={onDelete}
        onRetranscribe={noop}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /more|mais/i }));
    fireEvent.click(screen.getByRole("menuitem", { name: /delete|excluir/i }));
    expect(confirmSpy).toHaveBeenCalled();
    expect(onDelete).toHaveBeenCalledWith(1);
    confirmSpy.mockRestore();
  });

  it("does not delete if confirm is cancelled", () => {
    const onDelete = vi.fn();
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(
      <SegmentCard
        segment={mkSegment("a much longer sentence that exceeds twenty chars by some margin")}
        onEdit={noop}
        onDelete={onDelete}
        onRetranscribe={noop}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /more|mais/i }));
    fireEvent.click(screen.getByRole("menuitem", { name: /delete|excluir/i }));
    expect(onDelete).not.toHaveBeenCalled();
    confirmSpy.mockRestore();
  });

  it("overflow menu fires re-transcribe with the chosen mode", () => {
    const onRetranscribe = vi.fn();
    render(
      <SegmentCard
        segment={mkSegment()}
        onEdit={noop}
        onDelete={noop}
        onRetranscribe={onRetranscribe}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /more|mais/i }));
    fireEvent.click(screen.getByRole("menuitem", { name: /current vocab|vocabulário atual/i }));
    expect(onRetranscribe).toHaveBeenCalledWith(1, "current");

    fireEvent.click(screen.getByRole("button", { name: /more|mais/i }));
    fireEvent.click(screen.getByRole("menuitem", { name: /snapshot vocab|vocabulário original/i }));
    expect(onRetranscribe).toHaveBeenCalledWith(1, "snapshot");
  });

  it("renders the segment timestamp as HH:MM:SS when showTimestamps is true", () => {
    const seg = mkSegment("hi");
    seg.started_at = Date.UTC(2026, 4, 23, 14, 7, 42); // May = month 4
    render(
      <SegmentCard
        segment={seg}
        onEdit={noop}
        onDelete={noop}
        onRetranscribe={noop}
        showTimestamps
      />,
    );
    const tag = document.querySelector("time");
    expect(tag).not.toBeNull();
    expect(tag!.getAttribute("datetime")).toBe(new Date(seg.started_at).toISOString());
    expect(tag!.textContent).toMatch(/^\d{2}:\d{2}:\d{2}$/);
  });

  it("omits the timestamp when showTimestamps is false", () => {
    render(
      <SegmentCard
        segment={mkSegment("hi")}
        onEdit={noop}
        onDelete={noop}
        onRetranscribe={noop}
        showTimestamps={false}
      />,
    );
    expect(document.querySelector("time")).toBeNull();
  });

  it("defaults to NOT rendering the timestamp when prop is omitted", () => {
    render(
      <SegmentCard
        segment={mkSegment("hi")}
        onEdit={noop}
        onDelete={noop}
        onRetranscribe={noop}
      />,
    );
    expect(document.querySelector("time")).toBeNull();
  });

  it("Copy button writes segment.text to the clipboard", async () => {
    (clipboardWriteText as ReturnType<typeof vi.fn>).mockClear();
    render(
      <SegmentCard
        segment={mkSegment("hello clipboard")}
        onEdit={noop}
        onDelete={noop}
        onRetranscribe={noop}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /^copy segment$|^copiar segmento$/i }));
    expect(clipboardWriteText).toHaveBeenCalledWith("hello clipboard");
  });

  it("Copy button shows the 'Copied!' confirmation, then hides it", async () => {
    render(
      <SegmentCard
        segment={mkSegment("x")}
        onEdit={noop}
        onDelete={noop}
        onRetranscribe={noop}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /^copy segment$|^copiar segmento$/i }));
    // Wait for async clipboard.writeText to resolve and state to update.
    expect(await screen.findByText(/copied!|copiado!/i)).toBeInTheDocument();
    // The auto-hide setTimeout (1200 ms) fires under real timers.
    await new Promise((r) => setTimeout(r, 1300));
    expect(screen.queryByText(/copied!|copiado!/i)).toBeNull();
  });
});
