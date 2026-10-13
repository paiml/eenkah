//! The import job: download from Garmin Connect (or read a path), import, derive, train.
//! Shared by the CLI, the web UI (as a background job with progress) and MCP.

use anyhow::Result;
use chrono::NaiveDate;
use eenkah_store::{derive, import, DataDir, Db};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub fn tokens_path(dir: &DataDir) -> PathBuf {
    dir.root.join("connect/tokens.json")
}

pub fn inbox(dir: &DataDir) -> PathBuf {
    dir.root.join("connect/inbox")
}

pub fn signed_in(dir: &DataDir) -> bool {
    tokens_path(dir).exists()
}

#[derive(Clone, Debug, Default)]
pub struct ImportOpts {
    /// Import this path instead of downloading from Connect.
    pub path: Option<PathBuf>,
    pub since: Option<NaiveDate>,
    pub max_requests: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct ImportOutcome {
    pub connect: Option<eenkah_connect::Report>,
    pub import: import::Report,
    pub derive: derive::DeriveReport,
    pub train: eenkah_learn::TrainReport,
    pub secs: f64,
}

/// The steps of an import, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Fetch new data from Garmin Connect into the inbox.
    Download,
    /// Read the downloaded (or given) files into the database.
    Import,
    /// Compute daily metrics: HRV baselines, training load, sleep debt, ...
    Metrics,
    /// Train and backtest eenkah's own forecasting models on this computer.
    Train,
}

impl Phase {
    pub const ALL: [Phase; 4] = [Phase::Download, Phase::Import, Phase::Metrics, Phase::Train];
    pub fn label(self) -> &'static str {
        match self {
            Phase::Download => "Downloading from Garmin Connect",
            Phase::Import => "Reading files into the database",
            Phase::Metrics => "Computing metrics",
            Phase::Train => "Rebuilding eenkah's models on this computer",
        }
    }
}

/// What the import reports while it runs.
pub enum Update<'a> {
    Phase(Phase),
    Note(&'a str),
    Download(&'a eenkah_connect::Progress),
}

/// Download everything new from Garmin Connect into the inbox.
pub fn connect_sync(dir: &DataDir, db: &Db, since: Option<NaiveDate>, max_requests: Option<u64>, progress: &mut dyn FnMut(&Update)) -> Result<eenkah_connect::Report> {
    let tp = tokens_path(dir);
    let tokens = eenkah_connect::Tokens::load(&tp)?;
    let net = eenkah_connect::Net::new();
    let mut client = eenkah_connect::Client {
        t: &net,
        retry: eenkah_connect::Retry::default(),
        tokens,
        tokens_path: Some(tp),
        counters: Default::default(),
    };
    // one data folder, one Garmin account: never mix two people's data
    let me = client.profile()?;
    db.set_setting("connect_login", &serde_json::json!({"id": me.id, "name": me.name, "at": chrono::Utc::now().timestamp()}).to_string())?;
    match data_account(db)? {
        Some(owner) if owner.id != me.id => anyhow::bail!(
            "you are signed in as {}, but the data in this folder came from another Garmin account, {}. Nothing was downloaded, so \
             the two never mix. To replace it with this account's data, use “Start over” on the Data page or run `eenkah reset`; \
             to keep it, run `eenkah login` with that account instead",
            me.describe(),
            owner.describe()
        ),
        Some(_) => {}
        None => db.set_setting("connect_account", &serde_json::to_string(&me)?)?,
    }
    progress(&Update::Note(&format!("signed in as {}", me.describe())));
    let today = chrono::Utc::now().with_timezone(&db.tz()).date_naive();
    let known = eenkah_store::connect_known(db)?;
    let start = match since {
        Some(s) => s,
        None => match db.setting("connect_start")?.and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()) {
            Some(s) => s,
            None => known.daily.values().flatten().min().copied().unwrap_or(today - chrono::Duration::days(365)),
        },
    };
    progress(&Update::Note(&format!(
        "checking from {} to today; only what is not already here is downloaded",
        eenkah_core::when::day(start, today)
    )));
    let opts = eenkah_connect::Options {
        start,
        today,
        max_requests,
        account: me.id,
    };
    eenkah_connect::sync(&mut client, &inbox(dir), &known, &opts, &mut |p| progress(&Update::Download(p)))
}

