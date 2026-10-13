import type { NormalSignal, Personal, SessionLabel, ShiftRow, UnusualDay } from "../api";
import { fmt } from "../hooks";
import { Day } from "./When";

/** Pill class for a signal against its normal: outside in the good direction is good, else a warning. */
function tone(s: NormalSignal): string {
  if (s.status === "within") return "pill";
  const up = s.status === "above";
  if (s.better === 0) return "pill pill-warn";
  return (s.better > 0) === up ? "pill pill-good" : "pill pill-warn";
}

const word = { below: "below normal", within: "normal", above: "above normal" } as const;

/** The normal range as a track, with last night's value as a marker. */
function RangeBar({ s }: { s: NormalSignal }) {
  const lo = Math.min(s.lo, s.value);
  const hi = Math.max(s.hi, s.value);
  const pad = (hi - lo) * 0.15 || 1;
  const a = lo - pad;
  const span = hi + pad - a;
  const pos = (v: number) => `${((v - a) / span) * 100}%`;
  return (
    <div className="range-track" aria-hidden>
      <span className="range-band" style={{ left: pos(s.lo), width: `${((s.hi - s.lo) / span) * 100}%` }} />
      <span className={"range-dot " + s.status} style={{ left: pos(s.value) }} />
    </div>
  );
}

/** Model 1: each signal from the newest night against the athlete's own 80 % range. */
export function NormalCard({ p }: { p: Personal }) {
  if (p.signals.length === 0) return null;
  return (
    <section className="card panel">
      <div className="panel-title">How did last night compare with your normal?</div>
      <p className="panel-sub">
        Your normal is the range 8 in 10 of your nights fall in, learned from your previous 60 nights (aprender <code>NormalInverseGamma</code>). It restarts after a shift.
      </p>
      <table className="data-table compact">
        <tbody>
          {p.signals.map((s) => (
            <tr key={s.signal}>
              <td>
                <a href={`/suyo/?target=${encodeURIComponent(s.series)}&target=${encodeURIComponent(`eenkah.normal.${s.signal}.lo`)}&target=${encodeURIComponent(`eenkah.normal.${s.signal}.hi`)}&from=-120d`}>{s.label}</a>
              </td>
              <td className="num strong">
                {fmt(s.value, s.digits)} <span className="muted small">{s.unit}</span>
              </td>
              <td className="range-cell">
                <RangeBar s={s} />
              </td>
              <td className="num muted small nowrap">
                {fmt(s.lo, s.digits)}–{fmt(s.hi, s.digits)}
              </td>
              <td>
                <span className={tone(s)}>{word[s.status]}</span>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="small muted">
        {p.day ? (
          <>
            Night ending <Day day={p.day} />.{" "}
          </>
        ) : null}
        {p.disclaimer}
      </p>
    </section>
  );
}

function drivers(u: UnusualDay): string {
  return u.drivers.length ? u.drivers.join("; ") : "several signals were away from your normal at once";
}

/** Model 2: was the newest night unusual, and the other unusual nights lately. */
export function UnusualCard({ p, days = 60 }: { p: Personal; days?: number }) {
  const latest = p.unusual.latest;
  const since = new Date(Date.now() - days * 86400e3).toISOString().slice(0, 10);
  const recent = p.unusual.recent.filter((u) => u.day >= since && u.day !== latest?.day);
  return (
    <section className={"card panel" + (latest?.unusual ? " unusual" : "")}>
      <div className="panel-title">Was last night unusual for you?</div>
      {latest == null ? (
        <p className="muted small">Needs about 80 nights with HRV and resting heart rate.</p>
      ) : latest.unusual ? (
        <>
          <p>
            <strong>Yes.</strong> {drivers(latest)}.
          </p>
          <p className="small muted">
            More unusual than {fmt(latest.pct * 100, 0)} % of your past nights{latest.agree ? "; a second model agrees" : ""}. This describes your data; it is not a diagnosis.
          </p>
        </>
      ) : (
        <p>
          No. Your signals moved together the way they usually do <span className="muted small">(more unusual than {fmt(latest.pct * 100, 0)} % of your past nights)</span>.
        </p>
      )}
      {recent.length > 0 && (
        <>
          <div className="small strong">Other unusual nights in the last {days} days</div>
          <ul className="drivers">
            {recent.map((u) => (
              <li key={u.day}>
                <span className="nowrap">
                  <Day day={u.day} />
                </span>
                : {drivers(u)}
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  );
}

function shiftText(s: ShiftRow): string {
  const unit = s.unit ? ` ${s.unit}` : "";
  return `${s.label} ${s.direction} from ${fmt(s.before, s.digits)} to ${fmt(s.after, s.digits)}${unit}`;
}

/** Model 3: recent shifts in a signal's normal level. */
export function ShiftCard({ p, days = 90 }: { p: Personal; days?: number }) {
  const since = new Date(Date.now() - days * 86400e3).toISOString().slice(0, 10);
  const recent = p.shifts.filter((s) => s.day >= since);
  return (
    <section className="card panel">
      <div className="panel-title">Has your normal moved?</div>
      {recent.length === 0 ? (
        <p className="muted small">No shift in the last {days} days. {p.shifts.length > 0 && `${p.shifts.length} earlier.`}</p>
      ) : (
        <ul className="drivers">
          {recent.map((s) => (
            <li key={s.signal + s.day}>
              {shiftText(s)}, <span className="muted">starting about <Day day={s.day} /></span>
            </li>
          ))}
        </ul>
      )}
      <p className="small muted">A shift is a two-week average that has moved by at least one standard deviation of the 60 nights before it (aprender drift detector).</p>
    </section>
  );
}

export const SESSION_LABELS: SessionLabel[] = ["Easy", "Steady", "Long", "Tempo", "Intervals"];

export const sourceWord = { you: "your label", learned: "learned from your labels", groups: "grouped by heart-rate shape" } as const;
