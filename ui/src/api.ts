// Typed client for the eenkah HTTP API. Every type mirrors the JSON the Rust side returns
// (crates/eenkah-api/src/views.rs, jobs.rs and eenkah-learn's model_summary).

export interface Finding {
  subject: string;
  direction: "up" | "down" | "steady";
  size: number;
  unit: string;
  span_days: number;
  series: string;
  weight: number;
  sentence: string;
}

export interface TodayOk {
  day: string;
  data_age_days: number;
  hrv_ms: number | null;
  hrv_7d_ms: number | null;
  hrv_normal_ms: [number | null, number | null];
  hrv_status: string | null;
  rhr_bpm: number | null;
  rhr_z: number | null;
  hrv_z: number | null;
  sleep_hours: number | null;
  sleep_debt_7d_hours: number | null;
  fitness: number | null;
  fatigue: number | null;
  form: number | null;
  body_battery_at_wake: number | null;
  skin_temp_dev_c: number | null;
  findings: Finding[];
  brief: string;
  series: string[];
  disclaimer: string;
}

export interface TodayEmpty {
  error: string;
  disclaimer: string;
}

export type Today = TodayOk | TodayEmpty;

export const hasData = (t: Today): t is TodayOk => !("error" in t);

export interface ActivityRow {
  id: number;
  start_utc: number;
  end_utc: number;
  start_local: string | null;
  end_local: string | null;
  sport: string | null;
  sub_sport: string | null;
  type: string | null;
  name: string | null;
  moving_min: number | null;
  distance_km: number | null;
  ascent_m: number | null;
  avg_hr: number | null;
  max_hr: number | null;
  load: number | null;
  decoupling_pct: number | null;
  /** Session type from the personal model, or null when not classified. */
  session_type: SessionLabel | null;
  /** "you" (your correction), "learned" (forest trained on your corrections) or "groups". */
  session_type_source: "you" | "learned" | "groups" | null;
}

export type SessionLabel = "Easy" | "Steady" | "Long" | "Tempo" | "Intervals";

export type StreamName = "hr" | "speed" | "power" | "cadence" | "altitude";

export interface ActivityDetail {
  activity: ActivityRow;
  load: number | null;
  decoupling_pct: number | null;
  run_efficiency: number | null;
  zone_minutes: (number | null)[];
  thresholds: { rest_hr: number | null; max_hr: number | null; lthr: number | null };
  streams: StreamName[];
}

export type Family = "Persistence" | "Mean7" | "Mean60" | "Ridge" | "Forest";

export interface NextForecast {
  for_day: string;
  value: number;
  lo: number | null;
  hi: number | null;
  drivers: string[] | null;
}

export interface ForecastTarget {
  target: "hrv_next" | "rhr_next";
  label: string;
  unit: string;
  champion: Family | null;
  backtest_mae: Partial<Record<Family, number>> | null;
  backtest_coverage: number | null;
  rows: number | null;
  train_secs: number | null;
  peak_memory_kb: number | null;
  /** What the model was rebuilt from: data hash, date range, features, scores, seed, versions. */
  lineage: { data_hash: string; first_day: string; last_day: string; seed: number; aprender: string } | null;
  next: NextForecast | null;
  live_scored: number;
  live_inside_interval: number;
}

export interface Coverage {
  what: string;
  first: string | null;
  last: string | null;
  days: number;
  missing_days: number;
}

/** A Garmin account: the opaque id Garmin uses in URLs, and the person's name when known. */
export interface Account {
  id: string;
  name: string | null;
}

export interface ResetPreview {
  account: Account | null;
  files: number;
  bytes: number;
  days: number;
  activities: number;
  deletes: string[];
  keeps: string[];
}

export interface LastDownload {
  at: number;
  account: string | null;
  days_with_data: number;
  days_empty: number;
  new_activities: number;
  errors: number;
  stopped_early: boolean;
}

export interface Status {
  coverage: Coverage[];
  last_import: { started: number; finished: number | null } | null;
  newest_day: string | null;
  data_age_days: number | null;
  connect_signed_in: boolean;
  connect_last_download: LastDownload | null;
  /** The account this folder's downloaded data came from. */
  connect_data_account: Account | null;
  /** The account last seen signed in. */
  connect_login_account: Account | null;
  /** Today in the athlete's time zone, YYYY-MM-DD. */
  today: string;
  time_zone: string;
  raw_files: number;
}

export type PhaseKey = "download" | "import" | "metrics" | "train";
export type PhaseStatus = "pending" | "running" | "done" | "failed" | "skipped";

export interface PhaseRow {
  phase: PhaseKey;
  label: string;
  state: PhaseStatus;
  started: number | null;
  finished: number | null;
}

export interface DownloadProgress {
  step: "activities" | "rhr" | "weight" | "days";
  done: number;
  /** 0 while unknown. */
  total: number;
  item: string;
  counters: { requests: number; retries: number; bytes: number };
  days_with_data: number;
  days_empty: number;
}

/** One forecast target after training: either trained, or a reason it was not. */
export interface TrainTarget {
  target: string;
  status?: string;
  rows?: number;
  champion?: string;
  mae?: Record<string, number>;
  secs?: number;
  live?: { for_day: string; value: number } | null;
  refused?: string | null;
}

