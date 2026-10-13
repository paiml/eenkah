//! eenkah-learn: the self-learning loop (EENKAH-SPEC §7.4).
//!
//! build features → walk-forward backtest every family → promote a learned model only if it
//! beats every baseline (FC-2) → store forecasts before the outcome is known (FC-1) → fill in
//! the truth when it arrives → show the track record on the Model view.

#![allow(clippy::type_complexity)]

pub mod interval;
pub mod model;
pub mod normal;
pub mod personal;
pub mod sessions;
pub mod shift;
pub mod unusual;

use anyhow::{Context, Result};
use chrono::{Duration, NaiveDate};
use eenkah_core::features::{self, FEATURES};
use eenkah_core::stats::mean;
use eenkah_core::{window, Daily};
use eenkah_store::{derive, Db};
use model::{Family, Fitted, Row};
use rusqlite::params;
use serde::Serialize;
use serde_json::json;
use std::collections::{BTreeMap, HashMap};

pub const SEED: u64 = 20261010;
/// Rows needed before the first backtest prediction, and the retraining step (walk-forward).
pub const MIN_TRAIN: usize = 120;
pub const STEP: usize = 28;
/// 80 % intervals.
pub const ALPHA: f64 = 0.2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Target {
    HrvNext,
    RhrNext,
}

impl Target {
    pub const ALL: [Target; 2] = [Target::HrvNext, Target::RhrNext];
    pub fn key(self) -> &'static str {
        match self {
            Target::HrvNext => "hrv_next",
            Target::RhrNext => "rhr_next",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Target::HrvNext => "Tomorrow morning's HRV",
            Target::RhrNext => "Tomorrow morning's resting heart rate",
        }
    }
    pub fn unit(self) -> &'static str {
        match self {
            Target::HrvNext => "ms",
            Target::RhrNext => "bpm",
        }
    }
    fn series(self, i: &features::Inputs) -> &Daily {
        match self {
            Target::HrvNext => &i.hrv_ln,
            Target::RhrNext => &i.rhr,
        }
    }
    /// Model space → what the athlete reads (HRV is modelled as ln ms).
    pub fn display(self, v: f64) -> f64 {
        match self {
            Target::HrvNext => v.exp(),
            Target::RhrNext => v,
        }
    }
}

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS forecast(
  target TEXT NOT NULL, for_day TEXT NOT NULL, kind TEXT NOT NULL, made_on TEXT NOT NULL, family TEXT NOT NULL,
  model_version TEXT NOT NULL, value REAL NOT NULL, lo REAL, hi REAL, data_age_days INTEGER NOT NULL,
  truth REAL, drivers_json TEXT, PRIMARY KEY(target, for_day, kind));
CREATE TABLE IF NOT EXISTS model_run(
  id INTEGER PRIMARY KEY, target TEXT NOT NULL, finished INTEGER NOT NULL, secs REAL NOT NULL, peak_kb INTEGER,
  rows INTEGER NOT NULL, data_hash TEXT NOT NULL, scores_json TEXT NOT NULL, champion TEXT NOT NULL,
  model_file TEXT, coverage REAL, promoted INTEGER NOT NULL DEFAULT 0, lineage_json TEXT);
";

/// Create the learning tables, and add columns that later versions introduced.
pub fn ensure_schema(db: &Db) -> Result<()> {
    db.conn.execute_batch(SCHEMA)?;
    let has_lineage: i64 = db.conn.query_row("SELECT count(*) FROM pragma_table_info('model_run') WHERE name='lineage_json'", [], |r| r.get(0))?;
    if has_lineage == 0 {
        db.conn.execute_batch("ALTER TABLE model_run ADD COLUMN lineage_json TEXT")?;
    }
    personal::ensure_schema(db)
}

