//! The read calls behind the HTTP API, the CLI and MCP. All three call these functions, so
//! they return identical data for the same request (MCP-2).

use anyhow::Result;
use chrono::{Duration, NaiveDate};
use eenkah_core::findings::{self, Latest};
use eenkah_core::metrics;
use eenkah_core::{catalog, Daily};
use eenkah_store::{derive, Db};
use serde_json::{json, Value};

pub const DISCLAIMER: &str = "Not medical advice.";

fn last(s: &Daily) -> Option<(NaiveDate, f64)> {
    s.iter().next_back().map(|(d, v)| (*d, *v))
}

fn on(s: &Daily, d: NaiveDate) -> Option<f64> {
    s.get(&d).copied()
}

pub fn today_local(db: &Db) -> NaiveDate {
    chrono::Utc::now().with_timezone(&db.tz()).date_naive()
}

/// Body state on the newest morning with data, its age, and the findings.
pub fn today(db: &Db) -> Result<Value> {
    let hrv = db.daily("body.hrv")?;
    let hrv_ln = metrics::map(&hrv, metrics::hrv_ln);
    let rhr = db.daily("body.rhr")?;
    let sleep = db.daily("sleep.hours")?;
    let newest = [last(&hrv), last(&rhr), last(&sleep)].into_iter().flatten().map(|x| x.0).max();
    let Some(day) = newest else {
        return Ok(json!({"error": "no body-state data imported yet", "disclaimer": DISCLAIMER}));
    };
    let age = (today_local(db) - day).num_days();
    let hrv7 = metrics::rolling_mean(&hrv_ln, 7, 4);
    let band = metrics::normal_band(&hrv7, &[day], 60, 20, 0.1, 0.9);
    let hrv_trend = match (on(&hrv7, day), band.lo.get(&day), band.hi.get(&day)) {
        (Some(v), Some(lo), Some(hi)) if hi > lo => Some((v - band.mid[&day]) / ((hi - lo) / 2.0)),
        _ => None,
    };
    let inputs = derive::inputs(db)?;
    let (mut form, mut fitness, mut fatigue) = (None, None, None);
    if let Some((a, b)) = inputs.load_period {
        let ls = eenkah_core::load::compute(&inputs.load, a, b.max(day), derive::load_model(db));
        form = on(&ls.form, day);
        fitness = on(&ls.fitness, day - Duration::days(1));
        fatigue = on(&ls.fatigue, day - Duration::days(1));
    }
    let latest = Latest {
        hrv_z: on(&metrics::z_score(&hrv_ln, 60, 20), day),
        hrv_trend,
        rhr_z: on(&metrics::z_score(&rhr, 60, 20), day),
        rhr: on(&rhr, day),
        sleep_hours: on(&sleep, day),
        sleep_debt_7d: on(&metrics::sleep_debt_7d(&sleep), day),
        form,
        fitness,
        skin_temp_dev: on(&db.daily("body.skin_temp_dev")?, day),
        days_since_hard: None,
        // stored by the last training run; nothing is fitted on page load (VIEW-4)
        unusual: eenkah_learn::personal::unusual_on(db, day)?,
        shifts: eenkah_learn::personal::recent_shifts(db, day, 21)?,
    };
    let fs = findings::findings(&latest);
    Ok(json!({
        "day": day,
        "data_age_days": age,
        "hrv_ms": on(&hrv, day),
        "hrv_7d_ms": on(&hrv7, day).map(f64::exp),
        "hrv_normal_ms": [band.lo.get(&day).map(|x| x.exp()), band.hi.get(&day).map(|x| x.exp())],
        "hrv_status": db.daily_text("body.hrv_status")?.get(&day),
        "rhr_bpm": latest.rhr,
        "rhr_z": latest.rhr_z,
        "hrv_z": latest.hrv_z,
        "sleep_hours": latest.sleep_hours,
        "sleep_debt_7d_hours": latest.sleep_debt_7d,
        "fitness": fitness,
        "fatigue": fatigue,
        "form": form,
        "body_battery_at_wake": on(&db.daily("body.bb_wake")?, day),
        "skin_temp_dev_c": latest.skin_temp_dev,
        "findings": fs,
        "brief": findings::brief(&fs),
        "series": ["eenkah.body.hrv", "eenkah.body.hrv_7d", "eenkah.body.rhr", "eenkah.sleep.hours", "eenkah.sleep.debt_7d", "eenkah.load.form"],
        "disclaimer": DISCLAIMER,
    }))
}

/// Plain-text brief for the Today view: one or two sentences, then the data age.
pub fn brief_text(db: &Db) -> Result<String> {
    let t = today(db)?;
    let Some(day) = t["day"].as_str() else {
        return Ok("No data yet. Import your Garmin files with `eenkah import <folder or ZIP>`.".into());
    };
    let age = t["data_age_days"].as_i64().unwrap_or(0);
    let age_txt = if age <= 0 { "this morning".to_string() } else { eenkah_core::when::days(age) };
    let mut s = format!("{}\n", t["brief"].as_str().unwrap_or_default());
    s += &format!("Newest data: {age_txt} ({day}). {DISCLAIMER}");
    if age > 1 {
        s += " Data is too old to forecast tomorrow; import fresh files.";
    }
    Ok(s)
}

