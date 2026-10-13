import { api, type Family, type ForecastTarget, type Personal, type SportReport } from "../api";
import { RetrainCard } from "../components/Retrain";
import { fmt, useLoad } from "../hooks";
import { Day } from "../components/When";
import { DashboardPage } from "./DashboardPage";

const FAMILIES: { f: Family; label: string; learned: boolean }[] = [
  { f: "Persistence", label: "Same as today", learned: false },
  { f: "Mean7", label: "Your 7-day average", learned: false },
  { f: "Mean60", label: "Your 60-day average", learned: false },
  { f: "Ridge", label: "Linear model (aprender Ridge)", learned: true },
  { f: "Forest", label: "Random forest (aprender)", learned: true },
];

/** FC-2/FC-3 made visible: every family's backtest error, the winner, and interval coverage. */
function TrackRecord({ t }: { t: ForecastTarget }) {
  if (!t.champion || !t.backtest_mae) return <section className="card panel">{t.label}: not trained yet.</section>;
  const errs = FAMILIES.map((x) => t.backtest_mae?.[x.f] ?? null);
  const best = Math.min(...errs.filter((e): e is number => e != null));
  const worst = Math.max(...errs.filter((e): e is number => e != null));
  const unit = t.unit === "ms" ? "ln ms" : t.unit;
  return (
    <section className="card panel">
      <div className="panel-title">{t.label}</div>
      <p className="panel-sub">
        Mean absolute error in a walk-forward backtest over {t.rows} days ({unit}); lower is better. A learned model is used only if it beats every baseline.
      </p>
      <table className="data-table compact">
        <tbody>
          {FAMILIES.map((x, i) => {
            const e = errs[i] ?? null;
            const w = e == null || worst === 0 ? 0 : (e / worst) * 100;
            return (
              <tr key={x.f} className={x.f === t.champion ? "strong" : ""}>
                <td>
                  {x.label}
                  {x.f === t.champion && <span className="badge good">in use</span>}
                </td>
                <td className="bar-cell">
                  <span className={"hbar" + (x.learned ? " learned" : "")} style={{ width: `${w}%` }} />
                </td>
                <td className="num">{e == null ? "—" : e.toFixed(3)}</td>
                <td className="num muted">{e != null && e === best ? "best" : e != null ? `+${(((e - best) / best) * 100).toFixed(1)} %` : ""}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
      <p className="small">
        {t.backtest_coverage != null && (
          <>
            <strong>{fmt(t.backtest_coverage * 100, 0)} %</strong> of outcomes fell inside the stated 80 % interval.{" "}
          </>
        )}
        {t.live_scored > 0 && (
          <>
            Live forecasts so far: {t.live_inside_interval} of {t.live_scored} inside.{" "}
          </>
        )}
        <span className="muted">
          Trained in {fmt(t.train_secs, 1)} s{t.peak_memory_kb ? `, peak memory ${fmt(t.peak_memory_kb / 1024, 0)} MB` : ""}, CPU only.
        </span>
      </p>
      {t.lineage && (
        <p className="small muted">
          No model file is kept: it is rebuilt on every import from your data (<Day day={t.lineage.first_day} /> to <Day day={t.lineage.last_day} />, data <code>{t.lineage.data_hash.slice(0, 12)}</code>, seed {t.lineage.seed}, aprender {t.lineage.aprender}).
        </p>
      )}
    </section>
  );
}

const pct = (v: unknown) => (typeof v === "number" ? `${fmt(v * 100, 0)} %` : "—");

function timing(p: Personal, k: keyof Personal["runs"]): string {
  const r = p.runs[k];
  return r ? `Rebuilt in ${fmt(r.secs * 1000, 0)} ms on CPU (aprender ${r.method}).` : "Not built yet: run an import.";
}

/** The personal models' own track records: coverage, flag rate, shifts, session groups. */
function PersonalModels({ p }: { p: Personal }) {
  const normal = p.runs.normal;
  const inside = (normal?.scores as { inside_range?: Record<string, number> } | undefined)?.inside_range ?? {};
  const unusual = (p.runs.unusual?.scores ?? {}) as { flag_rate?: number; flagged?: number; sick_episodes?: number; sick_preceded?: number };
  const sports = (Array.isArray(p.runs.sessions?.scores) ? p.runs.sessions?.scores : []) as unknown as SportReport[];
  return (
    <div className="two-col">
      <section className="card panel">
        <div className="panel-title">Your normal ranges</div>
        <p className="panel-sub">Each range should hold 8 in 10 nights. This is the share that actually landed inside, over every night with a range.</p>
        <table className="data-table compact">
          <tbody>
            {p.signals.map((s) => (
              <tr key={s.signal}>
                <td>{s.label}</td>
                <td className="num">{pct(inside[s.signal])}</td>
              </tr>
            ))}
          </tbody>
        </table>
        <p className="small">
          All signals: <strong>{pct(normal?.coverage)}</strong> inside the stated 80 %. <span className="muted">{timing(p, "normal")}</span>
        </p>
        <p className="small">
          Shifts found: <strong>{p.shifts.length}</strong>. <span className="muted">{timing(p, "shift")}</span>
        </p>
      </section>
      <section className="card panel">
        <div className="panel-title">Unusual nights</div>
        <p className="panel-sub">An isolation forest trained only on earlier nights, retrained weekly, with a local outlier factor as a second opinion. About 1 night in 20 should be unusual.</p>
        <p>
          <strong>{fmt(unusual.flagged, 0)}</strong> unusual nights, {pct(unusual.flag_rate)} of the nights scored.
        </p>
        <p className="small">
          {unusual.sick_episodes
            ? `${unusual.sick_preceded} of ${unusual.sick_episodes} periods you marked as sick had an unusual night in the 3 days before or on their first day.`
            : "Mark days as sick (Data page or `eenkah mark`) to see how often an unusual night came first."}
        </p>
        <p className="small muted">{timing(p, "unusual")}</p>
      </section>
      {sports.map((r) => (
        <section className="card panel" key={r.sport}>
          <div className="panel-title">Session types: {r.sport.replace(/_/g, " ")}</div>
          <p className="panel-sub">
            {r.groups > 0
              ? `${r.sessions} sessions grouped by heart-rate shape with k-means; ${r.groups} groups chose themselves by silhouette (${fmt(r.silhouette, 2)}). Each group is named from its average session.`
              : `${r.sessions} sessions with heart-rate zones; grouping starts at 30.`}
          </p>
          {r.groups > 0 && (
            <div className="table-scroll">
              <table className="data-table compact">
                <thead>
                  <tr>
                    <th>Group</th>
                    <th className="num">Z1–2</th>
                    <th className="num">Z3</th>
                    <th className="num">Z4–5</th>
                    <th className="num">Hours</th>
                    <th className="num">Load/h</th>
                    <th className="num">Efforts/h</th>
                  </tr>
                </thead>
                <tbody>
                  {r.group_means.map((g, i) => {
                    const m = (j: number) => g[j] ?? 0;
                    return (
                    <tr key={i}>
                      <td>{r.group_names[i]}</td>
                      <td className="num">{pct(m(0) + m(1))}</td>
                      <td className="num">{pct(m(2))}</td>
                      <td className="num">{pct(m(3) + m(4))}</td>
                      <td className="num">{fmt(m(5), 1)}</td>
                      <td className="num">{fmt(m(6), 0)}</td>
                      <td className="num">{fmt(m(7), 1)}</td>
                    </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}
          <p className="small">
            {r.corrections} of your corrections.{" "}
            {r.forest_accuracy != null
              ? `A random forest trained on them matched ${pct(r.forest_accuracy)} in cross-validation against ${pct(r.groups_accuracy)} for the group names, so ${r.forest_used ? "the forest labels the rest" : "the group names are kept"}.`
              : "With 15 or more, a random forest learns your labels and is used if it matches them better than the group names."}
          </p>
        </section>
      ))}
    </div>
  );
}

export function ModelPage() {
  const [fc, reloadFc] = useLoad(api.forecast);
  const [personal, reloadPersonal] = useLoad(api.personal);
  const intro = (
    <>
      <RetrainCard
        onDone={() => {
          reloadFc();
          reloadPersonal();
        }}
      />
      {fc.state === "error" && <p className="error">{fc.error}</p>}
      {fc.state === "ok" && (
        <div className="two-col">
          {fc.data.map((t) => (
            <TrackRecord key={t.target} t={t} />
          ))}
        </div>
      )}
      {personal.state === "error" && <p className="error">{personal.error}</p>}
      {personal.state === "ok" && <PersonalModels p={personal.data} />}
    </>
  );
  return <DashboardPage name="model" intro={intro} />;
}
