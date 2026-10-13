//! The personal models, rebuilt from the athlete's data after every import (LRN-2, LRN-3):
//!
//! 1. normal ranges per body signal ([`crate::normal`]),
//! 3. shifts in a signal's normal level ([`crate::shift`]),
//! 2. unusual days ([`crate::unusual`]),
//! 6. session types ([`crate::sessions`]).
//!
//! Shifts run first because the normal ranges restart after a shift, and unusual days are
//! scored on distances from the normal ranges. Only results are stored; every model is fitted in
//! memory and dropped. The views read the stored results and never fit anything (VIEW-4).

use crate::normal::{self, Band, SIGNALS};
use crate::sessions::{self, Labelled, Session, SportReport};
use crate::shift::{self, Shift};
use crate::unusual::{self, DayScore};
use crate::SEED;
use anyhow::Result;
use chrono::{Duration, NaiveDate};
use eenkah_core::findings::ShiftNote;
use eenkah_store::{derive, Db};
use rusqlite::params;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS day_normal(
  day TEXT NOT NULL, signal TEXT NOT NULL, value REAL, mid REAL NOT NULL, lo REAL NOT NULL, hi REAL NOT NULL,
  status TEXT, dev REAL, n INTEGER NOT NULL, PRIMARY KEY(day, signal)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS shift(
  signal TEXT NOT NULL, day TEXT NOT NULL, detected TEXT NOT NULL, before REAL NOT NULL, after REAL NOT NULL,
  score REAL NOT NULL, PRIMARY KEY(signal, day)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS day_unusual(
  day TEXT PRIMARY KEY, pct REAL NOT NULL, lof_pct REAL NOT NULL, unusual INTEGER NOT NULL, agree INTEGER NOT NULL,
  drivers_json TEXT NOT NULL) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS session_type(
  activity_id INTEGER PRIMARY KEY, label TEXT NOT NULL, source TEXT NOT NULL, grp INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS session_label(
  activity_id INTEGER PRIMARY KEY, label TEXT NOT NULL, created INTEGER NOT NULL);
";

/// The `model_run` targets of the personal models.
pub const RUNS: [&str; 4] = ["normal", "shift", "unusual", "sessions"];

pub fn ensure_schema(db: &Db) -> Result<()> {
    db.conn.execute_batch(SCHEMA)?;
    Ok(())
}

/// One signal's normal ranges, in display units, and its distance from normal per day.
struct SignalResult {
    shifts: Vec<Shift>,
    bands: BTreeMap<NaiveDate, Band>,
    model: eenkah_core::Daily,
}

fn hash_inputs(parts: &[(&str, &eenkah_core::Daily)], sessions: &[Session], labels: &BTreeMap<i64, String>) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    for (name, s) in parts {
        h.update(name.as_bytes());
        for (d, v) in s.iter() {
            h.update(d.to_string());
            h.update(v.to_le_bytes());
        }
    }
    for s in sessions {
        h.update(s.id.to_le_bytes());
        for v in s.x {
            h.update(v.to_le_bytes());
        }
    }
    for (id, l) in labels {
        h.update(id.to_le_bytes());
        h.update(l.as_bytes());
    }
    hex::encode(h.finalize())
}

/// Workouts with heart-rate zones, as session-type features, grouped by sport.
fn load_sessions(db: &Db) -> Result<BTreeMap<String, Vec<Session>>> {
    let mut st = db.conn.prepare(
        "SELECT a.id, coalesce(a.sport, a.type_key, 'other'), coalesce(a.moving_s, a.duration_s, 0), a.start_utc,
                (SELECT value FROM activity_metric m WHERE m.activity_id=a.id AND m.metric='load'),
                (SELECT value FROM activity_metric m WHERE m.activity_id=a.id AND m.metric='thr.lthr')
         FROM activity a ORDER BY a.start_utc",
    )?;
    let acts: Vec<(i64, String, f64, i64, Option<f64>, Option<f64>)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut zq = db.conn.prepare("SELECT metric, value FROM activity_metric WHERE activity_id=?1 AND metric LIKE 'zone%_s'")?;
    let mut hq = db.conn.prepare("SELECT t, hr FROM activity_sample WHERE activity_id=?1 AND hr IS NOT NULL ORDER BY t")?;
    let mut out: BTreeMap<String, Vec<Session>> = BTreeMap::new();
    for (id, sport, moving_s, start, load, lthr) in acts {
        let zones: HashMap<String, f64> = zq.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        let z: Vec<f64> = (1..=5).map(|k| zones.get(&format!("zone{k}_s")).copied().unwrap_or(0.0)).collect();
        let total: f64 = z.iter().sum();
        let (Some(load), Some(lthr)) = (load, lthr) else { continue };
        if total < 600.0 || moving_s < 600.0 {
            continue; // under 10 minutes of heart rate: too little to describe
        }
        let hr: Vec<(f64, f64)> = hq
            .query_map([id], |r| Ok(((r.get::<_, i64>(0)? - start) as f64, r.get::<_, f64>(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let hours = moving_s / 3600.0;
        let efforts = sessions::hard_efforts(&hr, lthr) as f64;
        out.entry(sport.clone()).or_default().push(Session {
            id,
            sport,
            x: [z[0] / total, z[1] / total, z[2] / total, z[3] / total, z[4] / total, hours, load / hours, efforts / hours],
        });
    }
    Ok(out)
}

pub fn corrections(db: &Db) -> Result<BTreeMap<i64, String>> {
    let mut st = db.conn.prepare("SELECT activity_id, label FROM session_label")?;
    let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The athlete's correction of a session's type; `None` removes it. Rebuilds the session types.
pub fn set_label(db: &mut Db, activity_id: i64, label: Option<&str>) -> Result<()> {
    crate::ensure_schema(db)?;
    match label {
        Some(l) => {
            anyhow::ensure!(sessions::LABELS.contains(&l), "label must be one of {}", sessions::LABELS.join(", "));
            db.conn.execute(
                "INSERT OR REPLACE INTO session_label(activity_id, label, created) VALUES (?1, ?2, strftime('%s','now'))",
                params![activity_id, l],
            )?;
        }
        None => {
            db.conn.execute("DELETE FROM session_label WHERE activity_id=?1", [activity_id])?;
        }
    }
    run_sessions(db)?;
    Ok(())
}

/// Plain words for a day's distance from normal, e.g. "resting heart rate 6 bpm above your normal".
fn driver_words(sig: &normal::Signal, value_model: f64, b: &Band) -> String {
    let dir = if value_model >= b.mid { "above" } else { "below" };
    let label = sig.phrase;
    if sig.log {
        let pct = ((value_model - b.mid).exp() - 1.0).abs() * 100.0;
        return format!("{label} {pct:.0} % {dir} your normal");
    }
    let diff = (sig.to_display(value_model) - sig.to_display(b.mid)).abs();
    let unit = if sig.unit.is_empty() { " points".to_string() } else { format!(" {}", sig.unit) };
    format!("{label} {diff:.prec$}{unit} {dir} your normal", prec = sig.digits)
}

struct Timed<T> {
    value: T,
    secs: f64,
}

fn timed<T>(f: impl FnOnce() -> Result<T>) -> Result<Timed<T>> {
    let t0 = std::time::Instant::now();
    let value = f()?;
    Ok(Timed {
        value,
        secs: t0.elapsed().as_secs_f64(),
    })
}

#[allow(clippy::too_many_arguments)]
fn record_run(tx: &rusqlite::Transaction, target: &str, secs: f64, rows: usize, hash: &str, scores: &Value, champion: &str, coverage: Option<f64>, lineage: &Value) -> Result<()> {
    tx.execute(
        "INSERT INTO model_run(target, finished, secs, peak_kb, rows, data_hash, scores_json, champion, coverage, promoted, lineage_json) VALUES (?1, strftime('%s','now'), ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9)",
        params![target, secs, crate::peak_kb(), rows as i64, hash, scores.to_string(), champion, coverage, lineage.to_string()],
    )?;
    Ok(())
}

fn lineage(params: Value) -> Value {
    json!({"seed": SEED, "eenkah": env!("CARGO_PKG_VERSION"), "aprender": "0.70.3", "params": params})
}

/// Rebuild models 3, 1 and 2 (in that order) and model 6, store their results in one
/// transaction, and return a short report.
pub fn run(db: &mut Db) -> Result<Value> {
    ensure_schema(db)?;
    let raw: Vec<(&'static normal::Signal, eenkah_core::Daily)> = SIGNALS.iter().map(|s| Ok((s, db.daily(s.key)?))).collect::<Result<_>>()?;
    let sick = db.marks("sick")?;
    let sport_sessions = load_sessions(db)?;
    let labels = corrections(db)?;
    let all_sessions: Vec<Session> = sport_sessions.values().flatten().cloned().collect();
    let hash = hash_inputs(&raw.iter().map(|(s, d)| (s.key, d)).collect::<Vec<_>>(), &all_sessions, &labels);

    // 3 then 1: shifts, then normal ranges that restart after them
    let shifts_t = timed(|| Ok(raw.iter().map(|(s, d)| shift::detect(&s.model_series(d))).collect::<Vec<_>>()))?;
    let normal_t = timed(|| {
        Ok(raw
            .iter()
            .zip(&shifts_t.value)
            .map(|((s, d), sh)| {
                let model = s.model_series(d);
                let starts: Vec<(NaiveDate, NaiveDate)> = sh.iter().map(|x| (x.day, x.detected)).collect();
                let bands = normal::bands(&model, model.keys().copied(), &starts);
                SignalResult { shifts: sh.clone(), bands, model }
            })
            .collect::<Vec<_>>())
    })?;
    let results = &normal_t.value;

    // 2: unusual days, on distances from normal; HRV and resting HR must both be there
    let mut rows: Vec<(NaiveDate, Vec<f64>)> = vec![];
    let all_days: std::collections::BTreeSet<NaiveDate> = results.iter().flat_map(|r| r.bands.keys().copied()).collect();
    for d in &all_days {
        let devs: Vec<Option<f64>> = results.iter().map(|r| Some((r.model.get(d)? - r.bands.get(d)?.mid) / r.bands.get(d)?.scale)).collect();
        if devs[0].is_none() || devs[1].is_none() {
            continue;
        }
        rows.push((*d, devs.iter().map(|x| x.unwrap_or(0.0)).collect()));
    }
    let unusual_t = timed(|| unusual::score(&rows, SEED))?;

    // 6: session types per sport
    let sessions_t = timed(|| {
        let mut labelled: Vec<Labelled> = vec![];
        let mut reports: Vec<SportReport> = vec![];
        for (sport, list) in &sport_sessions {
            let (l, r) = sessions::classify(sport, list, &labels, SEED)?;
            labelled.extend(l);
            reports.push(r);
        }
        Ok((labelled, reports))
    })?;

    // ---- one transaction for every result (LRN-1)
    let tx = db.conn.transaction()?;
    tx.execute_batch("DELETE FROM day_normal; DELETE FROM shift; DELETE FROM day_unusual;")?;
    let (mut inside, mut judged) = (0usize, 0usize);
    let mut coverage: BTreeMap<&str, f64> = BTreeMap::new();
    {
        let mut ins = tx.prepare("INSERT INTO day_normal(day, signal, value, mid, lo, hi, status, dev, n) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)")?;
        for ((sig, _), r) in raw.iter().zip(results) {
            let (mut si, mut sj) = (0usize, 0usize);
            for (d, b) in &r.bands {
                let v = r.model.get(d).copied();
                let st = v.map(|v| normal::status(v, b));
                if let Some(s) = st {
                    sj += 1;
                    si += (s == "within") as usize;
                }
                ins.execute(params![
                    d.to_string(),
                    sig.name,
                    v.map(|v| sig.to_display(v)),
                    sig.to_display(b.mid),
                    sig.to_display(b.lo),
                    sig.to_display(b.hi),
                    st,
                    v.map(|v| (v - b.mid) / b.scale),
                    b.n as i64
                ])?;
            }
            if sj > 0 {
                coverage.insert(sig.name, si as f64 / sj as f64);
            }
            inside += si;
            judged += sj;
        }
        let mut ins = tx.prepare("INSERT INTO shift(signal, day, detected, before, after, score) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")?;
        for ((sig, _), r) in raw.iter().zip(results) {
            for s in &r.shifts {
                ins.execute(params![sig.name, s.day.to_string(), s.detected.to_string(), sig.to_display(s.before), sig.to_display(s.after), s.score])?;
            }
        }
        let mut ins = tx.prepare("INSERT INTO day_unusual(day, pct, lof_pct, unusual, agree, drivers_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")?;
        for s in &unusual_t.value {
            let mut drivers: Vec<(f64, String)> = raw
                .iter()
                .zip(results)
                .filter_map(|((sig, _), r)| {
                    let (v, b) = (r.model.get(&s.day)?, r.bands.get(&s.day)?);
                    let dev = (v - b.mid) / b.scale;
                    (dev.abs() >= 1.0).then(|| (dev.abs(), driver_words(sig, *v, b)))
                })
                .collect();
            drivers.sort_by(|a, b| b.0.total_cmp(&a.0));
            let words: Vec<String> = drivers.into_iter().take(3).map(|x| x.1).collect();
            ins.execute(params![s.day.to_string(), s.pct, s.lof_pct, s.unusual as i64, s.agree as i64, serde_json::to_string(&words)?])?;
        }
    }
    let (labelled, reports) = &sessions_t.value;
    write_session_types(&tx, labelled)?;

    let n_days = all_days.len();
    let total_cov = (judged > 0).then(|| inside as f64 / judged as f64);
    let n_shifts: usize = results.iter().map(|r| r.shifts.len()).sum();
    let flagged = unusual_t.value.iter().filter(|s| s.unusual).count();
    let sick_eval = sick_lead(&unusual_t.value, &sick);
    record_run(
        &tx,
        "normal",
        normal_t.secs,
        n_days,
        &hash,
        &json!({"inside_range": coverage}),
        "NormalInverseGamma",
        total_cov,
        &lineage(json!({"window_days": normal::WINDOW_DAYS, "min_prior": normal::MIN_PRIOR, "level": normal::LEVEL})),
    )?;
    record_run(
        &tx,
        "shift",
        shifts_t.secs,
        n_days,
        &hash,
        &json!({"shifts": n_shifts}),
        "DriftDetector",
        None,
        &lineage(json!({"current_days": shift::CURRENT_DAYS, "reference_days": shift::REFERENCE_DAYS, "shift_sd": shift::SHIFT_SD})),
    )?;
    record_run(
        &tx,
        "unusual",
        unusual_t.secs,
        unusual_t.value.len(),
        &hash,
        &json!({"flagged": flagged, "flag_rate": flagged as f64 / unusual_t.value.len().max(1) as f64, "sick_episodes": sick_eval.0, "sick_preceded": sick_eval.1}),
        "IsolationForest+LocalOutlierFactor",
        None,
        &lineage(json!({"min_train": unusual::MIN_TRAIN, "block": unusual::BLOCK, "flag_pct": unusual::FLAG_PCT, "trees": unusual::TREES})),
    )?;
    record_run(
        &tx,
        "sessions",
        sessions_t.secs,
        labelled.len(),
        &hash,
        &serde_json::to_value(reports)?,
        if reports.iter().any(|r| r.forest_used) { "RandomForestClassifier" } else { "KMeans" },
        None,
        &lineage(json!({"min_sessions": sessions::MIN_SESSIONS, "min_corrections": sessions::MIN_CORRECTIONS, "features": sessions::FEATURES})),
    )?;
    tx.commit()?;
    write_series(db)?;
    Ok(json!({
        "days": n_days,
        "inside_normal_range": total_cov,
        "shifts": n_shifts,
        "unusual_days": flagged,
        "sessions_labelled": labelled.len(),
        "secs": normal_t.secs + shifts_t.secs + unusual_t.secs + sessions_t.secs,
    }))
}

fn write_session_types(tx: &rusqlite::Transaction, labelled: &[Labelled]) -> Result<()> {
    tx.execute("DELETE FROM session_type", [])?;
    let mut ins = tx.prepare("INSERT INTO session_type(activity_id, label, source, grp) VALUES (?1, ?2, ?3, ?4)")?;
    for l in labelled {
        ins.execute(params![l.id, l.label, l.source, l.group as i64])?;
    }
    Ok(())
}

/// Rebuild only the session types (after a correction).
fn run_sessions(db: &mut Db) -> Result<()> {
    let sport_sessions = load_sessions(db)?;
    let labels = corrections(db)?;
    let t = timed(|| {
        let mut labelled = vec![];
        let mut reports = vec![];
        for (sport, list) in &sport_sessions {
            let (l, r) = sessions::classify(sport, list, &labels, SEED)?;
            labelled.extend(l);
            reports.push(r);
        }
        Ok((labelled, reports))
    })?;
    let all: Vec<Session> = sport_sessions.values().flatten().cloned().collect();
    let hash = hash_inputs(&[], &all, &labels);
    let tx = db.conn.transaction()?;
    write_session_types(&tx, &t.value.0)?;
    record_run(
        &tx,
        "sessions",
        t.secs,
        t.value.0.len(),
        &hash,
        &serde_json::to_value(&t.value.1)?,
        if t.value.1.iter().any(|r| r.forest_used) { "RandomForestClassifier" } else { "KMeans" },
        None,
        &lineage(json!({"min_sessions": sessions::MIN_SESSIONS, "min_corrections": sessions::MIN_CORRECTIONS, "features": sessions::FEATURES})),
    )?;
    tx.commit()?;
    Ok(())
}

/// Illness marks as episodes (runs of consecutive marked days): how many there are, and how many
/// had an unusual day in the 3 days before or on their first day.
fn sick_lead(scores: &[DayScore], sick: &eenkah_core::Daily) -> (usize, usize) {
    let flagged: std::collections::BTreeSet<NaiveDate> = scores.iter().filter(|s| s.unusual).map(|s| s.day).collect();
    let starts: Vec<NaiveDate> = sick.keys().filter(|d| !sick.contains_key(&(**d - Duration::days(1)))).copied().collect();
    let hit = starts.iter().filter(|s| flagged.range(**s - Duration::days(3)..=**s).next().is_some()).count();
    (starts.len(), hit)
}

/// Normal ranges and the unusual-day score as suyo series, so dashboards can draw them.
fn write_series(db: &Db) -> Result<()> {
    let tz = db.tz();
    let mut store = db.suyo()?;
    let mut pts = vec![];
    let mut hints = HashMap::new();
    for sig in SIGNALS {
        for part in ["lo", "hi"] {
            let name = format!("eenkah.normal.{}.{part}", sig.name);
            store.delete_series(&name)?;
            hints.insert(
                name,
                suyo::store::MetaHint {
                    unit: Some(sig.unit.into()),
                    agg: Some(suyo::store::AggFn::Avg),
                    interval: Some(86400),
                    description: Some(format!("{}: your normal range, {} edge", sig.label, if part == "lo" { "low" } else { "high" })),
                },
            );
        }
    }
    store.delete_series("eenkah.unusual.score")?;
    hints.insert(
        "eenkah.unusual.score".into(),
        suyo::store::MetaHint {
            unit: Some("%".into()),
            agg: Some(suyo::store::AggFn::Max),
            interval: Some(86400),
            description: Some("How unusual the night was: share of your own past days it exceeds".into()),
        },
    );
    let mut st = db.conn.prepare("SELECT day, signal, lo, hi FROM day_normal")?;
    for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, f64>(2)?, r.get::<_, f64>(3)?)))? {
        let (d, s, lo, hi) = r?;
        let Ok(day) = NaiveDate::parse_from_str(&d, "%Y-%m-%d") else { continue };
        let ts = derive::local_midnight(tz, day);
        pts.push((format!("eenkah.normal.{s}.lo"), ts, lo));
        pts.push((format!("eenkah.normal.{s}.hi"), ts, hi));
    }
    let mut st = db.conn.prepare("SELECT day, pct FROM day_unusual")?;
    for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))? {
        let (d, p) = r?;
        let Ok(day) = NaiveDate::parse_from_str(&d, "%Y-%m-%d") else { continue };
        pts.push(("eenkah.unusual.score".into(), derive::local_midnight(tz, day), p * 100.0));
    }
    store.write(&pts, &hints)?;
    Ok(())
}

// ---------------------------------------------------------------- reading the results

fn run_info(db: &Db, target: &str) -> Result<Option<Value>> {
    Ok(db
        .conn
        .query_row(
            "SELECT secs, rows, scores_json, champion, coverage, peak_kb, finished FROM model_run WHERE target=?1 ORDER BY id DESC LIMIT 1",
            [target],
            |r| {
                Ok(json!({
                    "secs": r.get::<_, f64>(0)?, "rows": r.get::<_, i64>(1)?,
                    "scores": serde_json::from_str::<Value>(&r.get::<_, String>(2)?).unwrap_or(Value::Null),
                    "method": r.get::<_, String>(3)?, "coverage": r.get::<_, Option<f64>>(4)?, "peak_memory_kb": r.get::<_, Option<i64>>(5)?,
                    "finished": r.get::<_, i64>(6)?,
                }))
            },
        )
        .ok())
}

/// The newest night's state against normal, shifts, unusual days and session types, read from
/// the stored results (no model runs here: VIEW-4).
pub fn summary(db: &Db) -> Result<Value> {
    crate::ensure_schema(db)?;
    let newest: Option<String> = db.conn.query_row("SELECT max(day) FROM day_normal WHERE value IS NOT NULL", [], |r| r.get(0))?;
    let mut signals = vec![];
    if let Some(day) = &newest {
        let mut st = db
            .conn
            .prepare("SELECT day, value, mid, lo, hi, status FROM day_normal WHERE signal=?1 AND value IS NOT NULL AND day<=?2 ORDER BY day DESC LIMIT 1")?;
        for sig in SIGNALS {
            let row = st
                .query_row(params![sig.name, day], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, f64>(1)?,
                        r.get::<_, f64>(2)?,
                        r.get::<_, f64>(3)?,
                        r.get::<_, f64>(4)?,
                        r.get::<_, String>(5)?,
                    ))
                })
                .ok();
            let Some((d, v, mid, lo, hi, status)) = row else { continue };
            if d != *day {
                continue; // only signals measured on the newest night
            }
            let since = (NaiveDate::parse_from_str(day, "%Y-%m-%d")? - Duration::days(364)).to_string();
            let (n, inside): (i64, i64) = db.conn.query_row(
                "SELECT count(*), coalesce(sum(status='within'), 0) FROM day_normal WHERE signal=?1 AND status IS NOT NULL AND day>=?2",
                params![sig.name, since],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            signals.push(json!({
                "signal": sig.name, "label": sig.label, "unit": sig.unit, "better": sig.better, "digits": sig.digits, "series": sig.series,
                "day": d, "value": v, "mid": mid, "lo": lo, "hi": hi, "status": status,
                "inside_last_year": (n > 0).then(|| inside as f64 / n as f64), "days_last_year": n,
            }));
        }
    }
    let mut st = db.conn.prepare("SELECT signal, day, detected, before, after, score FROM shift ORDER BY day DESC")?;
    let shifts: Vec<Value> = st
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, f64>(3)?,
                r.get::<_, f64>(4)?,
                r.get::<_, f64>(5)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .filter_map(|(s, day, det, before, after, score)| {
            let sig = normal::signal(&s)?;
            Some(
                json!({"signal": s, "label": sig.label, "unit": sig.unit, "digits": sig.digits, "series": sig.series, "day": day, "detected": det,
                "before": before, "after": after, "direction": if after >= before { "up" } else { "down" }, "score": score}),
            )
        })
        .collect();
    let day_row = |r: &rusqlite::Row| -> rusqlite::Result<Value> {
        Ok(
            json!({"day": r.get::<_, String>(0)?, "pct": r.get::<_, f64>(1)?, "lof_pct": r.get::<_, f64>(2)?, "unusual": r.get::<_, i64>(3)? == 1,
            "agree": r.get::<_, i64>(4)? == 1, "drivers": serde_json::from_str::<Value>(&r.get::<_, String>(5)?).unwrap_or(Value::Null)}),
        )
    };
    let latest = db
        .conn
        .query_row("SELECT day, pct, lof_pct, unusual, agree, drivers_json FROM day_unusual ORDER BY day DESC LIMIT 1", [], day_row)
        .ok();
    let mut st = db
        .conn
        .prepare("SELECT day, pct, lof_pct, unusual, agree, drivers_json FROM day_unusual WHERE unusual=1 ORDER BY day DESC LIMIT 30")?;
    let recent: Vec<Value> = st.query_map([], day_row)?.collect::<rusqlite::Result<_>>()?;
    let mut st = db.conn.prepare("SELECT label, source, count(*) FROM session_type GROUP BY label, source ORDER BY label")?;
    let counts: Vec<Value> = st
        .query_map([], |r| Ok(json!({"label": r.get::<_, String>(0)?, "source": r.get::<_, String>(1)?, "sessions": r.get::<_, i64>(2)?})))?
        .collect::<rusqlite::Result<_>>()?;
    let runs: BTreeMap<&str, Option<Value>> = RUNS.iter().map(|t| Ok((*t, run_info(db, t)?))).collect::<Result<_>>()?;
    Ok(json!({
        "day": newest,
        "signals": signals,
        "shifts": shifts,
        "unusual": {"latest": latest, "recent": recent},
        "sessions": {"labels": sessions::LABELS, "counts": counts, "corrections": corrections(db)?.len()},
        "runs": runs,
        "disclaimer": "Not medical advice.",
    }))
}

/// The newest night's unusual-day drivers, if that night was unusual, for the findings.
pub fn unusual_on(db: &Db, day: NaiveDate) -> Result<Option<Vec<String>>> {
    ensure_schema(db)?;
    let r: Option<String> = db
        .conn
        .query_row("SELECT drivers_json FROM day_unusual WHERE day=?1 AND unusual=1", [day.to_string()], |r| r.get(0))
        .ok();
    Ok(r.and_then(|s| serde_json::from_str(&s).ok()))
}

/// Shifts that started within `days` before `day`, for the findings.
pub fn recent_shifts(db: &Db, day: NaiveDate, days: i64) -> Result<Vec<ShiftNote>> {
    ensure_schema(db)?;
    let mut st = db.conn.prepare("SELECT signal, day, before, after FROM shift WHERE day>=?1 AND detected<=?2 ORDER BY day DESC")?;
    let rows = st.query_map(params![(day - Duration::days(days)).to_string(), day.to_string()], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, f64>(2)?, r.get::<_, f64>(3)?))
    })?;
    let mut out = vec![];
    for r in rows {
        let (s, since, before, after) = r?;
        let Some(sig) = normal::signal(&s) else { continue };
        let Ok(d) = NaiveDate::parse_from_str(&since, "%Y-%m-%d") else { continue };
        out.push(ShiftNote {
            label: sig.phrase,
            series: sig.series,
            unit: sig.unit,
            direction: if after >= before { "up" } else { "down" },
            change: (after - before).abs(),
            days_ago: (day - d).num_days(),
        });
    }
    Ok(out)
}