pub fn week(db: &Db) -> Result<Value> {
    let today = today_local(db);
    let inputs = derive::inputs(db)?;
    let sum = |s: &Daily, a: NaiveDate, b: NaiveDate| eenkah_core::window(s, a, b).iter().sum::<f64>();
    let avg = |s: &Daily, a: NaiveDate, b: NaiveDate| eenkah_core::stats::mean(&eenkah_core::window(s, a, b));
    let (w0, w1) = (today - Duration::days(6), today);
    let (p0, p1) = (today - Duration::days(13), today - Duration::days(7));
    let (y0, y1) = (w0 - Duration::days(364), w1 - Duration::days(364));
    let hrv = db.daily("body.hrv")?;
    Ok(json!({
        "week": [w0, w1],
        "load": {"this_week": sum(&inputs.load, w0, w1), "last_week": sum(&inputs.load, p0, p1), "same_week_last_year": sum(&inputs.load, y0, y1), "unit": "load"},
        "sleep_hours_avg": {"this_week": avg(&inputs.sleep_hours, w0, w1), "last_week": avg(&inputs.sleep_hours, p0, p1)},
        "hrv_ms_avg": {"this_week": avg(&hrv, w0, w1), "last_week": avg(&hrv, p0, p1)},
        "rhr_bpm_avg": {"this_week": avg(&inputs.rhr, w0, w1), "last_week": avg(&inputs.rhr, p0, p1)},
        "sessions_this_week": activities(db, Some(w0), 100)?.as_array().map(|a| a.len()),
        "minutes_by_type": minutes_by_type(db, w0, w1)?,
        "series": ["eenkah.load.daily", "eenkah.sleep.hours", "eenkah.body.hrv", "eenkah.body.rhr"],
        "disclaimer": DISCLAIMER,
    }))
}

/// Moving minutes per session type between two days (inclusive), from the stored labels.
fn minutes_by_type(db: &Db, a: NaiveDate, b: NaiveDate) -> Result<Value> {
    let tz = db.tz();
    let (from, until) = (derive::local_midnight(tz, a), derive::local_midnight(tz, b + Duration::days(1)));
    eenkah_learn::ensure_schema(db)?;
    let mut st = db.conn.prepare(
        "SELECT coalesce(t.label, 'Not classified'), sum(coalesce(a.moving_s, a.duration_s, 0)) / 60.0 FROM activity a LEFT JOIN session_type t ON t.activity_id=a.id
         WHERE a.start_utc >= ?1 AND a.start_utc < ?2 GROUP BY 1 ORDER BY 2 DESC",
    )?;
    let rows = st.query_map(rusqlite::params![from, until], |r| Ok(json!({"label": r.get::<_, String>(0)?, "minutes": r.get::<_, f64>(1)?.round()})))?;
    Ok(Value::Array(rows.collect::<rusqlite::Result<_>>()?))
}

pub fn activities(db: &Db, since: Option<NaiveDate>, limit: usize) -> Result<Value> {
    eenkah_learn::ensure_schema(db)?;
    let since_ts = since.map(|d| derive::local_midnight(db.tz(), d)).unwrap_or(i64::MIN);
    let mut st = db.conn.prepare(
        "SELECT a.id, a.start_utc, coalesce(a.utc_offset_s, 0), a.sport, a.sub_sport, a.type_key, a.name, a.moving_s, a.distance_m, a.ascent_m, a.avg_hr, a.max_hr,
                (SELECT value FROM activity_metric m WHERE m.activity_id=a.id AND m.metric='load'),
                (SELECT value FROM activity_metric m WHERE m.activity_id=a.id AND m.metric='decoupling'),
                coalesce(a.duration_s, a.moving_s),
                (SELECT label FROM session_type t WHERE t.activity_id=a.id),
                (SELECT source FROM session_type t WHERE t.activity_id=a.id)
         FROM activity a WHERE a.start_utc >= ?1 ORDER BY a.start_utc DESC LIMIT ?2",
    )?;
    let rows = st.query_map(rusqlite::params![since_ts, limit as i64], |r| {
        let start: i64 = r.get(1)?;
        let off: i32 = r.get(2)?;
        let dur: Option<f64> = r.get(14)?;
        let local = chrono::DateTime::from_timestamp(start + off as i64, 0).map(|t| t.naive_utc());
        Ok(json!({
            "id": r.get::<_, i64>(0)?,
            "start_utc": start,
            "end_utc": start + dur.unwrap_or(0.0).ceil() as i64,
            "start_local": local.map(|t| t.format("%Y-%m-%d %H:%M").to_string()),
            "end_local": local.map(|t| (t + Duration::seconds(dur.unwrap_or(0.0) as i64 + 60)).format("%Y-%m-%d %H:%M").to_string()),
            "sport": r.get::<_, Option<String>>(3)?,
            "sub_sport": r.get::<_, Option<String>>(4)?,
            "type": r.get::<_, Option<String>>(5)?,
            "name": r.get::<_, Option<String>>(6)?,
            "moving_min": r.get::<_, Option<f64>>(7)?.map(|s| (s / 60.0).round()),
            "distance_km": r.get::<_, Option<f64>>(8)?.map(|m| (m / 10.0).round() / 100.0),
            "ascent_m": r.get::<_, Option<f64>>(9)?.map(f64::round),
            "avg_hr": r.get::<_, Option<f64>>(10)?,
            "max_hr": r.get::<_, Option<f64>>(11)?,
            "load": r.get::<_, Option<f64>>(12)?.map(f64::round),
            "decoupling_pct": r.get::<_, Option<f64>>(13)?.map(|x| (x * 10.0).round() / 10.0),
            "session_type": r.get::<_, Option<String>>(15)?,
            "session_type_source": r.get::<_, Option<String>>(16)?,
        }))
    })?;
    Ok(Value::Array(rows.collect::<rusqlite::Result<_>>()?))
}

