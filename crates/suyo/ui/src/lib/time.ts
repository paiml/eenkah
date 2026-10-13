// Time formatting and calendar-aligned ticks, always in the server's zone.

const OPTS: Intl.DateTimeFormatOptions = { hourCycle: "h23", year: "numeric", month: "numeric", day: "numeric", hour: "numeric", minute: "numeric", second: "numeric", weekday: "short" };
const fmtCache = new Map<string, Intl.DateTimeFormat>();
function partsFmt(tz: string): Intl.DateTimeFormat {
  let f = fmtCache.get(tz);
  if (!f) {
    try {
      f = new Intl.DateTimeFormat("en-US", { ...OPTS, timeZone: tz });
    } catch {
      f = new Intl.DateTimeFormat("en-US", OPTS);
    }
    fmtCache.set(tz, f);
  }
  return f;
}

const WD: Record<string, number> = { Sun: 0, Mon: 1, Tue: 2, Wed: 3, Thu: 4, Fri: 5, Sat: 6 };
export const MON = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"] as const;
const MONTH = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"] as const;
const DAY = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"] as const;

export interface TzParts {
  y: number;
  m: number;
  d: number;
  H: number;
  M: number;
  S: number;
  /** 0 = Sunday */
  wd: number;
  monthShort: string;
}

/** Calendar parts of epoch seconds `t` in zone `tz`. */
export function tzParts(t: number, tz: string): TzParts {
  const p: Record<string, string> = {};
  for (const x of partsFmt(tz).formatToParts(new Date(t * 1000))) p[x.type] = x.value;
  const m = Number(p.month);
  return { y: Number(p.year), m, d: Number(p.day), H: Number(p.hour) % 24, M: Number(p.minute), S: Number(p.second), wd: WD[p.weekday ?? "Sun"] ?? 0, monthShort: MON[m - 1] ?? "" };
}

/** Epoch seconds of a wall-clock time in `tz`. */
export function tzTime(y: number, mo: number, d: number, H: number, M: number, tz: string): number {
  const guess = Date.UTC(y, mo - 1, d, H, M) / 1000;
  const p = tzParts(guess, tz);
  const off = Date.UTC(p.y, p.m - 1, p.d, p.H, p.M) / 1000 - guess;
  let t = guess - off;
  const q = tzParts(t, tz);
  if (q.H !== H) t += (H - q.H) * 3600; // DST edge
  return t;
}

const pad2 = (n: number): string => String(n).padStart(2, "0");

export type TimeMode = "bar" | "tip";

export function fmtTime(t: number, tz: string, mode: TimeMode, step = 3600): string {
  const p = tzParts(t, tz);
  const hm = `${pad2(p.H)}:${pad2(p.M)}`;
  if (mode === "bar") {
    if (step >= 360 * 86400) return String(p.y);
    if (step >= 27 * 86400) return p.m === 1 ? `${p.monthShort} ’${String(p.y).slice(2)}` : p.monthShort;
    if (step >= 86400) return `${p.monthShort} ${p.d}`;
    return hm;
  }
  if (step >= 360 * 86400) return String(p.y);
  if (step >= 27 * 86400) return `${MONTH[p.m - 1] ?? ""} ${p.y}`;
  if (step >= 6 * 86400) return `Week of ${p.monthShort} ${p.d}, ${p.y}`;
  if (step >= 86400) return `${DAY[p.wd] ?? ""}, ${p.monthShort} ${p.d}, ${p.y}`;
  return `${DAY[p.wd] ?? ""}, ${p.monthShort} ${p.d} · ${hm}`;
}

type TickKind = "min" | "h" | "d" | "w" | "mon" | "q" | "half" | "y" | "y5";
const CANDIDATES: [number, TickKind][] = [
  [60, "min"], [300, "min"], [900, "min"], [1800, "min"], [3600, "h"], [10800, "h"], [21600, "h"], [43200, "h"], [86400, "d"], [2 * 86400, "d"],
  [7 * 86400, "w"], [14 * 86400, "w"], [30 * 86400, "mon"], [91 * 86400, "q"], [182 * 86400, "half"], [365 * 86400, "y"], [5 * 365 * 86400, "y5"],
];
const EVERY: Partial<Record<TickKind, number>> = { mon: 1, q: 3, half: 6, y: 12, y5: 60 };

