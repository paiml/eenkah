//! Garmin Connect JSON files, as saved by `eenkah import` from Connect or found in a folder.
//! Key paths are documented in `docs/DATA.md`.

use crate::{Activity, Parsed, Rec};
use anyhow::{Context, Result};
use chrono::{NaiveDate, NaiveDateTime};
use serde_json::Value;

fn day(v: &Value) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(v.as_str()?, "%Y-%m-%d").ok()
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().filter(|x| x.is_finite())
}

fn put(out: &mut Vec<Rec>, day: NaiveDate, metric: &'static str, value: Option<f64>, prio: u8) {
    if let Some(value) = value {
        out.push(Rec::Day { day, metric, value, prio });
    }
}

fn parse_json(bytes: &[u8]) -> Result<Value> {
    serde_json::from_slice(bytes).context("not valid JSON")
}

/// Local clock minutes after midnight for a "local time encoded as UTC epoch ms" value.
fn clock_min(local_ms: f64) -> f64 {
    ((local_ms / 1000.0).rem_euclid(86400.0) / 60.0).round()
}

pub fn sleep(bytes: &[u8]) -> Result<Parsed> {
    let v = parse_json(bytes)?;
    let s = &v["dailySleepDTO"];
    let mut out = vec![];
    let mut p = Parsed {
        content: "sleep".into(),
        ..Default::default()
    };
    let Some(d) = day(&s["calendarDate"]) else {
        p.content = "sleep (no date)".into();
        return Ok(p);
    };
    // Days without a recorded night carry the date and nothing else; they add no rows.
    let Some(secs) = num(&s["sleepTimeSeconds"]).filter(|x| *x > 0.0) else {
        p.content = "sleep (no night recorded)".into();
        return Ok(p);
    };
    let h = |k: &str| num(&s[k]).map(|x| x / 3600.0);
    put(&mut out, d, "sleep.hours", Some(secs / 3600.0), 0);
    put(&mut out, d, "sleep.deep", h("deepSleepSeconds"), 0);
    put(&mut out, d, "sleep.light", h("lightSleepSeconds"), 0);
    put(&mut out, d, "sleep.rem", h("remSleepSeconds"), 0);
    put(&mut out, d, "sleep.awake", h("awakeSleepSeconds"), 0);
    put(&mut out, d, "sleep.naps", h("napTimeSeconds"), 0);
    put(&mut out, d, "sleep.score", num(&s["sleepScores"]["overall"]["value"]), 0);
    put(&mut out, d, "sleep.stress", num(&s["avgSleepStress"]), 0);
    put(&mut out, d, "sleep.respiration", num(&s["averageRespirationValue"]), 0);
    put(&mut out, d, "body.spo2", num(&s["averageSpO2Value"]), 0);
    if let (Some(sg), Some(sl), Some(eg), Some(el)) = (
        num(&s["sleepStartTimestampGMT"]),
        num(&s["sleepStartTimestampLocal"]),
        num(&s["sleepEndTimestampGMT"]),
        num(&s["sleepEndTimestampLocal"]),
    ) {
        put(&mut out, d, "sleep.bedtime", Some(clock_min(sl)), 0);
        put(&mut out, d, "sleep.waketime", Some(clock_min(el)), 0);
        put(&mut out, d, "sleep.start_utc", Some((sg / 1000.0).round()), 0);
        put(&mut out, d, "sleep.end_utc", Some((eg / 1000.0).round()), 0);
        put(&mut out, d, "tz.offset_s", Some(((el - eg) / 1000.0).round()), 0);
    }
    // Present only on newer exports; the HRV status FIT file is the primary source (prio 0).
    put(&mut out, d, "body.hrv", num(&v["avgOvernightHrv"]), 1);
    if let Some(st) = v["hrvStatus"].as_str() {
        out.push(Rec::DayText {
            day: d,
            metric: "body.hrv_status",
            text: st.to_ascii_lowercase(),
            prio: 1,
        });
    }
    put(&mut out, d, "body.skin_temp_dev", num(&v["avgSkinTempDeviationC"]), 1);
    put(&mut out, d, "body.rhr", num(&v["restingHeartRate"]), 2);
    p.recs = out;
    Ok(p)
}

pub fn rhr(bytes: &[u8]) -> Result<Parsed> {
    let v = parse_json(bytes)?;
    let mut out = vec![];
    for e in v["allMetrics"]["metricsMap"]["WELLNESS_RESTING_HEART_RATE"].as_array().into_iter().flatten() {
        if let Some(d) = day(&e["calendarDate"]) {
            put(&mut out, d, "body.rhr", num(&e["value"]).filter(|x| *x > 0.0), 0);
        }
    }
    Ok(Parsed {
        recs: out,
        content: "resting heart rate".into(),
        ..Default::default()
    })
}

