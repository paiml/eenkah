//! Derived data: per-activity metrics, then every catalogue metric written as a suyo series.

use crate::Db;
use anyhow::Result;
use chrono::{NaiveDate, TimeZone};
use eenkah_core::catalog::{self, METRICS};
use eenkah_core::features::Inputs;
use eenkah_core::load::{self, LoadModel};
use eenkah_core::metrics::{self, LTHR_ZONES};
use eenkah_core::stats::{median, percentile};
use eenkah_core::Daily;
use rusqlite::params;
use std::collections::{BTreeMap, HashMap};
use suyo::store::{AggFn, MetaHint};

/// Samples further apart than this are a pause, not training.
const MAX_GAP_S: f64 = 30.0;

#[derive(Debug, Default, serde::Serialize)]
pub struct DeriveReport {
    pub activities: usize,
    pub activities_with_hr_samples: usize,
    pub series_written: usize,
    pub points_written: usize,
    pub stream_activities_written: usize,
}

pub fn local_midnight(tz: chrono_tz::Tz, d: NaiveDate) -> i64 {
    tz.from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap())
        .earliest()
        .map(|t| t.timestamp())
        .unwrap_or_else(|| d.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp())
}

pub fn drop_series(db: &Db) -> Result<()> {
    let mut s = db.suyo()?;
    let names: Vec<String> = s.series()?.into_iter().map(|m| m.name).filter(|n| n.starts_with("eenkah.")).collect();
    for n in names {
        s.delete_series(&n)?;
    }
    Ok(())
}

struct Act {
    id: i64,
    start: i64,
    offset: i32,
    sport: String,
    moving_s: f64,
    avg_hr: Option<f64>,
    max_hr: Option<f64>,
    lthr: Option<f64>,
    max_hr_setting: Option<f64>,
}

impl Act {
    fn day(&self) -> NaiveDate {
        eenkah_ingest::local_day(self.start, self.offset)
    }
}

