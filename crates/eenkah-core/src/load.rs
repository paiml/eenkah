//! Training-load model: fitness, fatigue and form from daily load.
//!
//! Unlike body-state metrics, a day with no recorded session is a real zero load, not a
//! missing value, as long as it lies inside the period the athlete was recording. The caller
//! passes that period as `[from, to]`; outside it nothing is produced.

use crate::stats::{mean, sd};
use crate::{days, Daily};
use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};

/// Time constants in days. 42 and 7 are conventions (Banister 1975 lineage); eenkah may
/// replace them per athlete when a fitted pair wins a backtest (LRN-7).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoadModel {
    pub fitness_days: f64,
    pub fatigue_days: f64,
}

impl Default for LoadModel {
    fn default() -> Self {
        LoadModel {
            fitness_days: 42.0,
            fatigue_days: 7.0,
        }
    }
}

#[derive(Debug, Default)]
pub struct LoadSeries {
    pub daily: Daily,
    pub fitness: Daily,
    pub fatigue: Daily,
    /// Yesterday's fitness minus yesterday's fatigue: what the athlete brings into today.
    pub form: Daily,
    /// Fitness today minus fitness 7 days ago.
    pub ramp: Daily,
    /// 7-day mean load ÷ its standard deviation (Foster 1998). Absent when the spread is 0.
    pub monotony: Daily,
    /// 7-day total load × monotony.
    pub strain: Daily,
}

/// Exponentially weighted average with time constant `tau` days: each day moves the average
/// a fraction `1 − e^(−1/tau)` of the way to that day's load.
pub fn ewma_step(prev: f64, load: f64, tau: f64) -> f64 {
    prev + (load - prev) * (1.0 - (-1.0 / tau).exp())
}

pub fn compute(load: &Daily, from: NaiveDate, to: NaiveDate, m: LoadModel) -> LoadSeries {
    let mut out = LoadSeries::default();
    let (mut fit, mut fat) = (0.0, 0.0);
    let mut prev: Option<(f64, f64)> = None;
    for d in days(from, to) {
        let l = load.get(&d).copied().unwrap_or(0.0);
        out.daily.insert(d, l);
        if let Some((pf, pt)) = prev {
            out.form.insert(d, pf - pt);
        }
        fit = ewma_step(fit, l, m.fitness_days);
        fat = ewma_step(fat, l, m.fatigue_days);
        out.fitness.insert(d, fit);
        out.fatigue.insert(d, fat);
        prev = Some((fit, fat));
        if let Some(f7) = out.fitness.get(&(d - Duration::days(7))) {
            out.ramp.insert(d, fit - f7);
        }
        if d - from >= Duration::days(6) {
            let wk: Vec<f64> = days(d - Duration::days(6), d).map(|x| out.daily[&x]).collect();
            if let (Some(mu), Some(s)) = (mean(&wk), sd(&wk)) {
                if s > 0.0 {
                    out.monotony.insert(d, mu / s);
                    out.strain.insert(d, wk.iter().sum::<f64>() * mu / s);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(n: i64) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 1, 1).unwrap() + Duration::days(n)
    }

    #[test]
    fn steady_load_converges_and_form_uses_yesterday() {
        let load: Daily = (0..400).map(|i| (d(i), 100.0)).collect();
        let s = compute(&load, d(0), d(399), LoadModel::default());
        assert!((s.fitness[&d(399)] - 100.0).abs() < 0.01);
        assert!((s.fatigue[&d(399)] - 100.0).abs() < 1e-6);
        assert!(!s.form.contains_key(&d(0)));
        assert_eq!(s.form[&d(1)], s.fitness[&d(0)] - s.fatigue[&d(0)]);
        // constant load has zero spread → monotony is absent, not infinite
        assert!(s.monotony.is_empty());
    }

    #[test]
    fn first_day_by_hand() {
        let load: Daily = [(d(0), 70.0)].into_iter().collect();
        let s = compute(&load, d(0), d(1), LoadModel::default());
        assert!((s.fatigue[&d(0)] - 70.0 * (1.0 - (-1.0f64 / 7.0).exp())).abs() < 1e-12);
        assert_eq!(s.daily[&d(1)], 0.0); // a rest day inside the period is a real zero
        assert!(s.fitness[&d(1)] < s.fitness[&d(0)]);
    }

    #[test]
    fn rest_raises_form() {
        // property: after a hard block, form rises during rest
        let mut load: Daily = (0..60).map(|i| (d(i), 120.0)).collect();
        load.extend((60..74).map(|i| (d(i), 0.0)));
        let s = compute(&load, d(0), d(73), LoadModel::default());
        for i in 62..74 {
            assert!(s.form[&d(i)] > s.form[&d(i - 1)]);
        }
    }

    #[test]
    fn monotony_by_hand() {
        // loads 0,0,0,0,0,0,70: mean 10, sd sqrt(700) → monotony 10/26.4575 = 0.378
        let load: Daily = [(d(6), 70.0)].into_iter().collect();
        let s = compute(&load, d(0), d(6), LoadModel::default());
        assert!((s.monotony[&d(6)] - 10.0 / 700f64.sqrt()).abs() < 1e-12);
        assert!((s.strain[&d(6)] - 70.0 * 10.0 / 700f64.sqrt()).abs() < 1e-9);
    }
}
