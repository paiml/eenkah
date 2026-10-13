//! Model 1: the athlete's own normal range for each body signal, rebuilt for every day.
//!
//! For each day, aprender's `NormalInverseGamma` is updated with that signal's values from the
//! previous 60 days: never the day itself, never a later day (NRM-1). After a shift in the
//! signal's level (model 3), the window restarts at the shift once enough days follow it, so the
//! normal adapts in days instead of averaging the old level away (NRM-3).
//!
//! The range is the posterior predictive interval for one new night: Student-t with 2αₙ degrees
//! of freedom, centre μₙ and scale √(βₙ(κₙ+1)/(αₙκₙ)). aprender 0.70.3 returns only the point
//! (`posterior_predictive`), so the band is computed here from its posterior parameters
//! (docs/DOGFOOD.md gap G9).

use aprender::bayesian::NormalInverseGamma;
use chrono::{Duration, NaiveDate};
use eenkah_core::{window, Daily};
use serde::Serialize;
use std::collections::BTreeMap;

pub const WINDOW_DAYS: i64 = 60;
/// Prior nights needed for a range.
pub const MIN_PRIOR: usize = 20;
/// After a shift, the window restarts at the shift once this many nights follow it.
pub const MIN_AFTER_SHIFT: usize = 10;
/// The range holds 80 % of nights.
pub const LEVEL: f64 = 0.8;

/// A body signal with a personal normal. `key` is the stored daily metric.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Signal {
    pub key: &'static str,
    /// The suyo series of the same values.
    pub series: &'static str,
    pub name: &'static str,
    pub label: &'static str,
    /// The signal inside a sentence ("resting heart rate", "HRV").
    pub phrase: &'static str,
    pub unit: &'static str,
    /// Modelled as ln(value): HRV, whose day-to-day changes are proportional.
    pub log: bool,
    /// Which direction is good for the athlete: 1 up, -1 down, 0 neither.
    pub better: i8,
    pub digits: usize,
}

#[allow(clippy::too_many_arguments)]
const fn sig(key: &'static str, series: &'static str, name: &'static str, label: &'static str, phrase: &'static str, unit: &'static str, log: bool, better: i8, digits: usize) -> Signal {
    Signal {
        key,
        series,
        name,
        label,
        phrase,
        unit,
        log,
        better,
        digits,
    }
}

pub const SIGNALS: &[Signal] = &[
    sig("body.hrv", "eenkah.body.hrv", "hrv", "Overnight HRV", "HRV", "ms", true, 1, 0),
    sig("body.rhr", "eenkah.body.rhr", "rhr", "Resting heart rate", "resting heart rate", "bpm", false, -1, 0),
    sig("sleep.score", "eenkah.sleep.score", "sleep_score", "Sleep score", "sleep score", "", false, 1, 0),
    sig("body.bb_wake", "eenkah.body.bb_wake", "body_battery", "Body Battery at wake", "Body Battery at wake", "", false, 1, 0),
    sig("sleep.stress", "eenkah.sleep.stress", "sleep_stress", "Overnight stress", "overnight stress", "", false, -1, 0),
    sig("sleep.respiration", "eenkah.sleep.respiration", "respiration", "Breathing rate", "breathing rate", "brpm", false, 0, 1),
    sig("body.spo2", "eenkah.body.spo2", "spo2", "Blood oxygen", "blood oxygen", "%", false, 0, 0),
    sig(
        "body.skin_temp_dev",
        "eenkah.body.skin_temp_dev",
        "skin_temp",
        "Skin temperature",
        "skin temperature",
        "°C",
        false,
        0,
        1,
    ),
];

impl Signal {
    pub fn to_model(&self, v: f64) -> Option<f64> {
        if self.log {
            (v > 0.0).then(|| v.ln())
        } else {
            Some(v)
        }
    }
    pub fn to_display(&self, v: f64) -> f64 {
        if self.log {
            v.exp()
        } else {
            v
        }
    }
    pub fn model_series(&self, raw: &Daily) -> Daily {
        raw.iter().filter_map(|(d, v)| Some((*d, self.to_model(*v)?))).collect()
    }
}

/// A normal range in model space (ln ms for HRV).
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
pub struct Band {
    pub mid: f64,
    pub lo: f64,
    pub hi: f64,
    /// Scale of the predictive distribution; a day's distance from normal is (v − mid) ÷ scale.
    pub scale: f64,
    /// Nights the range was built from.
    pub n: usize,
}

/// Upper quantile of Student-t for probability `p` and `nu` degrees of freedom, by the
/// Cornish-Fisher expansion around the normal quantile (error < 0.002 for ν ≥ 10 at p = 0.9).
fn t_quantile_090(nu: f64) -> f64 {
    let z: f64 = 1.281_551_565_545; // Φ⁻¹(0.9)
    z + (z.powi(3) + z) / (4.0 * nu) + (5.0 * z.powi(5) + 16.0 * z.powi(3) + 3.0 * z) / (96.0 * nu * nu)
}