/// A target's rows, in date order: features on the morning of `d`, target on `d + 1`.
pub fn dataset(i: &features::Inputs, t: &features::Table, target: Target) -> (Vec<NaiveDate>, Vec<Row>) {
    let s = target.series(i);
    let mut dates = vec![];
    let mut rows = vec![];
    for (d, x) in &t.rows {
        let (Some(y), Some(last)) = (s.get(&(*d + Duration::days(1))), s.get(d)) else { continue };
        let w7 = window(s, *d - Duration::days(6), *d);
        let w60 = window(s, *d - Duration::days(59), *d);
        if w7.len() < 4 || w60.len() < 20 {
            continue;
        }
        dates.push(*d);
        rows.push(Row {
            x: x.clone(),
            y: *y,
            last: *last,
            mean7: mean(&w7).unwrap(),
            mean60: mean(&w60).unwrap(),
        });
    }
    (dates, rows)
}

#[derive(Debug, Serialize)]
pub struct Backtest {
    pub test_dates: Vec<NaiveDate>,
    pub truth: Vec<f64>,
    pub preds: BTreeMap<Family, Vec<f64>>,
    pub mae: BTreeMap<Family, f64>,
    pub champion: Family,
    /// Interval for each champion prediction (model space), once enough errors exist.
    pub intervals: Vec<Option<(f64, f64)>>,
    pub coverage: Option<f64>,
    /// Half-width for the next forecast.
    pub next_half_width: Option<f64>,
}

/// Walk-forward backtest (FC-4): each block of `STEP` rows is predicted by models fitted only
/// on rows before it. All families are scored on exactly the same rows.
pub fn backtest(dates: &[NaiveDate], rows: &[Row], names: &[&str]) -> Result<Option<Backtest>> {
    if rows.len() < MIN_TRAIN + STEP {
        return Ok(None);
    }
    // Blocks are independent, so fit them in parallel; results are joined in block order, so the
    // outcome is the same as a serial run (LRN-5).
    use rayon::prelude::*;
    let blocks: Vec<(usize, usize)> = (MIN_TRAIN..rows.len()).step_by(STEP).map(|i| (i, (i + STEP).min(rows.len()))).collect();
    let jobs: Vec<(Family, usize, usize)> = Family::ALL.iter().flat_map(|f| blocks.iter().map(move |(i, j)| (*f, *i, *j))).collect();
    let out: Vec<Result<(Family, Vec<f64>)>> = jobs
        .par_iter()
        .map(|(fam, i, j)| Ok((*fam, Fitted::fit(*fam, &rows[..*i], names, SEED)?.predict(&rows[*i..*j])?)))
        .collect();
    let mut preds: BTreeMap<Family, Vec<f64>> = BTreeMap::new();
    for r in out {
        let (fam, p) = r?;
        preds.entry(fam).or_default().extend(p);
    }
    let truth: Vec<f64> = rows[MIN_TRAIN..].iter().map(|r| r.y).collect();
    let mae: BTreeMap<Family, f64> = preds
        .iter()
        .map(|(f, p)| (*f, p.iter().zip(&truth).map(|(a, b)| (a - b).abs()).sum::<f64>() / truth.len() as f64))
        .collect();
    let champion = choose(&mae);
    let mut aci = interval::Aci::new(ALPHA, 0.01, 90);
    let mut intervals = vec![];
    let (mut hit, mut n) = (0.0, 0.0);
    for (p, y) in preds[&champion].iter().zip(&truth) {
        let iv = aci.half_width().map(|h| (p - h, p + h));
        if let Some((lo, hi)) = iv {
            n += 1.0;
            hit += (*y >= lo && *y <= hi) as u8 as f64;
        }
        intervals.push(iv);
        aci.update((p - y).abs());
    }
    Ok(Some(Backtest {
        test_dates: dates[MIN_TRAIN..].to_vec(),
        truth,
        preds,
        mae,
        champion,
        intervals,
        coverage: (n > 0.0).then(|| hit / n),
        next_half_width: aci.half_width(),
    }))
}

/// FC-2 / LRN-6: a learned family is used only if it beats every baseline, and among equal
/// scores the simpler family (earlier in [`Family::ALL`]) wins.
pub fn choose(mae: &BTreeMap<Family, f64>) -> Family {
    let best_baseline = mae
        .iter()
        .filter(|(f, _)| f.is_baseline())
        .min_by(|a, b| a.1.total_cmp(b.1))
        .map(|(f, m)| (*f, *m))
        .expect("baselines are always scored");
    mae.iter()
        .filter(|(f, m)| !f.is_baseline() && **m < best_baseline.1)
        .min_by(|a, b| a.1.total_cmp(b.1))
        .map(|(f, _)| *f)
        .unwrap_or(best_baseline.0)
}

