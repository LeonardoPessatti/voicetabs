import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { CaptureToggle } from "../components/CaptureToggle";
import { CaptureStatus } from "../lib/tauri";

describe("CaptureToggle", () => {
  it("renders OFF state with the off label", () => {
    render(
      <CaptureToggle
        status={{ state: "idle" }}
        onStart={() => {}}
        onStop={() => {}}
      />,
    );
    expect(screen.getByRole("button")).toHaveTextContent(/capture: off/i);
  });

  it("renders ON state with the capturing label", () => {
    const status: CaptureStatus = { state: "capturing", device_name: "Mic" };
    render(
      <CaptureToggle status={status} onStart={() => {}} onStop={() => {}} />,
    );
    expect(screen.getByRole("button")).toHaveTextContent(/capturing/i);
  });

  it("calls onStart when clicked while idle", () => {
    const onStart = vi.fn();
    render(
      <CaptureToggle
        status={{ state: "idle" }}
        onStart={onStart}
        onStop={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("button"));
    expect(onStart).toHaveBeenCalled();
  });

  it("calls onStop when clicked while capturing", () => {
    const onStop = vi.fn();
    const status: CaptureStatus = { state: "capturing", device_name: null };
    render(
      <CaptureToggle status={status} onStart={() => {}} onStop={onStop} />,
    );
    fireEvent.click(screen.getByRole("button"));
    expect(onStop).toHaveBeenCalled();
  });

  it("shows the error message when state is error", () => {
    const status: CaptureStatus = { state: "error", message: "no mic" };
    render(
      <CaptureToggle status={status} onStart={() => {}} onStop={() => {}} />,
    );
    expect(screen.getByRole("button")).toHaveTextContent(/no mic/i);
  });
});
