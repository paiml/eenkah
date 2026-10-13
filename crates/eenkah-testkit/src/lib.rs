//! A synthetic athlete: deterministic daily physiology with a known training response, written
//! as files in the same layout and formats Garmin uses. Every fixture comes from here (ING-7).
//!
//! The model is deliberately simple and known, so tests can check that eenkah recovers it:
//! HRV dips and resting heart rate rises the day after a hard session; short sleep lowers HRV.

use chrono::{Duration, NaiveDate};
use serde_json::json;
use std::path::Path;

/// Small deterministic PRNG (PCG-style LCG); no dependency, same numbers on every platform.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1)
    }
    /// Uniform in [0, 1).
    pub fn f(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Approximately standard normal (sum of 12 uniforms).
    pub fn n(&mut self) -> f64 {
        (0..12).map(|_| self.f()).sum::<f64>() - 6.0
    }
}

#[derive(Clone, Debug)]
pub struct Day {
    pub date: NaiveDate,
    pub hrv: f64,
    pub rhr: f64,
    pub sleep_h: f64,
    pub bed_min: f64,
    /// Session (start minute of day, minutes, average HR), if any.
    pub session: Option<(f64, f64, f64)>,
}

/// Generate `n` days starting at `start`.
pub fn athlete(start: NaiveDate, n: i64, seed: u64) -> Vec<Day> {
    let mut r = Rng::new(seed);
    let mut out: Vec<Day> = vec![];
    let mut hard_yesterday = false;
    for k in 0..n {
        let date = start + Duration::days(k);
        let sleep_h = (7.4 + 0.6 * r.n()).clamp(4.0, 10.0);
        let mut hrv = 32.0 + 2.0 * r.n() + 1.5 * (sleep_h - 7.4);
        let mut rhr = 52.0 + 1.0 * r.n();
        if hard_yesterday {
            hrv -= 5.0;
            rhr += 2.5;
        }
        let session = match k % 7 {
            1 | 3 => Some((18.0 * 60.0, 60.0, 128.0 + 4.0 * r.n())),
            5 => Some((9.0 * 60.0, 240.0, 135.0 + 4.0 * r.n())),
            _ => None,
        };
        hard_yesterday = matches!(session, Some((_, m, _)) if m > 120.0);
        out.push(Day {
            date,
            hrv,
            rhr,
            sleep_h,
            bed_min: 23.0 * 60.0 + 20.0 * r.n(),
            session,
        });
    }
    out
}

/// Write the athlete as a folder in the Garmin Connect file layout: Sleep/, RHR/ and FitFiles/Activities/ JSON.
/// Offsets are UTC+2 throughout.
pub fn write_health_data(dir: &Path, days: &[Day]) -> std::io::Result<()> {
    let off_ms = 7_200_000i64;
    for sub in ["Sleep", "RHR", "FitFiles/Activities"] {
        std::fs::create_dir_all(dir.join(sub))?;
    }
    for (i, d) in days.iter().enumerate() {
        let midnight_utc_ms = d.date.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp_millis() - off_ms;
        let start_gmt = midnight_utc_ms + ((d.bed_min - 24.0 * 60.0) * 60_000.0) as i64;
        let end_gmt = start_gmt + (d.sleep_h * 3_600_000.0) as i64 + 1_800_000;
        let sleep = json!({
            "dailySleepDTO": {
                "calendarDate": d.date.to_string(),
                "sleepTimeSeconds": (d.sleep_h * 3600.0).round(),
                "deepSleepSeconds": (d.sleep_h * 0.2 * 3600.0).round(),
                "remSleepSeconds": (d.sleep_h * 0.2 * 3600.0).round(),
                "lightSleepSeconds": (d.sleep_h * 0.6 * 3600.0).round(),
                "awakeSleepSeconds": 1800, "napTimeSeconds": 0,
                "sleepStartTimestampGMT": start_gmt, "sleepStartTimestampLocal": start_gmt + off_ms,
                "sleepEndTimestampGMT": end_gmt, "sleepEndTimestampLocal": end_gmt + off_ms,
                "sleepScores": {"overall": {"value": (60.0 + d.sleep_h * 3.0).round()}}
            },
            "avgOvernightHrv": (d.hrv * 10.0).round() / 10.0,
            "hrvStatus": "BALANCED"
        });
        std::fs::write(dir.join(format!("Sleep/sleep_{}.json", d.date)), sleep.to_string())?;
        let rhr = json!({"allMetrics": {"metricsMap": {"WELLNESS_RESTING_HEART_RATE": [{"value": d.rhr.round(), "calendarDate": d.date.to_string()}]}}});
        std::fs::write(dir.join(format!("RHR/rhr_{}.json", d.date)), rhr.to_string())?;
        if let Some((start_min, minutes, hr)) = d.session {
            let id = 10_000 + i as i64;
            let gmt = d.date.and_hms_opt(0, 0, 0).unwrap() + Duration::minutes(start_min as i64) - Duration::hours(2);
            let a = json!({
                "activityId": id, "activityName": "Synthetic ride",
                "startTimeGMT": gmt.format("%Y-%m-%d %H:%M:%S").to_string(),
                "startTimeLocal": (gmt + Duration::hours(2)).format("%Y-%m-%d %H:%M:%S").to_string(),
                "activityType": {"typeKey": "road_biking"}, "duration": minutes * 60.0, "movingDuration": minutes * 60.0,
                "averageHR": hr.round(), "maxHR": (hr + 25.0).round()
            });
            std::fs::write(dir.join(format!("FitFiles/Activities/activity_{id}.json")), a.to_string())?;
        }
    }
    Ok(())
}
