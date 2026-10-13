//! eenkah-ingest: Garmin files → typed records.
//!
//! [`classify`] names a file's kind from its name (cheap), [`parse`] turns its bytes into
//! [`Rec`]s. Nothing here touches the database or the network. Anything not understood is
//! counted in [`Parsed::unknown`] so the importer can report it (ING-3).

pub mod fit;
pub mod json;

use anyhow::Result;
use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone, Utc};
use serde::Serialize;
use std::collections::BTreeMap;

/// Bump when parsing changes, so `eenkah import` knows to re-parse archived files.
pub const PARSER_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub enum Kind {
    SleepJson,
    RhrJson,
    WeightJson,
    /// `/hrv-service/hrv/daily/{start}/{end}`: overnight HRV, baseline band, status.
    HrvJson,
    DailySummaryJson,
    ActivityJson,
    /// Any FIT file; the content decides what it holds.
    Fit,
    /// Known Garmin files eenkah does not read yet (named so the report can say so).
    NotYetSupported(&'static str),
    Unknown,
}

impl Kind {
    pub fn name(&self) -> String {
        match self {
            Kind::NotYetSupported(n) => format!("not yet supported: {n}"),
            k => format!("{k:?}"),
        }
    }
    pub fn supported(&self) -> bool {
        !matches!(self, Kind::NotYetSupported(_) | Kind::Unknown)
    }
}

/// Decide a file's kind from its file name. FIT contents are sniffed later in [`parse`].
pub fn classify(file_name: &str) -> Kind {
    let n = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    let lower = n.to_ascii_lowercase();
    if lower.ends_with(".fit") {
        for (suffix, label) in [
            ("_wellness.fit", "WELLNESS (per-minute monitoring)"),
            ("_metrics.fit", "METRICS (undocumented messages)"),
            ("_nap.fit", "NAP (undocumented; naps come from sleep JSON)"),
            ("_sleep_disruptions.fit", "SLEEP_DISRUPTIONS (undocumented)"),
        ] {
            if lower.ends_with(suffix) {
                return Kind::NotYetSupported(label);
            }
        }
        return Kind::Fit;
    }
    if !lower.ends_with(".json") {
        return Kind::Unknown;
    }
    if lower.starts_with("sleep_") {
        Kind::SleepJson
    } else if lower.starts_with("rhr_") {
        Kind::RhrJson
    } else if lower.starts_with("weight_") {
        Kind::WeightJson
    } else if lower.starts_with("hrv_") {
        Kind::HrvJson
    } else if lower.starts_with("daily_summary_") {
        Kind::DailySummaryJson
    } else if lower.starts_with("activity_details_") {
        Kind::NotYetSupported("activity details JSON (summary comes from the FIT file)")
    } else if lower.starts_with("activity_") && lower != "activity_types.json" {
        Kind::ActivityJson
    } else if lower.starts_with("hydration_") {
        Kind::NotYetSupported("hydration (not tracked)")
    } else {
        Kind::Unknown
    }
}

/// One activity session. Fields are optional because the JSON list entry and the FIT file
/// each carry a subset; the store merges them by `id`.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Activity {
    pub id: i64,
    pub start_utc: i64,
    /// Local time minus UTC, in seconds, at the start.
    pub utc_offset_s: Option<i32>,
    pub sport: Option<String>,
    pub sub_sport: Option<String>,
    /// Garmin Connect activity type key, e.g. "mountain_biking" (from the JSON list entry).
    pub type_key: Option<String>,
    pub name: Option<String>,
    pub duration_s: Option<f64>,
    pub moving_s: Option<f64>,
    pub distance_m: Option<f64>,
    pub ascent_m: Option<f64>,
    pub avg_hr: Option<f64>,
    pub max_hr: Option<f64>,
    pub calories: Option<f64>,
    pub device_load: Option<f64>,
    /// Threshold heart rate the device used for this session (zones_target), if any.
    pub lthr: Option<f64>,
    pub max_hr_setting: Option<f64>,
    pub rest_hr_setting: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq)]