fn activities(db: &Db, tz: chrono_tz::Tz) -> Result<Vec<Act>> {
    let mut st = db
        .conn
        .prepare("SELECT id, start_utc, utc_offset_s, coalesce(sport, type_key, ''), coalesce(moving_s, duration_s, 0), avg_hr, max_hr, lthr, max_hr_setting FROM activity ORDER BY start_utc")?;
    let rows = st.query_map([], |r| {
        let start: i64 = r.get(1)?;
        Ok(Act {
            id: r.get(0)?,
            start,
            offset: r.get::<_, Option<i32>>(2)?.unwrap_or_else(|| eenkah_ingest::Ctx { tz }.offset_at(start)),
            sport: r.get(3)?,
            moving_s: r.get(4)?,
            avg_hr: r.get(5)?,
            max_hr: r.get(6)?,
            lthr: r.get(7)?,
            max_hr_setting: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The latest value on or before `d`, if it is at most `max_age` days old.
fn as_of(s: &Daily, d: NaiveDate, max_age: i64) -> Option<f64> {
    s.range(..=d).next_back().filter(|(k, _)| (d - **k).num_days() <= max_age).map(|(_, v)| *v)
}

pub fn run(db: &mut Db) -> Result<DeriveReport> {
    let tz = db.tz();
    let mut rep = DeriveReport::default();
    let k: f64 = db.setting("trimp_k")?.and_then(|s| s.parse().ok()).unwrap_or(1.92);
    let rhr = db.daily("body.rhr")?;
    // Fallbacks use only what was known by each session (FC-4), so a session's load and zones
    // never change when later data arrives (LRN-4).
    let rhr_median_by = |day: NaiveDate| median(&rhr.range(..=day).map(|(_, v)| *v).collect::<Vec<_>>());
    let acts = activities(db, tz)?;
    rep.activities = acts.len();
    // acts are in start order: the observed maximum is over this session and earlier ones
    let observed_max: Vec<Option<f64>> = (0..acts.len())
        .map(|i| percentile(&acts[..=i].iter().filter_map(|a| a.max_hr).filter(|x| *x > 100.0).collect::<Vec<_>>(), 0.99))
        .collect();

    // ---- per-activity metrics
    let mut daily_load = Daily::new();
    let mut zones: [Daily; 5] = Default::default();
    let mut run_eff = Daily::new();
    let mut decoup = Daily::new();
    let mut last_lthr: Option<f64> = None;
    let tx = db.conn.transaction()?;
    tx.execute("DELETE FROM activity_metric", [])?;
    {
        let mut samples_q = tx.prepare("SELECT t, hr, speed, power FROM activity_sample WHERE activity_id=?1 ORDER BY t")?;
        let mut put = tx.prepare("INSERT OR REPLACE INTO activity_metric(activity_id, metric, value) VALUES (?1, ?2, ?3)")?;
        for (i, a) in acts.iter().enumerate() {
            let day = a.day();
            let over = |kind: &str| -> Result<Option<f64>> {
                Ok(tx
                    .query_row(
                        "SELECT value FROM threshold_override WHERE kind=?1 AND from_day<=?2 ORDER BY from_day DESC LIMIT 1",
                        params![kind, day.to_string()],
                        |r| r.get(0),
                    )
                    .ok())
            };
            let rest = over("rest_hr")?.or_else(|| as_of(&rhr, day, 7)).or_else(|| rhr_median_by(day));
            let max = over("max_hr")?.or(a.max_hr_setting).or(observed_max[i]);
            if a.lthr.is_some() {
                last_lthr = a.lthr;
            }
            let lthr = over("lthr")?.or(a.lthr).or(last_lthr).or(max.map(|m| m * 0.9));
            let rows: Vec<(i64, Option<f64>, Option<f64>, Option<f64>)> = samples_q.query_map([a.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<_>>()?;
            let hr: Vec<(f64, f64)> = rows.iter().filter_map(|(t, h, _, _)| Some(((*t - a.start) as f64, (*h)?))).collect();
            if hr.len() > 10 {
                rep.activities_with_hr_samples += 1;
            }
            let (Some(rest), Some(max)) = (rest, max) else { continue };
            for (m, v) in [("thr.rest_hr", rest), ("thr.max_hr", max)] {
                put.execute(params![a.id, m, v])?;
            }
            // load: from samples when we have them, else from the session average
            let load = if hr.len() > 10 {
                metrics::session_trimp(&hr, rest, max, k, MAX_GAP_S)
            } else {
                a.avg_hr.and_then(|h| metrics::hr_reserve(h, rest, max)).map(|r| metrics::trimp(a.moving_s / 60.0, r, k))
            };
            if let Some(l) = load {
                put.execute(params![a.id, "load", l])?;
                *daily_load.entry(day).or_insert(0.0) += l;
            }
            if let Some(lthr) = lthr {
                put.execute(params![a.id, "thr.lthr", lthr])?;
                if hr.len() > 10 {
                    let z = metrics::time_in_zones(&hr, lthr, &LTHR_ZONES, MAX_GAP_S);
                    for (i, secs) in z.iter().enumerate() {
                        put.execute(params![a.id, format!("zone{}_s", i + 1), secs])?;
                        *zones[i].entry(day).or_insert(0.0) += secs / 60.0;
                    }
                }
            }
            // efficiency and decoupling on steady sessions
            let use_power = rows.iter().filter(|r| r.3.is_some_and(|p| p > 0.0)).count() > rows.len() / 2;
            let out: Vec<(f64, f64, f64)> = rows
                .iter()
                .filter_map(|(t, h, s, p)| {
                    let o = if use_power { (*p)? } else { (*s)? * 60.0 };
                    let h = (*h)?;
                    (o > 0.0).then(|| ((*t - a.start) as f64, o, h))
                })
                .collect();
            if a.moving_s >= 40.0 * 60.0 {
                if let Some(dc) = metrics::decoupling(&out) {
                    put.execute(params![a.id, "decoupling", dc])?;
                    decoup.insert(day, dc);
                }
            }
            if a.sport == "running" && a.moving_s >= 20.0 * 60.0 {
                // speed per heartbeat; wrist running power is a model of speed, so speed is used
                let speed: Vec<(f64, f64, f64)> = rows
                    .iter()
                    .filter_map(|(t, h, s, _)| Some(((*t - a.start) as f64, (*s)? * 60.0, (*h)?)))
                    .filter(|x| x.1 > 0.0)
                    .collect();
                if let Some(e) = metrics::efficiency(&speed) {
                    put.execute(params![a.id, "run_efficiency", e])?;
                    run_eff.insert(day, e);
                }
            }
        }
    }
    tx.commit()?;

    // ---- daily series
    let mut s: BTreeMap<&'static str, Daily> = BTreeMap::new();
    let hrv = db.daily("body.hrv")?;
    let hrv_ln = metrics::map(&hrv, metrics::hrv_ln);
    let hrv7_ln = metrics::rolling_mean(&hrv_ln, 7, 4);
    let days7: Vec<NaiveDate> = hrv7_ln.keys().copied().collect();
    let band = metrics::normal_band(&hrv7_ln, &days7, 60, 20, 0.1, 0.9);
    s.insert("eenkah.body.hrv", hrv.clone());
    s.insert("eenkah.body.hrv_ln", hrv_ln.clone());
    s.insert("eenkah.body.hrv_7d", metrics::map(&hrv7_ln, |x| Some(x.exp())));
    s.insert("eenkah.body.hrv_band_lo", metrics::map(&band.lo, |x| Some(x.exp())));
    s.insert("eenkah.body.hrv_band_hi", metrics::map(&band.hi, |x| Some(x.exp())));
    s.insert("eenkah.body.hrv_z", metrics::z_score(&hrv_ln, 60, 20));
    s.insert("eenkah.body.rhr", rhr.clone());
    s.insert("eenkah.body.rhr_z", metrics::z_score(&rhr, 60, 20));
    for (series, metric) in [
        ("eenkah.body.skin_temp_dev", "body.skin_temp_dev"),
        ("eenkah.body.bb_wake", "body.bb_wake"),
        ("eenkah.body.spo2", "body.spo2"),
        ("eenkah.body.weight", "body.weight"),
        ("eenkah.sleep.hours", "sleep.hours"),
        ("eenkah.sleep.deep", "sleep.deep"),
        ("eenkah.sleep.rem", "sleep.rem"),
        ("eenkah.sleep.light", "sleep.light"),
        ("eenkah.sleep.awake", "sleep.awake"),
        ("eenkah.sleep.naps", "sleep.naps"),
        ("eenkah.sleep.score", "sleep.score"),
        ("eenkah.sleep.stress", "sleep.stress"),
        ("eenkah.sleep.respiration", "sleep.respiration"),
        ("eenkah.sleep.bedtime", "sleep.bedtime"),
        ("eenkah.sleep.waketime", "sleep.waketime"),
    ] {
        s.insert(series, db.daily(metric)?);
    }
    s.insert("eenkah.sleep.debt_7d", metrics::sleep_debt_7d(&s["eenkah.sleep.hours"]));
    s.insert("eenkah.sleep.regularity", metrics::sleep_regularity(&s["eenkah.sleep.bedtime"], &s["eenkah.sleep.waketime"]));

    let period = load_period(&acts, &rhr);
    if let Some((a, b)) = period {
        let ls = load::compute(&daily_load, a, b, load_model(db));
        s.insert("eenkah.load.daily", ls.daily);
        s.insert("eenkah.load.fitness", ls.fitness);
        s.insert("eenkah.load.fatigue", ls.fatigue);
        s.insert("eenkah.load.form", ls.form);
        s.insert("eenkah.load.ramp", ls.ramp);
        s.insert("eenkah.load.monotony", ls.monotony);
        s.insert("eenkah.load.strain", ls.strain);
    }
    for (i, name) in ["eenkah.zones.z1", "eenkah.zones.z2", "eenkah.zones.z3", "eenkah.zones.z4", "eenkah.zones.z5"].into_iter().enumerate() {
        s.insert(name, std::mem::take(&mut zones[i]));
    }
    s.insert("eenkah.perf.run_efficiency", run_eff);
    s.insert("eenkah.perf.decoupling", decoup);
    let marks: Vec<String> = {
        let mut st = db.conn.prepare("SELECT DISTINCT kind FROM mark ORDER BY kind")?;
        let r = st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        r
    };

    // ---- write: replace every daily eenkah series so removed days disappear
    let mut store = db.suyo()?;
    let old: Vec<String> = store
        .series()?
        .into_iter()
        .map(|m| m.name)
        .filter(|n| n.starts_with("eenkah.") && !n.starts_with("eenkah.stream.") && !n.starts_with("eenkah.forecast."))
        .collect();
    for n in old {
        store.delete_series(&n)?;
    }
    let mut pts: Vec<(String, i64, f64)> = vec![];
    let mut hints: HashMap<String, MetaHint> = HashMap::new();
    for (name, series) in &s {
        if series.is_empty() {
            continue;
        }
        rep.series_written += 1;
        hints.insert(name.to_string(), hint(name));
        pts.extend(series.iter().map(|(d, v)| (name.to_string(), local_midnight(tz, *d), *v)));
    }
    for kind in marks {
        let name = format!("eenkah.mark.{}", suyo::store::sanitize_segment(&kind));
        for (d, v) in db.marks(&kind)? {
            pts.push((name.clone(), local_midnight(tz, d), v));
        }
        hints.insert(
            name,
            MetaHint {
                unit: Some(String::new()),
                agg: Some(AggFn::Max),
                interval: Some(86400),
                description: Some(format!("Days you marked as {kind}")),
            },
        );
    }
    rep.points_written += store.write(&pts, &hints)?;

    // ---- activity streams, only for activities not yet written
    let todo: Vec<i64> = {
        let mut st = db
            .conn
            .prepare("SELECT id FROM activity WHERE id NOT IN (SELECT activity_id FROM streams_written) ORDER BY start_utc")?;
        let r = st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        r
    };
    let stream_hint = |n: &str| (n.to_string(), hint(n));
    let stream_hints: HashMap<String, MetaHint> = ["eenkah.stream.hr", "eenkah.stream.speed", "eenkah.stream.power", "eenkah.stream.cadence", "eenkah.stream.altitude"]
        .into_iter()
        .map(stream_hint)
        .collect();
    for chunk in todo.chunks(50) {
        let mut pts = vec![];
        for id in chunk {
            let mut st = db
                .conn
                .prepare_cached("SELECT t, hr, speed, power, cadence, altitude FROM activity_sample WHERE activity_id=?1 ORDER BY t")?;
            let rows = st.query_map([id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<f64>>(1)?,
                    r.get::<_, Option<f64>>(2)?,
                    r.get::<_, Option<f64>>(3)?,
                    r.get::<_, Option<f64>>(4)?,
                    r.get::<_, Option<f64>>(5)?,
                ))
            })?;
            for r in rows {
                let (t, h, sp, p, c, al) = r?;
                for (name, v) in [
                    ("eenkah.stream.hr", h),
                    ("eenkah.stream.speed", sp.map(|x| x * 3.6)),
                    ("eenkah.stream.power", p),
                    ("eenkah.stream.cadence", c),
                    ("eenkah.stream.altitude", al),
                ] {
                    if let Some(v) = v {
                        pts.push((name.to_string(), t, v));
                    }
                }
            }
        }
        rep.points_written += store.write(&pts, &stream_hints)?;
        let tx = db.conn.transaction()?;
        for id in chunk {
            tx.execute("INSERT OR REPLACE INTO streams_written(activity_id) VALUES (?1)", [id])?;
        }
        tx.commit()?;
        rep.stream_activities_written += chunk.len();
    }
    Ok(rep)
}

pub fn hint(name: &str) -> MetaHint {
    let def = catalog::find(name);
    MetaHint {
        unit: def.map(|d| d.unit.to_string()),
        agg: def.and_then(|d| AggFn::parse(d.agg).ok()),
        interval: Some(if name.starts_with("eenkah.stream.") { 1 } else { 86400 }),
        description: def.map(|d| format!("{} — {}", d.label, d.formula)),
    }
}

fn load_period(acts: &[Act], rhr: &Daily) -> Option<(NaiveDate, NaiveDate)> {
    let first = acts.first()?.day();
    let last_act = acts.last()?.day();
    let last = rhr.keys().next_back().copied().map_or(last_act, |d| d.max(last_act));
    Some((first, last))
}

pub fn load_model(db: &Db) -> LoadModel {
    db.setting("load_model").ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Everything the forecasting layer needs, read from the database.
pub fn inputs(db: &Db) -> Result<Inputs> {
    let tz = db.tz();
    let acts = activities(db, tz)?;
    let mut load = Daily::new();
    {
        let mut st = db.conn.prepare("SELECT activity_id, value FROM activity_metric WHERE metric='load'")?;
        let by_id: HashMap<i64, f64> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        for a in &acts {
            if let Some(l) = by_id.get(&a.id) {
                *load.entry(a.day()).or_insert(0.0) += l;
            }
        }
    }
    let rhr = db.daily("body.rhr")?;
    Ok(Inputs {
        hrv_ln: metrics::map(&db.daily("body.hrv")?, metrics::hrv_ln),
        load_period: load_period(&acts, &rhr),
        rhr,
        sleep_hours: db.daily("sleep.hours")?,
        bedtime_min: db.daily("sleep.bedtime")?,
        waketime_min: db.daily("sleep.waketime")?,
        load,
        sick: db.marks("sick")?,
    })
}

/// Every catalogue entry is either written by `run` or is a stream; checked in tests.
pub fn catalogue_names() -> Vec<&'static str> {
    METRICS.iter().map(|m| m.series).collect()
}