pub fn metric_catalog() -> Value {
    json!(catalog::METRICS)
}

/// One session: summary, thresholds used, load, time in zones, and which streams it recorded.
pub fn activity(db: &Db, id: i64) -> Result<Option<Value>> {
    let all = activities(db, None, 1_000_000)?;
    let Some(a) = all.as_array().into_iter().flatten().find(|a| a["id"].as_i64() == Some(id)).cloned() else {
        return Ok(None);
    };
    let mut st = db.conn.prepare("SELECT metric, value FROM activity_metric WHERE activity_id=?1")?;
    let m: std::collections::BTreeMap<String, f64> = st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    let zones: Vec<Option<f64>> = (1..=5).map(|z| m.get(&format!("zone{z}_s")).map(|s| (s / 60.0 * 10.0).round() / 10.0)).collect();
    let (hr, speed, power, cadence, altitude): (i64, i64, i64, i64, i64) = db.conn.query_row(
        "SELECT count(hr), count(speed), count(power), count(cadence), count(altitude) FROM activity_sample WHERE activity_id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    let streams: Vec<&str> = [("hr", hr), ("speed", speed), ("power", power), ("cadence", cadence), ("altitude", altitude)]
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|(s, _)| *s)
        .collect();
    Ok(Some(json!({
        "activity": a,
        "load": m.get("load"),
        "decoupling_pct": m.get("decoupling"),
        "run_efficiency": m.get("run_efficiency"),
        "zone_minutes": zones,
        "thresholds": {"rest_hr": m.get("thr.rest_hr"), "max_hr": m.get("thr.max_hr"), "lthr": m.get("thr.lthr")},
        "streams": streams,
    })))
}

/// What is imported, how fresh it is, and whether Garmin Connect is connected.
pub fn status(db: &Db, signed_in: bool) -> Result<Value> {
    let cov = eenkah_store::import::coverage(db)?;
    let last: Option<(i64, Option<i64>)> = db
        .conn
        .query_row("SELECT started, finished FROM import_run ORDER BY id DESC LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .ok();
    let t = today(db)?;
    Ok(json!({
        "coverage": cov,
        "last_import": last.map(|(s, f)| json!({"started": s, "finished": f})),
        "newest_day": t["day"],
        "data_age_days": t["data_age_days"],
        "connect_signed_in": signed_in,
        "connect_data_account": crate::jobs::data_account(db)?,
        "connect_login_account": crate::jobs::login_account(db)?,
        "connect_last_download": db.setting("connect_last_download")?.and_then(|s| serde_json::from_str::<Value>(&s).ok()),
        "today": chrono::Utc::now().with_timezone(&db.tz()).date_naive().to_string(),
        "time_zone": db.tz().name(),
        "raw_files": db.conn.query_row("SELECT count(*) FROM raw_file", [], |r| r.get::<_, i64>(0))?,
    }))
}

/// Normal ranges, shifts, unusual days and session types (eenkah-learn's personal models).
pub fn personal(db: &Db) -> Result<Value> {
    eenkah_learn::personal::summary(db)
}

pub fn marks(db: &Db) -> Result<Value> {
    let mut st = db.conn.prepare("SELECT day, kind, value, note FROM mark ORDER BY day DESC, kind")?;
    let rows = st.query_map([], |r| {
        Ok(json!({"day": r.get::<_, String>(0)?, "kind": r.get::<_, String>(1)?, "value": r.get::<_, f64>(2)?, "note": r.get::<_, String>(3)?}))
    })?;
    Ok(Value::Array(rows.collect::<rusqlite::Result<_>>()?))
}
