//! Model 3: shifts in a signal's normal level.
//!
//! Each day, aprender's window drift detector (`metrics::drift::DriftDetector`) compares the
//! last 14 nights with the 60 before them: the score is the change in mean in units of the
//! reference spread. A score of 1.0 or more is a shift (SHF-1). The shift's start is then placed
//! at the split that best separates the two levels, and the next search uses only nights after
//! it, so one shift is reported once.
//!
//! aprender's streaming detectors (`PageHinkley`, `ADWIN`, `DDM`) take only a true/false error
//! stream, so they cannot watch a measured value (docs/DOGFOOD.md gap G11).

use aprender::metrics::drift::{DriftConfig, DriftDetector, DriftStatus};
use aprender::primitives::Vector;
use chrono::{Duration, NaiveDate};
use eenkah_core::{window, Daily};
use serde::Serialize;

pub const CURRENT_DAYS: i64 = 14;
pub const REFERENCE_DAYS: i64 = 60;
pub const MIN_CURRENT: usize = 10;
pub const MIN_REFERENCE: usize = 30;
/// Change in mean, in reference standard deviations, that counts as a shift.
pub const SHIFT_SD: f32 = 1.0;
/// Nights on each side of a shift's start.
const MIN_SIDE: usize = 5;

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
pub struct Shift {
    /// First night at the new level (estimated).
    pub day: NaiveDate,
    /// Night on which the shift was first detectable.
    pub detected: NaiveDate,
    /// Mean level before and after, in model space.
    pub before: f64,
    pub after: f64,
    /// Change in reference standard deviations.
    pub score: f64,
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

fn vector(v: &[f64]) -> Vector<f32> {
    Vector::from_slice(&v.iter().map(|x| *x as f32).collect::<Vec<_>>())
}

/// The split in `pts` (date order) that best separates two levels, by the two-sample t
/// statistic, among splits leaving at least `MIN_SIDE` nights on each side.
fn best_split(pts: &[(NaiveDate, f64)], from: NaiveDate) -> Option<usize> {
    let n = pts.len();
    (MIN_SIDE..=n.saturating_sub(MIN_SIDE))
        .filter(|i| pts[*i].0 >= from)
        .map(|i| {
            let (a, b): (Vec<f64>, Vec<f64>) = (pts[..i].iter().map(|p| p.1).collect(), pts[i..].iter().map(|p| p.1).collect());
            let (na, nb) = (a.len() as f64, b.len() as f64);
            (i, (mean(&b) - mean(&a)).abs() * (na * nb / (na + nb)).sqrt())
        })
        .max_by(|x, y| x.1.total_cmp(&y.1))
        .map(|x| x.0)
}

/// Every shift in `s` (model space), oldest first. Each check uses only nights up to the day
/// being checked.
pub fn detect(s: &Daily) -> Vec<Shift> {
    let det = DriftDetector::new(DriftConfig::new(SHIFT_SD * 0.6, SHIFT_SD).with_min_samples(MIN_CURRENT));
    let mut out: Vec<Shift> = vec![];
    for d in s.keys().copied() {
        let cur_start = d - Duration::days(CURRENT_DAYS - 1);
        let mut ref_start = cur_start - Duration::days(REFERENCE_DAYS);
        if let Some(last) = out.last() {
            ref_start = ref_start.max(last.day);
        }
        let reference = window(s, ref_start, cur_start - Duration::days(1));
        let current = window(s, cur_start, d);
        if reference.len() < MIN_REFERENCE || current.len() < MIN_CURRENT {
            continue;
        }
        let DriftStatus::Drift { score } = det.detect_univariate(&vector(&reference), &vector(&current)) else {
            continue;
        };
        let pts: Vec<(NaiveDate, f64)> = s.range(ref_start..=d).map(|(k, v)| (*k, *v)).collect();
        let Some(i) = best_split(&pts, cur_start - Duration::days(7)) else { continue };
        let before: Vec<f64> = pts[..i].iter().map(|p| p.1).collect();
        let after: Vec<f64> = pts[i..].iter().map(|p| p.1).collect();
        out.push(Shift {
            day: pts[i].0,
            detected: d,
            before: mean(&before),
            after: mean(&after),
            score: score as f64,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series(vals: &[f64]) -> Daily {
        let d0 = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        vals.iter().enumerate().map(|(i, v)| (d0 + Duration::days(i as i64), *v)).collect()
    }

    #[test]
    fn shf1_a_sustained_level_change_is_found_once_near_its_start() {
        let mut r = eenkah_testkit::Rng::new(11);
        let vals: Vec<f64> = (0..200).map(|i| if i < 120 { 50.0 } else { 53.0 } + 1.5 * r.n()).collect();
        let s = series(&vals);
        let found = detect(&s);
        assert_eq!(found.len(), 1, "{found:?}");
        let start = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap() + Duration::days(120);
        assert!((found[0].day - start).num_days().abs() <= 4, "{found:?}");
        assert!(found[0].after > found[0].before);
        assert!((found[0].detected - start).num_days() <= 14, "detected within two weeks");
    }

    #[test]
    fn shf1_steady_noise_has_no_shifts() {
        let mut r = eenkah_testkit::Rng::new(12);
        let vals: Vec<f64> = (0..365).map(|_| 50.0 + 1.5 * r.n()).collect();
        assert!(detect(&series(&vals)).is_empty());
    }

    #[test]
    fn shf1_detection_uses_only_the_past() {
        let mut r = eenkah_testkit::Rng::new(13);
        let vals: Vec<f64> = (0..200).map(|i| if i < 120 { 50.0 } else { 53.0 } + 1.5 * r.n()).collect();
        let s = series(&vals);
        let first = detect(&s)[0];
        let cut: Daily = s.range(..=first.detected).map(|(k, v)| (*k, *v)).collect();
        assert_eq!(detect(&cut).last().map(|x| x.detected), Some(first.detected));
    }
}
