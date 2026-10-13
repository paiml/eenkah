//! Forecasting families. Layer 1 are personal baselines (no training); layer 2 are aprender
//! models trained on the athlete's own days, tried simplest first (LRN-6).

use anyhow::{anyhow, Result};
use aprender::prelude::{Estimator, Matrix, RandomForestRegressor, Ridge, Vector};
use serde::{Deserialize, Serialize};

/// One training/prediction row: features on the morning of `d`, plus the reference values the
/// baselines and residual models need.
#[derive(Clone, Debug)]
pub struct Row {
    pub x: Vec<f64>,
    /// Target value on the morning of `d + 1`.
    pub y: f64,
    /// Value on `d` (persistence baseline).
    pub last: f64,
    /// Mean of the last 7 days up to `d`.
    pub mean7: f64,
    /// Mean of the last 60 days up to `d`.
    pub mean60: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum Family {
    /// Tomorrow = today.
    Persistence,
    /// Tomorrow = mean of the last 7 days.
    Mean7,
    /// Tomorrow = mean of the last 60 days.
    Mean60,
    /// aprender Ridge on standardised features, predicting the residual from the 7-day mean.
    Ridge,
    /// aprender random forest on the same residual.
    Forest,
}

impl Family {
    pub const ALL: [Family; 5] = [Family::Persistence, Family::Mean7, Family::Mean60, Family::Ridge, Family::Forest];
    pub fn is_baseline(self) -> bool {
        matches!(self, Family::Persistence | Family::Mean7 | Family::Mean60)
    }
    pub fn label(self) -> &'static str {
        match self {
            Family::Persistence => "same as today",
            Family::Mean7 => "your 7-day average",
            Family::Mean60 => "your 60-day average",
            Family::Ridge => "linear model (aprender Ridge)",
            Family::Forest => "random forest (aprender)",
        }
    }
}

/// A fitted model, as saved in the `.apr` file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fitted {
    pub family: Family,
    pub feature_names: Vec<String>,
    /// Standardisation fitted on training rows only.
    pub mean: Vec<f64>,
    pub scale: Vec<f64>,
    pub ridge: Option<Ridge>,
    pub forest: Option<RandomForestRegressor>,
}

pub const RIDGE_ALPHA: f32 = 3.0;
pub const FOREST_TREES: usize = 60;
pub const FOREST_DEPTH: usize = 4;

#[allow(clippy::needless_range_loop)]
fn standardise(rows: &[Row]) -> (Vec<f64>, Vec<f64>) {
    let k = rows[0].x.len();
    let n = rows.len() as f64;
    let mut mean = vec![0.0; k];
    let mut scale = vec![0.0; k];
    for r in rows {
        for j in 0..k {
            mean[j] += r.x[j] / n;
        }
    }
    for r in rows {
        for j in 0..k {
            scale[j] += (r.x[j] - mean[j]).powi(2) / n;
        }
    }
    for s in scale.iter_mut() {
        *s = if *s > 1e-12 { s.sqrt() } else { 1.0 };
    }
    (mean, scale)
}

impl Fitted {
    fn matrix(&self, rows: &[&[f64]]) -> Result<Matrix<f32>> {
        let k = self.mean.len();
        let data: Vec<f32> = rows.iter().flat_map(|x| (0..k).map(move |j| ((x[j] - self.mean[j]) / self.scale[j]) as f32)).collect();
        Matrix::from_vec(rows.len(), k, data).map_err(|e| anyhow!("matrix: {e}"))
    }

    /// Fit `family` on `rows`. Baselines need no fitting.
    pub fn fit(family: Family, rows: &[Row], names: &[&str], seed: u64) -> Result<Fitted> {
        let (mean, scale) = if rows.is_empty() { (vec![], vec![]) } else { standardise(rows) };
        let mut f = Fitted {
            family,
            feature_names: names.iter().map(|s| s.to_string()).collect(),
            mean,
            scale,
            ridge: None,
            forest: None,
        };
        if family.is_baseline() {
            return Ok(f);
        }
        let xs: Vec<&[f64]> = rows.iter().map(|r| r.x.as_slice()).collect();
        let x = f.matrix(&xs)?;
        let y = Vector::from_vec(rows.iter().map(|r| (r.y - r.mean7) as f32).collect());
        match family {
            Family::Ridge => {
                let mut m = Ridge::new(RIDGE_ALPHA);
                m.fit(&x, &y).map_err(|e| anyhow!("ridge fit: {e}"))?;
                f.ridge = Some(m);
            }
            Family::Forest => {
                let mut m = RandomForestRegressor::new(FOREST_TREES).with_max_depth(FOREST_DEPTH).with_random_state(seed);
                m.fit(&x, &y).map_err(|e| anyhow!("forest fit: {e}"))?;
                f.forest = Some(m);
            }
            _ => unreachable!(),
        }
        Ok(f)
    }

    pub fn predict(&self, rows: &[Row]) -> Result<Vec<f64>> {
        match self.family {
            Family::Persistence => return Ok(rows.iter().map(|r| r.last).collect()),
            Family::Mean7 => return Ok(rows.iter().map(|r| r.mean7).collect()),
            Family::Mean60 => return Ok(rows.iter().map(|r| r.mean60).collect()),
            _ => {}
        }
        if rows.is_empty() {
            return Ok(vec![]);
        }
        let xs: Vec<&[f64]> = rows.iter().map(|r| r.x.as_slice()).collect();
        let x = self.matrix(&xs)?;
        let resid = match (&self.ridge, &self.forest) {
            (Some(m), _) => m.predict(&x),
            (_, Some(m)) => m.predict(&x),
            _ => return Err(anyhow!("model not fitted")),
        };
        Ok(rows.iter().zip(resid.as_slice()).map(|(r, e)| r.mean7 + *e as f64).collect())
    }

    /// Per-feature contribution to one prediction (linear models only): coefficient × standardised value.
    pub fn contributions(&self, x: &[f64]) -> Vec<(String, f64)> {
        let Some(m) = &self.ridge else { return vec![] };
        let c = m.coefficients();
        let mut out: Vec<(String, f64)> = (0..self.mean.len())
            .map(|j| (self.feature_names[j].clone(), c.as_slice()[j] as f64 * (x[j] - self.mean[j]) / self.scale[j]))
            .collect();
        out.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()));
        out
    }
}
