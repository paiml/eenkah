//! Body-state and per-session metrics. Constants and their sources are listed in
//! `docs/METRICS.md`; the catalogue in [`crate::catalog`] is the single description.

use crate::stats::{mean, percentile, sd};
use crate::{window, Daily};
use chrono::{Duration, NaiveDate};

/// Natural log of overnight HRV (ms). `None` for non-positive input.
pub fn hrv_ln(hrv_ms: f64) -> Option<f64> {
    (hrv_ms > 0.0).then(|| hrv_ms.ln())
}

/// Map every value of a series through `f`, dropping days where `f` gives `None`.
pub fn map(s: &Daily, f: impl Fn(f64) -> Option<f64>) -> Daily {
    s.iter().filter_map(|(d, v)| f(*v).map(|x| (*d, x))).collect()
}

/// Trailing mean over `days` calendar days ending on (and including) each day that has a
/// value. A day gets a value only if at least `min_count` values fall in its window.
pub fn rolling_mean(s: &Daily, days: i64, min_count: usize) -> Daily {
    s.keys()
        .filter_map(|d| {
            let w = window(s, *d - Duration::days(days - 1), *d);
            (w.len() >= min_count).then(|| mean(&w)).flatten().map(|m| (*d, m))
        })
        .collect()
}

/// The athlete's own normal band for each day: the `lo`/`hi` percentiles of the `days`
/// calendar days *before* it (the day itself is excluded so it is compared to its past).
pub struct Band {
    pub lo: Daily,
    pub mid: Daily,
    pub hi: Daily,
}

pub fn normal_band(s: &Daily, on: &[NaiveDate], days: i64, min_count: usize, lo: f64, hi: f64) -> Band {
    let mut b = Band {
        lo: Daily::new(),
        mid: Daily::new(),
        hi: Daily::new(),
    };
    for d in on {
        let w = window(s, *d - Duration::days(days), *d - Duration::days(1));
        if w.len() < min_count {
            continue;
        }
        if let (Some(a), Some(m), Some(z)) = (percentile(&w, lo), percentile(&w, 0.5), percentile(&w, hi)) {
            b.lo.insert(*d, a);
            b.mid.insert(*d, m);
            b.hi.insert(*d, z);
        }
    }
    b
}

/// Distance of each day's value from the mean of the previous `days` calendar days, in
/// standard deviations. Needs `min_count` prior values and a non-zero spread.
pub fn z_score(s: &Daily, days: i64, min_count: usize) -> Daily {
    s.iter()
        .filter_map(|(d, v)| {
            let w = window(s, *d - Duration::days(days), *d - Duration::days(1));
            if w.len() < min_count {
                return None;
            }
            let (m, sd) = (mean(&w)?, sd(&w)?);
            (sd > 0.0).then(|| (*d, (v - m) / sd))
        })
        .collect()
}

/// Running sleep shortfall over the last 7 days, in hours: Σ(normal − slept), where normal
/// is the mean of the 28 days before the 7-day window. Positive = debt. Needs ≥ 5 of the 7
/// nights and ≥ 14 of the 28.
pub fn sleep_debt_7d(hours: &Daily) -> Daily {
    hours
        .keys()
        .filter_map(|d| {
            let wk = window(hours, *d - Duration::days(6), *d);
            let base = window(hours, *d - Duration::days(34), *d - Duration::days(7));
            if wk.len() < 5 || base.len() < 14 {
                return None;
            }
            let normal = mean(&base)?;
            Some((*d, wk.iter().map(|h| normal - h).sum()))
        })
        .collect()
}

/// Sleep regularity: mean of the standard deviations of bedtime and wake time over the last
/// 7 nights, in minutes. Inputs are local clock minutes; bedtime is measured from 18:00 so
/// that 23:30 and 00:30 are an hour apart, not 23 hours. Needs ≥ 4 nights.
pub fn sleep_regularity(bed_min: &Daily, wake_min: &Daily) -> Daily {
    let shift = |m: f64| (m - 18.0 * 60.0).rem_euclid(24.0 * 60.0);
    bed_min
        .keys()
        .filter_map(|d| {
            let b: Vec<f64> = window(bed_min, *d - Duration::days(6), *d).into_iter().map(shift).collect();
            let w = window(wake_min, *d - Duration::days(6), *d);
            if b.len() < 4 || w.len() < 4 {
                return None;
            }
            Some((*d, (sd(&b)? + sd(&w)?) / 2.0))
        })
        .collect()
}