/// The CLI's text for the personal models.
pub fn text(db: &Db) -> Result<String> {
    let v = summary(db)?;
    let mut s = String::new();
    let Some(day) = v["day"].as_str() else {
        return Ok("Personal models: not built yet (run `eenkah train`).\n".into());
    };
    let today = chrono::Utc::now().with_timezone(&db.tz()).date_naive();
    let rel = |d: &str| NaiveDate::parse_from_str(d, "%Y-%m-%d").map(|d| eenkah_core::when::day(d, today)).unwrap_or_else(|_| d.to_string());
    s += &format!("Last night against your normal (night ending {}; 80 % ranges from your previous 60 nights):\n", rel(day));
    for x in v["signals"].as_array().into_iter().flatten() {
        let d = x["digits"].as_u64().unwrap_or(0) as usize;
        s += &format!(
            "  {:<22} {:>7.d$} {:<5} normal {:.d$}–{:.d$}  {}\n",
            x["label"].as_str().unwrap_or(""),
            x["value"].as_f64().unwrap_or(f64::NAN),
            x["unit"].as_str().unwrap_or(""),
            x["lo"].as_f64().unwrap_or(f64::NAN),
            x["hi"].as_f64().unwrap_or(f64::NAN),
            x["status"].as_str().unwrap_or("")
        );
    }
    let latest = &v["unusual"]["latest"];
    if latest["unusual"].as_bool() == Some(true) {
        let drivers: Vec<&str> = latest["drivers"].as_array().into_iter().flatten().filter_map(|x| x.as_str()).collect();
        s += &format!("The night ending {} was unusual for you: {}.\n", rel(latest["day"].as_str().unwrap_or("")), drivers.join("; "));
    }
    let shifts = v["shifts"].as_array().cloned().unwrap_or_default();
    s += &format!("Shifts in your normal: {}.\n", shifts.len());
    for x in shifts.iter().take(5) {
        let d = x["digits"].as_u64().unwrap_or(0) as usize;
        s += &format!(
            "  {} {} from {:.d$} to {:.d$} {}, starting about {}\n",
            x["label"].as_str().unwrap_or(""),
            x["direction"].as_str().unwrap_or(""),
            x["before"].as_f64().unwrap_or(f64::NAN),
            x["after"].as_f64().unwrap_or(f64::NAN),
            x["unit"].as_str().unwrap_or(""),
            rel(x["day"].as_str().unwrap_or(""))
        );
    }
    let counts: Vec<String> = v["sessions"]["counts"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| format!("{} {} ({})", c["sessions"], c["label"].as_str().unwrap_or(""), c["source"].as_str().unwrap_or("")))
        .collect();
    if !counts.is_empty() {
        s += &format!("Session types: {}.\n", counts.join(", "));
    }
    let secs: f64 = RUNS.iter().filter_map(|t| v["runs"][t]["secs"].as_f64()).sum();
    s += &format!("Rebuilt from your data in {secs:.2} s. Not medical advice.\n");
    Ok(s)
}
