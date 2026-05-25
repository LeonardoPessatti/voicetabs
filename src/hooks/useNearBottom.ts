import { useCallback, useEffect, useRef, useState } from "react";

/**
 * Tracks whether the attached scroll container is currently within
 * `threshold` pixels of its bottom edge.
 *
 * Usage:
 *   const [setRef, isNearBottom] = useNearBottom(80);
 *   <div ref={setRef}>…</div>
 *
 * We intentionally use a callback ref (not a `useRef<HTMLElement>`) so
 * React calls us when the element mounts/unmounts, and we can attach/
 * detach listeners exactly once. `ResizeObserver` covers the case where
 * the content height changes (e.g. an image inside a segment finishes
 * loading); we feature-check it because jsdom does not implement it.
 */
export function useNearBottom(
  threshold = 80,
): [(el: HTMLElement | null) => void, boolean] {
  const [near, setNear] = useState(true);
  const elRef = useRef<HTMLElement | null>(null);

  const compute = useCallback(() => {
    const el = elRef.current;
    if (!el) return;
    const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
    setNear(distance <= threshold);
  }, [threshold]);

  const setRef = useCallback(
    (el: HTMLElement | null) => {
      // Detach from previous.
      const prev = elRef.current;
      if (prev) {
        prev.removeEventListener("scroll", compute);
      }
      elRef.current = el;
      if (el) {
        el.addEventListener("scroll", compute, { passive: true });
        compute();
      }
    },
    [compute],
  );

  useEffect(() => {
    const el = elRef.current;
    if (!el) return;
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(() => compute());
    ro.observe(el);
    return () => ro.disconnect();
  }, [compute]);

  return [setRef, near];
}
