// Normalised chart model: plain arrays per series plus colours read from the theme tokens.

import type { OptionValue, PanelOptions, RenderResponse, SeriesStyle } from "../types";

export interface Tokens {
  surface: string;
  ink: string;
  ink2: string;
  muted: string;
  grid: string;
  axis: string;
  dim: string;
  good: string;
  bad: string;
  warn: string;
  /** Categorical palette --s1..--s8 */
  pal: string[];
  /** Sequential ramp --seq-0..--seq-5 */
  seq: string[];
}

export function readTokens(): Tokens {
  const cs = getComputedStyle(document.documentElement);
  const g = (k: string): string => cs.getPropertyValue(k).trim();
  return {
    surface: g("--surface"), ink: g("--ink"), ink2: g("--ink-2"), muted: g("--muted"),
    grid: g("--grid"), axis: g("--axis"), dim: g("--dim"), good: g("--good"), bad: g("--bad"), warn: g("--warn"),
    pal: [1, 2, 3, 4, 5, 6, 7, 8].map((i) => g(`--s${i}`)),
    seq: [0, 1, 2, 3, 4, 5].map((i) => g(`--seq-${i}`)),
  };
}

export interface MSeries {
  id: number;
  name: string;
  unit: string;
  style: SeriesStyle;
  ts: number[];
  v: (number | null)[];
  color: string;
  step: number;
  agg: string;
}

export interface Model {
  series: MSeries[];
  from: number;
  until: number;
  tz: string;
  step: number;
  stepLabel: string;
  errors: { target: string; error: string }[];
  T: Tokens;
}

/** Normalise a suyo response and assign colours in fixed order. */
export function model(data: RenderResponse, T: Tokens = readTokens()): Model {
  let slot = 0;
  const series = (data.series || []).map((s, i): MSeries => {
    const n = s.datapoints.length;
    const ts = new Array<number>(n);
    const v = new Array<number | null>(n);
    s.datapoints.forEach((p, j) => {
      v[j] = p[0];
      ts[j] = p[1];
    });
    const style = s.style || {};
    let color: string;
    if (style.kind === "threshold") color = style.color || T.ink2;
    else if ((style.emphasis ?? 0) < 0) color = T.dim;
    else if (style.color) color = style.color;
    else color = slot < 8 ? (T.pal[slot++] ?? T.dim) : T.dim;
    return { id: i, name: s.target, unit: s.unit || "", style, ts, v, color, step: s.stepSeconds || data.stepSeconds, agg: s.agg };
  });
  return { series, from: data.from, until: data.until, tz: data.tz, step: data.stepSeconds, stepLabel: data.step, errors: data.errors || [], T };
}

// ------------------------------------------------------------------ helpers

export const isRef = (s: MSeries): boolean => s.style.kind === "threshold";
export const sharedUnit = (list: MSeries[]): string => {
  const u = list.filter((s) => !isRef(s)).map((s) => s.unit);
  return u.length && u.every((x) => x === u[0]) ? (u[0] ?? "") : "";
};
export const lastIdx = (s: MSeries): number => {
  for (let i = s.v.length - 1; i >= 0; i--) if (s.v[i] != null) return i;
  return -1;
};
export const lastVal = (s: MSeries): number | null => {
  const i = lastIdx(s);
  return i < 0 ? null : (s.v[i] ?? null);
};
export const firstVal = (s: MSeries): number | null => {
  for (const x of s.v) if (x != null) return x;
  return null;
};
export const vals = (s: MSeries): number[] => s.v.filter((x): x is number => x != null);
export const mean = (a: number[]): number => a.reduce((x, y) => x + y, 0) / (a.length || 1);
export function median(a: number[]): number {
  const s = [...a].sort((x, y) => x - y);
  const n = s.length;
  if (!n) return NaN;
  return n % 2 ? (s[(n - 1) / 2] as number) : ((s[n / 2 - 1] as number) + (s[n / 2] as number)) / 2;
}
export const first = <T,>(a: T[]): T => a[0] as T;
export const last = <T,>(a: T[]): T => a[a.length - 1] as T;
export const f1 = (n: number): string => n.toFixed(1);

/** Short label for the span of the range ("7d", "24h"). */
export function spanLabel(m: Model): string {
  const d = (m.until - m.from) / 86400;
  if (d >= 360) return `${Math.round(d / 365)}y`;
  if (d >= 1) return `${Math.round(d)}d`;
  if (d * 24 >= 1) return `${Math.round(d * 24)}h`;
  return `${Math.round(d * 1440)}m`;
}

// ------------------------------------------------------------------ chart options

/** Options a chart reads, as stored in a panel's `options` map. */
export type ChartOpts = PanelOptions;

const present = (v: OptionValue | undefined): v is OptionValue => v != null && v !== "";
export const optStr = (o: ChartOpts, k: string): string | undefined => {
  const v = o[k];
  return present(v) ? String(v) : undefined;
};
export const optNum = (o: ChartOpts, k: string): number | undefined => {
  const v = o[k];
  return present(v) ? Number(v) : undefined;
};
export const optTruthy = (o: ChartOpts, k: string): boolean => Boolean(o[k]);

/** Rounded-at-the-data-end bar path (square at the baseline). */
export function barPath(x: number, y: number, w: number, hh: number, r: number, up: boolean): string {
  const X = f1(x), Y = f1(y);
  if (r <= 0.5) return `M${X},${Y}h${f1(w)}v${f1(hh)}h${f1(-w)}Z`;
  return up
    ? `M${X},${f1(y + hh)}V${f1(y + r)}Q${X},${Y} ${f1(x + r)},${Y}H${f1(x + w - r)}Q${f1(x + w)},${Y} ${f1(x + w)},${f1(y + r)}V${f1(y + hh)}Z`
    : `M${X},${Y}V${f1(y + hh - r)}Q${X},${f1(y + hh)} ${f1(x + r)},${f1(y + hh)}H${f1(x + w - r)}Q${f1(x + w)},${f1(y + hh)} ${f1(x + w)},${f1(y + hh - r)}V${Y}Z`;
}