/// HRV summaries from the HRV service. The same numbers as the HRV status FIT file, so the
/// same metrics at the same priority; nights without a reading are simply absent.
pub fn hrv(bytes: &[u8]) -> Result<Parsed> {
    let v = parse_json(bytes)?;
    let mut out = vec![];
    for e in v["hrvSummaries"].as_array().into_iter().flatten() {
        let Some(d) = day(&e["calendarDate"]) else { continue };
        let pos = |x: &Value| num(x).filter(|x| *x > 0.0);
        put(&mut out, d, "body.hrv", pos(&e["lastNightAvg"]), 0);
        put(&mut out, d, "body.hrv_weekly", pos(&e["weeklyAvg"]), 0);
        put(&mut out, d, "body.hrv_baseline_lo", pos(&e["baseline"]["balancedLow"]), 0);
        put(&mut out, d, "body.hrv_baseline_hi", pos(&e["baseline"]["balancedUpper"]), 0);
        if let Some(st) = e["status"].as_str().filter(|s| !s.is_empty() && *s != "NONE") {
            out.push(Rec::DayText {
                day: d,
                metric: "body.hrv_status",
                text: st.to_ascii_lowercase(),
                prio: 0,
            });
        }
    }
    Ok(Parsed {
        recs: out,
        content: "hrv".into(),
        ..Default::default()
    })
}

pub fn weight(bytes: &[u8]) -> Result<Parsed> {
    let v = parse_json(bytes)?;
    let mut out = vec![];
    for e in v["dateWeightList"].as_array().into_iter().flatten() {
        if let Some(d) = day(&e["calendarDate"]) {
            // Connect reports grams regardless of the display unit.
            put(&mut out, d, "body.weight", num(&e["weight"]).filter(|x| *x > 0.0).map(|g| g / 1000.0), 0);
        }
    }
    Ok(Parsed {
        recs: out,
        content: "weight".into(),
        ..Default::default()
    })
}