pub fn run_import(dir: &DataDir, db: &mut Db, o: &ImportOpts, progress: &mut dyn FnMut(&Update)) -> Result<ImportOutcome> {
    let t = std::time::Instant::now();
    let (connect, path) = match &o.path {
        Some(p) => (None, p.clone()),
        None => {
            progress(&Update::Phase(Phase::Download));
            let r = connect_sync(dir, db, o.since, o.max_requests, progress)?;
            db.set_setting(
                "connect_last_download",
                &serde_json::json!({
                    "at": chrono::Utc::now().timestamp(),
                    "account": r.account,
                    "days_with_data": r.days_with_data,
                    "days_empty": r.days_empty,
                    "new_activities": r.activities_new,
                    "errors": r.errors.len(),
                    "stopped_early": r.stopped_early,
                })
                .to_string(),
            )?;
            (Some(r), inbox(dir))
        }
    };
    progress(&Update::Phase(Phase::Import));
    let import = import::import_path(db, &path)?;
    progress(&Update::Phase(Phase::Metrics));
    let derive = derive::run(db)?;
    progress(&Update::Phase(Phase::Train));
    let train = eenkah_learn::train(db)?;
    Ok(ImportOutcome {
        connect,
        import,
        derive,
        train,
        secs: t.elapsed().as_secs_f64(),
    })
}

/// The Garmin account this folder's downloaded data came from, if any was downloaded.
pub fn data_account(db: &Db) -> Result<Option<eenkah_connect::Account>> {
    Ok(db.setting("connect_account")?.and_then(|s| serde_json::from_str(&s).ok()))
}

/// The account last seen signed in (by `eenkah login` or a download).
pub fn login_account(db: &Db) -> Result<Option<eenkah_connect::Account>> {
    Ok(db.setting("connect_login")?.and_then(|s| serde_json::from_str(&s).ok()))
}

/// Settings that describe downloaded data, not the athlete's choices.
const DATA_SETTINGS: &str = "('connect_account', 'connect_last_download', 'connect_start')";

/// What "start over" would delete and keep.
#[derive(Debug, Serialize)]
pub struct ResetPreview {
    pub account: Option<eenkah_connect::Account>,
    pub files: i64,
    pub bytes: i64,
    pub days: i64,
    pub activities: i64,
    pub deletes: Vec<&'static str>,
    pub keeps: Vec<&'static str>,
}

pub fn reset_preview(db: &Db, all: bool) -> Result<ResetPreview> {
    let one = |sql: &str| db.conn.query_row(sql, [], |r| r.get::<_, i64>(0));
    let mut deletes = vec![
        "downloaded and imported files (the raw/ archive and the download inbox)",
        "daily values, activities and everything computed from them",
        "model results and forecasts",
        "the record of what has been downloaded",
    ];
    let mut keeps = vec![
        "sign-in",
        "marked days",
        "session-type corrections",
        "threshold overrides",
        "settings such as the time zone",
        "dashboards",
    ];
    if all {
        deletes.append(&mut keeps);
    }
    Ok(ResetPreview {
        account: data_account(db)?,
        files: one("SELECT count(*) FROM raw_file")?,
        bytes: one("SELECT coalesce(sum(size), 0) FROM raw_file")?,
        days: one("SELECT count(DISTINCT day) FROM day_value")?,
        activities: one("SELECT count(*) FROM activity")?,
        deletes,
        keeps,
    })
}

fn remove(path: &std::path::Path) -> Result<()> {
    let r = if path.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
    match r {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(anyhow::anyhow!("removing {}: {e}", path.display())),
        _ => Ok(()),
    }
}

/// Start over: delete everything downloaded or computed. Keeps sign-in, marks, thresholds,
/// settings and dashboards unless `all`, which leaves the folder as if eenkah had never run.
/// Tables are emptied rather than the file deleted, so a running server keeps working.
pub fn reset(db: &mut Db, all: bool) -> Result<ResetPreview> {
    let before = reset_preview(db, all)?;
    eenkah_store::derive::drop_series(db)?;
    let tx = db.conn.transaction()?;
    tx.execute_batch(
        "DELETE FROM raw_file; DELETE FROM day_value; DELETE FROM activity; DELETE FROM activity_sample; DELETE FROM activity_metric; \
         DELETE FROM streams_written; DELETE FROM import_run;",
    )?;
    for t in ["forecast", "model_run", "day_normal", "shift", "day_unusual", "session_type"] {
        let exists: i64 = tx.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1", [t], |r| r.get(0))?;
        if exists > 0 {
            tx.execute(&format!("DELETE FROM {t}"), [])?;
        }
    }
    tx.execute(&format!("DELETE FROM setting WHERE key IN {DATA_SETTINGS}"), [])?;
    if all {
        tx.execute_batch("DELETE FROM mark; DELETE FROM threshold_override; DELETE FROM setting;")?;
        let labels: i64 = tx.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='session_label'", [], |r| r.get(0))?;
        if labels > 0 {
            tx.execute("DELETE FROM session_label", [])?; // the athlete's corrections go with the marks
        }
        let dash: i64 = tx.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='dashboards'", [], |r| r.get(0))?;
        if dash > 0 {
            tx.execute("DELETE FROM dashboards", [])?;
        }
    }
    tx.commit()?;
    let root = &db.dir.root;
    remove(&root.join("raw"))?;
    std::fs::create_dir_all(root.join("raw"))?;
    remove(&db.dir.models_dir())?;
    remove(&root.join("connect/inbox"))?;
    remove(&root.join("connect/state.json"))?;
    if all {
        remove(&root.join("connect/tokens.json"))?;
    }
    // give the space back; best effort, since another connection (a server worker) may be busy
    let _ = db.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;");
    log::info!("reset: removed {} files, {} days, {} activities", before.files, before.days, before.activities);
    Ok(before)
}

