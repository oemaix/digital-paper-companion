/**
 * Minimal row virtualization (NFR-PRF-2): renders only the rows that
 * intersect the scroll viewport (plus overscan), so 10 000-entry listings
 * scroll without degradation. No library — rows just need known, fixed
 * heights (the library list uses two: group headers and entry rows).
 */
import { useEffect, useMemo, useRef, useState } from "react";

export interface VirtualRows {
  /** Attach to the scrollable container. */
  containerRef: React.RefObject<HTMLDivElement | null>;
  /** First and one-past-last visible row index. */
  start: number;
  end: number;
  /** `offsets[i]` is the top of row `i`; `offsets[count]` the total height. */
  offsets: number[];
  /** Total content height for the spacer element. */
  total: number;
  /** Current container client width (for column layouts). */
  width: number;
}

export function useVirtualRows(heights: number[], overscan = 10): VirtualRows {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [size, setSize] = useState({ w: 0, h: 0 });

  const offsets = useMemo(() => {
    const out = new Array<number>(heights.length + 1);
    out[0] = 0;
    for (let i = 0; i < heights.length; i++) out[i + 1] = out[i] + heights[i];
    return out;
  }, [heights]);
  const total = offsets[offsets.length - 1];

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const measure = () => setSize({ w: el.clientWidth, h: el.clientHeight });
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    const onScroll = () => setScrollTop(el.scrollTop);
    el.addEventListener("scroll", onScroll, { passive: true });
    return () => {
      ro.disconnect();
      el.removeEventListener("scroll", onScroll);
    };
  }, []);

  // When the content shrinks (navigation, search) the browser clamps
  // scrollTop without necessarily firing a scroll event — re-read it.
  useEffect(() => {
    const el = containerRef.current;
    if (el && el.scrollTop !== scrollTop) setScrollTop(el.scrollTop);
  }, [total]);

  // Binary search: last row starting at or above the viewport top.
  let lo = 0;
  let hi = heights.length;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (offsets[mid] <= scrollTop) lo = mid;
    else hi = mid - 1;
  }
  const start = Math.max(0, lo - overscan);
  let end = lo;
  const bottom = scrollTop + size.h;
  while (end < heights.length && offsets[end] < bottom) end++;
  end = Math.min(heights.length, end + overscan);

  return { containerRef, start, end, offsets, total, width: size.w };
}
