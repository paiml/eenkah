import { useEffect, useState, type FormEvent, type ReactNode } from "react";
import { api, ApiError, type Account, type DownloadProgress, type JobState, type JobSummary, type Mark, type PhaseRow, type PhaseStatus, type ResetPreview, type Status, type TrainTarget } from "../api";
import { fmt, useLoad } from "../hooks";
import { RetrainCard } from "../components/Retrain";
import { Ago, Day } from "../components/When";
import { ago, daysAgo, dayWords, exact, parseLocal, relDay, short } from "../time";

// ---- time helpers -------------------------------------------------------------------------

const nowSecs = () => Date.now() / 1000;

/** 75 → "1m 15s". */
function duration(secs: number): string {
  const s = Math.max(0, Math.round(secs));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, "0")}s`;
  return `${Math.floor(s / 3600)}h ${String(Math.floor((s % 3600) / 60)).padStart(2, "0")}m`;
}

/** Whole days from `day` to `today` (both YYYY-MM-DD, today in the athlete's zone). */
const daysBetween = (day: string, today: string) => daysAgo(day, parseLocal(today));

/** Re-render every second while `on`, so elapsed times tick. */
function useTick(on: boolean) {
  const [, set] = useState(0);
  useEffect(() => {
    if (!on) return;
    const id = setInterval(() => set((x) => x + 1), 1000);
    return () => clearInterval(id);
  }, [on]);
}

// ---- status pills (icon + word, never colour alone) -------------------------------------

type Level = "good" | "warn" | "bad" | "none";

function Pill({ level, children }: { level: Level; children: ReactNode }) {
  const icon = { good: "✓", warn: "!", bad: "✕", none: "–" }[level];
  return (
    <span className={`pill pill-${level}`}>
      <span aria-hidden="true">{icon}</span> {children}
    </span>
  );
}

/** Data is fresh if it reaches yesterday's night; stale after 3 days. */
function freshness(age: number | null): Level {
  if (age == null) return "none";
  return age <= 1 ? "good" : age <= 3 ? "warn" : "bad";
}

const describe = (a: Account) => `${a.name ?? "unnamed account"} (${a.id.slice(0, 8)}…)`;

/** True when the signed-in account is not the one this folder's data came from. */
function mismatch(s: Status): boolean {
  return s.connect_data_account != null && s.connect_login_account != null && s.connect_data_account.id !== s.connect_login_account.id;
}

// ---- freshness overview -------------------------------------------------------------------

function Freshness({ status }: { status: Status }) {
  const age = status.newest_day ? daysBetween(status.newest_day, status.today) : null;
  const level = freshness(age);
  const dl = status.connect_last_download;
  const acts = status.coverage.find((c) => c.what === "Activities");
  const actAge = acts?.last ? daysBetween(acts.last, status.today) : null;
  const imp = status.last_import;
  return (
    <div className="stats-row">
      <div className="card stat-card">
        <div className="panel-title">Data through</div>
        <div className="small-value">{status.newest_day ? <Day day={status.newest_day} /> : "no data yet"}</div>
        <div className="small muted">
          <Pill level={level}>{age == null ? "nothing imported" : level === "good" ? "fresh" : level === "warn" ? "a little behind" : "too old to forecast"}</Pill>
        </div>
      </div>
      <div className="card stat-card">
        <div className="panel-title">Last download from Garmin</div>
        <div className="small-value">{dl ? <Ago t={dl.at} /> : "never"}</div>
        <div className="small muted">
          {dl ? (
            dl.days_with_data === 0 && dl.days_empty > 0 && dl.new_activities === 0 ? (
              <Pill level="bad">every day came back empty</Pill>
            ) : (
              `${dl.days_with_data} days with data · ${dl.new_activities} new activities`
            )
          ) : status.connect_signed_in ? (
            "signed in, nothing downloaded yet"
          ) : (
            "not signed in"
          )}
        </div>
      </div>
      <div className="card stat-card">
        <div className="panel-title">Newest activity</div>
        <div className="small-value">{acts?.last ? <Day day={acts.last} /> : "none"}</div>
        <div className="small muted">{actAge == null ? "no activities imported" : `${acts?.days.toLocaleString()} days with activities in total`}</div>
      </div>
      <div className="card stat-card">
        <div className="panel-title">Last import</div>
        <div className="small-value">{imp ? <Ago t={imp.finished ?? imp.started} /> : "never"}</div>
        <div className="small muted">
          {imp == null ? "nothing imported yet" : imp.finished == null ? <Pill level="warn">did not finish</Pill> : `metrics and models updated ${ago(imp.finished)}`}
        </div>
      </div>
    </div>
  );
}

// ---- the import job ---------------------------------------------------------------------

const STEP_HELP: Record<string, string> = {
  download: "Fetches only what is not already on this computer. The last two days are always fetched again, because Garmin keeps updating them.",
  import: "Reads the downloaded files into the local database.",
  metrics: "Computes HRV baselines, training load, sleep debt and the other daily metrics.",
  train: "eenkah fits and backtests its own forecasting models on this computer's CPU (aprender). Nothing is sent to Garmin or anywhere else.",
};

const STEP_ICON: Record<PhaseStatus, string> = { pending: "○", running: "", done: "✓", failed: "✕", skipped: "–" };

const SUBSTEP: Record<DownloadProgress["step"], string> = {
  activities: "Activities",
  rhr: "Resting heart rate",
  weight: "Weight",
  days: "Daily data (sleep, HRV, stress, steps)",
};

function Meter({ done, total, label }: { done: number; total: number; label: string }) {
  const pct = total > 0 ? Math.min(100, (done / total) * 100) : null;
  return (
    <div className="meter-wrap">
      <div className="meter" role="progressbar" aria-label={label} aria-valuemin={0} aria-valuemax={total || undefined} aria-valuenow={total ? done : undefined}>
        <div className={pct == null ? "meter-fill indeterminate" : "meter-fill"} style={pct == null ? undefined : { width: `${pct}%` }} />
      </div>
      <span className="small nowrap num">{pct == null ? `${done.toLocaleString()} so far` : `${done.toLocaleString()} / ${total.toLocaleString()} · ${pct.toFixed(0)}%`}</span>
    </div>
  );
}

function DownloadDetail({ p, live }: { p: DownloadProgress; live: boolean }) {
  const c = p.counters;
  return (
    <div className="step-detail">
      <div className="small strong">{SUBSTEP[p.step]}</div>
      {live && <Meter done={p.done} total={p.total} label={SUBSTEP[p.step]} />}
      <div className="small muted">
        {live && p.item && <>now: {p.item} · </>}
        {c.requests.toLocaleString()} requests · {(c.bytes / 1e6).toFixed(1)} MB
        {c.retries > 0 && <> · {c.retries} retried</>}
        {(p.days_with_data > 0 || p.days_empty > 0) && (
          <>
            {" "}
            · {p.days_with_data} days with data
          </>
        )}
      </div>
      {p.days_empty > 0 && (
        <div className="small">
          <Pill level={p.days_with_data === 0 ? "bad" : "warn"}>
            {p.days_empty} {p.days_empty === 1 ? "day" : "days"} came back empty
          </Pill>
        </div>
      )}
    </div>
  );
}

function Step({ row, job }: { row: PhaseRow; job: JobState }) {
  const end = row.finished ?? (row.state === "running" ? nowSecs() : null);
  const took = row.started != null && end != null ? duration(end - row.started) : null;
  return (
    <li className={`step step-${row.state}`}>
      <span className="step-icon" aria-hidden="true">
        {row.state === "running" ? <span className="spinner" /> : STEP_ICON[row.state]}
      </span>
      <div className="grow">
        <div className="row gap">
          <span className="step-label">{row.label}</span>
          <span className="small muted">{row.state === "running" ? `running · ${took}` : row.state === "done" ? took : row.state === "skipped" ? "skipped" : row.state === "failed" ? "failed" : ""}</span>
        </div>
        {(row.state === "running" || row.phase === "train") && row.state !== "done" && row.state !== "skipped" && <div className="small muted">{STEP_HELP[row.phase]}</div>}
        {row.phase === "download" && job.download && row.state !== "pending" && <DownloadDetail p={job.download} live={row.state === "running"} />}
      </div>
    </li>
  );
}

const TARGET: Record<string, { label: string; unit: string }> = {
  hrv_next: { label: "Overnight HRV, next morning", unit: "ms" },
  rhr_next: { label: "Resting heart rate, next morning", unit: "bpm" },
};

function TrainLine({ t }: { t: TrainTarget }) {
  const info = TARGET[t.target] ?? { label: t.target, unit: "" };
  if (t.status) {
    const m = /(\d+) usable days, (\d+) needed/.exec(t.status);
    return (
      <li>
        <Pill level="none">not trained yet</Pill> {info.label}:{" "}
        {m
          ? `needs ${m[2]} days with every input recorded and has ${m[1]}. Your own averages are shown meanwhile. Fetching older history (above) is the quickest way to get there.`
          : t.status}
      </li>
    );
  }
  const err = t.champion && t.mae ? t.mae[t.champion] : undefined;
  return (
    <li>
      <Pill level="good">trained</Pill> {info.label}: best model {t.champion} on {t.rows} days, typical error {fmt(err, 1, info.unit)}
      {t.live ? ` · forecast for ${relDay(t.live.for_day)}: ${fmt(t.live.value, 1, info.unit)}` : t.refused ? ` · ${t.refused}` : ""}
    </li>
  );
}

function Outcome({ job }: { job: JobState }) {
  const s: JobSummary | null = job.summary;
  const took = job.started != null && job.finished != null ? duration(job.finished - job.started) : "";
  const failedAt = job.phases.find((p) => p.state === "failed");
  if (job.error) {
    return (
      <div className="outcome outcome-bad" role="status">
        <div className="strong">
          <span aria-hidden="true">✕</span> Failed{failedAt ? ` at step “${failedAt.label}”` : ""}
          {job.finished ? ` ${ago(job.finished)}` : ""}
        </div>
        <div className="small">{job.error}</div>
      </div>
    );
  }
  if (!s) return null;
  const warn = s.warnings.length > 0;
  const c = s.connect;
  return (
    <div className={`outcome ${warn ? "outcome-warn" : "outcome-good"}`} role="status">
      <div className="strong">
        <span aria-hidden="true">{warn ? "!" : "✓"}</span> {warn ? "Finished with problems" : "Finished"}
        {job.finished ? ` ${ago(job.finished)}` : ""} · took {took}
      </div>
      {warn && (
        <ul className="outcome-list">
          {s.warnings.map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
      )}
      <dl className="outcome-facts small">
        {c && (
          <>
            <dt>Downloaded</dt>
            <dd>
              {c.new_activities} new activities · {c.days_with_data} days with data
              {c.days_empty > 0 ? ` · ${c.days_empty} empty` : ""} · {c.requests.toLocaleString()} requests ({(c.bytes / 1e6).toFixed(1)} MB)
            </dd>
          </>
        )}
        <dt>Read</dt>
        <dd>
          {s.import.new.toLocaleString()} new files{s.import.unchanged ? `, ${s.import.unchanged.toLocaleString()} unchanged` : ""}
          {s.import.failed ? `, ${s.import.failed} failed` : ""}
        </dd>
        <dt>Metrics</dt>
        <dd>
          {s.metrics.points.toLocaleString()} values in {s.metrics.series} series · {s.metrics.activities.toLocaleString()} activities
        </dd>
      </dl>
      <div className="small strong">Forecasts (trained on this computer)</div>
      <ul className="outcome-list plain small">
        {s.train.map((t) => (
          <TrainLine key={t.target} t={t} />
        ))}
      </ul>
    </div>
  );
}

/** Download from Garmin Connect with live, step-by-step progress (polls the background job). */
function Download({ status, onDone, kick }: { status: Status; onDone: () => void; kick: number }) {
  const [job, setJob] = useState<JobState | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useTick(job?.running ?? false);
  useEffect(() => {
    let live = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      try {
        const s = await api.importState();
        if (!live) return;
        setErr(null);
        setJob((prev) => {
          if (prev?.running && !s.running) onDone();
          return s;
        });
        timer = setTimeout(poll, s.running ? 1000 : 5000);
      } catch (e) {
        if (!live) return;
        setErr(`Lost contact with the eenkah server (${e instanceof Error ? e.message : String(e)}). Retrying…`);
        timer = setTimeout(poll, 3000);
      }
    };
    void poll();
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [onDone, kick]);
  const start = async () => {
    setErr(null);
    try {
      await api.startImport();
      setJob(await api.importState());
    } catch (e) {
      setErr(e instanceof ApiError ? e.message : String(e));
    }
  };
  const signedIn = status.connect_signed_in;
  const ran = job != null && job.phases.length > 0;
  const firstDay = status.coverage.map((c) => c.first).filter((d): d is string => d != null).sort()[0] ?? null;
  const [since, setSince] = useState("");
  const [pickHistory, setPickHistory] = useState(false);
  const fetchOlder = async () => {
    setErr(null);
    try {
      await api.startImport(since);
      setPickHistory(false);
      setJob(await api.importState());
    } catch (e) {
      setErr(e instanceof ApiError ? e.message : String(e));
    }
  };
  return (
    <section className="card panel">
      <div className="row gap wrap">
        <div className="grow">
          <div className="panel-title">Garmin Connect</div>
          <p className="panel-sub">
            {signedIn ? (
              <>
                Signed in{status.connect_login_account ? ` as ${describe(status.connect_login_account)}` : ""}. Downloads fetch only what is not here yet.
              </>
            ) : (
              <>
                Not signed in. Run <code>eenkah login</code> once in a terminal: the password is used once and never stored.
              </>
            )}
          </p>
        </div>
        <button className="btn primary" disabled={!signedIn || job?.running || mismatch(status)} onClick={() => void start()}>
          {job?.running ? "Working…" : "Download new data"}
        </button>
      </div>
      {signedIn && (
        <div className="history small">
          <span className="muted">History here starts</span> <b>{firstDay ?? "—"}</b>
          {!pickHistory ? (
            <button className="btn ghost small" disabled={job?.running || mismatch(status)} onClick={() => setPickHistory(true)}>
              Fetch older history…
            </button>
          ) : (
            <span className="row gap wrap">
              <span className="muted">from</span>
              <input className="input" type="date" value={since} max={firstDay ?? undefined} onChange={(e) => setSince(e.target.value)} aria-label="Fetch history from" />
              <button className="btn primary small" disabled={!since || job?.running} onClick={() => void fetchOlder()}>
                Fetch
              </button>
              <button className="btn ghost small" onClick={() => setPickHistory(false)}>
                Cancel
              </button>
              <span className="muted">About 2 requests per day of history; several years take a while and continue on the next download if interrupted.</span>
            </span>
          )}
        </div>
      )}
      {err && <p className="error">{err}</p>}
      {ran && (
        <>
          <div className="small muted">
            {job.running ? `Running for ${duration(nowSecs() - (job.started ?? nowSecs()))}` : `Last run started ${ago(job.started ?? 0)}`}
          </div>
          <ol className="steps">
            {job.phases.map((r) => (
              <Step key={r.phase} row={r} job={job} />
            ))}
          </ol>
          {!job.running && <Outcome job={job} />}
          {job.log.length > 0 && (
            <details className="small">
              <summary className="muted">Log ({job.log.length} lines)</summary>
              <pre className="log">{job.log.join("\n")}</pre>
            </details>
          )}
        </>
      )}
      {!ran && status.last_import && (
        <p className="small muted">No download has run since the server started. The last import finished {status.last_import.finished ? <Ago t={status.last_import.finished} /> : "— it did not finish"}.</p>
      )}
      <p className="small muted">Downloading contacts Garmin's servers with your saved sign-in. Nothing else ever leaves this computer.</p>
    </section>
  );
}

function CoverageTable({ status }: { status: Status }) {
  return (
    <section className="card panel">
      <div className="panel-title">What is here, and how fresh</div>
      <p className="panel-sub">
        {status.raw_files.toLocaleString()} archived files · time zone {status.time_zone}
      </p>
      <div className="table-scroll">
      <table className="data-table compact">
        <thead>
          <tr>
            <th>Data</th>
            <th>Freshness</th>
            <th>From</th>
            <th>To</th>
            <th className="num">Days</th>
            <th className="num">Missing</th>
          </tr>
        </thead>
        <tbody>
          {status.coverage.map((c) => {
            const age = c.last ? daysBetween(c.last, status.today) : null;
            return (
              <tr key={c.what}>
                <td>{c.what}</td>
                <td className="nowrap">
                  <Pill level={c.what === "Activities" ? (age == null ? "none" : "good") : freshness(age)}>{age == null ? "no data" : dayWords(age)}</Pill>
                </td>
                <td className="nowrap">{c.first ? <time title={exact(parseLocal(c.first))}>{short(c.first)}</time> : "—"}</td>
                <td className="nowrap">{c.last ? <time title={exact(parseLocal(c.last))}>{short(c.last)}</time> : "—"}</td>
                <td className="num">{c.days.toLocaleString()}</td>
                <td className="num">{c.what === "Activities" ? "" : c.missing_days}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
      </div>
      <p className="small muted">✓ reaches last night · ! 2–3 days behind · ✕ more than 3 days behind. Activities are not graded: rest days are normal.</p>
    </section>
  );
}

/** VIEW-3: days the watch cannot explain (illness, travel, races) become marks on every chart. */
function Marks() {
  const [marks, reload] = useLoad(api.marks);
  const [form, setForm] = useState<Mark>({ day: new Date().toISOString().slice(0, 10), kind: "sick", value: 1, note: "" });
  const [err, setErr] = useState<string | null>(null);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setErr(null);
    try {
      await api.addMark(form);
      reload();
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    }
  };
  return (
    <section className="card panel">
      <div className="panel-title">Marked days</div>
      <p className="panel-sub">Illness, travel, races, altitude, soreness: anything the watch cannot tell. Marks appear as series (eenkah.mark.*) and the forecasts use “sick”.</p>
      <form className="row gap wrap" onSubmit={(e) => void submit(e)}>
        <input className="input" type="date" value={form.day} onChange={(e) => setForm({ ...form, day: e.target.value })} aria-label="Day" />
        <input className="input" list="mark-kinds" value={form.kind} onChange={(e) => setForm({ ...form, kind: e.target.value })} aria-label="Kind" />
        <datalist id="mark-kinds">
          {["sick", "travel", "race", "altitude", "soreness"].map((k) => (
            <option key={k} value={k} />
          ))}
        </datalist>
        <input className="input" type="number" step="1" min="0" max="10" value={form.value} onChange={(e) => setForm({ ...form, value: Number(e.target.value) })} aria-label="Value" style={{ width: 70 }} />
        <input className="input grow" placeholder="Note (optional)" value={form.note} onChange={(e) => setForm({ ...form, note: e.target.value })} aria-label="Note" />
        <button className="btn primary" type="submit">
          Add
        </button>
      </form>
      {err && <p className="error">{err}</p>}
      {marks.state === "ok" && marks.data.length > 0 && (
        <table className="data-table compact">
          <tbody>
            {marks.data.slice(0, 60).map((m) => (
              <tr key={m.day + m.kind}>
                <td className="nowrap">
                  <Day day={m.day} />
                </td>
                <td>{m.kind}</td>
                <td className="num">{m.value}</td>
                <td>{m.note}</td>
                <td className="num">
                  <button
                    className="btn ghost small"
                    onClick={() =>
                      void api.deleteMark(m.day, m.kind).then(reload, (x: unknown) => setErr(String(x)))
                    }
                  >
                    Remove
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}

/** Shown when the signed-in account differs from the one the data came from. */
function AccountMismatch({ status, onStartOver }: { status: Status; onStartOver: () => void }) {
  const owner = status.connect_data_account;
  const me = status.connect_login_account;
  if (!owner || !me || !mismatch(status)) return null;
  return (
    <div className="outcome outcome-bad banner" role="alert">
      <div className="strong">
        <span aria-hidden="true">✕</span> This data belongs to a different Garmin account
      </div>
      <div className="small">
        You are signed in as <b>{describe(me)}</b>, but the data here was downloaded from <b>{describe(owner)}</b>. Downloads are paused so the two accounts never mix.
        Start over to replace it with your current account's data, or run <code>eenkah login</code> with the other account to keep it.
      </div>
      <div>
        <button className="btn primary" onClick={onStartOver}>
          Start over with {me.name ?? "this account"}…
        </button>
      </div>
    </div>
  );
}

/** Delete everything downloaded or computed, after showing exactly what goes and what stays. */
function StartOver({ status, open, setOpen, onDone }: { status: Status; open: boolean; setOpen: (o: boolean) => void; onDone: (download: boolean) => void }) {
  const [preview, setPreview] = useState<ResetPreview | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [thenDownload, setThenDownload] = useState(true);
  useEffect(() => {
    if (!open) return;
    setErr(null);
    api.resetPreview().then(setPreview, (e: unknown) => setErr(e instanceof Error ? e.message : String(e)));
  }, [open]);
  const go = async () => {
    setBusy(true);
    setErr(null);
    try {
      await api.reset();
      setOpen(false);
      onDone(thenDownload && status.connect_signed_in);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <section className="card panel" id="start-over">
      <div className="row gap wrap">
        <div className="grow">
          <div className="panel-title">Start over</div>
          <p className="panel-sub">Delete everything downloaded from Garmin and everything computed from it, then download again. Use it after switching Garmin accounts, or when the data looks wrong.</p>
        </div>
        {!open && (
          <button className="btn" onClick={() => setOpen(true)}>
            Start over…
          </button>
        )}
      </div>
      {open && (
        <div className="confirm">
          {preview ? (
            <>
              <div className="strong">
                This deletes {preview.files.toLocaleString()} files ({(preview.bytes / 1e6).toFixed(1)} MB): {preview.days.toLocaleString()} days of data and{" "}
                {preview.activities.toLocaleString()} activities{preview.account ? `, downloaded from ${describe(preview.account)}` : ""}.
              </div>
              <div className="confirm-cols small">
                <ul className="outcome-list plain">
                  {preview.deletes.map((d) => (
                    <li key={d}>
                      <span className="mark-bad" aria-hidden="true">✕</span> {d}
                    </li>
                  ))}
                </ul>
                <ul className="outcome-list plain">
                  {preview.keeps.map((k) => (
                    <li key={k}>
                      <span className="mark-good" aria-hidden="true">✓</span> keeps {k}
                    </li>
                  ))}
                </ul>
              </div>
              <label className="small row gap">
                <input type="checkbox" checked={thenDownload && status.connect_signed_in} disabled={!status.connect_signed_in} onChange={(e) => setThenDownload(e.target.checked)} />
                Download from Garmin right after{status.connect_login_account ? `, as ${describe(status.connect_login_account)}` : ""}
              </label>
              <div className="row gap">
                <button className="btn danger" disabled={busy} onClick={() => void go()}>
                  {busy ? "Deleting…" : `Delete ${preview.files.toLocaleString()} files and start over`}
                </button>
                <button className="btn ghost" disabled={busy} onClick={() => setOpen(false)}>
                  Cancel
                </button>
              </div>
            </>
          ) : (
            !err && <div className="small muted">Counting what would be deleted…</div>
          )}
          {err && <p className="error">{err}</p>}
        </div>
      )}
    </section>
  );
}

export function DataPage() {
  const [status, reload] = useLoad(api.status);
  const [resetOpen, setResetOpen] = useState(() => location.hash === "#start-over");
  const [kick, setKick] = useState(0);
  const openReset = () => {
    setResetOpen(true);
    setTimeout(() => document.getElementById("start-over")?.scrollIntoView({ behavior: "smooth", block: "center" }), 0);
  };
  const afterReset = (download: boolean) => {
    reload();
    if (download) {
      void api.startImport().then(
        () => setKick((k) => k + 1),
        () => setKick((k) => k + 1),
      );
    } else {
      setKick((k) => k + 1);
    }
  };
  return (
    <main className="page">
      <div className="page-head">
        <div>
          <h1>What do I have, and how fresh is it?</h1>
          <p className="muted">Everything lives in one SQLite file and a raw/ archive on this computer.</p>
        </div>
      </div>
      {status.state === "error" && <p className="error">{status.error}</p>}
      {status.state === "ok" && (
        <>
          <AccountMismatch status={status.data} onStartOver={openReset} />
          <Freshness status={status.data} />
          <div className="two-col">
            <Download status={status.data} onDone={reload} kick={kick} />
            <CoverageTable status={status.data} />
          </div>
        </>
      )}
      <RetrainCard onDone={reload} />
      <Marks />
      {status.state === "ok" && <StartOver status={status.data} open={resetOpen} setOpen={setResetOpen} onDone={afterReset} />}
    </main>
  );
}