/// Plain-language problems with a finished import, worst first. Shared by the CLI and the UI.
pub fn warnings(o: &ImportOutcome) -> Vec<String> {
    let mut w = vec![];
    if let Some(c) = &o.connect {
        let account = c.account.as_deref().unwrap_or("unknown");
        if c.days_empty > 0 && c.days_with_data == 0 && c.activities_new == 0 {
            w.push(format!(
                "Garmin answered every request, but all {} days came back empty and there were no new activities. The signed-in \
                 account ({account}) is probably not the one your watch syncs to: check the email in the Garmin Connect app, run \
                 `eenkah login` with that email, then start over (Data page, or `eenkah reset`).",
                c.days_empty
            ));
        } else if c.days_empty > 0 {
            w.push(format!(
                "{} of {} days had nothing recorded on Garmin (watch not worn, or not synced yet).",
                c.days_empty,
                c.days_empty + c.days_with_data
            ));
        }
        if !c.errors.is_empty() {
            let groups = error_groups(&c.errors);
            let parts: Vec<String> = groups.iter().map(|g| format!("{} × {} ({})", g.count, g.what, g.cause)).collect();
            let hint = if groups.iter().all(|g| g.transient) {
                "Garmin or the network had trouble; the next download retries them."
            } else if groups.iter().any(|g| g.transient) {
                "Some are temporary and are retried next time; the others are refused by Garmin and will fail again until eenkah is fixed."
            } else {
                "Garmin refuses these requests, so retrying will not help; this needs a fix in eenkah."
            };
            w.push(format!("{} downloads failed: {}. {hint}", c.errors.len(), parts.join(", ")));
        }
        if c.stopped_early {
            w.push("The download stopped at its request limit; run it again to continue.".into());
        }
    }
    if o.import.failed > 0 {
        w.push(format!("{} files could not be read.", o.import.failed));
    }
    if o.derive.points_written == 0 && o.derive.activities == 0 {
        w.push("There is nothing to chart yet: no metric could be computed from the data.".into());
    }
    w
}

/// Download failures with the same cause, e.g. 366 × "daily files" (HTTP 406).
#[derive(Debug, PartialEq)]
pub struct ErrorGroup {
    pub what: &'static str,
    pub cause: String,
    pub count: usize,
    /// A retry can succeed: rate limits, server errors, network trouble.
    pub transient: bool,
}

/// Group the connect report's error lines ("activity 123: HTTP 406 for …", "Sleep 2026-10-01: …").
pub fn error_groups(errors: &[String]) -> Vec<ErrorGroup> {
    let mut out: Vec<ErrorGroup> = vec![];
    for e in errors {
        let what = match e.split_whitespace().next().unwrap_or("") {
            "activity" => "activity files",
            "Sleep" => "sleep",
            "Summary" => "daily summaries",
            "Wellness" => "daily files",
            "Rhr" => "resting heart rate",
            "Weight" => "weight",
            "Hrv" => "HRV",
            _ => "other",
        };
        let status = e.split("HTTP ").nth(1).and_then(|r| r.get(..3)).and_then(|c| c.parse::<u16>().ok());
        let (cause, transient) = match status {
            Some(c) => (format!("HTTP {c}"), c == 429 || c >= 500),
            None => ("network or reply problem".to_string(), true),
        };
        match out.iter_mut().find(|g| g.what == what && g.cause == cause) {
            Some(g) => g.count += 1,
            None => out.push(ErrorGroup { what, cause, count: 1, transient }),
        }
    }
    out.sort_by_key(|g| std::cmp::Reverse(g.count));
    out
}

