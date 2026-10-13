//! FIT files. What a file holds is decided by its messages, not its name, so files from the
//! Connect download (`123_HRV_STATUS.fit`), the account export (generic names) and the
//! watch's own folders all work.

use crate::{local_day, Activity, Ctx, Parsed, Rec, Sample};
use anyhow::{Context, Result};
use fitparser::{FitDataRecord, Value};
use std::collections::BTreeMap;

struct Msg {
    name: String,
    fields: BTreeMap<String, Value>,
}

impl Msg {
    fn f(&self, k: &str) -> Option<f64> {
        let v = self.fields.get(k)?.clone();
        let x: f64 = v.try_into().ok()?;
        x.is_finite().then_some(x)
    }
    fn ts(&self, k: &str) -> Option<i64> {
        match self.fields.get(k)? {
            Value::Timestamp(t) => Some(t.timestamp()),
            _ => None,
        }
    }
    fn s(&self, k: &str) -> Option<String> {
        match self.fields.get(k)? {
            Value::String(s) => Some(s.clone()),
            v => Some(v.to_string()),
        }
    }
}

fn msgs(recs: Vec<FitDataRecord>) -> Vec<Msg> {
    recs.into_iter()
        .map(|r| {
            let name = r.kind().to_string();
            let fields = r.into_vec().into_iter().map(|f| (f.name().to_string(), f.into_value())).collect();
            Msg { name, fields }
        })
        .collect()
}

/// Local minus UTC in seconds, from the file's own records if it has them.
fn file_offset(ms: &[Msg]) -> Option<i32> {
    ms.iter().find_map(|m| match m.name.as_str() {
        "activity" | "timestamp_correlation" => Some((m.ts("local_timestamp")? - m.ts("timestamp")?) as i32),
        _ => None,
    })
}

pub fn parse(file_name: &str, bytes: &[u8], ctx: &Ctx) -> Result<Parsed> {
    let recs = fitparser::from_bytes(bytes).context("FIT decode failed")?;
    let ms = msgs(recs);
    let has = |n: &str| ms.iter().any(|m| m.name == n);
    let mut p = if has("session") || has("record") {
        activity(file_name, &ms, ctx)
    } else if has("hrv_status_summary") {
        hrv(&ms, ctx)
    } else if has("skin_temp_overnight") {
        skin_temp(&ms)
    } else if has("sleep_assessment") || has("sleep_level") {
        sleep(&ms, ctx)
    } else {
        let ty = ms.iter().find(|m| m.name == "file_id").and_then(|m| m.s("type")).unwrap_or_else(|| "?".into());
        Parsed {
            content: format!("FIT with nothing eenkah reads (file type {ty})"),
            ..Default::default()
        }
    };
    // ING-3: report every message the chosen reader did not use.
    let used = used_messages(&p.content);
    for m in &ms {
        if !used.contains(&m.name.as_str()) && !matches!(m.name.as_str(), "file_id" | "file_creator" | "device_info" | "event" | "software" | "timestamp_correlation") {
            *p.unknown.entry(m.name.clone()).or_default() += 1;
        }
    }
    Ok(p)
}

fn used_messages(content: &str) -> &'static [&'static str] {
    match content {
        "activity" => &["session", "record", "activity", "zones_target", "user_profile", "sport"],
        "hrv status" => &["hrv_status_summary", "hrv_value"],
        "skin temperature" => &["skin_temp_overnight"],
        "sleep" => &["sleep_assessment", "sleep_level"],
        _ => &[],
    }
}

fn activity(file_name: &str, ms: &[Msg], ctx: &Ctx) -> Parsed {
    let session = ms.iter().find(|m| m.name == "session");
    let start = session
        .and_then(|s| s.ts("start_time"))
        .or_else(|| ms.iter().find(|m| m.name == "record").and_then(|r| r.ts("timestamp")))
        .unwrap_or_default();
    // Connect names files "<activityId>_ACTIVITY.fit"; otherwise use the creation time, negated
    // so it can never collide with a Connect id.
    let base = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    let id = base
        .split('_')
        .next()
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|_| base.to_ascii_uppercase().contains("_ACTIVITY"))
        .or_else(|| ms.iter().find(|m| m.name == "file_id").and_then(|f| f.ts("time_created")).map(|t| -t))
        .unwrap_or(-start);
    let zones = ms.iter().find(|m| m.name == "zones_target");
    let profile = ms.iter().find(|m| m.name == "user_profile");
    let mut a = Activity {
        id,
        start_utc: start,
        utc_offset_s: file_offset(ms).or_else(|| Some(ctx.offset_at(start))),
        lthr: zones.and_then(|z| z.f("threshold_heart_rate")).filter(|x| *x > 0.0),
        max_hr_setting: zones.and_then(|z| z.f("max_heart_rate")).filter(|x| *x > 0.0),
        rest_hr_setting: profile.and_then(|u| u.f("resting_heart_rate")).filter(|x| *x > 0.0),
        ..Default::default()
    };
    if let Some(s) = session {
        a.sport = s.s("sport");
        a.sub_sport = s.s("sub_sport");
        a.duration_s = s.f("total_elapsed_time");
        a.moving_s = s.f("total_timer_time");
        a.distance_m = s.f("total_distance");
        a.ascent_m = s.f("total_ascent");
        a.avg_hr = s.f("avg_heart_rate");
        a.max_hr = s.f("max_heart_rate");
        a.calories = s.f("total_calories");
    }
    let f32o = |x: Option<f64>| x.map(|v| v as f32);
    let samples: Vec<Sample> = ms
        .iter()
        .filter(|m| m.name == "record")
        .filter_map(|r| {
            Some(Sample {
                t: r.ts("timestamp")?,
                hr: f32o(r.f("heart_rate").filter(|x| *x > 0.0 && *x < 250.0)),
                speed: f32o(r.f("enhanced_speed").or_else(|| r.f("speed"))),
                power: f32o(r.f("power")),
                cadence: f32o(r.f("cadence")),
                altitude: f32o(r.f("enhanced_altitude").or_else(|| r.f("altitude"))),
                distance: f32o(r.f("distance")),
                temp: f32o(r.f("temperature")),
            })
        })
        .collect();
    let mut recs = vec![Rec::Activity(Box::new(a))];
    if !samples.is_empty() {
        recs.push(Rec::Samples { activity_id: id, samples });
    }
    Parsed {
        recs,
        content: "activity".into(),
        ..Default::default()
    }
}

