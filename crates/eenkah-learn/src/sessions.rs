//! Model 6: session types (easy, steady, long, tempo, intervals) for each workout.
//!
//! Sessions of one sport are described by their heart-rate shape: the share of time in each
//! zone, length, load per hour and hard efforts per hour (entries into zone 4 or above).
//! aprender's `KMeans` groups them; the number of groups is the one with the best
//! `silhouette_score`. Each group is named from its average session (SES-1).
//!
//! The athlete can correct any label. A correction always wins for that session. Once there are
//! enough corrections, aprender's `RandomForestClassifier` learns from them and labels the other
//! sessions, but only if it matches the corrections better than the group names do in
//! cross-validation (SES-2).

use anyhow::{anyhow, Result};
use aprender::prelude::{silhouette_score, KMeans, Matrix, UnsupervisedEstimator};
use aprender::tree::RandomForestClassifier;
use serde::Serialize;
use std::collections::BTreeMap;

pub const LABELS: [&str; 5] = ["Easy", "Steady", "Long", "Tempo", "Intervals"];
/// Sessions of a sport needed before it is grouped.
pub const MIN_SESSIONS: usize = 30;
/// Corrections needed before the forest is tried.
pub const MIN_CORRECTIONS: usize = 15;
pub const FOLDS: usize = 5;
/// Heart rate (as a share of threshold) that starts a hard effort, and the level it must fall
/// below before the next one counts.
pub const HARD_ON: f64 = 0.94;
pub const HARD_OFF: f64 = 0.90;
/// Heart-rate smoothing for effort counting, seconds.
pub const SMOOTH_S: f64 = 30.0;

pub const FEATURES: [&str; 8] = [
    "zone 1 share",
    "zone 2 share",
    "zone 3 share",
    "zone 4 share",
    "zone 5 share",
    "hours",
    "load per hour",
    "hard efforts per hour",
];

