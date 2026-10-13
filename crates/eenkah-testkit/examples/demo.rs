//! Writes a richer synthetic athlete for demos and README screenshots: the test athlete plus
//! the signals it leaves out (sleep stress, respiration, skin temperature, Body Battery, steps,
//! day stress), runs alongside the rides, one illness spell and a slow fitness gain.
//! No real personal data.
//!
//!   cargo run -p eenkah-testkit --example demo -- <out dir> [days]
//!
//! Prints the illness spell's first and last day on stdout, for `eenkah mark ... sick`.

use chrono::{Duration, Local};
use eenkah_testkit::{athlete, write_health_data, Rng};
use serde_json::{json, Value};
use std::path::PathBuf;

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(args.next().expect("usage: demo <out dir> [days]"));
    let n: i64 = args.next().map(|s| s.parse().expect("days")).unwrap_or(400);
    let end = Local::now().date_naive();
    let start = end - Duration::days(n - 1);
    let mut days = athlete(start, n, 7);
    let mut r = Rng::new(11);

    // Illness spell 45–40 days ago; resting HR drifts down 3 bpm over the last 120 days.
    let sick = |k: i64| (n - 46..n - 40).contains(&k);
    for (k, d) in days.iter_mut().enumerate() {
        let k = k as i64;
        let gain = ((k - (n - 120)) as f64 / 60.0).clamp(0.0, 1.0);
        d.rhr -= 3.0 * gain;
        d.hrv += 2.0 * gain;
        if sick(k) {
            d.hrv -= 6.0;
            d.rhr += 4.0;
            d.sleep_h += 0.8;
            d.session = None;
        }
    }
    write_health_data(&out, &days)?;

    std::fs::create_dir_all(out.join("Summaries"))?;
    let mut hard_yesterday = false;
    for (k, d) in days.iter().enumerate() {
        let k = k as i64;
        let ill = sick(k);
        let path = out.join(format!("Sleep/sleep_{}.json", d.date));
        let mut v: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        let s = &mut v["dailySleepDTO"];
        s["avgSleepStress"] = json!((16.0 + 2.5 * r.n() + if hard_yesterday { 6.0 } else { 0.0 } + if ill { 9.0 } else { 0.0 }).round());
        s["averageRespirationValue"] = json!(((13.4 + 0.3 * r.n() + if ill { 1.6 } else { 0.0 }) * 10.0).round() / 10.0);
        v["avgSkinTempDeviationC"] = json!(((0.15 * r.n() + if ill { 0.7 } else { 0.0 }) * 10.0).round() / 10.0);
        std::fs::write(&path, v.to_string())?;

        let bb = 62.0 + 6.0 * (d.sleep_h - 7.4) + 5.0 * r.n() - if hard_yesterday { 14.0 } else { 0.0 } - if ill { 20.0 } else { 0.0 };
        let summary = json!({
            "calendarDate": d.date.to_string(),
            "bodyBatteryAtWakeTime": bb.clamp(5.0, 100.0).round(),
            "totalSteps": (9000.0 + 2500.0 * r.n()).max(1500.0).round(),
            "averageStressLevel": (28.0 + 5.0 * r.n() + if ill { 12.0 } else { 0.0 }).round(),
        });
        std::fs::write(out.join(format!("Summaries/daily_summary_{}.json", d.date)), summary.to_string())?;

        // Easy runs on Wednesdays and Sundays.
        if !ill && (k % 7 == 2 || k % 7 == 6) {
            let id = 50_000 + k;
            let gmt = d.date.and_hms_opt(5, 0, 0).unwrap();
            let hr = 140.0 + 5.0 * r.n();
            let a = json!({
                "activityId": id, "activityName": "Synthetic run",
                "startTimeGMT": gmt.format("%Y-%m-%d %H:%M:%S").to_string(),
                "startTimeLocal": (gmt + Duration::hours(2)).format("%Y-%m-%d %H:%M:%S").to_string(),
                "activityType": {"typeKey": "running"}, "duration": 2700.0, "movingDuration": 2650.0,
                "distance": 8000.0 + 800.0 * r.n(), "averageHR": hr.round(), "maxHR": (hr + 20.0).round()
            });
            std::fs::write(out.join(format!("FitFiles/Activities/activity_{id}.json")), a.to_string())?;
        }
        hard_yesterday = matches!(d.session, Some((_, m, _)) if m > 120.0);
    }
    eprintln!("wrote {n} days ({start} → {end}) to {}", out.display());
    println!("{} {}", start + Duration::days(n - 46), start + Duration::days(n - 41));
    Ok(())
}