fn hrv(ms: &[Msg], ctx: &Ctx) -> Parsed {
    let mut recs = vec![];
    let off = file_offset(ms);
    for m in ms.iter().filter(|m| m.name == "hrv_status_summary") {
        let Some(t) = m.ts("timestamp") else { continue };
        let d = local_day(t, off.unwrap_or_else(|| ctx.offset_at(t)));
        let mut put = |metric: &'static str, k: &str| {
            if let Some(v) = m.f(k).filter(|x| *x > 0.0) {
                recs.push(Rec::Day { day: d, metric, value: v, prio: 0 });
            }
        };
        put("body.hrv", "last_night_average");
        put("body.hrv_weekly", "weekly_average");
        put("body.hrv_baseline_lo", "baseline_balanced_lower");
        put("body.hrv_baseline_hi", "baseline_balanced_upper");
        if let Some(s) = m.s("status") {
            recs.push(Rec::DayText {
                day: d,
                metric: "body.hrv_status",
                text: s.to_ascii_lowercase(),
                prio: 0,
            });
        }
    }
    Parsed {
        recs,
        content: "hrv status".into(),
        ..Default::default()
    }
}

fn skin_temp(ms: &[Msg]) -> Parsed {
    let mut recs = vec![];
    for m in ms.iter().filter(|m| m.name == "skin_temp_overnight") {
        // local_timestamp is local wall time, so its "UTC" date is the local date.
        let Some(lt) = m.ts("local_timestamp") else { continue };
        if let Some(v) = m.f("average_deviation") {
            recs.push(Rec::Day {
                day: local_day(lt, 0),
                metric: "body.skin_temp_dev",
                value: v,
                prio: 0,
            });
        }
    }
    Parsed {
        recs,
        content: "skin temperature".into(),
        ..Default::default()
    }
}

fn sleep(ms: &[Msg], ctx: &Ctx) -> Parsed {
    let mut recs = vec![];
    let off = file_offset(ms);
    // the night belongs to the morning it ends on: the last sleep_level sample
    let end = ms.iter().filter(|m| m.name == "sleep_level").filter_map(|m| m.ts("timestamp")).max();
    if let (Some(end), Some(a)) = (end, ms.iter().find(|m| m.name == "sleep_assessment")) {
        let d = local_day(end, off.unwrap_or_else(|| ctx.offset_at(end)));
        if let Some(v) = a.f("overall_sleep_score").filter(|x| *x > 0.0 && *x <= 100.0) {
            recs.push(Rec::Day {
                day: d,
                metric: "sleep.score",
                value: v,
                prio: 1,
            });
        }
    }
    Parsed {
        recs,
        content: "sleep".into(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn garbage_is_an_error() {
        let ctx = Ctx { tz: chrono_tz::UTC };
        assert!(parse("x.fit", b"definitely not a fit file", &ctx).is_err());
    }

    /// Runs against the owner's real files when EENKAH_REAL_DATA points at a folder of Garmin files.
    /// Never part of CI (ING-7: no personal data in fixtures).
    #[test]
    #[ignore]
    fn real_files_decode() {
        let Ok(dir) = std::env::var("EENKAH_REAL_DATA") else { return };
        let ctx = Ctx { tz: chrono_tz::Europe::Paris };
        for sub in ["FitFiles/Activities", "FitFiles/Monitoring/2026"] {
            let mut n = 0;
            let fits = std::fs::read_dir(format!("{dir}/{sub}"))
                .unwrap()
                .flatten()
                .filter(|e| crate::classify(&e.file_name().to_string_lossy()) == crate::Kind::Fit);
            for e in fits.take(40) {
                let name = e.file_name().to_string_lossy().to_string();
                let p = parse(&name, &std::fs::read(e.path()).unwrap(), &ctx).unwrap();
                eprintln!("{name}: {} recs, {} ({:?})", p.recs.len(), p.content, p.unknown);
                n += 1;
            }
            assert!(n > 0);
        }
    }
}