/// Heart-rate reserve fraction, clamped to [0, 1].
pub fn hr_reserve(hr: f64, rest: f64, max: f64) -> Option<f64> {
    (max > rest && hr > 0.0).then(|| ((hr - rest) / (max - rest)).clamp(0.0, 1.0))
}

/// Banister training impulse for `minutes` at reserve fraction `hrr`:
/// `minutes × hrr × 0.64 × e^(k × hrr)` with k = 1.92 (men) or 1.67 (women).
pub fn trimp(minutes: f64, hrr: f64, k: f64) -> f64 {
    minutes * hrr * 0.64 * (k * hrr).exp()
}

/// Session load from heart-rate samples `(seconds since start, bpm)`. Each sample covers the
/// time to the next one, capped at `max_gap_s` so pauses are not counted.
pub fn session_trimp(samples: &[(f64, f64)], rest: f64, max: f64, k: f64, max_gap_s: f64) -> Option<f64> {
    if samples.len() < 2 {
        return None;
    }
    let mut total = 0.0;
    for w in samples.windows(2) {
        let dt = (w[1].0 - w[0].0).clamp(0.0, max_gap_s);
        if let Some(r) = hr_reserve(w[0].1, rest, max) {
            total += trimp(dt / 60.0, r, k);
        }
    }
    Some(total)
}

/// Heart-rate zone boundaries as fractions of threshold heart rate (LTHR). Zone 1 is below
/// the first bound, zone 5 at or above the last. Default: Friel's LTHR zones.
pub const LTHR_ZONES: [f64; 4] = [0.81, 0.90, 0.94, 1.00];

/// Seconds in each of 5 zones from `(seconds, bpm)` samples, same gap rule as [`session_trimp`].
pub fn time_in_zones(samples: &[(f64, f64)], lthr: f64, bounds: &[f64; 4], max_gap_s: f64) -> [f64; 5] {
    let mut z = [0.0; 5];
    if lthr <= 0.0 {
        return z;
    }
    for w in samples.windows(2) {
        let dt = (w[1].0 - w[0].0).clamp(0.0, max_gap_s);
        let f = w[0].1 / lthr;
        let k = bounds.iter().take_while(|b| f >= **b).count();
        z[k] += dt;
    }
    z
}

/// Aerobic decoupling, %: efficiency (output ÷ heart rate) in the first half minus the second
/// half, as a share of the first. `samples` are `(seconds, output, bpm)`. Positive = drift.
pub fn decoupling(samples: &[(f64, f64, f64)]) -> Option<f64> {
    if samples.len() < 20 {
        return None;
    }
    let t_mid = (samples[0].0 + samples[samples.len() - 1].0) / 2.0;
    let ef = |it: &mut dyn Iterator<Item = &(f64, f64, f64)>| {
        let (mut o, mut h, mut n) = (0.0, 0.0, 0usize);
        for s in it {
            o += s.1;
            h += s.2;
            n += 1;
        }
        (n > 0 && h > 0.0).then(|| o / h)
    };
    let a = ef(&mut samples.iter().filter(|s| s.0 < t_mid))?;
    let b = ef(&mut samples.iter().filter(|s| s.0 >= t_mid))?;
    (a > 0.0).then(|| (a - b) / a * 100.0)
}

