import { useState } from "react";
import { api, type RetrainResult } from "../api";
import { fmt, useLoad } from "../hooks";
import { Ago } from "./When";

/**
 * Rebuild every model from the data already here, without downloading (LRN-2). Results are
 * stored in eenkah.db, so they survive a restart; this is only needed after changing marks,
 * thresholds or settings, or to see the models rebuilt.
 */
export function RetrainCard({ onDone }: { onDone?: () => void }) {
  const [personal, reload] = useLoad(api.personal);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [res, setRes] = useState<RetrainResult | null>(null);
  const last = personal.state === "ok" ? personal.data.runs.normal?.finished : undefined;
  const go = () => {
    setBusy(true);
    setErr(null);
    api.retrain().then(
      (r) => {
        setBusy(false);
        setRes(r);
        reload();
        onDone?.();
      },
      (e: unknown) => {
        setBusy(false);
        setErr(e instanceof Error ? e.message : String(e));
      },
    );
  };
  return (
    <section className="card panel">
      <div className="row gap wrap">
        <div className="grow">
          <div className="panel-title">Models</div>
          <p className="panel-sub">
            Every model is rebuilt from your data after each download; only its results are kept, in eenkah.db, so they are still here after a restart.{" "}
            {last ? (
              <>
                Last rebuilt <Ago t={last} />.
              </>
            ) : (
              "Not built yet."
            )}
          </p>
        </div>
        <button className="btn" disabled={busy} onClick={go}>
          {busy ? "Retraining…" : "Retrain now"}
        </button>
      </div>
      {err && <p className="error small">{err}</p>}
      {res && (
        <p className="small">
          Recomputed metrics and rebuilt every model in <strong>{fmt(res.secs, 1)} s</strong>: {res.train.filter((t) => t.champion).length} forecasts,
          normal ranges for {fmt(res.personal.days, 0)} nights ({fmt((res.personal.inside_normal_range ?? 0) * 100, 0)} % inside), {res.personal.shifts} shifts,{" "}
          {res.personal.unusual_days} unusual nights, {res.personal.sessions_labelled} session types.
        </p>
      )}
    </section>
  );
}
