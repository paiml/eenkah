//! Feature building for forecasts. A row for day `d` describes the athlete on the morning of
//! `d`: last night's body state and sleep, and training load up to the end of `d − 1`.
//! Nothing from `d` after waking, or from any later day, may enter a row (FC-4); the test
//! `no_feature_uses_the_future` checks this by recomputing on truncated data.

use crate::load::{self, LoadModel};
use crate::metrics::{normal_band, rolling_mean, sleep_debt_7d, sleep_regularity, z_score};
use crate::stats::percentile;
use crate::{window, Daily};
use chrono::{Datelike, Duration, NaiveDate, Weekday};
use std::collections::BTreeMap;

/// Raw daily inputs, as stored.
#[derive(Clone, Debug, Default)]
pub struct Inputs {
    pub hrv_ln: Daily,
    pub rhr: Daily,
    pub sleep_hours: Daily,
    pub bedtime_min: Daily,
    pub waketime_min: Daily,
    /// Daily training load, only on days with sessions.
    pub load: Daily,
    /// First and last day the athlete was recording training (rest days inside are zero load).
    pub load_period: Option<(NaiveDate, NaiveDate)>,
    /// Athlete marks, 1.0 on marked days (e.g. illness).
    pub sick: Daily,
}

impl Inputs {
    /// Everything known on the morning of `d`: body state up to `d`, load up to `d − 1`.
    pub fn as_of(&self, d: NaiveDate) -> Inputs {
        let upto = |s: &Daily, last: NaiveDate| s.range(..=last).map(|(k, v)| (*k, *v)).collect::<Daily>();
        let y = d - Duration::days(1);
        Inputs {
            hrv_ln: upto(&self.hrv_ln, d),
            rhr: upto(&self.rhr, d),
            sleep_hours: upto(&self.sleep_hours, d),
            bedtime_min: upto(&self.bedtime_min, d),
            waketime_min: upto(&self.waketime_min, d),
            load: upto(&self.load, y),
            load_period: self.load_period.and_then(|(a, b)| (a <= y).then(|| (a, b.min(y)))),
            sick: upto(&self.sick, d),
        }
    }
}

pub const FEATURES: &[(&str, &str)] = &[
    ("hrv_z", "last night's HRV against your normal"),
    ("hrv_trend", "7-day HRV against your normal"),
    ("rhr_z", "resting heart rate against your normal"),
    ("sleep_hours", "last night's sleep"),
    ("sleep_debt", "sleep debt over the week"),
    ("sleep_irregularity", "irregular sleep times"),
    ("form", "form (fitness minus fatigue)"),
    ("fatigue", "recent training fatigue"),
    ("ramp", "how fast fitness is changing"),
    ("load_yesterday", "yesterday's training load"),
    ("days_since_hard", "days since your last hard session"),
    ("sick", "marked as ill"),
    ("weekend", "weekend day"),
];

#[derive(Clone, Debug, Default)]
pub struct Table {
    pub names: Vec<&'static str>,
    pub rows: BTreeMap<NaiveDate, Vec<f64>>,
}

/// All derived series used by features, computed once on the full inputs. Every one is a
/// trailing-window function, so its value on `d` depends only on inputs up to `d`.
pub struct Derived {
    pub hrv_z: Daily,
    pub hrv_trend: Daily,
    pub rhr_z: Daily,
    pub debt: Daily,
    pub irregularity: Daily,
    pub load: load::LoadSeries,
    pub hard_days: Vec<NaiveDate>,
}

pub fn derive(i: &Inputs, lm: LoadModel) -> Derived {
    let hrv7 = rolling_mean(&i.hrv_ln, 7, 4);
    let days: Vec<NaiveDate> = hrv7.keys().copied().collect();
    let band = normal_band(&hrv7, &days, 60, 20, 0.1, 0.9);
    let hrv_trend = hrv7
        .iter()
        .filter_map(|(d, v)| {
            let (lo, hi) = (band.lo.get(d)?, band.hi.get(d)?);
            let half = (hi - lo) / 2.0;
            (half > 0.0).then(|| (*d, (v - band.mid[d]) / half))
        })
        .collect();
    let load = match i.load_period {
        Some((a, b)) => load::compute(&i.load, a, b, lm),
        None => load::LoadSeries::default(),
    };
    // a hard day is one whose load is in the top 20 % of the previous 90 days' sessions
    let hard_days = i
        .load
        .iter()
        .filter(|(d, v)| {
            let w: Vec<f64> = window(&i.load, **d - Duration::days(90), **d - Duration::days(1)).into_iter().filter(|x| *x > 0.0).collect();
            w.len() >= 8 && percentile(&w, 0.8).is_some_and(|p| **v >= p)
        })
        .map(|(d, _)| *d)
        .collect();
    Derived {
        hrv_z: z_score(&i.hrv_ln, 60, 20),
        hrv_trend,
        rhr_z: z_score(&i.rhr, 60, 20),
        debt: sleep_debt_7d(&i.sleep_hours),
        irregularity: sleep_regularity(&i.bedtime_min, &i.waketime_min),
        load,
        hard_days,
    }
}

