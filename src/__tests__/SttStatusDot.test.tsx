import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { SttStatusDot } from "../components/SttStatusDot";
import { SttStatus } from "../lib/tauri";

describe("SttStatusDot", () => {
  it("renders a green dot when ready", () => {
    const status: SttStatus = { state: "ready", backend: "cpu", model_id: "x" };
    render(<SttStatusDot status={status} />);
    const dot = screen.getByTestId("stt-dot");
    expect(dot).toHaveClass("stt-dot--ready");
  });

  it("renders a yellow dot when loading", () => {
    const status: SttStatus = { state: "loading", backend: "cpu" };
    render(<SttStatusDot status={status} />);
    expect(screen.getByTestId("stt-dot")).toHaveClass("stt-dot--loading");
  });

  it("renders a yellow dot when restarting", () => {
    const status: SttStatus = { state: "restarting", backend: "cpu" };
    render(<SttStatusDot status={status} />);
    expect(screen.getByTestId("stt-dot")).toHaveClass("stt-dot--restarting");
  });

  it("renders a red dot when error", () => {
    const status: SttStatus = { state: "error", message: "boom" };
    render(<SttStatusDot status={status} />);
    expect(screen.getByTestId("stt-dot")).toHaveClass("stt-dot--error");
  });

  it("exposes the status text via title attribute for tooltip", () => {
    const status: SttStatus = { state: "ready", backend: "cpu", model_id: "ggml-small-q5_1" };
    render(<SttStatusDot status={status} />);
    const dot = screen.getByTestId("stt-dot");
    expect(dot).toHaveAttribute("title");
    // backend "cpu" → pretty-printed as "Local CPU"
    expect(dot.getAttribute("title")).toMatch(/Local CPU/);
    expect(dot.getAttribute("title")).toMatch(/ggml-small-q5_1/);
  });
});
