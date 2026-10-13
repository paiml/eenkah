//! Findings: typed statements about the athlete's latest state, ranked by how unusual they
//! are for this athlete. Each kind has a hand-written template sentence (INS-3), so the
//! product is complete without a language model. Templates describe and warn; they never
//! diagnose or tell the athlete to train or rest (INS-6).

use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Finding {
    pub subject: &'static str,
    /// "up", "down" or "steady"
    pub direction: &'static str,
    /// Size against the athlete's normal: standard deviations, or the unit in `unit`.
    pub size: f64,
    pub unit: &'static str,
    /// How many days the finding covers.
    pub span_days: u32,
    /// Series the numbers came from.
    pub series: &'static str,
    /// Ranking score: higher is more unusual / more relevant.
    pub weight: f64,
    pub sentence: String,
}

/// The latest values the finding rules look at. `None` = not available (never assumed).
#[derive(Clone, Debug, Default, Serialize)]
pub struct Latest {
    pub hrv_z: Option<f64>,
    /// 7-day HRV position in the normal band: −1 at the low edge, +1 at the high edge.
    pub hrv_trend: Option<f64>,
    pub rhr_z: Option<f64>,
    pub rhr: Option<f64>,
    pub sleep_hours: Option<f64>,
    pub sleep_debt_7d: Option<f64>,
    pub form: Option<f64>,
    pub fitness: Option<f64>,
    pub skin_temp_dev: Option<f64>,
    pub days_since_hard: Option<i64>,
    /// The newest night was unusual for this athlete (several signals off normal at once):
    /// the signals that drove it, in plain words.
    pub unusual: Option<Vec<String>>,
    /// Recent shifts in a signal's normal level.
    pub shifts: Vec<ShiftNote>,
}

/// A shift in one signal's normal level (eenkah-learn model 3).
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ShiftNote {
    /// The signal as it reads inside a sentence ("resting heart rate").
    pub label: &'static str,
    pub series: &'static str,
    pub unit: &'static str,
    /// "up" or "down"
    pub direction: &'static str,
    /// Size of the change, in `unit` (absolute).
    pub change: f64,
    /// Days since the first night at the new level.
    pub days_ago: i64,
}

#[allow(clippy::too_many_arguments)]
fn f(subject: &'static str, direction: &'static str, size: f64, unit: &'static str, span: u32, series: &'static str, weight: f64, sentence: String) -> Finding {
    Finding {
        subject,
        direction,
        size,
        unit,
        span_days: span,
        series,
        weight,
        sentence,
    }
}

pub fn findings(l: &Latest) -> Vec<Finding> {
    let mut out = vec![];
    if let Some(z) = l.hrv_z {
        if z <= -1.0 {
            out.push(f(
                "hrv",
                "down",
                z,
                "sd",
                1,
                "eenkah.body.hrv_z",
                -z,
                format!("Last night's HRV was {:.1} standard deviations below your normal.", -z),
            ));
        } else if z >= 1.5 {
            out.push(f(
                "hrv",
                "up",
                z,
                "sd",
                1,
                "eenkah.body.hrv_z",
                z * 0.5,
                format!("Last night's HRV was {z:.1} standard deviations above your normal."),
            ));
        }
    }
    if let Some(t) = l.hrv_trend {
        if t < -1.0 {
            out.push(f(
                "hrv_week",
                "down",
                t,
                "band",
                7,
                "eenkah.body.hrv_7d",
                1.5 - t,
                "Your 7-day HRV has dropped below your normal range.".into(),
            ));
        } else if t > 1.0 {
            out.push(f(
                "hrv_week",
                "up",
                t,
                "band",
                7,
                "eenkah.body.hrv_7d",
                0.5 + t * 0.3,
                "Your 7-day HRV is above your normal range.".into(),
            ));
        }
    }
    if let (Some(z), Some(r)) = (l.rhr_z, l.rhr) {
        if z >= 1.5 {
            out.push(f(
                "rhr",
                "up",
                z,
                "sd",
                1,
                "eenkah.body.rhr_z",
                z,
                format!("Resting heart rate is {r:.0} bpm, {z:.1} standard deviations above your normal."),
            ));
        }
    }
    if let Some(d) = l.sleep_debt_7d {
        if d >= 3.0 {
            out.push(f(
                "sleep_debt",
                "up",
                d,
                "h",
                7,
                "eenkah.sleep.debt_7d",
                d / 2.0,
                format!("This week you slept about {:.0} hours less than your normal.", d),
            ));
        }
    }
    if let Some(h) = l.sleep_hours {
        if h < 5.5 {
            out.push(f("sleep", "down", h, "h", 1, "eenkah.sleep.hours", 6.0 - h, format!("Last night was short: {h:.1} hours of sleep.")));
        }
    }
    if let Some(t) = l.skin_temp_dev {
        if t >= 0.5 {
            out.push(f(
                "skin_temp",
                "up",
                t,
                "°C",
                1,
                "eenkah.body.skin_temp_dev",
                t * 2.0,
                format!("Skin temperature was {t:.1} °C above your baseline overnight."),
            ));
        }
    }
    if let (Some(form), Some(fit)) = (l.form, l.fitness) {
        let rel = if fit > 1.0 { form / fit } else { 0.0 };
        if rel < -0.3 {
            out.push(f(
                "form",
                "down",
                form,
                "load",
                7,
                "eenkah.load.form",
                -rel * 3.0,
                "Recent training has been heavy compared with what you are used to; fatigue is well above fitness.".into(),
            ));
        } else if rel > 0.15 {
            out.push(f("form", "up", form, "load", 7, "eenkah.load.form", rel * 2.0, "You are fresh: fatigue is below your fitness.".into()));
        }
    }
    if let Some(drivers) = &l.unusual {
        let what = match drivers.as_slice() {
            [] => "several signals were away from your normal at once".to_string(),
            [a] => a.clone(),
            [a, b] => format!("{a} and {b}"),
            [a, b, c, ..] => format!("{a}, {b} and {c}"),
        };
        out.push(f(
            "unusual",
            "up",
            drivers.len() as f64,
            "signals",
            1,
            "eenkah.unusual.score",
            3.5,
            format!("Last night was unusual for you: {what}."),
        ));
    }
    for sh in &l.shifts {
        let unit = if sh.unit.is_empty() { String::new() } else { format!(" {}", sh.unit) };
        out.push(f(
            "shift",
            sh.direction,
            sh.change,
            sh.unit,
            sh.days_ago.max(1) as u32,
            sh.series,
            1.2,
            format!(
                "Your normal {} has moved {} by about {:.0}{unit}, starting {}.",
                sh.label,
                sh.direction,
                sh.change,
                crate::when::days(sh.days_ago)
            ),
        ));
    }
    out.sort_by(|a, b| b.weight.total_cmp(&a.weight));
    out
}