#[cfg(test)]
mod choose_tests {
    use super::*;

    #[test]
    fn lrn6_fc2_simplest_family_wins_ties_and_learned_must_beat_baselines() {
        let m = |v: [f64; 5]| Family::ALL.iter().copied().zip(v).collect::<BTreeMap<_, _>>();
        assert_eq!(choose(&m([0.3, 0.2, 0.25, 0.1, 0.1])), Family::Ridge); // tie → simpler
        assert_eq!(choose(&m([0.3, 0.2, 0.25, 0.2, 0.2])), Family::Mean7); // equal to baseline is not better
        assert_eq!(choose(&m([0.3, 0.2, 0.25, 0.19, 0.1])), Family::Forest); // forest only when clearly better
    }
}

#[derive(Debug, Default, Serialize)]
pub struct TrainReport {
    pub targets: Vec<serde_json::Value>,
    /// What the personal models found: normal ranges, shifts, unusual days, session types.
    pub personal: serde_json::Value,
}

pub(crate) fn peak_kb() -> Option<i64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    s.lines().find(|l| l.starts_with("VmHWM:"))?.split_whitespace().nth(1)?.parse().ok()
}

pub fn data_hash(dates: &[NaiveDate], rows: &[Row]) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    for (d, r) in dates.iter().zip(rows) {
        h.update(d.to_string());
        for v in r.x.iter().chain([r.y, r.last, r.mean7, r.mean60].iter()) {
            h.update(v.to_le_bytes());
        }
    }
    hex::encode(h.finalize())
}

/// Model files written by eenkah before LRN-3 changed to "rebuild, don't store". They are
/// derived data the product no longer reads, so the next run removes them.
fn remove_legacy_model_files(db: &Db) -> Result<()> {
    let dir = db.dir.models_dir();
    if dir.exists() {
        std::fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()))?;
    }
    Ok(())
}