/// The summary the Data page shows after an import.
pub fn summary(o: &ImportOutcome) -> serde_json::Value {
    serde_json::json!({
        "secs": o.secs,
        "connect": o.connect.as_ref().map(|c| serde_json::json!({
            "account": c.account,
            "requests": c.counters.requests,
            "retries": c.counters.retries,
            "bytes": c.counters.bytes,
            "files": c.files_written,
            "new_activities": c.activities_new,
            "days_with_data": c.days_with_data,
            "days_empty": c.days_empty,
            "errors": c.errors.len(),
            "stopped_early": c.stopped_early,
        })),
        "import": {"files_seen": o.import.files_seen, "new": o.import.new, "unchanged": o.import.unchanged, "failed": o.import.failed, "contents": o.import.contents},
        "metrics": {"series": o.derive.series_written, "points": o.derive.points_written, "activities": o.derive.activities},
        "train": o.train.targets,
        "warnings": warnings(o),
    })
}

/// One step of the web UI's background import.
#[derive(Clone, Debug, Serialize)]
pub struct PhaseRow {
    pub phase: Phase,
    pub label: &'static str,
    /// "pending", "running", "done", "failed" or "skipped".
    pub state: &'static str,
    pub started: Option<i64>,
    pub finished: Option<i64>,
}

/// State of the web UI's background import, polled by the Data page.
#[derive(Clone, Debug, Default, Serialize)]
pub struct JobState {
    pub running: bool,
    pub started: Option<i64>,
    pub finished: Option<i64>,
    pub phases: Vec<PhaseRow>,
    /// The latest download progress, while and after downloading.
    pub download: Option<eenkah_connect::Progress>,
    pub log: Vec<String>,
    pub error: Option<String>,
    pub summary: Option<serde_json::Value>,
}

impl JobState {
    fn note(&mut self, m: &str) {
        self.log.push(format!("{} {m}", chrono::Local::now().format("%H:%M:%S")));
        if self.log.len() > 200 {
            self.log.remove(0);
        }
    }

    /// Close the running phase as `state`, and start `next` if given.
    fn advance(&mut self, state: &'static str, next: Option<Phase>) {
        let now = chrono::Utc::now().timestamp();
        for p in self.phases.iter_mut().filter(|p| p.state == "running") {
            p.state = state;
            p.finished = Some(now);
        }
        if let Some(n) = next {
            // steps jumped over (e.g. no download when importing a path) are skipped
            let idx = Phase::ALL.iter().position(|x| *x == n).unwrap_or(0);
            for p in self.phases.iter_mut().take(idx).filter(|p| p.state == "pending") {
                p.state = "skipped";
            }
            if let Some(p) = self.phases.iter_mut().find(|p| p.phase == n) {
                p.state = "running";
                p.started = Some(now);
            }
            self.note(n.label());
        }
    }
}

pub type Job = Arc<Mutex<JobState>>;

/// Start over from the web UI. Refused while an import runs; holds the job lock throughout
/// so an import cannot start halfway through.
pub fn reset_unless_running(job: &Job, db: &mut Db) -> Result<ResetPreview> {
    let mut s = job.lock().unwrap();
    if s.running {
        anyhow::bail!("an import is running; wait for it to finish, then start over");
    }
    let r = reset(db, false)?;
    *s = JobState::default();
    Ok(r)
}

/// Recompute metrics from the data already stored and rebuild every model, without
/// downloading or reading files (the web UI's "Retrain now"). Refused while an import runs;
/// holds the job lock throughout so an import cannot start halfway through.
pub fn retrain_unless_running(job: &Job, db: &mut Db) -> Result<serde_json::Value> {
    let s = job.lock().unwrap();
    if s.running {
        anyhow::bail!("an import is running; it retrains every model when it finishes");
    }
    let t = std::time::Instant::now();
    let derive = derive::run(db)?;
    let train = eenkah_learn::train(db)?;
    drop(s);
    Ok(serde_json::json!({
        "secs": t.elapsed().as_secs_f64(),
        "metrics": {"series": derive.series_written, "points": derive.points_written, "activities": derive.activities},
        "train": train.targets,
        "personal": train.personal,
        "finished": chrono::Utc::now().timestamp(),
    }))
}