#[derive(Clone, Debug)]
pub struct Session {
    pub id: i64,
    pub sport: String,
    /// `FEATURES`, in order.
    pub x: [f64; 8],
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Labelled {
    pub id: i64,
    pub label: &'static str,
    /// "you" (a correction), "learned" (the forest) or "groups" (the named group).
    pub source: &'static str,
    pub group: usize,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct SportReport {
    pub sport: String,
    pub sessions: usize,
    pub groups: usize,
    pub silhouette: f64,
    /// Average session of each group, in `FEATURES` order, with its name.
    pub group_names: Vec<&'static str>,
    pub group_means: Vec<Vec<f64>>,
    pub corrections: usize,
    /// Cross-validated agreement with the corrections, when there are enough of them.
    pub groups_accuracy: Option<f64>,
    pub forest_accuracy: Option<f64>,
    pub forest_used: bool,
}

/// Hard efforts per hour from `(seconds, bpm)` samples: entries into ≥ `HARD_ON` × threshold,
/// on a trailing `SMOOTH_S` mean, counted again only after dropping below `HARD_OFF`.
pub fn hard_efforts(samples: &[(f64, f64)], lthr: f64) -> usize {
    let (mut n, mut hard, mut lo) = (0, false, 0usize);
    let mut sum = 0.0;
    for (i, (t, hr)) in samples.iter().enumerate() {
        sum += hr;
        while samples[lo].0 < t - SMOOTH_S {
            sum -= samples[lo].1;
            lo += 1;
        }
        let m = sum / (i + 1 - lo) as f64 / lthr;
        if !hard && m >= HARD_ON {
            hard = true;
            n += 1;
        } else if hard && m < HARD_OFF {
            hard = false;
        }
    }
    n
}

/// The name of a group from its average session: intervals (repeated hard efforts), tempo
/// (much time in zone 3 and above), long, steady (mostly zones 2–3), else easy.
pub fn name(mean: &[f64], median_hours: f64) -> &'static str {
    let hard = mean[3] + mean[4];
    let intensity = mean[2] + hard;
    if mean[7] >= 2.0 && hard >= 0.10 {
        "Intervals"
    } else if intensity >= 0.35 {
        "Tempo"
    } else if mean[5] >= (1.5 * median_hours).max(2.5) {
        "Long"
    } else if mean[1] + mean[2] >= 0.4 {
        "Steady"
    } else {
        "Easy"
    }
}

fn standardise(xs: &[[f64; 8]]) -> Vec<Vec<f64>> {
    let n = xs.len() as f64;
    let mean: Vec<f64> = (0..8).map(|j| xs.iter().map(|x| x[j]).sum::<f64>() / n).collect();
    let sd: Vec<f64> = (0..8)
        .map(|j| (xs.iter().map(|x| (x[j] - mean[j]).powi(2)).sum::<f64>() / n).sqrt())
        .map(|s| if s > 1e-9 { s } else { 1.0 })
        .collect();
    xs.iter().map(|x| (0..8).map(|j| (x[j] - mean[j]) / sd[j]).collect()).collect()
}

fn matrix(rows: &[Vec<f64>]) -> Result<Matrix<f32>> {
    let data: Vec<f32> = rows.iter().flat_map(|r| r.iter().map(|v| *v as f32)).collect();
    Matrix::from_vec(rows.len(), rows.first().map(|r| r.len()).unwrap_or(0), data).map_err(|e| anyhow!("matrix: {e}"))
}

fn label_index(l: &str) -> Option<usize> {
    LABELS.iter().position(|x| *x == l)
}

/// Fold of the `i`-th correction: fixed, so cross-validation is deterministic.
fn fold(i: usize) -> usize {
    i % FOLDS
}

/// Label the sessions of one sport. `corrections` maps session id to the athlete's label.
pub fn classify(sport: &str, sessions: &[Session], corrections: &BTreeMap<i64, String>, seed: u64) -> Result<(Vec<Labelled>, SportReport)> {
    let mut rep = SportReport {
        sport: sport.to_string(),
        sessions: sessions.len(),
        ..Default::default()
    };
    if sessions.len() < MIN_SESSIONS {
        return Ok((vec![], rep));
    }
    let raw: Vec<[f64; 8]> = sessions.iter().map(|s| s.x).collect();
    let z = standardise(&raw);
    let x = matrix(&z)?;
    let mut best: Option<(f32, usize, Vec<usize>)> = None;
    for k in 2..=5usize {
        if sessions.len() < 10 * k {
            break;
        }
        let mut km = KMeans::new(k).with_random_state(seed);
        km.fit(&x).map_err(|e| anyhow!("k-means: {e}"))?;
        let labels = km.predict(&x);
        let s = silhouette_score(&x, &labels);
        if best.as_ref().is_none_or(|b| s > b.0) {
            best = Some((s, k, labels));
        }
    }
    let (sil, k, groups) = best.ok_or_else(|| anyhow!("no grouping"))?;
    let mut hours: Vec<f64> = raw.iter().map(|x| x[5]).collect();
    hours.sort_by(f64::total_cmp);
    let median_hours = hours[hours.len() / 2];
    let means: Vec<Vec<f64>> = (0..k)
        .map(|g| {
            let m: Vec<&[f64; 8]> = raw.iter().zip(&groups).filter(|(_, l)| **l == g).map(|(x, _)| x).collect();
            (0..8).map(|j| m.iter().map(|x| x[j]).sum::<f64>() / m.len().max(1) as f64).collect()
        })
        .collect();
    let names: Vec<&'static str> = means.iter().map(|m| name(m, median_hours)).collect();
    rep.groups = k;
    rep.silhouette = sil as f64;
    rep.group_names = names.clone();
    rep.group_means = means;

    // corrections on these sessions, in session order
    let corrected: Vec<(usize, usize)> = sessions.iter().enumerate().filter_map(|(i, s)| Some((i, label_index(corrections.get(&s.id)?)?))).collect();
    rep.corrections = corrected.len();
    let classes = corrected.iter().map(|c| c.1).collect::<std::collections::BTreeSet<_>>().len();
    let mut forest_pred: Option<Vec<usize>> = None;
    if corrected.len() >= MIN_CORRECTIONS && classes >= 2 {
        let fit = |rows: &[(usize, usize)]| -> Result<RandomForestClassifier> {
            let mut f = RandomForestClassifier::new(50).with_max_depth(4).with_random_state(seed);
            f.fit(&matrix(&rows.iter().map(|(i, _)| z[*i].clone()).collect::<Vec<_>>())?, &rows.iter().map(|c| c.1).collect::<Vec<_>>())
                .map_err(|e| anyhow!("random forest: {e}"))?;
            Ok(f)
        };
        let (mut hit_f, mut hit_g) = (0usize, 0usize);
        for f in 0..FOLDS {
            let train: Vec<(usize, usize)> = corrected.iter().enumerate().filter(|(j, _)| fold(*j) != f).map(|(_, c)| *c).collect();
            let test: Vec<(usize, usize)> = corrected.iter().enumerate().filter(|(j, _)| fold(*j) == f).map(|(_, c)| *c).collect();
            if test.is_empty() || train.iter().map(|c| c.1).collect::<std::collections::BTreeSet<_>>().len() < 2 {
                continue;
            }
            let m = fit(&train)?;
            let p = m.predict(&matrix(&test.iter().map(|(i, _)| z[*i].clone()).collect::<Vec<_>>())?);
            hit_f += p.iter().zip(&test).filter(|(p, c)| **p == c.1).count();
            hit_g += test.iter().filter(|(i, c)| label_index(names[groups[*i]]) == Some(*c)).count();
        }
        let n = corrected.len() as f64;
        rep.forest_accuracy = Some(hit_f as f64 / n);
        rep.groups_accuracy = Some(hit_g as f64 / n);
        if hit_f > hit_g {
            rep.forest_used = true;
            forest_pred = Some(fit(&corrected)?.predict(&x));
        }
    }
    let out = sessions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let (label, source) = match (corrections.get(&s.id).and_then(|l| label_index(l)), &forest_pred) {
                (Some(c), _) => (LABELS[c], "you"),
                (None, Some(p)) => (LABELS[p[i].min(LABELS.len() - 1)], "learned"),
                (None, None) => (names[groups[i]], "groups"),
            };
            Labelled {
                id: s.id,
                label,
                source,
                group: groups[i],
            }
        })
        .collect();
    Ok((out, rep))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three kinds of synthetic ride with known shapes.
    fn rides(n: usize, seed: u64) -> (Vec<Session>, Vec<&'static str>) {
        let mut r = eenkah_testkit::Rng::new(seed);
        let mut out = vec![];
        let mut truth = vec![];
        for i in 0..n {
            let j = |r: &mut eenkah_testkit::Rng| 0.02 * r.n();
            let (x, t) = match i % 3 {
                0 => ([0.70 + j(&mut r), 0.25, 0.04, 0.01, 0.0, 1.2 + 0.1 * r.n(), 45.0 + 3.0 * r.n(), 0.2], "Easy"),
                1 => ([0.15, 0.30, 0.15, 0.25 + j(&mut r), 0.15, 1.1 + 0.1 * r.n(), 95.0 + 5.0 * r.n(), 6.0 + r.n()], "Intervals"),
                _ => ([0.35, 0.55 + j(&mut r), 0.08, 0.02, 0.0, 4.0 + 0.3 * r.n(), 50.0 + 3.0 * r.n(), 0.3], "Long"),
            };
            out.push(Session {
                id: i as i64,
                sport: "cycling".into(),
                x,
            });
            truth.push(t);
        }
        (out, truth)
    }

    #[test]
    fn ses1_groups_recover_known_session_types() {
        let (s, truth) = rides(90, 31);
        let (lab, rep) = classify("cycling", &s, &BTreeMap::new(), 7).unwrap();
        assert_eq!(rep.groups, 3, "{rep:?}");
        let hit = lab.iter().zip(&truth).filter(|(l, t)| l.label == **t).count();
        assert!(hit >= 85, "{hit}/90 {:?}", rep.group_names);
        assert!(lab.iter().all(|l| l.source == "groups"));
    }

    #[test]
    fn ses1_too_few_sessions_are_not_grouped() {
        let (s, _) = rides(MIN_SESSIONS - 1, 32);
        assert!(classify("cycling", &s, &BTreeMap::new(), 7).unwrap().0.is_empty());
    }

    #[test]
    fn ses2_corrections_win_and_the_forest_is_used_only_when_it_matches_them_better() {
        let (s, truth) = rides(90, 33);
        // the athlete calls their interval sessions "Tempo": group names cannot know that
        let corr: BTreeMap<i64, String> = (0..30).map(|i| (i as i64, if truth[i] == "Intervals" { "Tempo" } else { truth[i] }.to_string())).collect();
        let (lab, rep) = classify("cycling", &s, &corr, 7).unwrap();
        assert!(lab.iter().take(30).all(|l| l.source == "you"));
        assert!(rep.forest_used && rep.forest_accuracy > rep.groups_accuracy, "{rep:?}");
        // uncorrected interval sessions now follow the athlete's word
        let later: Vec<&Labelled> = lab.iter().skip(30).filter(|l| truth[l.id as usize] == "Intervals").collect();
        assert!(later.iter().all(|l| l.label == "Tempo" && l.source == "learned"), "{later:?}");

        // corrections that agree with the group names: the forest is not needed
        let agree: BTreeMap<i64, String> = (0..30).map(|i| (i as i64, truth[i].to_string())).collect();
        let (_, rep) = classify("cycling", &s, &agree, 7).unwrap();
        assert!(!rep.forest_used, "{rep:?}");
    }

    #[test]
    fn hard_efforts_count_entries_not_seconds() {
        // 3 blocks of 3 min at 100 % threshold separated by 4 min at 75 %
        let mut s = vec![];
        let mut t = 0.0;
        for _ in 0..3 {
            for _ in 0..240 {
                s.push((t, 120.0));
                t += 1.0;
            }
            for _ in 0..180 {
                s.push((t, 160.0));
                t += 1.0;
            }
        }
        assert_eq!(hard_efforts(&s, 160.0), 3);
    }
}