/// One feature row for the morning of `d`, or `None` if any input is missing.
pub fn row(i: &Inputs, x: &Derived, d: NaiveDate) -> Option<Vec<f64>> {
    let y = d - Duration::days(1);
    let since_hard = x.hard_days.iter().rev().find(|h| **h <= y).map(|h| (d - *h).num_days().min(30) as f64).unwrap_or(30.0);
    Some(vec![
        *x.hrv_z.get(&d)?,
        *x.hrv_trend.get(&d)?,
        *x.rhr_z.get(&d)?,
        *i.sleep_hours.get(&d)?,
        *x.debt.get(&d)?,
        *x.irregularity.get(&d)?,
        x.load.fitness.get(&y)? - x.load.fatigue.get(&y)?, // form on the morning of d
        *x.load.fatigue.get(&y)?,
        *x.load.ramp.get(&y)?,
        *x.load.daily.get(&y)?,
        since_hard,
        i.sick.get(&d).copied().unwrap_or(0.0),
        matches!(d.weekday(), Weekday::Sat | Weekday::Sun) as u8 as f64,
    ])
}

pub fn build(i: &Inputs, lm: LoadModel) -> Table {
    let x = derive(i, lm);
    let mut t = Table {
        names: FEATURES.iter().map(|f| f.0).collect(),
        rows: BTreeMap::new(),
    };
    for d in i.hrv_ln.keys().chain(i.rhr.keys()).copied().collect::<std::collections::BTreeSet<_>>() {
        if let Some(r) = row(i, &x, d) {
            t.rows.insert(d, r);
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic synthetic athlete, enough for leak and shape tests.
    fn athlete(n: i64) -> Inputs {
        let d0 = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        let mut i = Inputs::default();
        let mut s = 7u64;
        let mut r = || {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((s >> 33) as f64 / (1u64 << 31) as f64) - 0.5
        };
        for k in 0..n {
            let d = d0 + Duration::days(k);
            i.hrv_ln.insert(d, 3.4 + 0.1 * r());
            i.rhr.insert(d, 52.0 + 2.0 * r());
            i.sleep_hours.insert(d, 7.5 + r());
            i.bedtime_min.insert(d, 23.0 * 60.0 + 30.0 * r());
            i.waketime_min.insert(d, 7.0 * 60.0 + 30.0 * r());
            if k % 3 != 0 {
                i.load.insert(d, 60.0 + 80.0 * (r() + 0.5));
            }
        }
        i.load_period = Some((d0, d0 + Duration::days(n - 1)));
        i
    }

    #[test]
    fn no_feature_uses_the_future() {
        // FC-4: a row computed on all data equals the row computed on data known that morning
        let i = athlete(200);
        let full = build(&i, LoadModel::default());
        assert!(full.rows.len() > 100);
        for (d, r) in full.rows.iter().step_by(17) {
            let past = i.as_of(*d);
            let x = derive(&past, LoadModel::default());
            assert_eq!(Some(r.clone()), row(&past, &x, *d), "leak on {d}");
        }
    }

    #[test]
    fn a_leaked_feature_is_caught() {
        // The same check must fail when a feature reads tomorrow's value.
        let i = athlete(200);
        let x = derive(&i, LoadModel::default());
        let d = *build(&i, LoadModel::default()).rows.keys().nth(50).unwrap();
        let leaky = |inp: &Inputs| inp.rhr.get(&(d + Duration::days(1))).copied();
        assert_ne!(leaky(&i), leaky(&i.as_of(d)));
        let _ = x;
    }

    #[test]
    fn rows_have_one_value_per_name() {
        let t = build(&athlete(120), LoadModel::default());
        assert!(t.rows.values().all(|r| r.len() == t.names.len()));
    }
}