/// Train, backtest, promote and forecast every target, then rebuild the personal models
/// (normal ranges, shifts, unusual days, session types). Nothing trained is stored: every run
/// rebuilds each model from the data in memory and keeps only results and lineage (LRN-3).
/// Safe to interrupt: results change only in the final transaction of each step (LRN-1).
pub fn train(db: &mut Db) -> Result<TrainReport> {
    ensure_schema(db)?;
    remove_legacy_model_files(db)?;
    let inputs = derive::inputs(db)?;
    let table = features::build(&inputs, derive::load_model(db));
    let names: Vec<&str> = FEATURES.iter().map(|f| f.0).collect();
    let tz = db.tz();
    let today = chrono::Utc::now().with_timezone(&tz).date_naive();
    let mut rep = TrainReport::default();
    for target in Target::ALL {
        let t0 = std::time::Instant::now();
        let (dates, rows) = dataset(&inputs, &table, target);
        let Some(bt) = backtest(&dates, &rows, &names)? else {
            rep.targets
                .push(json!({"target": target.key(), "status": format!("not enough history: {} usable days, {} needed", rows.len(), MIN_TRAIN + STEP)}));
            continue;
        };
        let champion = Fitted::fit(bt.champion, &rows, &names, SEED)?;
        let hash = data_hash(&dates, &rows);
        let scores: BTreeMap<String, f64> = bt.mae.iter().map(|(f, m)| (format!("{f:?}"), *m)).collect();
        let lineage = json!({"target": target.key(), "family": format!("{:?}", bt.champion), "data_hash": hash, "first_day": dates[0], "last_day": dates[dates.len() - 1],
            "features": names, "backtest_mae": scores, "coverage": bt.coverage, "seed": SEED, "eenkah": env!("CARGO_PKG_VERSION"), "aprender": "0.70.3"});
        // LRN-3/LRN-4: the model is not stored. Its version names the family and the data it was
        // fitted on, so the same version can be rebuilt from the same data and seed (LRN-5).
        let version = if bt.champion.is_baseline() {
            format!("baseline-{:?}", bt.champion).to_lowercase()
        } else {
            format!("{:?}-{}", bt.champion, &hash[..12]).to_lowercase()
        };

        // live forecast for the morning after the newest feature row
        let last_feat = table.rows.keys().next_back().copied();
        let mut live = None;
        let mut refused = None;
        if let Some(d) = last_feat {
            let age = (today - d).num_days();
            if age > 1 {
                refused = Some(format!("data too old to forecast: the newest complete morning was {}", eenkah_core::when::days(age)));
            // FC-6
            } else if let Some(row) = live_row(&inputs, &table, target, d) {
                let v = champion.predict(std::slice::from_ref(&row))?[0];
                let drivers = drivers(&champion, &row, bt.champion);
                live = Some((d + Duration::days(1), v, bt.next_half_width, age, drivers));
            }
        }
        let secs = t0.elapsed().as_secs_f64();
        let tx = db.conn.transaction()?;
        tx.execute("DELETE FROM forecast WHERE target=?1 AND kind='backtest'", [target.key()])?;
        {
            let mut ins = tx.prepare("INSERT OR REPLACE INTO forecast(target, for_day, kind, made_on, family, model_version, value, lo, hi, data_age_days, truth) VALUES (?1, ?2, 'backtest', ?3, ?4, 'walk-forward', ?5, ?6, ?7, 0, ?8)")?;
            for (k, d) in bt.test_dates.iter().enumerate() {
                let p = bt.preds[&bt.champion][k];
                let iv = bt.intervals[k];
                ins.execute(params![
                    target.key(),
                    (*d + Duration::days(1)).to_string(),
                    d.to_string(),
                    format!("{:?}", bt.champion),
                    target.display(p),
                    iv.map(|x| target.display(x.0)),
                    iv.map(|x| target.display(x.1)),
                    target.display(bt.truth[k])
                ])?;
            }
        }
        if let Some((for_day, v, hw, age, drivers)) = &live {
            // FC-1: stored before the outcome exists; an existing live forecast is never rewritten
            tx.execute(
                "INSERT OR IGNORE INTO forecast(target, for_day, kind, made_on, family, model_version, value, lo, hi, data_age_days, drivers_json) VALUES (?1, ?2, 'live', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![target.key(), for_day.to_string(), today.to_string(), format!("{:?}", bt.champion), version, target.display(*v), hw.map(|h| target.display(v - h)), hw.map(|h| target.display(v + h)), age, serde_json::to_string(drivers)?],
            )?;
        }
        tx.execute(
            "INSERT INTO model_run(target, finished, secs, peak_kb, rows, data_hash, scores_json, champion, coverage, promoted, lineage_json) VALUES (?1, strftime('%s','now'), ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9)",
            params![target.key(), secs, peak_kb(), rows.len() as i64, hash, serde_json::to_string(&scores)?, format!("{:?}", bt.champion), bt.coverage, lineage.to_string()],
        )?;
        tx.execute("INSERT OR REPLACE INTO setting(key, value) VALUES (?1, ?2)", params![format!("champion.{}", target.key()), version])?;
        tx.commit()?;
        rep.targets.push(
            json!({"target": target.key(), "rows": rows.len(), "champion": format!("{:?}", bt.champion), "mae": scores, "coverage": bt.coverage, "secs": secs,
            "live": live.as_ref().map(|l| json!({"for_day": l.0, "value": target.display(l.1)})), "refused": refused}),
        );
    }
    fill_truth(db, &inputs)?;
    write_series(db)?;
    rep.personal = personal::run(db)?;
    Ok(rep)
}

