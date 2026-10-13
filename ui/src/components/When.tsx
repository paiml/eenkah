import { ago, agoLocal, exact, parseLocal, relDay } from "../time";

/** A calendar day ("YYYY-MM-DD") as "3 days ago", with the exact date on hover. */
export function Day({ day }: { day: string }) {
  return (
    <time dateTime={day} title={exact(parseLocal(day))}>
      {relDay(day)}
    </time>
  );
}

/** A Unix time (seconds) as "12 min ago", with the exact date and time on hover. */
export function Ago({ t }: { t: number }) {
  const d = new Date(t * 1000);
  return (
    <time dateTime={d.toISOString()} title={exact(d, true)}>
      {ago(t)}
    </time>
  );
}

/** A local date-time ("YYYY-MM-DD HH:MM") as "2 weeks ago", with the exact date and time on hover. */
export function AgoLocal({ s }: { s: string }) {
  return (
    <time dateTime={s.replace(" ", "T")} title={exact(parseLocal(s), true)}>
      {agoLocal(s)}
    </time>
  );
}
