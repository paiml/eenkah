import { Panel, type PanelSpec } from "suyo-ui";
import { useState } from "react";
import { api, type ActivityDetail, type ActivityRow, type SessionLabel, type StreamName } from "../api";
import { SESSION_LABELS, sourceWord } from "../components/Personal";
import { fmt, useLoad } from "../hooks";
import { exact, parseLocal } from "../time";
import { AgoLocal } from "../components/When";
import { Link } from "../router";
import { kindOf } from "./Activities";

const STREAMS: Record<StreamName, string> = {
  hr: 'alias(eenkah.stream.hr, "Heart rate")',
  speed: 'alias(eenkah.stream.speed, "Speed")',
  altitude: 'alias(eenkah.stream.altitude, "Altitude")',
  power: 'alias(eenkah.stream.power, "Power")',
  cadence: 'alias(eenkah.stream.cadence, "Cadence")',
};
const ORDER: StreamName[] = ["hr", "speed", "power", "cadence", "altitude"];
const ZONES = ["Z1 easy", "Z2 endurance", "Z3 tempo", "Z4 threshold", "Z5 above"];

/** Time in each heart-rate zone as one stacked bar, with the minutes written out. */
function Zones({ d }: { d: ActivityDetail }) {
  const mins = d.zone_minutes.map((m) => m ?? 0);
  const total = mins.reduce((a, b) => a + b, 0);
  if (!total) return <p className="muted small">No heart-rate zones for this session.</p>;
  return (
    <div>
      <div className="zone-bar" role="img" aria-label={ZONES.map((z, i) => `${z}: ${fmt(mins[i], 0)} min`).join(", ")}>
        {mins.map((m, i) => (m > 0 ? <span key={i} className={`zone z${i + 1}`} style={{ flexGrow: m }} title={`${ZONES[i]}: ${fmt(m, 0)} min`} /> : null))}
      </div>
      <div className="zone-legend">
        {mins.map((m, i) => (
          <span key={i}>
            <span className={`key zone z${i + 1}`} /> {ZONES[i]} <strong>{fmt(m, 0)}</strong> min
          </span>
        ))}
      </div>
      {d.thresholds.lthr != null && <p className="small muted">Zones from threshold heart rate {fmt(d.thresholds.lthr, 0)} bpm (Friel). Change it with <code>eenkah threshold lthr</code>.</p>}
    </div>
  );
}

/** Model 6: the session's type, and the athlete's correction (which always wins and teaches the model). */
function SessionType({ a, onChange }: { a: ActivityRow; onChange: (d: ActivityDetail) => void }) {
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const set = (label: SessionLabel | null) => {
    setBusy(true);
    setErr(null);
    api.setSessionType(a.id, label).then(
      (d) => {
        setBusy(false);
        onChange(d);
      },
      (e: unknown) => {
        setBusy(false);
        setErr(e instanceof Error ? e.message : String(e));
      },
    );
  };
  return (
    <div className="row gap wrap">
      {a.session_type ? (
        <span className={"type-badge" + (a.session_type_source === "you" ? " you" : "")}>{a.session_type}</span>
      ) : (
        <span className="muted small">Not classified (needs heart-rate zones and 30 sessions of this sport)</span>
      )}
      {a.session_type_source && <span className="small muted">{sourceWord[a.session_type_source]}</span>}
      <label className="row gap small">
        <span className="muted">Correct it:</span>
        <select className="select" disabled={busy} value={a.session_type_source === "you" ? (a.session_type ?? "") : ""} onChange={(e) => set(e.target.value ? (e.target.value as SessionLabel) : null)}>
          <option value="">{a.session_type_source === "you" ? "Remove my label" : "Choose…"}</option>
          {SESSION_LABELS.map((l) => (
            <option key={l} value={l}>
              {l}
            </option>
          ))}
        </select>
      </label>
      {err && <span className="error small">{err}</span>}
    </div>
  );
}

export function ActivityPage({ id }: { id: number }) {
  const [res] = useLoad(() => api.activity(id), [id]);
  const [edited, setEdited] = useState<ActivityDetail | null>(null);
  if (res.state === "loading") return <main className="page muted">Loading…</main>;
  if (res.state === "error") return <main className="page error">{res.error}</main>;
  const d = edited?.activity.id === id ? edited : res.data;
  const a = d.activity;
  const streams = ORDER.filter((s) => d.streams.includes(s));
  const spec: PanelSpec = { chart: "streams", title: "Streams", h: Math.max(4, streams.length * 2 + 1), targets: streams.map((s) => STREAMS[s]) };
  // exact instants: the session's own UTC offset may differ from the athlete's time zone
  const range = a.end_utc > a.start_utc ? { from: String(a.start_utc), until: String(a.end_utc + 60) } : undefined;
  return (
    <main className="page">
      <div className="page-head">
        <div>
          <p className="small">
            <Link to="/activities">← All sessions</Link>
          </p>
          <h1>{a.name ?? kindOf(a)}</h1>
          <p className="muted">
            {kindOf(a)}
            {a.start_local && (
              <>
                {" "}
                · <AgoLocal s={a.start_local} /> · {exact(parseLocal(a.start_local), true)}
              </>
            )}
          </p>
          <SessionType a={a} onChange={setEdited} />
        </div>
      </div>
      <div className="stats-row">
        {[
          ["Moving", fmt(a.moving_min, 0, "min")],
          ["Distance", fmt(a.distance_km, 1, "km")],
          ["Climb", fmt(a.ascent_m, 0, "m")],
          ["Avg / max HR", `${fmt(a.avg_hr, 0)} / ${fmt(a.max_hr, 0)} bpm`],
          ["Load", fmt(d.load, 0)],
          ["Decoupling", fmt(d.decoupling_pct, 1, "%")],
        ].map(([k, v]) => (
          <section className="card stat-card" key={k}>
            <div className="panel-title">{k}</div>
            <div className="stat-value small-value">{v}</div>
          </section>
        ))}
      </div>
      <section className="card panel">
        <div className="panel-title">Where did the time go?</div>
        <Zones d={d} />
      </section>
      {range && streams.length > 0 ? (
        <div className="streams-wrap">
          <Panel spec={spec} range={range} height={streams.length * 110 + 30} />
        </div>
      ) : (
        <p className="muted">This session has no recorded streams.</p>
      )}
    </main>
  );
}
