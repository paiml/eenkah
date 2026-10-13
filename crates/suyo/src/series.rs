//! In-memory series produced by the evaluator.

use crate::store::{Agg, AggFn};
use crate::timeutil::{Bucket, Zone};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Style {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub dashed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    /// 1 = highlighted, -1 = de-emphasised (drawn in gray), 0 = normal.
    #[serde(skip_serializing_if = "is_zero")]
    pub emphasis: i8,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub stack: bool,
    /// "threshold" for reference lines (goals, limits).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

fn is_zero(v: &i8) -> bool {
    *v == 0
}

#[derive(Clone, Debug)]
pub struct Series {
    pub name: String,
    /// Underlying metric path, used by aliasByNode and friends.
    pub path: String,
    pub ts: Vec<i64>,
    pub vals: Vec<Option<f64>>,
    /// Raw per-bucket aggregates, kept until a transform changes values so
    /// later re-bucketing (summarize, consolidateBy) stays exact.
    pub stats: Option<Vec<Option<Agg>>>,
    pub bucket: Bucket,
    pub unit: String,
    pub agg: AggFn,
    pub style: Style,
}

impl Series {
    pub fn new(
        name: impl Into<String>,
        ts: Vec<i64>,
        vals: Vec<Option<f64>>,
        bucket: Bucket,
    ) -> Series {
        let name = name.into();
        Series {
            path: name.clone(),
            name,
            ts,
            vals,
            stats: None,
            bucket,
            unit: String::new(),
            agg: AggFn::Avg,
            style: Style::default(),
        }
    }

    /// Copy with new values; drops the raw stats since they no longer apply.
    pub fn with_vals(&self, name: String, vals: Vec<Option<f64>>) -> Series {
        Series {
            name,
            vals,
            stats: None,
            ..self.clone()
        }
    }

    pub fn map(&self, name: String, f: impl Fn(f64) -> Option<f64>) -> Series {
        let vals = self
            .vals
            .iter()
            .map(|v| v.and_then(&f).filter(|x| x.is_finite()))
            .collect();
        self.with_vals(name, vals)
    }

    pub fn non_null(&self) -> impl Iterator<Item = f64> + '_ {
        self.vals.iter().flatten().copied()
    }

    pub fn avg(&self) -> Option<f64> {
        let (s, n) = self
            .non_null()
            .fold((0.0, 0usize), |(s, n), v| (s + v, n + 1));
        (n > 0).then(|| s / n as f64)
    }
    pub fn sum(&self) -> Option<f64> {
        let mut it = self.non_null().peekable();
        it.peek()?;
        Some(it.sum())
    }
    pub fn max(&self) -> Option<f64> {
        self.non_null().reduce(f64::max)
    }
    pub fn min(&self) -> Option<f64> {
        self.non_null().reduce(f64::min)
    }
    pub fn last(&self) -> Option<f64> {
        self.vals.iter().rev().flatten().next().copied()
    }
    pub fn first(&self) -> Option<f64> {
        self.vals.iter().flatten().next().copied()
    }

    /// Re-bucket into `bucket` with `f`. Uses raw stats when available.
    pub fn rebucket(&self, bucket: Bucket, f: AggFn, zone: Zone, from: i64, until: i64) -> Series {
        let lo = self.ts.first().copied().unwrap_or(from).min(from);
        let hi = self.ts.last().map(|t| t + 1).unwrap_or(until).max(until);
        let starts = bucket.range(lo, hi, zone);
        let mut aggs: Vec<Option<Agg>> = vec![None; starts.len()];
        let mut idx = 0usize;
        for (i, t) in self.ts.iter().enumerate() {
            if starts.is_empty() {
                break;
            }
            while idx + 1 < starts.len() && starts[idx + 1] <= *t {
                idx += 1;
            }
            if *t < starts[idx] {
                continue;
            }
            let a = match &self.stats {
                Some(st) => st[i],
                None => self.vals[i].map(Agg::one),
            };
            if let Some(a) = a {
                match &mut aggs[idx] {
                    Some(e) => e.merge(&a),
                    slot => *slot = Some(a),
                }
            }
        }
        let vals = aggs.iter().map(|a| a.map(|a| a.value(f))).collect();
        Series {
            ts: starts,
            vals,
            stats: Some(aggs),
            bucket,
            agg: f,
            ..self.clone()
        }
    }

    /// Drop points before `from`/after `until` (used after prefetching).
    pub fn trim(&mut self, from: i64, until: i64) {
        let keep: Vec<bool> = self.ts.iter().map(|t| *t >= from && *t < until).collect();
        let mut i = 0;
        self.ts.retain(|_| {
            i += 1;
            keep[i - 1]
        });
        let mut i = 0;
        self.vals.retain(|_| {
            i += 1;
            keep[i - 1]
        });
        if let Some(st) = &mut self.stats {
            let mut i = 0;
            st.retain(|_| {
                i += 1;
                keep[i - 1]
            });
        }
    }
}

/// Bring several series onto one time grid: re-bucket everything to the
/// coarsest bucket present, then align on the union of timestamps.
pub fn align(
    list: &[Series],
    zone: Zone,
    from: i64,
    until: i64,
) -> (Vec<i64>, Vec<Vec<Option<f64>>>, Bucket) {
    let Some(coarsest) = list.iter().map(|s| s.bucket).max_by_key(|b| b.nominal()) else {
        return (vec![], vec![], Bucket::secs(60));
    };
    let regridded: Vec<Series> = list
        .iter()
        .map(|s| {
            if s.bucket == coarsest {
                s.clone()
            } else {
                s.rebucket(coarsest, s.agg, zone, from, until)
            }
        })
        .collect();
    let all: BTreeSet<i64> = regridded
        .iter()
        .flat_map(|s| s.ts.iter().copied())
        .collect();
    let ts: Vec<i64> = all.into_iter().collect();
    let pos: HashMap<i64, usize> = ts.iter().enumerate().map(|(i, t)| (*t, i)).collect();
    let cols = regridded
        .iter()
        .map(|s| {
            let mut col = vec![None; ts.len()];
            for (t, v) in s.ts.iter().zip(&s.vals) {
                col[pos[t]] = *v;
            }
            col
        })
        .collect();
    (ts, cols, coarsest)
}
