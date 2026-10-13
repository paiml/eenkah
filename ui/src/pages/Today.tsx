import { useState } from "react";
import { api, hasData, type ForecastTarget, type TodayOk } from "../api";
import { NormalCard, ShiftCard, UnusualCard } from "../components/Personal";
import { dayWords, exact, parseLocal, relDay } from "../time";
import { fmt, signed, useLoad } from "../hooks";
import { Link } from "../router";
import { DashboardPage } from "./DashboardPage";

function age(days: number): string {
  return days <= 0 ? "this morning" : dayWords(days);
}

/** INS-7: the sentence, and one click away, the findings it came from. */
function Brief({ t }: { t: TodayOk }) {
  const [open, setOpen] = useState(false);
  const stale = t.data_age_days > 1;
  return (
    <section className="card brief">
      <div className="brief-head">
        <h2>{t.brief}</h2>
        <span className={"badge" + (stale ? " warn" : "")} title={`Newest morning with body data: ${exact(parseLocal(t.day))}`}>
          Data from {age(t.data_age_days)}
        </span>
      </div>
      {stale && (
        <p className="warn-text">
          Data is too old to forecast tomorrow. <Link to="/data">Download fresh data</Link>.
        </p>
      )}
      {t.findings.length > 0 && (
        <button className="btn ghost small" onClick={() => setOpen(!open)} aria-expanded={open}>
          {open ? "Hide" : "Show"} the {t.findings.length} finding{t.findings.length > 1 ? "s" : ""} behind this
        </button>
      )}
      {open && (
        <table className="data-table compact">
          <thead>
            <tr>
              <th>Finding</th>
              <th className="num">Size</th>
              <th className="num">Span</th>
              <th>Series</th>
            </tr>
          </thead>
          <tbody>
            {t.findings.map((f) => (
              <tr key={f.subject + f.series}>
                <td>{f.sentence}</td>
                <td className="num">
                  {signed(f.size, 1)} {f.unit}
                </td>
                <td className="num">{f.span_days} d</td>
                <td>
                  <a href={`/suyo/?target=${encodeURIComponent(f.series)}&from=-90d`}>
                    <code>{f.series}</code>
                  </a>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <p className="small muted">{t.disclaimer}</p>
    </section>
  );
}

/** FC-1/3/5/6: tomorrow's forecast with its interval, its drivers, and how it was chosen. */
function ForecastCard({ f, stale }: { f: ForecastTarget; stale: boolean }) {
  const n = f.next;
  const learned = f.champion === "Ridge" || f.champion === "Forest";
  return (
    <section className="card forecast">
      <div className="panel-title">{f.label}</div>
      {stale || !n ? (
        <p className="muted">{stale ? "Data too old to forecast." : "No forecast yet: run an import."}</p>
      ) : (
        <>
          <div className="stat-value">
            {fmt(n.value, 0)}
            <span className="stat-unit"> {f.unit}</span>
          </div>
          {n.lo != null && n.hi != null && (
            <div className="small">
              likely {fmt(n.lo, 0)}–{fmt(n.hi, 0)} {f.unit} <span className="muted">(80 % interval)</span>
            </div>
          )}
          <div className="small muted" title={exact(parseLocal(n.for_day))}>
            for {relDay(n.for_day) === "tomorrow" ? "tomorrow morning" : relDay(n.for_day) === "today" ? "this morning" : `the morning of ${relDay(n.for_day)}`}
          </div>
          {n.drivers && n.drivers.length > 0 && (
            <ul className="drivers">
              {n.drivers.map((d) => (
                <li key={d}>{d}</li>
              ))}
            </ul>
          )}
        </>
      )}
      <div className="small muted">
        {learned ? "Learned model" : "Your own baseline"} · <Link to="/model">track record</Link>
      </div>
    </section>
  );
}

export function TodayPage() {
  const [today] = useLoad(api.today);
  const [fc] = useLoad(api.forecast);
  const [personal] = useLoad(api.personal);
  let text = null;
  if (today.state === "error") text = <p className="error">{today.error}</p>;
  if (today.state === "ok") {
    const t = today.data;
    if (!hasData(t)) {
      text = (
        <section className="card brief">
          <h2>No data yet</h2>
          <p>
            Sign in once with <code>eenkah login</code>, then <Link to="/data">download from Garmin Connect</Link>.
          </p>
        </section>
      );
    } else {
      text = (
        <>
          <div className="today-top">
            <Brief t={t} />
            {fc.state === "ok" && fc.data.map((f) => <ForecastCard key={f.target} f={f} stale={t.data_age_days > 1} />)}
          </div>
          {personal.state === "ok" && personal.data.signals.length > 0 && (
            <div className="today-normal">
              <NormalCard p={personal.data} />
              <div>
                <UnusualCard p={personal.data} />
                <ShiftCard p={personal.data} />
              </div>
            </div>
          )}
        </>
      );
    }
  }
  return <DashboardPage name="today" outro={text} />;
}