pub fn daily_summary(bytes: &[u8]) -> Result<Parsed> {
    let v = parse_json(bytes)?;
    let mut out = vec![];
    let Some(d) = day(&v["calendarDate"]) else {
        return Ok(Parsed {
            content: "daily summary (no date)".into(),
            ..Default::default()
        });
    };
    put(&mut out, d, "body.bb_wake", num(&v["bodyBatteryAtWakeTime"]), 0);
    put(&mut out, d, "body.rhr", num(&v["restingHeartRate"]).filter(|x| *x > 0.0), 1);
    put(&mut out, d, "day.steps", num(&v["totalSteps"]), 0);
    put(&mut out, d, "day.stress", num(&v["averageStressLevel"]).filter(|x| *x >= 0.0), 0);
    let ts = |k: &str| v[k].as_str().and_then(|s| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").ok());
    if let (Some(g), Some(l)) = (ts("wellnessStartTimeGmt"), ts("wellnessStartTimeLocal")) {
        put(&mut out, d, "tz.offset_s", Some((l - g).num_seconds() as f64), 1);
    }
    Ok(Parsed {
        recs: out,
        content: "daily summary".into(),
        ..Default::default()
    })
}

pub fn activity(bytes: &[u8]) -> Result<Parsed> {
    let v = parse_json(bytes)?;
    let Some(id) = v["activityId"].as_i64() else {
        return Ok(Parsed {
            content: "activity (no id)".into(),
            ..Default::default()
        });
    };
    let ts = |k: &str| v[k].as_str().and_then(|s| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok());
    let (Some(g), l) = (ts("startTimeGMT"), ts("startTimeLocal")) else {
        return Ok(Parsed {
            content: "activity (no start time)".into(),
            ..Default::default()
        });
    };
    let a = Activity {
        id,
        start_utc: g.and_utc().timestamp(),
        utc_offset_s: l.map(|l| (l - g).num_seconds() as i32),
        type_key: v["activityType"]["typeKey"].as_str().map(String::from),
        name: v["activityName"].as_str().map(String::from),
        duration_s: num(&v["duration"]),
        moving_s: num(&v["movingDuration"]),
        distance_m: num(&v["distance"]),
        ascent_m: num(&v["elevationGain"]),
        avg_hr: num(&v["averageHR"]),
        max_hr: num(&v["maxHR"]),
        calories: num(&v["calories"]),
        device_load: num(&v["activityTrainingLoad"]),
        ..Default::default()
    };
    Ok(Parsed {
        recs: vec![Rec::Activity(Box::new(a))],
        content: "activity summary".into(),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day_val(p: &Parsed, metric: &str) -> Option<f64> {
        p.recs.iter().find_map(|r| match r {
            Rec::Day { metric: m, value, .. } if *m == metric => Some(*value),
            _ => None,
        })
    }

    #[test]
    fn sleep_night_belongs_to_the_morning_and_keeps_offset() {
        // synthetic file: 23:00 → 07:00 local at UTC+2
        let gmt_start = 1_791_500_400_000f64; // any instant
        let off = 7_200_000f64;
        let j = serde_json::json!({
            "dailySleepDTO": {
                "calendarDate": "2026-10-09", "sleepTimeSeconds": 27000, "deepSleepSeconds": 3600,
                "remSleepSeconds": 5400, "lightSleepSeconds": 18000, "awakeSleepSeconds": 1800, "napTimeSeconds": 0,
                "sleepStartTimestampGMT": gmt_start, "sleepStartTimestampLocal": gmt_start + off,
                "sleepEndTimestampGMT": gmt_start + 28_800_000.0, "sleepEndTimestampLocal": gmt_start + 28_800_000.0 + off,
                "sleepScores": {"overall": {"value": 81}}, "avgSleepStress": 18.5, "averageRespirationValue": 14.0
            },
            "avgOvernightHrv": 31.0, "hrvStatus": "BALANCED", "restingHeartRate": 52
        });
        let p = sleep(j.to_string().as_bytes()).unwrap();
        assert_eq!(day_val(&p, "sleep.hours"), Some(7.5));
        assert_eq!(day_val(&p, "sleep.score"), Some(81.0));
        assert_eq!(day_val(&p, "tz.offset_s"), Some(7200.0));
        assert_eq!(day_val(&p, "body.hrv"), Some(31.0));
        assert!(p.recs.iter().all(|r| match r {
            Rec::Day { day, .. } | Rec::DayText { day, .. } => *day == NaiveDate::from_ymd_opt(2026, 10, 9).unwrap(),
            _ => false,
        }));
    }

    #[test]
    fn empty_night_adds_nothing() {
        let j = serde_json::json!({"dailySleepDTO": {"calendarDate": "2020-01-01", "sleepTimeSeconds": null}});
        assert!(sleep(j.to_string().as_bytes()).unwrap().recs.is_empty());
    }

    #[test]
    fn hrv_range_from_the_hrv_service() {
        // shape of a real reply (2026-05), trimmed
        let j = serde_json::json!({"hrvSummaries": [
            {"calendarDate": "2026-05-04", "weeklyAvg": 34, "lastNightAvg": 37, "lastNight5MinHigh": 89,
             "baseline": {"lowUpper": 28, "balancedLow": 30, "balancedUpper": 39, "markerValue": 0.47}, "status": "BALANCED"},
            {"calendarDate": "2026-05-05", "weeklyAvg": 34, "lastNightAvg": null, "baseline": null, "status": "NONE"}
        ], "userProfilePk": 1});
        let p = hrv(j.to_string().as_bytes()).unwrap();
        assert_eq!(day_val(&p, "body.hrv"), Some(37.0));
        assert_eq!(day_val(&p, "body.hrv_baseline_hi"), Some(39.0));
        let vals = p.recs.iter().filter(|r| matches!(r, Rec::Day { metric: "body.hrv", .. })).count();
        assert_eq!(vals, 1, "a night without a reading adds nothing");
        assert!(p.recs.iter().any(|r| matches!(r, Rec::DayText { metric: "body.hrv_status", text, .. } if text == "balanced")));
    }

    #[test]
    fn weight_is_grams() {
        let j = serde_json::json!({"dateWeightList": [{"calendarDate": "2026-07-18", "weight": 88450.0}]});
        assert_eq!(day_val(&weight(j.to_string().as_bytes()).unwrap(), "body.weight"), Some(88.45));
    }

    #[test]
    fn activity_offset_from_local_and_gmt() {
        let j = serde_json::json!({"activityId": 42, "startTimeGMT": "2026-10-04 06:00:00", "startTimeLocal": "2026-10-04 08:00:00",
            "activityType": {"typeKey": "mountain_biking"}, "duration": 3600.5, "averageHR": 131.0});
        let p = activity(j.to_string().as_bytes()).unwrap();
        let Rec::Activity(a) = &p.recs[0] else { panic!() };
        assert_eq!((a.id, a.utc_offset_s, a.avg_hr), (42, Some(7200), Some(131.0)));
        assert_eq!(a.type_key.as_deref(), Some("mountain_biking"));
    }

    #[test]
    fn bad_json_is_an_error_not_a_panic() {
        assert!(sleep(b"{not json").is_err());
    }
}