/// Start a background import unless one is running. Returns false if one already is.
pub fn start(job: &Job, dir: DataDir, o: ImportOpts) -> bool {
    {
        let mut s = job.lock().unwrap();
        if s.running {
            return false;
        }
        *s = JobState {
            running: true,
            started: Some(chrono::Utc::now().timestamp()),
            phases: Phase::ALL
                .iter()
                .map(|p| PhaseRow {
                    phase: *p,
                    label: p.label(),
                    state: "pending",
                    started: None,
                    finished: None,
                })
                .collect(),
            ..Default::default()
        };
    }
    let job = job.clone();
    std::thread::spawn(move || {
        let on = |u: &Update| {
            let mut s = job.lock().unwrap();
            match u {
                Update::Phase(p) => s.advance("done", Some(*p)),
                Update::Note(m) => s.note(m),
                Update::Download(p) => s.download = Some((*p).clone()),
            }
        };
        let res = Db::open(&dir).and_then(|mut db| run_import(&dir, &mut db, &o, &mut |u| on(u)));
        let mut s = job.lock().unwrap();
        s.running = false;
        s.finished = Some(chrono::Utc::now().timestamp());
        match res {
            Ok(out) => {
                s.advance("done", None);
                s.note("finished");
                s.summary = Some(summary(&out));
            }
            Err(e) => {
                s.advance("failed", None);
                s.error = Some(format!("{e:#}"));
                s.note("failed");
            }
        }
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder() -> (tempfile::TempDir, Db) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = DataDir::new(tmp.path()).unwrap();
        let db = Db::open(&dir).unwrap();
        db.conn
            .execute_batch(
                "INSERT INTO raw_file VALUES ('abcd', 'fit', '', 'a.fit', 10, 0, 1, 'ok', NULL, NULL);
                 INSERT INTO day_value VALUES ('2026-10-01', 'rhr', 0, 50, NULL, 'abcd');
                 INSERT INTO mark(day, kind, created) VALUES ('2026-10-01', 'sick', 0);",
            )
            .unwrap();
        db.set_setting("tz", "Europe/Madrid").unwrap();
        db.set_setting("connect_account", r#"{"id":"old","name":null}"#).unwrap();
        for f in ["raw/ab/abcd", "connect/inbox/Sleep/s.json", "connect/state.json", "connect/tokens.json", "models/m.apr"] {
            let p = tmp.path().join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
        }
        (tmp, db)
    }

    #[test]
    fn start_over_deletes_downloads_and_keeps_what_the_athlete_entered() {
        let (tmp, mut db) = folder();
        let p = reset(&mut db, false).unwrap();
        assert_eq!((p.files, p.days, p.account.map(|a| a.id)), (1, 1, Some("old".to_string())));
        let n = |sql: &str| db.conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!((n("SELECT count(*) FROM raw_file"), n("SELECT count(*) FROM day_value")), (0, 0));
        assert_eq!(n("SELECT count(*) FROM mark"), 1, "marks are kept");
        assert_eq!(db.setting("tz").unwrap().as_deref(), Some("Europe/Madrid"));
        assert!(data_account(&db).unwrap().is_none(), "the folder no longer belongs to an account");
        for gone in ["raw/ab/abcd", "connect/inbox", "connect/state.json", "models"] {
            assert!(!tmp.path().join(gone).exists(), "{gone} should be gone");
        }
        assert!(tmp.path().join("connect/tokens.json").exists(), "still signed in");
        assert!(tmp.path().join("raw").is_dir());
    }

    #[test]
    fn failures_are_grouped_by_cause_and_only_temporary_ones_promise_a_retry() {
        let errs: Vec<String> = (0..3)
            .map(|i| format!("activity {i}: HTTP 406 for /download-service/files/activity/{i}"))
            .chain(["Sleep 2026-10-01: HTTP 503 from x".to_string()])
            .collect();
        let g = error_groups(&errs);
        assert_eq!((g[0].what, g[0].count, g[0].transient), ("activity files", 3, false));
        assert_eq!((g[1].what, g[1].cause.as_str(), g[1].transient), ("sleep", "HTTP 503", true));
    }

    #[test]
    fn start_over_with_all_leaves_a_fresh_folder() {
        let (tmp, mut db) = folder();
        reset(&mut db, true).unwrap();
        let n = |sql: &str| db.conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!((n("SELECT count(*) FROM mark"), n("SELECT count(*) FROM setting")), (0, 0));
        assert!(!tmp.path().join("connect/tokens.json").exists(), "signed out");
    }

    #[test]
    fn start_over_is_refused_while_an_import_runs() {
        let (_tmp, mut db) = folder();
        let job: Job = Default::default();
        job.lock().unwrap().running = true;
        assert!(reset_unless_running(&job, &mut db).is_err());
        assert_eq!(db.conn.query_row("SELECT count(*) FROM raw_file", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    }
}
