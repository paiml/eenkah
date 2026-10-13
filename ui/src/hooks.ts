import { useCallback, useEffect, useState } from "react";

export type Load<T> = { state: "loading" } | { state: "error"; error: string } | { state: "ok"; data: T };

/** Run an async loader on mount (and when `deps` change); `reload` runs it again. */
export function useLoad<T>(loader: () => Promise<T>, deps: readonly unknown[] = []): [Load<T>, () => void] {
  const [s, set] = useState<Load<T>>({ state: "loading" });
  const [tick, setTick] = useState(0);
  useEffect(() => {
    let live = true;
    loader().then(
      (data) => live && set({ state: "ok", data }),
      (e: unknown) => live && set({ state: "error", error: e instanceof Error ? e.message : String(e) }),
    );
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tick, ...deps]);
  const reload = useCallback(() => setTick((t) => t + 1), []);
  return [s, reload];
}

export const fmt = (v: number | null | undefined, digits = 0, unit = ""): string =>
  v == null || !Number.isFinite(v) ? "—" : `${v.toLocaleString(undefined, { maximumFractionDigits: digits, minimumFractionDigits: digits })}${unit ? ` ${unit}` : ""}`;

export const signed = (v: number | null | undefined, digits = 1): string => (v == null ? "—" : `${v > 0 ? "+" : v < 0 ? "−" : ""}${Math.abs(v).toFixed(digits)}`);
