//! Adaptive conformal intervals (Gibbs & Candès 2021) over a stream of past absolute errors.
//!
//! STOPGAP: aprender 0.70.3 has no conformal prediction (docs/DOGFOOD.md, gap G1). The spec
//! says gaps are built in aprender, not worked around here; this module is kept minimal and
//! isolated so it can be replaced by the aprender implementation without touching callers.

use eenkah_core::stats::percentile;

pub struct Aci {
    /// Target miss rate, e.g. 0.2 for an 80 % interval.
    pub alpha: f64,
    /// Step size of the adaptation.
    pub gamma: f64,
    /// How many recent errors size the interval.
    pub window: usize,
    alpha_t: f64,
    errors: Vec<f64>,
}

impl Aci {
    pub fn new(alpha: f64, gamma: f64, window: usize) -> Aci {
        Aci {
            alpha,
            gamma,
            window,
            alpha_t: alpha,
            errors: vec![],
        }
    }

    /// Half-width for the next prediction, or `None` until enough errors are seen.
    pub fn half_width(&self) -> Option<f64> {
        if self.errors.len() < 20 {
            return None;
        }
        let w = &self.errors[self.errors.len().saturating_sub(self.window)..];
        let q = (1.0 - self.alpha_t).clamp(0.0, 1.0);
        percentile(w, q)
    }

    /// Record the outcome of a prediction made with the current half-width.
    pub fn update(&mut self, abs_error: f64) {
        if let Some(h) = self.half_width() {
            let miss = if abs_error > h { 1.0 } else { 0.0 };
            self.alpha_t += self.gamma * (self.alpha - miss);
        }
        self.errors.push(abs_error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_approaches_target_on_shifting_noise() {
        // errors whose scale doubles half-way: adaptive intervals still cover ≈ 80 %
        let mut s = 3u64;
        let mut r = || {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (s >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut aci = Aci::new(0.2, 0.01, 90);
        let (mut hit, mut n) = (0.0, 0.0);
        for i in 0..2000 {
            let scale = if i < 1000 { 1.0 } else { 2.0 };
            let e = scale * r();
            if let Some(h) = aci.half_width() {
                n += 1.0;
                hit += (e <= h) as u8 as f64;
            }
            aci.update(e);
        }
        let cov = hit / n;
        assert!((cov - 0.8).abs() < 0.04, "coverage {cov}");
    }
}