export interface JobSummary {
  secs: number;
  connect: {
    account: string | null;
    requests: number;
    retries: number;
    bytes: number;
    files: number;
    new_activities: number;
    days_with_data: number;
    days_empty: number;
    errors: number;
    stopped_early: boolean;
  } | null;
  import: { files_seen: number; new: number; unchanged: number; failed: number; contents: Record<string, number> };
  metrics: { series: number; points: number; activities: number };
  train: TrainTarget[];
  warnings: string[];
}

export interface JobState {
  running: boolean;
  started: number | null;
  finished: number | null;
  phases: PhaseRow[];
  download: DownloadProgress | null;
  log: string[];
  error: string | null;
  summary: JobSummary | null;
}

export interface Mark {
  day: string;
  kind: string;
  value: number;
  note: string;
}

export interface WeekCompare {
  this_week: number | null;
  last_week: number | null;
}

export interface Week {
  week: [string, string];
  load: { this_week: number; last_week: number; same_week_last_year: number; unit: string };
  sleep_hours_avg: WeekCompare;
  hrv_ms_avg: WeekCompare;
  rhr_bpm_avg: WeekCompare;
  sessions_this_week: number | null;
  minutes_by_type: { label: string; minutes: number }[];
  disclaimer: string;
}

/** One body signal on the newest night against the athlete's own normal range. */
export interface NormalSignal {
  signal: string;
  label: string;
  unit: string;
  /** 1: up is good, -1: down is good, 0: neither. */
  better: number;
  digits: number;
  series: string;
  day: string;
  value: number;
  mid: number;
  lo: number;
  hi: number;
  status: "below" | "within" | "above";
  inside_last_year: number | null;
  days_last_year: number;
}

export interface ShiftRow {
  signal: string;
  label: string;
  unit: string;
  digits: number;
  series: string;
  day: string;
  detected: string;
  before: number;
  after: number;
  direction: "up" | "down";
  score: number;
}

export interface UnusualDay {
  day: string;
  pct: number;
  lof_pct: number;
  unusual: boolean;
  agree: boolean;
  drivers: string[];
}

export interface PersonalRun {
  secs: number;
  rows: number;
  scores: Record<string, unknown> | Record<string, unknown>[];
  method: string;
  coverage: number | null;
  peak_memory_kb: number | null;
  finished: number;
}

export interface SportReport {
  sport: string;
  sessions: number;
  groups: number;
  silhouette: number;
  group_names: SessionLabel[];
  group_means: number[][];
  corrections: number;
  groups_accuracy: number | null;
  forest_accuracy: number | null;
  forest_used: boolean;
}

export interface Personal {
  day: string | null;
  signals: NormalSignal[];
  shifts: ShiftRow[];
  unusual: { latest: UnusualDay | null; recent: UnusualDay[] };
  sessions: { labels: SessionLabel[]; counts: { label: SessionLabel; source: string; sessions: number }[]; corrections: number };
  runs: Record<"normal" | "shift" | "unusual" | "sessions", PersonalRun | null>;
  disclaimer: string;
}

/** What "Retrain now" did: metrics recomputed from stored data, then every model rebuilt. */
export interface RetrainResult {
  secs: number;
  metrics: { series: number; points: number; activities: number };
  train: TrainTarget[];
  personal: { days: number; inside_normal_range: number | null; shifts: number; unusual_days: number; sessions_labelled: number; secs: number };
  finished: number;
}

export class ApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message);
  }
}

async function call<T>(path: string, init?: RequestInit): Promise<T> {
  const r = await fetch(path, init);
  const text = await r.text();
  let body: unknown = text;
  try {
    body = JSON.parse(text);
  } catch {
    /* plain text */
  }
  if (!r.ok) {
    const msg = typeof body === "object" && body !== null && "error" in body && typeof body.error === "string" ? body.error : `HTTP ${r.status}`;
    throw new ApiError(msg, r.status);
  }
  return body as T;
}

const json = (method: string, body: unknown): RequestInit => ({ method, headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });

export const api = {
  today: () => call<Today>("/api/today"),
  week: () => call<Week>("/api/week"),
  activities: (limit = 2000) => call<ActivityRow[]>(`/api/activities?limit=${limit}`),
  activity: (id: number) => call<ActivityDetail>(`/api/activities/${id}`),
  forecast: () => call<ForecastTarget[]>("/api/forecast"),
  personal: () => call<Personal>("/api/personal"),
  retrain: () => call<RetrainResult>("/api/train", { method: "POST" }),
  setSessionType: (id: number, label: SessionLabel | null) => call<ActivityDetail>(`/api/activities/${id}/type`, json("POST", { label })),
  status: () => call<Status>("/api/status"),
  importState: () => call<JobState>("/api/import"),
  startImport: (since?: string) => call<{ started: boolean }>("/api/import", json("POST", since ? { since } : {})),
  resetPreview: () => call<ResetPreview>("/api/reset"),
  reset: () => call<{ removed: ResetPreview }>("/api/reset", json("POST", { confirm: true })),
  marks: () => call<Mark[]>("/api/marks"),
  addMark: (m: Mark) => call<Mark[]>("/api/marks", json("POST", m)),
  deleteMark: (day: string, kind: string) => call<Mark[]>(`/api/marks?day=${encodeURIComponent(day)}&kind=${encodeURIComponent(kind)}`, { method: "DELETE" }),
};
