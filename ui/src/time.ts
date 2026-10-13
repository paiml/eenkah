// Human-friendly dates: "just now", "12 min ago", "3 hours ago", "1 day ago", "2 weeks ago",
// "4 months ago", "tomorrow". The same words as the Rust side (eenkah_core::when), so the
// sentences the server writes read the same way. The exact date goes in a tooltip.

const plural = (n: number, unit: string) => `${n} ${unit}${n === 1 ? "" : "s"}`;

/** Local calendar day of a Date, as days since the epoch (time-zone safe). */
const dayNumber = (d: Date) => Math.round(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()) / 86400000);

/** Parse "YYYY-MM-DD" or "YYYY-MM-DD HH:MM" as a local date-time. */
export function parseLocal(s: string): Date {
  const [day = "", time = "00:00"] = s.split(/[ T]/);
  const [y = 0, m = 1, d = 1] = day.split("-").map(Number);
  const [hh = 0, mm = 0] = time.split(":").map(Number);
  return new Date(y, m - 1, d, hh, mm);
}

/** Whole calendar days from `day` to today (negative for future days). */
export function daysAgo(day: string, today = new Date()): number {
  return dayNumber(today) - dayNumber(parseLocal(day));
}

/** A count of days as words: "today", "1 day ago", "3 weeks ago", "tomorrow", "in 3 days". */
export function dayWords(n: number): string {
  if (n === 0) return "today";
  if (n === -1) return "tomorrow";
  if (n < 0) return `in ${plural(-n, "day")}`;
  if (n < 7) return `${plural(n, "day")} ago`;
  if (n < 30) return `${plural(Math.floor(n / 7), "week")} ago`;
  if (n < 365) return `${plural(Math.max(1, Math.floor(n / 30.44)), "month")} ago`;
  return `${plural(Math.floor(n / 365.25), "year")} ago`;
}

/** A calendar day ("YYYY-MM-DD") relative to today. */
export const relDay = (day: string, today = new Date()) => dayWords(daysAgo(day, today));

/** A Unix time (seconds) relative to now: "just now", "12 min ago", "3 hours ago", then days. */
export function ago(unixSecs: number, now = Date.now() / 1000): string {
  const s = now - unixSecs;
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${plural(Math.floor(s / 3600), "hour")} ago`;
  return dayWords(Math.max(1, dayNumber(new Date(now * 1000)) - dayNumber(new Date(unixSecs * 1000))));
}

/** A local date-time string ("YYYY-MM-DD HH:MM") relative to now. */
export const agoLocal = (s: string) => ago(parseLocal(s).getTime() / 1000);

/** The exact date for a tooltip, e.g. "Mon 12 Oct 2026" or "Mon 12 Oct 2026, 09:17". */
export function exact(d: Date, withTime = false): string {
  return d.toLocaleString(undefined, { weekday: "short", day: "numeric", month: "short", year: "numeric", ...(withTime ? { hour: "2-digit", minute: "2-digit" } : {}) });
}

/** A short calendar date, e.g. "12 Oct" (this year) or "12 Oct 2025". */
export function short(day: string, today = new Date()): string {
  const d = parseLocal(day);
  return d.toLocaleDateString(undefined, { day: "numeric", month: "short", ...(d.getFullYear() === today.getFullYear() ? {} : { year: "numeric" }) });
}
