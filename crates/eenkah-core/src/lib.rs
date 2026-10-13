//! eenkah-core: pure functions only. No files, no database, no network.
//!
//! A daily series is a [`Daily`]: calendar day → value. A day that is absent has no value
//! (MET-1): functions never fill gaps with zero or interpolate.

pub mod catalog;
pub mod features;
pub mod findings;
pub mod load;
pub mod metrics;
pub mod stats;
pub mod when;

use chrono::NaiveDate;
use std::collections::BTreeMap;

/// Calendar day → value. Absent days have no value.
pub type Daily = BTreeMap<NaiveDate, f64>;

/// Values of `s` on the days in `[from, to]` inclusive, in date order.
pub fn window(s: &Daily, from: NaiveDate, to: NaiveDate) -> Vec<f64> {
    if from > to {
        return vec![];
    }
    s.range(from..=to).map(|(_, v)| *v).collect()
}

/// Every day from `a` to `b` inclusive.
pub fn days(a: NaiveDate, b: NaiveDate) -> impl Iterator<Item = NaiveDate> {
    a.iter_days().take_while(move |d| *d <= b)
}