/// One or two sentences from the top findings, or a neutral line when nothing stands out.
pub fn brief(fs: &[Finding]) -> String {
    match fs {
        [] => "Nothing unusual in last night's data compared with your normal.".into(),
        [a] => a.sentence.clone(),
        [a, b, ..] => format!("{} {}", a.sentence, b.sentence),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_unusual_is_said_plainly() {
        assert_eq!(brief(&findings(&Latest::default())), "Nothing unusual in last night's data compared with your normal.");
    }

    #[test]
    fn biggest_deviation_comes_first() {
        let l = Latest {
            hrv_z: Some(-2.5),
            rhr_z: Some(1.6),
            rhr: Some(56.0),
            sleep_debt_7d: Some(4.0),
            ..Default::default()
        };
        let fs = findings(&l);
        assert_eq!(fs[0].subject, "hrv");
        assert!(brief(&fs).starts_with("Last night's HRV was 2.5 standard deviations below"));
    }

    #[test]
    fn templates_never_instruct() {
        // INS-6: no instruction to train or rest, no diagnosis
        let l = Latest {
            hrv_z: Some(-3.0),
            hrv_trend: Some(-2.0),
            rhr_z: Some(3.0),
            rhr: Some(60.0),
            sleep_hours: Some(4.0),
            sleep_debt_7d: Some(6.0),
            form: Some(-40.0),
            fitness: Some(60.0),
            skin_temp_dev: Some(1.0),
            days_since_hard: Some(0),
            unusual: Some(vec!["resting heart rate 6 bpm above your normal".into(), "breathing rate 1.2 brpm above your normal".into()]),
            shifts: vec![ShiftNote {
                label: "resting heart rate",
                series: "eenkah.body.rhr",
                unit: "bpm",
                direction: "up",
                change: 3.0,
                days_ago: 10,
            }],
        };
        for fd in findings(&l) {
            let s = fd.sentence.to_lowercase();
            for banned in ["you should", "rest ", "take it easy", "sick", "ill", "infection", "train ", "skip"] {
                assert!(!s.contains(banned), "'{banned}' in: {s}");
            }
        }
    }

    #[test]
    fn unu2_an_unusual_night_is_described_not_diagnosed() {
        let l = Latest {
            unusual: Some(vec!["resting heart rate 6 bpm above your normal".into(), "HRV 20 % below your normal".into()]),
            ..Default::default()
        };
        let fs = findings(&l);
        assert_eq!(fs[0].subject, "unusual");
        assert_eq!(
            fs[0].sentence,
            "Last night was unusual for you: resting heart rate 6 bpm above your normal and HRV 20 % below your normal."
        );
    }
}
