// Number formatting and axis ticks.

const trim = (s: string): string => s.replace(/\.0+$/, "").replace(/(\.\d*?)0+$/, "$1");

/** Compact values for axis ticks. */
export function fmtVal(v: number | null | undefined): string {
  if (v == null || !isFinite(v)) return "—";
  const a = Math.abs(v);
  if (a >= 1e9) return trim((v / 1e9).toFixed(1)) + "B";
  if (a >= 1e6) return trim((v / 1e6).toFixed(1)) + "M";
  if (a >= 1e4) return trim((v / 1e3).toFixed(1)) + "K";
  if (a >= 1000) return Math.round(v).toLocaleString("en-US");
  if (a >= 100 || Number.isInteger(v)) return String(Math.round(v));
  if (a >= 1) return trim(v.toFixed(1));
  if (a === 0) return "0";
  return trim(v.toPrecision(2));
}

/** Values in tooltips/tables: more precision than axis ticks. */
export function fmtNum(v: number | null | undefined, decimals?: number): string {
  if (v == null || !isFinite(v)) return "—";
  if (decimals != null) return v.toLocaleString("en-US", { minimumFractionDigits: decimals, maximumFractionDigits: decimals });
  const a = Math.abs(v);
  if (a >= 1e6) return fmtVal(v);
  if (a >= 100) return Math.round(v).toLocaleString("en-US");
  if (a >= 10) return trim(v.toFixed(1));
  if (a >= 1) return trim(v.toFixed(2));
  return trim(v.toPrecision(3));
}

export const unitSuffix = (u: string): string => (u === "%" ? "%" : ` ${u}`);

export function niceTicks(lo: number, hi: number, n: number): number[] {
  if (!(hi > lo)) return [lo, lo + 1];
  const raw = (hi - lo) / Math.max(1, n);
  const mag = Math.pow(10, Math.floor(Math.log10(raw)));
  const step = [1, 2, 2.5, 5, 10].map((x) => x * mag).find((s) => s >= raw) ?? 10 * mag;
  const out: number[] = [];
  for (let t = Math.floor(lo / step) * step; t <= hi + step * 0.999; t += step) out.push(Math.abs(t) < step * 1e-9 ? 0 : +t.toPrecision(12));
  return out;
}

/** Rough width of a string in px at a font size (no DOM measuring). */
export const textWidth = (str: string, size = 12): number => String(str).length * size * 0.58;

export function fmtDuration(secIn: number): string {
  const sec = Math.round(secIn);
  const d = Math.floor(sec / 86400), hh = Math.floor((sec % 86400) / 3600), mm = Math.floor((sec % 3600) / 60), ss = sec % 60;
  if (d) return `${d}d ${hh}h`;
  if (hh) return `${hh}h ${String(mm).padStart(2, "0")}m`;
  return `${mm}:${String(ss).padStart(2, "0")}`;
}