/// Efficiency: mean output ÷ mean heart rate over samples where both are present.
pub fn efficiency(samples: &[(f64, f64, f64)]) -> Option<f64> {
    let o: Vec<f64> = samples.iter().map(|s| s.1).collect();
    let h: Vec<f64> = samples.iter().map(|s| s.2).collect();
    let (mo, mh) = (mean(&o)?, mean(&h)?);
    (mh > 0.0).then(|| mo / mh)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(n: i64) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 1, 1).unwrap() + Duration::days(n)
    }

    #[test]
    fn missing_days_stay_missing() {
        // MET-1: a gap in the input is a gap in the output, never a zero.
        let s: Daily = [(d(0), 50.0), (d(1), 52.0), (d(5), 54.0)].into_iter().collect();
        let m = rolling_mean(&s, 3, 1);
        assert!(!m.contains_key(&d(2)) && !m.contains_key(&d(3)));
        assert_eq!(m[&d(1)], 51.0);
        assert_eq!(m[&d(5)], 54.0); // window d3..d5 holds only d5
    }

    #[test]
    fn z_score_by_hand() {
        // prior values 50, 52, 54 (mean 52, sd 2); today 56 → z = 2
        let s: Daily = [(d(0), 50.0), (d(1), 52.0), (d(2), 54.0), (d(3), 56.0)].into_iter().collect();
        let z = z_score(&s, 60, 3);
        assert!((z[&d(3)] - 2.0).abs() < 1e-12);
        assert!(!z.contains_key(&d(2))); // only two prior values
    }

    #[test]
    fn band_excludes_the_day_itself() {
        let mut s: Daily = (0..30).map(|i| (d(i), 10.0)).collect();
        s.insert(d(30), 100.0);
        let b = normal_band(&s, &[d(30)], 60, 20, 0.1, 0.9);
        assert_eq!(b.hi[&d(30)], 10.0);
    }

    #[test]
    fn sleep_debt_by_hand() {
        // 28 nights of 8 h, then 7 nights of 7 h → debt 7 h
        let mut h: Daily = (0..28).map(|i| (d(i), 8.0)).collect();
        for i in 28..35 {
            h.insert(d(i), 7.0);
        }
        let debt = sleep_debt_7d(&h);
        assert!((debt[&d(34)] - 7.0).abs() < 1e-9);
    }

    #[test]
    fn regularity_wraps_midnight() {
        // bedtimes alternate 23:30 / 00:30 → spread is about an hour, not 23 hours
        let bed: Daily = (0..7).map(|i| (d(i), if i % 2 == 0 { 23.5 * 60.0 } else { 30.0 })).collect();
        let wake: Daily = (0..7).map(|i| (d(i), 7.0 * 60.0)).collect();
        let r = sleep_regularity(&bed, &wake)[&d(6)];
        assert!(r > 10.0 && r < 40.0, "{r}");
    }

    #[test]
    fn trimp_by_hand() {
        // 60 min at hrr 0.5, k 1.92: 60 × 0.5 × 0.64 × e^0.96 = 50.14…
        let t = trimp(60.0, 0.5, 1.92);
        assert!((t - 60.0 * 0.5 * 0.64 * 0.96f64.exp()).abs() < 1e-9);
        assert!((t - 50.14).abs() < 0.01);
    }

    #[test]
    fn session_trimp_matches_constant_effort_and_caps_pauses() {
        // 61 samples one minute apart at a constant heart rate = 60 minutes of trimp
        let s: Vec<(f64, f64)> = (0..=60).map(|i| (i as f64 * 60.0, 120.0)).collect();
        let hrr = hr_reserve(120.0, 50.0, 190.0).unwrap();
        let t = session_trimp(&s, 50.0, 190.0, 1.92, 120.0).unwrap();
        assert!((t - trimp(60.0, hrr, 1.92)).abs() < 1e-9);
        // a 1-hour pause between two samples counts only max_gap_s
        let p = session_trimp(&[(0.0, 120.0), (3600.0, 120.0)], 50.0, 190.0, 1.92, 10.0).unwrap();
        assert!((p - trimp(10.0 / 60.0, hrr, 1.92)).abs() < 1e-9);
    }

    #[test]
    fn zones_sum_to_session_time() {
        let s: Vec<(f64, f64)> = (0..100).map(|i| (i as f64, 100.0 + i as f64)).collect();
        let z = time_in_zones(&s, 163.0, &LTHR_ZONES, 5.0);
        assert!((z.iter().sum::<f64>() - 99.0).abs() < 1e-9);
        // 100..131 bpm (< 0.81 × 163 = 132.03) is zone 1: samples 0..=32 → 33 s
        assert_eq!(z[0], 33.0);
    }

    #[test]
    fn decoupling_detects_drift() {
        // same output, heart rate 10 % higher in the second half → ≈ 9.1 % decoupling
        let s: Vec<(f64, f64, f64)> = (0..100).map(|i| (i as f64, 3.0, if i < 50 { 130.0 } else { 143.0 })).collect();
        let dc = decoupling(&s).unwrap();
        assert!((dc - (1.0 - 130.0 / 143.0) * 100.0).abs() < 1e-9);
        let flat: Vec<(f64, f64, f64)> = (0..100).map(|i| (i as f64, 3.0, 130.0)).collect();
        assert!(decoupling(&flat).unwrap().abs() < 1e-12);
    }
}