pub struct Sample {
    pub t: i64,
    pub hr: Option<f32>,
    /// m/s
    pub speed: Option<f32>,
    pub power: Option<f32>,
    pub cadence: Option<f32>,
    pub altitude: Option<f32>,
    pub distance: Option<f32>,
    pub temp: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum Rec {
    /// A daily value. `prio`: lower wins when two sources give the same metric for a day.
    Day {
        day: NaiveDate,
        metric: &'static str,
        value: f64,
        prio: u8,
    },
    DayText {
        day: NaiveDate,
        metric: &'static str,
        text: String,
        prio: u8,
    },
    Activity(Box<Activity>),
    Samples {
        activity_id: i64,
        samples: Vec<Sample>,
    },
}

#[derive(Debug, Default)]
pub struct Parsed {
    pub recs: Vec<Rec>,
    /// What the file turned out to hold (for FIT files, decided by content).
    pub content: String,
    /// Message or field names seen but not used, with counts.
    pub unknown: BTreeMap<String, usize>,
}

/// Context the parsers need: the athlete's time zone, used only when a file carries no
/// offset of its own.
#[derive(Clone, Copy, Debug)]
pub struct Ctx {
    pub tz: chrono_tz::Tz,
}

impl Ctx {
    pub fn offset_at(&self, utc: i64) -> i32 {
        let t = Utc.timestamp_opt(utc, 0).single().unwrap_or_default();
        let local: DateTime<chrono_tz::Tz> = t.with_timezone(&self.tz);
        use chrono::Offset;
        local.offset().fix().local_minus_utc()
    }
}

/// Local calendar day of a UTC instant with a known offset.
pub fn local_day(utc: i64, offset_s: i32) -> NaiveDate {
    let off = FixedOffset::east_opt(offset_s).unwrap_or(FixedOffset::east_opt(0).unwrap());
    Utc.timestamp_opt(utc, 0).single().unwrap_or_default().with_timezone(&off).date_naive()
}

pub fn parse(kind: Kind, file_name: &str, bytes: &[u8], ctx: &Ctx) -> Result<Parsed> {
    match kind {
        Kind::SleepJson => json::sleep(bytes),
        Kind::RhrJson => json::rhr(bytes),
        Kind::WeightJson => json::weight(bytes),
        Kind::HrvJson => json::hrv(bytes),
        Kind::DailySummaryJson => json::daily_summary(bytes),
        Kind::ActivityJson => json::activity(bytes),
        Kind::Fit => fit::parse(file_name, bytes, ctx),
        Kind::NotYetSupported(_) | Kind::Unknown => Ok(Parsed {
            content: kind.name(),
            ..Default::default()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_by_name() {
        assert_eq!(classify("Sleep/sleep_2026-10-01.json"), Kind::SleepJson);
        assert_eq!(classify("HRV/hrv_2026-05-04_2026-05-31.json"), Kind::HrvJson);
        assert_eq!(classify("rhr_2026-10-01.json"), Kind::RhrJson);
        assert_eq!(classify("x/activity_123.json"), Kind::ActivityJson);
        assert!(matches!(classify("activity_details_123.json"), Kind::NotYetSupported(_)));
        assert_eq!(classify("activity_types.json"), Kind::Unknown);
        assert_eq!(classify("123_HRV_STATUS.fit"), Kind::Fit);
        assert_eq!(classify("anything.FIT"), Kind::Fit);
        assert!(matches!(classify("1_WELLNESS.fit"), Kind::NotYetSupported(_)));
        assert_eq!(classify("notes.txt"), Kind::Unknown);
    }

    #[test]
    fn local_day_crosses_midnight() {
        // 2026-10-08 23:30 UTC is already 10-09 in Paris summer time (+2 h)
        let utc = Utc.with_ymd_and_hms(2026, 10, 8, 23, 30, 0).unwrap().timestamp();
        assert_eq!(local_day(utc, 7200), NaiveDate::from_ymd_opt(2026, 10, 9).unwrap());
        assert_eq!(local_day(utc, 0), NaiveDate::from_ymd_opt(2026, 10, 8).unwrap());
        let ctx = Ctx { tz: chrono_tz::Europe::Paris };
        assert_eq!(ctx.offset_at(utc), 7200);
    }
}