/** Calendar-aligned axis ticks in `tz`: [[epoch, label], ...]. */
export function timeTicks(from: number, until: number, tz: string, maxTicks: number): [number, string][] {
  const span = until - from;
  const last = CANDIDATES[CANDIDATES.length - 1] as [number, TickKind];
  const [step, kind] = CANDIDATES.find(([s]) => span / s <= maxTicks) ?? last;
  const out: [number, string][] = [];
  const p0 = tzParts(from, tz);
  if (step < 86400) {
    const off = Date.UTC(p0.y, p0.m - 1, p0.d, p0.H, p0.M, p0.S) / 1000 - from;
    for (let t = Math.ceil((from + off) / step) * step - off; t <= until && out.length < 60; t += step) {
      const p = tzParts(t, tz);
      out.push([t, p.H === 0 && p.M === 0 ? `${p.monthShort} ${p.d}` : `${pad2(p.H)}:${pad2(p.M)}`]);
    }
  } else if (kind === "d" || kind === "w") {
    const n = step / 86400;
    let t = tzTime(p0.y, p0.m, p0.d, 0, 0, tz);
    if (t < from) t = tzTime(p0.y, p0.m, p0.d + 1, 0, 0, tz);
    if (kind === "w") {
      const p = tzParts(t, tz);
      const shift = (8 - p.wd) % 7;
      t = tzTime(p.y, p.m, p.d + shift, 0, 0, tz);
    }
    while (t <= until && out.length < 60) {
      const p = tzParts(t, tz);
      out.push([t, `${p.monthShort} ${p.d}`]);
      t = tzTime(p.y, p.m, p.d + n, 0, 0, tz);
    }
  } else {
    const every = EVERY[kind] ?? 12;
    let y = p0.y;
    let mo = p0.m + 1;
    mo = Math.ceil((mo - 1) / every) * every + 1;
    while (mo > 12) { mo -= 12; y++; }
    for (let i = 0; i < 80; i++) {
      const t = tzTime(y, mo, 1, 0, 0, tz);
      if (t > until) break;
      if (t >= from) out.push([t, every >= 12 || mo === 1 ? String(y) : (MON[mo - 1] ?? "")]);
      mo += every;
      while (mo > 12) { mo -= 12; y++; }
    }
  }
  return out;
}

/** Epoch seconds → absolute time string for zoomed ranges ("2026-01-31 08:00"). */
export function absTime(t: number, tz: string): string {
  const p = tzParts(t, tz);
  return `${p.y}-${pad2(p.m)}-${pad2(p.d)} ${pad2(p.H)}:${pad2(p.M)}`;
}

// ------------------------------------------------------------------ range presets

export const RANGES: [string, string, string][] = [
  ["Last 1 hour", "-1h", "now"], ["Last 6 hours", "-6h", "now"], ["Last 24 hours", "-24h", "now"], ["Last 3 days", "-3d", "now"],
  ["Last 7 days", "-7d", "now"], ["Last 30 days", "-30d", "now"], ["Last 90 days", "-90d", "now"], ["Last 6 months", "-6mon", "now"],
  ["Last year", "-1y", "now"], ["Last 2 years", "-2y", "now"], ["Last 5 years", "-5y", "now"],
  ["Today", "now/d", "now"], ["This week", "now/w", "now"], ["This month", "now/M", "now"], ["Last month", "now-1M/M", "now-1M/M"],
  ["This year", "now/y", "now"], ["Last year (calendar)", "now-1y/y", "now-1y/y"],
];

const EXTRA_RANGES: [string, string, string][] = [["This calendar year", "now/y", "now/y"], ["Last month → now", "now-1M/M", "now/M"], ["Last month → now", "now-1M/M", "now"]];
const REL_UNITS: Record<string, string> = { m: "minutes", min: "minutes", h: "hours", d: "days", w: "weeks", mon: "months", M: "months", y: "years" };

/** Human label for a from/until pair ("Last 7 days", "This month", "a → b"). */
export function rangeLabel(from: string, until?: string): string {
  const u = until || "now";
  const hit = [...RANGES, ...EXTRA_RANGES].find(([, f, uu]) => f === from && uu === u);
  if (hit) return hit[0];
  const rel = /^-(\d+)(m|min|h|d|w|mon|M|y)$/.exec(from || "");
  if (rel && u === "now") return `Last ${rel[1]} ${REL_UNITS[rel[2] ?? ""] ?? ""}`.replace(/^Last 1 (\w+)s$/, "Last $1");
  return `${from} → ${u}`;
}
