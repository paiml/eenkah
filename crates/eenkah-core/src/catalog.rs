//! The metric catalogue (MET-4). Every series eenkah writes is described here and only
//! here; `/api/metrics`, the MCP tool and `docs/METRICS.md` are generated from this list.

use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct MetricDef {
    pub series: &'static str,
    pub unit: &'static str,
    /// suyo aggregation when zooming out: avg, sum, last, max, min.
    pub agg: &'static str,
    pub label: &'static str,
    pub why: &'static str,
    pub formula: &'static str,
    pub source: &'static str,
}

const fn m(series: &'static str, unit: &'static str, agg: &'static str, label: &'static str, why: &'static str, formula: &'static str, source: &'static str) -> MetricDef {
    MetricDef {
        series,
        unit,
        agg,
        label,
        why,
        formula,
        source,
    }
}

const GARMIN: &str = "Garmin device";
const PLEWS: &str = "Plews et al. 2013, Sports Med";
const BANISTER: &str = "Banister et al. 1975, Aust J Sports Med";
const FOSTER: &str = "Foster 1998, Med Sci Sports Exerc";
const FRIEL: &str = "Friel, The Triathlete's Training Bible (LTHR zones)";

pub const METRICS: &[MetricDef] = &[
    // body state
    m(
        "eenkah.body.hrv",
        "ms",
        "avg",
        "Overnight HRV",
        "Recovery signal from the nervous system",
        "Last-night average HRV reported by the watch (HRV status summary)",
        GARMIN,
    ),
    m(
        "eenkah.body.hrv_ln",
        "ln ms",
        "avg",
        "Overnight HRV (log)",
        "The log makes day-to-day changes comparable",
        "ln(hrv)",
        PLEWS,
    ),
    m(
        "eenkah.body.hrv_7d",
        "ms",
        "avg",
        "HRV 7-day",
        "A week-long drop matters; one bad night does not",
        "exp(mean of hrv_ln over the last 7 days), needs ≥ 4 nights",
        PLEWS,
    ),
    m(
        "eenkah.body.hrv_band_lo",
        "ms",
        "avg",
        "HRV normal (low)",
        "Your own normal range",
        "10th percentile of hrv_7d over the previous 60 days, needs ≥ 20 values",
        PLEWS,
    ),
    m(
        "eenkah.body.hrv_band_hi",
        "ms",
        "avg",
        "HRV normal (high)",
        "Your own normal range",
        "90th percentile of hrv_7d over the previous 60 days, needs ≥ 20 values",
        PLEWS,
    ),
    m(
        "eenkah.body.hrv_z",
        "sd",
        "avg",
        "HRV vs normal",
        "How unusual last night was for you",
        "(hrv_ln − mean of previous 60 days) ÷ their standard deviation, needs ≥ 20 values",
        PLEWS,
    ),
    m(
        "eenkah.body.rhr",
        "bpm",
        "avg",
        "Resting heart rate",
        "Early sign of illness, heat or fatigue",
        "Daily resting heart rate reported by the watch",
        GARMIN,
    ),
    m(
        "eenkah.body.rhr_z",
        "sd",
        "avg",
        "Resting HR vs normal",
        "How unusual today's resting HR is for you",
        "(rhr − mean of previous 60 days) ÷ their standard deviation, needs ≥ 20 values",
        "eenkah",
    ),
    m(
        "eenkah.body.skin_temp_dev",
        "°C",
        "avg",
        "Skin temperature deviation",
        "Rises with illness and heat load",
        "Overnight deviation from the watch's own baseline",
        GARMIN,
    ),
    m(
        "eenkah.body.bb_wake",
        "",
        "avg",
        "Body Battery at wake",
        "Garmin's own recovery estimate",
        "bodyBatteryAtWakeTime from the daily summary",
        GARMIN,
    ),
    m(
        "eenkah.body.spo2",
        "%",
        "avg",
        "Blood oxygen (sleep)",
        "Altitude and breathing context",
        "Average SpO2 during sleep",
        GARMIN,
    ),
    m("eenkah.body.weight", "kg", "avg", "Weight", "", "Body weight from weigh-ins", GARMIN),
    m(
        "eenkah.sleep.hours",
        "h",
        "avg",
        "Sleep",
        "Recovery happens here",
        "Sleep time (excludes awake time) for the night ending on this morning",
        GARMIN,
    ),
    m("eenkah.sleep.deep", "h", "avg", "Deep sleep", "", "Deep sleep time", GARMIN),
    m("eenkah.sleep.rem", "h", "avg", "REM sleep", "", "REM sleep time", GARMIN),
    m("eenkah.sleep.light", "h", "avg", "Light sleep", "", "Light sleep time", GARMIN),
    m("eenkah.sleep.awake", "h", "avg", "Awake in bed", "", "Awake time during the sleep window", GARMIN),
    m("eenkah.sleep.naps", "h", "sum", "Naps", "", "Nap time on this day", GARMIN),
    m("eenkah.sleep.score", "", "avg", "Sleep score", "Garmin's summary of the night", "Overall sleep score", GARMIN),
    m(
        "eenkah.sleep.stress",
        "",
        "avg",
        "Sleep stress",
        "Stays high when the body is fighting something",
        "Average stress during sleep",
        GARMIN,
    ),
    m(
        "eenkah.sleep.respiration",
        "brpm",
        "avg",
        "Sleep respiration",
        "Rises with illness",
        "Average breathing rate during sleep",
        GARMIN,
    ),
    m(
        "eenkah.sleep.debt_7d",
        "h",
        "last",
        "Sleep debt (7 days)",
        "Debt accumulates across a week",
        "Σ over the last 7 nights of (mean of the 28 nights before − hours slept); needs ≥ 5 and ≥ 14 nights",
        "eenkah",
    ),
    m(
        "eenkah.sleep.regularity",
        "min",
        "avg",
        "Sleep irregularity",
        "Travel and late nights show here first",
        "Mean of the standard deviations of bedtime and wake time over 7 nights; needs ≥ 4",
        "eenkah",
    ),
    m("eenkah.sleep.bedtime", "min", "avg", "Bedtime", "", "Local clock minutes after midnight when sleep started", GARMIN),
    m("eenkah.sleep.waketime", "min", "avg", "Wake time", "", "Local clock minutes after midnight when sleep ended", GARMIN),
    // training load
    m(
        "eenkah.load.daily",
        "load",
        "sum",
        "Daily load",
        "How much training stress the day added",
        "Σ session heart-rate impulse: Σ minutes × HRr × 0.64 × e^(1.92 × HRr), HRr = (HR − resting) ÷ (max − resting)",
        BANISTER,
    ),
    m(
        "eenkah.load.fitness",
        "load",
        "last",
        "Fitness",
        "What you have built up over weeks",
        "Exponential average of daily load, time constant 42 days",
        BANISTER,
    ),
    m(
        "eenkah.load.fatigue",
        "load",
        "last",
        "Fatigue",
        "What the last days took out of you",
        "Exponential average of daily load, time constant 7 days",
        BANISTER,
    ),
    m(
        "eenkah.load.form",
        "load",
        "last",
        "Form",
        "Fresh when positive, tired when very negative",
        "Yesterday's fitness − yesterday's fatigue",
        BANISTER,
    ),
    m(
        "eenkah.load.ramp",
        "load",
        "last",
        "Ramp",
        "How fast fitness is changing",
        "Fitness today − fitness 7 days ago",
        "eenkah",
    ),
    m(
        "eenkah.load.monotony",
        "",
        "last",
        "Monotony",
        "Same load every day leaves no room to absorb it",
        "7-day mean load ÷ 7-day standard deviation",
        FOSTER,
    ),
    m(
        "eenkah.load.strain",
        "load",
        "last",
        "Strain",
        "High load and high monotony together",
        "7-day total load × monotony",
        FOSTER,
    ),
    m("eenkah.zones.z1", "min", "sum", "Zone 1 time", "", "Minutes below 81 % of threshold heart rate", FRIEL),
    m("eenkah.zones.z2", "min", "sum", "Zone 2 time", "", "Minutes at 81–89 % of threshold heart rate", FRIEL),
    m("eenkah.zones.z3", "min", "sum", "Zone 3 time", "", "Minutes at 90–93 % of threshold heart rate", FRIEL),
    m("eenkah.zones.z4", "min", "sum", "Zone 4 time", "", "Minutes at 94–99 % of threshold heart rate", FRIEL),
    m("eenkah.zones.z5", "min", "sum", "Zone 5 time", "", "Minutes at or above threshold heart rate", FRIEL),
    // performance
    m(
        "eenkah.perf.run_efficiency",
        "m/min/bpm",
        "avg",
        "Running efficiency",
        "More speed for the same heart rate means aerobic gain",
        "Mean speed (m/min) ÷ mean heart rate over a steady run (≥ 20 min)",
        "eenkah",
    ),
    m(
        "eenkah.perf.decoupling",
        "%",
        "avg",
        "Decoupling",
        "Drift means the session outlasted current endurance",
        "Efficiency (speed or power ÷ HR) in the first half minus the second half, as % of the first; sessions ≥ 40 min",
        "eenkah",
    ),
    // activity streams (per second, at real timestamps)
    m("eenkah.stream.hr", "bpm", "avg", "Heart rate", "", "Per-record heart rate from activity files", GARMIN),
    m("eenkah.stream.speed", "km/h", "avg", "Speed", "", "Per-record speed", GARMIN),
    m("eenkah.stream.power", "W", "avg", "Power", "", "Per-record power (running power on supported watches)", GARMIN),
    m("eenkah.stream.cadence", "rpm", "avg", "Cadence", "", "Per-record cadence", GARMIN),
    m("eenkah.stream.altitude", "m", "avg", "Altitude", "", "Per-record altitude", GARMIN),
];

pub fn find(series: &str) -> Option<&'static MetricDef> {
    METRICS.iter().find(|m| m.series == series)
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_are_unique_and_namespaced() {
        let mut seen = std::collections::HashSet::new();
        for m in super::METRICS {
            assert!(m.series.starts_with("eenkah."), "{}", m.series);
            assert!(seen.insert(m.series), "duplicate {}", m.series);
            assert!(!m.formula.is_empty());
        }
    }
}
