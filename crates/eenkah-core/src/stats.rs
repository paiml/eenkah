//! Small, exact statistics helpers. Every function returns `None` instead of a made-up value.

/// Arithmetic mean; `None` for an empty slice.
pub fn mean(v: &[f64]) -> Option<f64> {
    (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
}

/// Sample standard deviation (n − 1); `None` for fewer than two values.
pub fn sd(v: &[f64]) -> Option<f64> {
    if v.len() < 2 {
        return None;
    }
    let m = mean(v)?;
    Some((v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (v.len() - 1) as f64).sqrt())
}

/// Percentile `p` in [0, 1] with linear interpolation between closest ranks.
pub fn percentile(v: &[f64], p: f64) -> Option<f64> {
    if v.is_empty() || !(0.0..=1.0).contains(&p) {
        return None;
    }
    let mut s: Vec<f64> = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let pos = p * (s.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    Some(s[lo] + (s[hi] - s[lo]) * (pos - lo as f64))
}

/// Median.
pub fn median(v: &[f64]) -> Option<f64> {
    percentile(v, 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basics_by_hand() {
        assert_eq!(mean(&[1.0, 2.0, 3.0, 4.0]), Some(2.5));
        assert_eq!(mean(&[]), None);
        // sample sd of 2,4,4,4,5,5,7,9 is sqrt(32/7)
        let s = sd(&[2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]).unwrap();
        assert!((s - (32.0f64 / 7.0).sqrt()).abs() < 1e-12);
        assert_eq!(sd(&[1.0]), None);
        assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0, 5.0], 0.5), Some(3.0));
        assert_eq!(percentile(&[10.0, 20.0], 0.25), Some(12.5));
        assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
    }

    #[test]
    fn percentile_is_monotone_and_bounded() {
        // property: for any data, p10 <= p50 <= p90 and all within [min, max]
        let mut seed = 42u64;
        for _ in 0..200 {
            let n = 1 + (seed % 50) as usize;
            let v: Vec<f64> = (0..n)
                .map(|_| {
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    (seed >> 33) as f64 / 1e6
                })
                .collect();
            let (a, b, c) = (percentile(&v, 0.1).unwrap(), percentile(&v, 0.5).unwrap(), percentile(&v, 0.9).unwrap());
            let lo = v.iter().cloned().fold(f64::INFINITY, f64::min);
            let hi = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            assert!(lo <= a && a <= b && b <= c && c <= hi);
        }
    }
}