/// The prediction row for the morning of `d` (its target is not known yet).
fn live_row(i: &features::Inputs, t: &features::Table, target: Target, d: NaiveDate) -> Option<Row> {
    let s = target.series(i);
    let w7 = window(s, d - Duration::days(6), d);
    let w60 = window(s, d - Duration::days(59), d);
    if w7.len() < 4 || w60.len() < 20 {
        return None;
    }
    Some(Row {
        x: t.rows.get(&d)?.clone(),
        y: f64::NAN,
        last: *s.get(&d)?,
        mean7: mean(&w7)?,
        mean60: mean(&w60)?,
    })
}

/// FC-5: the top contributing features in plain words.
fn drivers(m: &Fitted, row: &Row, fam: Family) -> Vec<String> {
    if fam.is_baseline() {
        return vec![format!("based on {}", fam.label())];
    }
    let words: HashMap<&str, &str> = FEATURES.iter().copied().collect();
    let c = m.contributions(&row.x);
    if c.is_empty() {
        return vec![format!("{} on your recent days", fam.label())];
    }
    c.into_iter()
        .take(3)
        .map(|(n, v)| format!("{} ({})", words.get(n.as_str()).copied().unwrap_or("?"), if v >= 0.0 { "pushes up" } else { "pushes down" }))
        .collect()
}

/// Record what actually happened for live forecasts whose day has arrived.
pub fn fill_truth(db: &Db, i: &features::Inputs) -> Result<()> {
    let mut st = db.conn.prepare("SELECT target, for_day FROM forecast WHERE kind='live' AND truth IS NULL")?;
    let open: Vec<(String, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    for (t, d) in open {
        let target = if t == "hrv_next" { Target::HrvNext } else { Target::RhrNext };
        if let Some(v) = NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok().and_then(|day| target.series(i).get(&day).copied()) {
            db.conn
                .execute("UPDATE forecast SET truth=?3 WHERE target=?1 AND for_day=?2 AND kind='live'", params![t, d, target.display(v)])?;
        }
    }
    Ok(())
}

/// Forecasts as suyo series for the Model view: backtest first, live forecasts on top.
fn write_series(db: &Db) -> Result<()> {
    let tz = db.tz();
    let mut store = db.suyo()?;
    let mut pts = vec![];
    let mut hints = HashMap::new();
    for target in Target::ALL {
        for part in ["value", "lo", "hi"] {
            let name = format!("eenkah.forecast.{}.{part}", target.key());
            store.delete_series(&name)?;
            hints.insert(
                name,
                suyo::store::MetaHint {
                    unit: Some(target.unit().into()),
                    agg: Some(suyo::store::AggFn::Avg),
                    interval: Some(86400),
                    description: Some(format!("{} — forecast {part}", target.label())),
                },
            );
        }
        let mut st = db.conn.prepare("SELECT for_day, value, lo, hi FROM forecast WHERE target=?1 ORDER BY kind")?; // 'backtest' < 'live'
        let rows = st.query_map([target.key()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?, r.get::<_, Option<f64>>(2)?, r.get::<_, Option<f64>>(3)?))
        })?;
        for r in rows {
            let (d, v, lo, hi) = r?;
            let Ok(day) = NaiveDate::parse_from_str(&d, "%Y-%m-%d") else { continue };
            let ts = derive::local_midnight(tz, day);
            pts.push((format!("eenkah.forecast.{}.value", target.key()), ts, v));
            if let (Some(lo), Some(hi)) = (lo, hi) {
                pts.push((format!("eenkah.forecast.{}.lo", target.key()), ts, lo));
                pts.push((format!("eenkah.forecast.{}.hi", target.key()), ts, hi));
            }
        }
    }
    store.write(&pts, &hints)?;
    Ok(())
}

