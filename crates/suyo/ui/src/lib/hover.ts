// Hover bus: every time chart publishes the hovered time so the others draw a crosshair there.

import { useCallback, useEffect, useRef } from "react";

export interface HoverEvent {
  /** Epoch seconds, or null when the pointer left the chart. */
  t: number | null;
  /** Identifies the publishing chart so it ignores its own events. */
  src: symbol;
}

type Listener = (e: HoverEvent) => void;
const listeners = new Set<Listener>();

export const hoverBus = {
  on(fn: Listener): () => void {
    listeners.add(fn);
    return () => {
      listeners.delete(fn);
    };
  },
  emit(e: HoverEvent): void {
    for (const fn of [...listeners]) fn(e);
  },
};

/**
 * Join the synced crosshair. `onOther` is called with the time hovered in any *other* chart
 * (null when it left). Returns `emit(t)` to publish this chart's hover time.
 */
export function useHover(onOther: (t: number | null) => void): (t: number | null) => void {
  const id = useRef<symbol>(Symbol("hover"));
  const cb = useRef(onOther);
  cb.current = onOther;
  useEffect(
    () =>
      hoverBus.on((e) => {
        if (e.src !== id.current) cb.current(e.t);
      }),
    [],
  );
  return useCallback((t: number | null) => hoverBus.emit({ t, src: id.current }), []);
}
