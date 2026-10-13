//! Server-side SVG rendering for `format=svg`, so charts can be embedded
//! with a plain <img> tag (Graphite-style). Covers line, area and bar
//! (grouped or stacked); the interactive UI renders every chart type.

use crate::render::{Params, Rendered};
use crate::series::Series;
use crate::timeutil::{Unit, Zone};
use chrono::{Datelike, Timelike};
use std::fmt::Write;

pub struct Theme {
    pub surface: &'static str,
    pub ink: &'static str,
    pub ink2: &'static str,
    pub muted: &'static str,
    pub grid: &'static str,
    pub axis: &'static str,
    pub dim: &'static str,
    pub palette: [&'static str; 8],
}

const LIGHT_PAL: [&str; 8] = [
    "#2a78d6", "#eb6834", "#1baf7a", "#eda100", "#e87ba4", "#008300", "#4a3aa7", "#e34948",
];
const DARK_PAL: [&str; 8] = [
    "#3987e5", "#d95926", "#199e70", "#c98500", "#d55181", "#008300", "#9085e9", "#e66767",
];

pub fn theme(name: &str) -> Theme {
    match name {
        "dark" => Theme {
            surface: "#1a1a19",
            ink: "#ffffff",
            ink2: "#c3c2b7",
            muted: "#898781",
            grid: "#2c2c2a",
            axis: "#383835",
            dim: "#5c5b57",
            palette: DARK_PAL,
        },
        "nord" => Theme {
            surface: "#2e3440",
            ink: "#eceff4",
            ink2: "#d8dee9",
            muted: "#8f99ab",
            grid: "#3b4252",
            axis: "#4c566a",
            dim: "#616b7e",
            palette: DARK_PAL,
        },
        "midnight" => Theme {
            surface: "#0f172a",
            ink: "#f8fafc",
            ink2: "#cbd5e1",
            muted: "#8391a7",
            grid: "#1e293b",
            axis: "#334155",
            dim: "#475569",
            palette: DARK_PAL,
        },
        "paper" => Theme {
            surface: "#fbf8f1",
            ink: "#1d1b16",
            ink2: "#5b564b",
            muted: "#8d877a",
            grid: "#e6e0d1",
            axis: "#cfc8b6",
            dim: "#c2bba9",
            palette: LIGHT_PAL,
        },
        _ => Theme {
            surface: "#fcfcfb",
            ink: "#0b0b0b",
            ink2: "#52514e",
            muted: "#898781",
            grid: "#e1e0d9",
            axis: "#c3c2b7",
            dim: "#c3c2b7",
            palette: LIGHT_PAL,
        },
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn nice_ticks(lo: f64, hi: f64, n: usize) -> Vec<f64> {
    // also true when either bound is NaN
    if hi.partial_cmp(&lo) != Some(std::cmp::Ordering::Greater) {
        return vec![lo];
    }
    let raw = (hi - lo) / n.max(1) as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0]
        .iter()
        .map(|m| m * mag)
        .find(|s| *s >= raw)
        .unwrap_or(10.0 * mag);
    let mut t = (lo / step).ceil() * step;
    let mut out = vec![];
    while t <= hi + step * 1e-9 {
        out.push(if t.abs() < step * 1e-9 { 0.0 } else { t });
        t += step;
    }
    out
}

pub fn fmt_val(v: f64) -> String {
    let a = v.abs();
    if a >= 1e9 {
        format!("{:.1}B", v / 1e9)
    } else if a >= 1e6 {
        format!("{:.1}M", v / 1e6)
    } else if a >= 1e4 {
        format!("{:.1}K", v / 1e3)
    } else if a >= 1000.0 {
        let i = v.round() as i64;
        let s = i.abs().to_string();
        let mut out = String::new();
        for (k, c) in s.chars().enumerate() {
            if k > 0 && (s.len() - k).is_multiple_of(3) {
                out.push(',');
            }
            out.push(c);
        }
        if i < 0 {
            format!("-{out}")
        } else {
            out
        }
    } else if a >= 100.0 || v.fract() == 0.0 {
        format!("{:.0}", v)
    } else if a >= 1.0 {
        format!("{:.1}", v)
    } else {
        format!("{:.2}", v)
    }
}

fn time_ticks(from: i64, until: i64, zone: Zone, n: usize) -> Vec<(i64, String)> {
    let span = (until - from).max(1);
    let candidates: &[(i64, Unit)] = &[
        (60, Unit::Minute),
        (300, Unit::Minute),
        (900, Unit::Minute),
        (1800, Unit::Minute),
        (3600, Unit::Hour),
        (3 * 3600, Unit::Hour),
        (6 * 3600, Unit::Hour),
        (12 * 3600, Unit::Hour),
        (86400, Unit::Day),
        (2 * 86400, Unit::Day),
        (7 * 86400, Unit::Week),
        (30 * 86400, Unit::Month),
        (91 * 86400, Unit::Month),
        (365 * 86400, Unit::Year),
    ];
    let (step, unit) = candidates
        .iter()
        .copied()
        .find(|(s, _)| span / s <= n as i64)
        .unwrap_or((365 * 86400, Unit::Year));
    let bucket = match unit {
        Unit::Month if step > 40 * 86400 => crate::timeutil::Bucket(crate::timeutil::Span {
            n: 3,
            unit: Unit::Month,
        }),
        Unit::Day | Unit::Week | Unit::Month | Unit::Year => {
            crate::timeutil::Bucket(crate::timeutil::Span {
                n: (step / unit.secs()).max(1),
                unit,
            })
        }
        _ => crate::timeutil::Bucket::secs(step),
    };
    let mut t = bucket.start_of(from, zone);
    if t < from {
        t = bucket.next(t, zone);
    }
    let mut out = vec![];
    while t <= until && out.len() < 50 {
        let l = zone.to_local(t);
        let label = match unit {
            Unit::Minute | Unit::Hour if l.hour() == 0 && l.minute() == 0 => {
                l.format("%b %-d").to_string()
            }
            Unit::Minute | Unit::Hour => l.format("%H:%M").to_string(),
            Unit::Day | Unit::Week => l.format("%b %-d").to_string(),
            Unit::Month if l.month() == 1 => l.format("%Y").to_string(),
            Unit::Month => l.format("%b").to_string(),
            _ => l.format("%Y").to_string(),
        };
        out.push((t, label));
        t = bucket.next(t, zone);
    }
    out
}

pub fn render(r: &Rendered, p: &Params) -> String {
    let w: f64 = p
        .get("width")
        .and_then(|v| v.parse().ok())
        .unwrap_or(800.0f64)
        .clamp(200.0, 4000.0);
    let h: f64 = p
        .get("height")
        .and_then(|v| v.parse().ok())
        .unwrap_or(360.0f64)
        .clamp(120.0, 3000.0);
    let th = theme(p.get("theme").unwrap_or("light"));
    let chart = p.get("chart").unwrap_or("line");
    let title = p.get("title");
    let show_legend = p.get("legend").map(|v| v != "hide").unwrap_or(true) && r.series.len() > 1;
    let font = "system-ui,-apple-system,'Segoe UI',sans-serif";

    let mut s = String::new();
    let _ = write!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" font-family="{font}" font-size="12"><rect width="100%" height="100%" fill="{}"/>"#,
        th.surface
    );
    let mut top = 14.0;
    if let Some(t) = title {
        let _ = write!(
            s,
            r#"<text x="16" y="28" fill="{}" font-size="15" font-weight="600">{}</text>"#,
            th.ink,
            esc(t)
        );
        top = 44.0;
    }
    if show_legend {
        let mut x = 16.0;
        for (i, se) in r.series.iter().enumerate() {
            let c = color_of(se, i, &th);
            let _ = write!(
                s,
                r#"<rect x="{x}" y="{}" width="10" height="10" rx="2" fill="{c}"/><text x="{}" y="{}" fill="{}">{}</text>"#,
                top,
                x + 14.0,
                top + 9.5,
                th.ink2,
                esc(&se.name)
            );
            x += 28.0 + se.name.chars().count() as f64 * 6.6;
        }
        top += 24.0;
    }

    let stacked = chart == "stacked" || r.series.iter().any(|x| x.style.stack);
    let bars = chart == "bar" || chart == "stacked";
    // Union of timestamps for bar slots / stacking.
    let mut xs: Vec<i64> = r.series.iter().flat_map(|x| x.ts.iter().copied()).collect();
    xs.sort_unstable();
    xs.dedup();
    let val_at = |se: &Series, t: i64| se.ts.binary_search(&t).ok().and_then(|i| se.vals[i]);

    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    if stacked {
        for t in &xs {
            let tot: f64 = r
                .series
                .iter()
                .filter(|x| x.style.kind.is_none())
                .filter_map(|x| val_at(x, *t))
                .sum();
            hi = hi.max(tot);
            lo = lo.min(0.0);
        }
    }
    for se in &r.series {
        for v in se.vals.iter().flatten() {
            lo = lo.min(*v);
            hi = hi.max(*v);
        }
    }
    if !lo.is_finite() {
        lo = 0.0;
        hi = 1.0;
    }
    if bars || chart == "area" {
        lo = lo.min(0.0);
    }
    if let Some(v) = p.get("yMin").and_then(|v| v.parse().ok()) {
        lo = v;
    }
    if let Some(v) = p.get("yMax").and_then(|v| v.parse().ok()) {
        hi = v;
    }
    if hi <= lo {
        hi = lo + 1.0;
    }
    let ticks = nice_ticks(lo, hi + (hi - lo) * 0.05, ((h - top) / 50.0) as usize);
    let (lo, hi) = (lo.min(ticks[0]), hi.max(*ticks.last().unwrap()));
    let label_w = ticks.iter().map(|t| fmt_val(*t).len()).max().unwrap_or(3) as f64 * 7.0 + 12.0;
    let (x0, x1, y0, y1) = (16.0 + label_w, w - 20.0, top + 6.0, h - 28.0);
    let sx = |t: i64| x0 + (t - r.from) as f64 / (r.until - r.from).max(1) as f64 * (x1 - x0);
    let sy = |v: f64| y1 - (v - lo) / (hi - lo) * (y1 - y0);

    for t in &ticks {
        let y = sy(*t);
        let _ = write!(
            s,
            r#"<line x1="{x0}" x2="{x1}" y1="{y:.1}" y2="{y:.1}" stroke="{}" stroke-width="1"/><text x="{}" y="{:.1}" fill="{}" text-anchor="end" style="font-variant-numeric:tabular-nums">{}</text>"#,
            if *t == 0.0 { th.axis } else { th.grid },
            x0 - 8.0,
            y + 4.0,
            th.muted,
            fmt_val(*t)
        );
    }
    let _ = write!(
        s,
        r#"<line x1="{x0}" x2="{x1}" y1="{y1}" y2="{y1}" stroke="{}" stroke-width="1"/>"#,
        th.axis
    );
    for (t, label) in time_ticks(r.from, r.until, r.zone, ((x1 - x0) / 90.0) as usize) {
        let _ = write!(
            s,
            r#"<text x="{:.1}" y="{}" fill="{}" text-anchor="middle">{}</text>"#,
            sx(t),
            y1 + 18.0,
            th.muted,
            esc(&label)
        );
    }

    if bars {
        let data: Vec<&Series> = r.series.iter().filter(|x| x.style.kind.is_none()).collect();
        let slot = if xs.len() > 1 {
            (sx(xs[1]) - sx(xs[0])).abs()
        } else {
            (x1 - x0) / 2.0
        };
        let groups = if stacked { 1 } else { data.len().max(1) };
        let bw = ((slot * 0.75) / groups as f64 - 2.0).clamp(1.0, 24.0);
        for t in &xs {
            let mut base = 0.0;
            for (k, se) in data.iter().enumerate() {
                let Some(v) = val_at(se, *t) else { continue };
                let idx = r
                    .series
                    .iter()
                    .position(|x| std::ptr::eq(x, *se))
                    .unwrap_or(k);
                let c = color_of(se, idx, &th);
                let cx = sx(*t) + slot / 2.0;
                let x = if stacked {
                    cx - bw / 2.0
                } else {
                    cx - (groups as f64 * (bw + 2.0)) / 2.0 + k as f64 * (bw + 2.0)
                };
                let (ya, yb) = if stacked {
                    (sy(base + v), sy(base))
                } else {
                    (sy(v.max(0.0)), sy(v.min(0.0)))
                };
                let hgt = (yb - ya - if stacked && base > 0.0 { 2.0 } else { 0.0 }).max(0.5);
                let _ = write!(
                    s,
                    r#"<rect x="{x:.1}" y="{ya:.1}" width="{bw:.1}" height="{hgt:.1}" rx="{}" fill="{c}"/>"#,
                    if hgt > 6.0 { 2 } else { 0 }
                );
                if stacked {
                    base += v;
                }
            }
        }
    }
    let mut stack_base: Vec<f64> = vec![0.0; xs.len()];
    for (i, se) in r.series.iter().enumerate() {
        if bars && se.style.kind.is_none() {
            continue;
        }
        let c = color_of(se, i, &th);
        let width = se.style.width.unwrap_or(2.0);
        let dash = if se.style.dashed || se.style.kind.is_some() {
            r#" stroke-dasharray="5 4""#
        } else {
            ""
        };
        let mut d = String::new();
        let mut pen = false;
        let mut area_top: Vec<(f64, f64, f64)> = vec![];
        for (j, t) in xs.iter().enumerate() {
            let v = val_at(se, *t);
            match v {
                Some(v) => {
                    let off = if stacked && chart == "area" {
                        stack_base[j]
                    } else {
                        0.0
                    };
                    let (x, y) = (sx(*t), sy(v + off));
                    let _ = write!(d, "{}{x:.1},{y:.1}", if pen { "L" } else { "M" });
                    pen = true;
                    area_top.push((x, y, sy(off.max(lo))));
                    if stacked && chart == "area" {
                        stack_base[j] += v;
                    }
                }
                None => pen = false,
            }
        }
        if chart == "area" && !area_top.is_empty() {
            let mut a = String::new();
            for (k, (x, y, _)) in area_top.iter().enumerate() {
                let _ = write!(a, "{}{x:.1},{y:.1}", if k == 0 { "M" } else { "L" });
            }
            for (x, _, b) in area_top.iter().rev() {
                let _ = write!(a, "L{x:.1},{b:.1}");
            }
            let _ = write!(
                s,
                r#"<path d="{a}Z" fill="{c}" fill-opacity="{}"/>"#,
                if stacked { 0.55 } else { 0.12 }
            );
        }
        let _ = write!(
            s,
            r#"<path d="{d}" fill="none" stroke="{c}" stroke-width="{width}" stroke-linejoin="round" stroke-linecap="round"{dash}/>"#
        );
    }
    if r.series.is_empty() {
        let _ = write!(
            s,
            r#"<text x="{}" y="{}" fill="{}" text-anchor="middle">No data</text>"#,
            w / 2.0,
            h / 2.0,
            th.muted
        );
    }
    s.push_str("</svg>");
    s
}

fn color_of(se: &Series, i: usize, th: &Theme) -> String {
    if se.style.emphasis < 0 {
        return th.dim.to_string();
    }
    se.style
        .color
        .clone()
        .unwrap_or_else(|| th.palette[i % 8].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ticks_and_values() {
        assert_eq!(
            nice_ticks(0.0, 100.0, 5),
            vec![0.0, 20.0, 40.0, 60.0, 80.0, 100.0]
        );
        assert_eq!(fmt_val(12500.0), "12.5K");
        assert_eq!(fmt_val(1234.0), "1,234");
        assert_eq!(fmt_val(0.256), "0.26");
    }
}