/// The Model view's text: per target, the champion and its track record against the baselines.
pub fn model_summary(db: &Db) -> Result<serde_json::Value> {
    ensure_schema(db)?;
    let mut out = vec![];
    for target in Target::ALL {
        let run: Option<(String, String, Option<f64>, i64, f64, Option<i64>, Option<String>)> = db
            .conn
            .query_row(
                "SELECT champion, scores_json, coverage, rows, secs, peak_kb, lineage_json FROM model_run WHERE target=?1 ORDER BY id DESC LIMIT 1",
                [target.key()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
            )
            .ok();
        let live: Option<(String, f64, Option<f64>, Option<f64>, Option<String>)> = db
            .conn
            .query_row(
                "SELECT for_day, value, lo, hi, drivers_json FROM forecast WHERE target=?1 AND kind='live' ORDER BY for_day DESC LIMIT 1",
                [target.key()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .ok();
        let (n_live, live_hits): (i64, i64) = db.conn.query_row(
            "SELECT count(*), coalesce(sum(truth BETWEEN lo AND hi), 0) FROM forecast WHERE target=?1 AND kind='live' AND truth IS NOT NULL AND lo IS NOT NULL",
            [target.key()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        out.push(json!({
            "target": target.key(), "label": target.label(), "unit": target.unit(),
            "champion": run.as_ref().map(|r| r.0.clone()),
            "backtest_mae": run.as_ref().and_then(|r| serde_json::from_str::<serde_json::Value>(&r.1).ok()),
            "backtest_coverage": run.as_ref().and_then(|r| r.2),
            "rows": run.as_ref().map(|r| r.3), "train_secs": run.as_ref().map(|r| r.4), "peak_memory_kb": run.as_ref().and_then(|r| r.5),
            "lineage": run.as_ref().and_then(|r| r.6.as_deref()).and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok()),
            "next": live.map(|l| json!({"for_day": l.0, "value": l.1, "lo": l.2, "hi": l.3, "drivers": l.4.and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())})),
            "live_scored": n_live, "live_inside_interval": live_hits,
        }));
    }
    Ok(json!(out))
}

pub fn model_text(db: &Db) -> Result<String> {
    let v = model_summary(db)?;
    let mut s = String::new();
    let today = chrono::Utc::now().with_timezone(&db.tz()).date_naive();
    for t in v.as_array().into_iter().flatten() {
        let unit = t["unit"].as_str().unwrap_or("");
        let label = t["label"].as_str().unwrap_or("");
        let Some(ch) = t["champion"].as_str() else {
            s += &format!("{label}: not trained yet (run `eenkah train`).\n");
            continue;
        };
        let fam = match ch {
            "Ridge" => Family::Ridge,
            "Forest" => Family::Forest,
            "Mean7" => Family::Mean7,
            "Mean60" => Family::Mean60,
            _ => Family::Persistence,
        };
        let mae = &t["backtest_mae"];
        let scale = if unit == "ms" { "ln ms" } else { unit };
        let fmt = |k: &str| mae[k].as_f64().map(|x| format!("{x:.3}")).unwrap_or_else(|| "—".into());
        s += &format!(
            "{label}: using {} — {}. Backtest error (mean absolute, {scale}): learned Ridge {}, Forest {}; baselines today {}, 7-day {}, 60-day {}.",
            fam.label(),
            if fam.is_baseline() {
                "no learned model beat your own baselines yet, so the best baseline is shown"
            } else {
                "it beat every baseline in the walk-forward backtest"
            },
            fmt("Ridge"),
            fmt("Forest"),
            fmt("Persistence"),
            fmt("Mean7"),
            fmt("Mean60")
        );
        if let Some(c) = t["backtest_coverage"].as_f64() {
            s += &format!(" {:.0} % of outcomes fell inside the stated 80 % interval.", c * 100.0);
        }
        if let Some(n) = t["next"].as_object() {
            s += &format!(
                " Next ({}): {:.0} {unit}, likely {:.0}–{:.0}.",
                n["for_day"]
                    .as_str()
                    .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
                    .map(|d| eenkah_core::when::morning(d, today))
                    .unwrap_or_default(),
                n["value"].as_f64().unwrap_or(f64::NAN),
                n["lo"].as_f64().unwrap_or(f64::NAN),
                n["hi"].as_f64().unwrap_or(f64::NAN)
            );
        }
        s += &format!(" Trained on {} days in {:.1} s.\n", t["rows"], t["train_secs"].as_f64().unwrap_or(0.0));
    }
    s += "Intervals are adaptive conformal (Gibbs & Candès 2021). Not medical advice.\n\n";
    s += &personal::text(db)?;
    Ok(s)
}