/// The 80 % predictive range for one new value after observing `values`.
pub fn predictive(values: &[f64]) -> Option<Band> {
    if values.len() < 2 {
        return None;
    }
    // centre first: aprender's prior mean is 0 and its parameters are f32
    let c = values.iter().sum::<f64>() / values.len() as f64;
    let centred: Vec<f32> = values.iter().map(|v| (v - c) as f32).collect();
    let mut m = NormalInverseGamma::noninformative();
    m.update(&centred);
    let (mu, kappa, alpha, beta) = (m.mu() as f64 + c, m.kappa() as f64, m.alpha() as f64, m.beta() as f64);
    if alpha <= 1.0 || kappa <= 0.0 || beta <= 0.0 {
        return None;
    }
    let scale = (beta * (kappa + 1.0) / (alpha * kappa)).sqrt();
    let half = t_quantile_090(2.0 * alpha) * scale;
    Some(Band {
        mid: mu,
        lo: mu - half,
        hi: mu + half,
        scale,
        n: values.len(),
    })
}

/// The normal range for each day in `days`, from that day's past only (NRM-1). `shifts` are
/// detected level shifts in this signal as (first night at the new level, night detected),
/// oldest first; a shift counts only from the night after it was detected (NRM-3).
pub fn bands(s: &Daily, days: impl IntoIterator<Item = NaiveDate>, shifts: &[(NaiveDate, NaiveDate)]) -> BTreeMap<NaiveDate, Band> {
    let mut out = BTreeMap::new();
    for d in days {
        let y = d - Duration::days(1);
        let start = d - Duration::days(WINDOW_DAYS);
        let after_shift = shifts
            .iter()
            .rev()
            .find(|(first, detected)| *detected <= y && *first > start)
            .map(|(first, _)| window(s, *first, y))
            .filter(|v| v.len() >= MIN_AFTER_SHIFT);
        let prior = match after_shift {
            Some(v) => v,
            None => {
                let v = window(s, start, y);
                if v.len() < MIN_PRIOR {
                    continue;
                }
                v
            }
        };
        if let Some(b) = predictive(&prior) {
            out.insert(d, b);
        }
    }
    out
}

pub fn signal(name: &str) -> Option<&'static Signal> {
    SIGNALS.iter().find(|s| s.name == name)
}

pub fn status(v: f64, b: &Band) -> &'static str {
    if v < b.lo {
        "below"
    } else if v > b.hi {
        "above"
    } else {
        "within"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series(vals: &[f64]) -> Daily {
        let d0 = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        vals.iter().enumerate().map(|(i, v)| (d0 + Duration::days(i as i64), *v)).collect()
    }

    #[test]
    fn nrm1_range_uses_only_earlier_days() {
        // NRM-1: changing the day itself or any later day leaves its range unchanged
        let mut r = eenkah_testkit::Rng::new(3);
        let vals: Vec<f64> = (0..120).map(|_| 50.0 + 2.0 * r.n()).collect();
        let s = series(&vals);
        let d = *s.keys().nth(90).unwrap();
        let a = bands(&s, [d], &[])[&d];
        let mut t = s.clone();
        for (k, v) in t.iter_mut() {
            if *k >= d {
                *v += 30.0;
            }
        }
        assert_eq!(a, bands(&t, [d], &[])[&d]);
    }

    #[test]
    fn nrm2_range_holds_about_80_percent_of_nights() {
        let mut r = eenkah_testkit::Rng::new(4);
        let vals: Vec<f64> = (0..800).map(|_| 52.0 + 1.5 * r.n()).collect();
        let s = series(&vals);
        let b = bands(&s, s.keys().copied(), &[]);
        let inside = b.iter().filter(|(d, b)| status(s[*d], b) == "within").count() as f64 / b.len() as f64;
        assert!((0.75..=0.85).contains(&inside), "{inside}");
    }

    #[test]
    fn predictive_matches_textbook_t_interval() {
        // n = 40, sample mean 0, sample sd 1: t(0.9, 39) × sd × √(1 + 1/n) ≈ 1.304 × 1.0124
        let mut r = eenkah_testkit::Rng::new(5);
        let mut v: Vec<f64> = (0..40).map(|_| r.n()).collect();
        let m = v.iter().sum::<f64>() / 40.0;
        let sd = (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / 39.0).sqrt();
        v.iter_mut().for_each(|x| *x = (*x - m) / sd);
        let b = predictive(&v).unwrap();
        let half = (b.hi - b.lo) / 2.0;
        assert!(b.mid.abs() < 1e-3);
        // the noninformative prior uses n (not n − 1) degrees of freedom, so allow 3 %
        assert!((half - 1.304 * (1.0f64 + 1.0 / 40.0).sqrt()).abs() < 0.04, "{half}");
    }

    #[test]
    fn nrm3_range_restarts_after_a_shift() {
        let mut r = eenkah_testkit::Rng::new(6);
        let vals: Vec<f64> = (0..140).map(|i| if i < 100 { 50.0 } else { 56.0 } + r.n()).collect();
        let s = series(&vals);
        let shift = (*s.keys().nth(100).unwrap(), *s.keys().nth(108).unwrap());
        let d = *s.keys().nth(115).unwrap();
        let with = bands(&s, [d], &[shift])[&d];
        let without = bands(&s, [d], &[])[&d];
        assert!((with.mid - 56.0).abs() < 1.0, "{with:?}");
        assert!(without.mid < 54.0 && without.hi - without.lo > with.hi - with.lo);
    }
}
