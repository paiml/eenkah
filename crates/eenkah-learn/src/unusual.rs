//! Model 2: unusual days, when several body signals leave the athlete's normal at once.
//!
//! A day is described by each signal's distance from its normal range (model 1), in units of
//! the range's scale. aprender's `IsolationForest` is trained on earlier days only, retrained
//! every week (UNU-1), and scores the new days. A day is unusual when it scores above 97 % of the
//! days the forest was trained on. Days a forest was trained on score a little less unusual
//! than new days, so 97 % against the training days flags about 5 % of new days. `LocalOutlierFactor` scores the same days as a second opinion.
//! Nothing here diagnoses: the finding says "unusual for you" (UNU-2).

use anyhow::{anyhow, Result};
use aprender::cluster::{IsolationForest, LocalOutlierFactor};
use aprender::primitives::Matrix;
use chrono::NaiveDate;
use serde::Serialize;

/// Days of history before the first day is scored.
pub const MIN_TRAIN: usize = 60;
/// Retrain every this many days.
pub const BLOCK: usize = 7;
/// Share of the athlete's own past days a day must score above to be unusual.
pub const FLAG_PCT: f64 = 0.97;
pub const TREES: usize = 100;
pub const LOF_NEIGHBOURS: usize = 20;
/// Distances are capped so one broken reading cannot dominate.
pub const CAP: f64 = 6.0;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct DayScore {
    pub day: NaiveDate,
    /// Share of past days this day is more unusual than (isolation forest).
    pub pct: f64,
    /// The same share from the local outlier factor.
    pub lof_pct: f64,
    pub unusual: bool,
    /// Both models place the day above `FLAG_PCT`.
    pub agree: bool,
}

fn matrix(rows: &[&[f64]]) -> Result<Matrix<f32>> {
    let k = rows.first().map(|r| r.len()).unwrap_or(0);
    let data: Vec<f32> = rows.iter().flat_map(|r| r.iter().map(|v| v.clamp(-CAP, CAP) as f32)).collect();
    Matrix::from_vec(rows.len(), k, data).map_err(|e| anyhow!("matrix: {e}"))
}

/// Share of `past` strictly below `x`.
fn pct(past: &[f32], x: f32) -> f64 {
    past.iter().filter(|p| **p < x).count() as f64 / past.len().max(1) as f64
}

/// Score every day after the first `MIN_TRAIN`, each from a model trained only on earlier days.
/// `rows` are in date order; each row is the day's distances from normal.
pub fn score(rows: &[(NaiveDate, Vec<f64>)], seed: u64) -> Result<Vec<DayScore>> {
    if rows.len() <= MIN_TRAIN {
        return Ok(vec![]);
    }
    use rayon::prelude::*;
    let blocks: Vec<(usize, usize)> = (MIN_TRAIN..rows.len()).step_by(BLOCK).map(|i| (i, (i + BLOCK).min(rows.len()))).collect();
    // blocks are independent and joined in order, so the result equals a serial run (LRN-5)
    let out: Vec<Result<Vec<DayScore>>> = blocks
        .par_iter()
        .map(|(i, j)| {
            let train: Vec<&[f64]> = rows[..*i].iter().map(|r| r.1.as_slice()).collect();
            let test: Vec<&[f64]> = rows[*i..*j].iter().map(|r| r.1.as_slice()).collect();
            let (xt, xs) = (matrix(&train)?, matrix(&test)?);
            let mut forest = IsolationForest::new().with_n_estimators(TREES).with_random_state(seed);
            forest.fit(&xt).map_err(|e| anyhow!("isolation forest: {e}"))?;
            // aprender scores: lower = more anomalous; negate so higher = more unusual
            let past: Vec<f32> = forest.score_samples(&xt).iter().map(|s| -s).collect();
            let now: Vec<f32> = forest.score_samples(&xs).iter().map(|s| -s).collect();
            let mut lof = LocalOutlierFactor::new().with_n_neighbors(LOF_NEIGHBOURS.min(train.len() - 1));
            lof.fit(&xt).map_err(|e| anyhow!("local outlier factor: {e}"))?;
            let lof_past: Vec<f32> = lof.negative_outlier_factor().iter().map(|s| -s).collect();
            let lof_now = lof.score_samples(&xs);
            Ok(rows[*i..*j]
                .iter()
                .enumerate()
                .map(|(k, (day, _))| {
                    let (p, lp) = (pct(&past, now[k]), pct(&lof_past, lof_now[k]));
                    DayScore {
                        day: *day,
                        pct: p,
                        lof_pct: lp,
                        unusual: p >= FLAG_PCT,
                        agree: p >= FLAG_PCT && lp >= FLAG_PCT,
                    }
                })
                .collect())
        })
        .collect();
    let mut all = vec![];
    for r in out {
        all.extend(r?);
    }
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn days(n: usize, seed: u64, odd: &[usize]) -> Vec<(NaiveDate, Vec<f64>)> {
        let mut r = eenkah_testkit::Rng::new(seed);
        let d0 = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        (0..n)
            .map(|i| {
                let mut x: Vec<f64> = (0..6).map(|_| r.n()).collect();
                if odd.contains(&i) {
                    // resting HR up, HRV down, breathing up together
                    x[0] += 4.0;
                    x[1] -= 4.0;
                    x[2] += 4.0;
                }
                (d0 + Duration::days(i as i64), x)
            })
            .collect()
    }

    #[test]
    fn unu1_joint_departures_are_flagged_and_ordinary_days_rarely() {
        let odd = [150, 220, 290];
        let s = score(&days(330, 21, &odd), 7).unwrap();
        for i in odd {
            let d = s.iter().find(|x| x.day == NaiveDate::from_ymd_opt(2025, 1, 1).unwrap() + Duration::days(i as i64)).unwrap();
            assert!(d.unusual && d.agree, "{d:?}");
        }
        let rate = s.iter().filter(|x| x.unusual).count() as f64 / s.len() as f64;
        assert!(rate < 0.10, "flag rate {rate}");
    }

    #[test]
    fn unu1_a_day_is_scored_only_from_earlier_days() {
        let base = days(200, 22, &[]);
        let a = score(&base, 7).unwrap();
        let mut later = base.clone();
        for r in later.iter_mut().skip(140) {
            r.1.iter_mut().for_each(|v| *v += 5.0);
        }
        let b = score(&later, 7).unwrap();
        // days scored by blocks that end before day 140 are identical
        let cut = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap() + Duration::days(137);
        let keep = |v: &[DayScore]| v.iter().filter(|x| x.day < cut).cloned().collect::<Vec<_>>();
        assert_eq!(keep(&a), keep(&b));
    }
}
