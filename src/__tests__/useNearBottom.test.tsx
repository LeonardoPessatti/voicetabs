import { renderHook, act } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { useNearBottom } from "../hooks/useNearBottom";

function makeContainer(scrollTop: number, scrollHeight: number, clientHeight: number) {
  const div = document.createElement("div");
  Object.defineProperty(div, "scrollTop",    { value: scrollTop,    writable: true });
  Object.defineProperty(div, "scrollHeight", { value: scrollHeight, writable: true });
  Object.defineProperty(div, "clientHeight", { value: clientHeight, writable: true });
  return div;
}

describe("useNearBottom", () => {
  it("reports near-bottom when distance to bottom is under threshold", () => {
    const { result } = renderHook(() => useNearBottom(80));
    const div = makeContainer(900, 1000, 100); // bottom: 1000, viewport bottom at 1000, dist 0
    act(() => result.current[0](div));
    act(() => { div.dispatchEvent(new Event("scroll")); });
    expect(result.current[1]).toBe(true);
  });

  it("reports not-near-bottom when scrolled up beyond the threshold", () => {
    const { result } = renderHook(() => useNearBottom(80));
    const div = makeContainer(0, 1000, 100); // dist = 900
    act(() => result.current[0](div));
    act(() => { div.dispatchEvent(new Event("scroll")); });
    expect(result.current[1]).toBe(false);
  });

  it("re-evaluates when scrollTop changes via subsequent scroll event", () => {
    const { result } = renderHook(() => useNearBottom(80));
    const div = makeContainer(0, 1000, 100);
    act(() => result.current[0](div));
    act(() => { div.dispatchEvent(new Event("scroll")); });
    expect(result.current[1]).toBe(false);
    (div as unknown as { scrollTop: number }).scrollTop = 920;
    act(() => { div.dispatchEvent(new Event("scroll")); });
    expect(result.current[1]).toBe(true);
  });
});
