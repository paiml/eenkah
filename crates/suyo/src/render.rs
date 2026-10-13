//! /render: turn query args into evaluated series and serialise them.

use crate::eval::{evaluate, Ctx};
use crate::parser;
use crate::series::Series;
use crate::store::Store;
use crate::timeutil::{self, fmt_local, Bucket, Zone};
use anyhow::{bail, Result};
use serde_json::{json, Value};

pub struct Params(pub Vec<(String, String)>);

impl Params {
    pub fn get(&self, k: &str) -> Option<&str> {
        self.0
            .iter()
            .rev()
            .find(|(a, _)| a == k)
            .map(|(_, v)| v.as_str())
            .filter(|v| !v.is_empty())
    }
    pub fn all(&self, k: &str) -> Vec<&str> {
        self.0
            .iter()
            .filter(|(a, _)| a == k)
            .map(|(_, v)| v.as_str())
            .filter(|v| !v.trim().is_empty())
            .collect()
    }
}

pub struct Rendered {
    pub from: i64,
    pub until: i64,
    pub zone: Zone,
    pub bucket: Bucket,
    pub series: Vec<Series>,
    /// (target, message) for targets that failed; the rest still render.
    pub errors: Vec<(String, String)>,
}

pub fn run(store: &Store, p: &Params, default_zone: Zone) -> Result<Rendered> {
    let zone = match p.get("tz") {
        Some(z) => Zone::parse(z)?,
        None => default_zone,
    };
    let now = timeutil::now();
    let from = timeutil::parse_time(p.get("from").unwrap_or("-1d"), now, zone, false)?;
    let until = timeutil::parse_time(p.get("until").unwrap_or("now"), now, zone, true)?;
    if until <= from {
        bail!("'until' must be after 'from'");
    }
    let max_points = p
        .get("maxDataPoints")
        .or_else(|| p.get("width"))
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(800)
        .clamp(10, 20_000);
    let (bucket, forced) = match p.get("step") {
        Some(s) => (Bucket::parse(s)?, true),
        None => (Bucket::auto(from, until, max_points), false),
    };
    if forced && (until - from) / bucket.nominal().max(1) > 200_000 {
        bail!("step '{bucket}' is too fine for this range (over 200k points per series)");
    }
    let index = store.series()?;
    let ctx = Ctx {
        store,
        index: &index,
        from,
        until,
        zone,
        bucket,
        forced,
        depth: 0,
    };
    let mut series = Vec::new();
    let mut errors = Vec::new();
    for t in p.all("target") {
        match parser::parse(t).and_then(|e| evaluate(&e, &ctx)) {
            Ok(list) => series.extend(list),
            Err(e) => errors.push((t.to_string(), format!("{e:#}"))),
        }
    }
    Ok(Rendered {
        from,
        until,
        zone,
        bucket,
        series,
        errors,
    })
}

fn round(v: f64) -> Value {
    // Keep payloads small: 6 significant decimals is plenty for charts.
    let r = (v * 1e6).round() / 1e6;
    json!(r)
}

fn datapoints(s: &Series) -> Vec<Value> {
    s.ts.iter()
        .zip(&s.vals)
        .map(|(t, v)| json!([v.map(round), t]))
        .collect()
}

/// Graphite-compatible: `[{"target": ..., "datapoints": [[value, ts], ...]}]`.
pub fn graphite_json(r: &Rendered) -> Value {
    Value::Array(
        r.series
            .iter()
            .map(|s| json!({"target": s.name, "tags": {"name": s.path}, "datapoints": datapoints(s)}))
            .collect(),
    )
}

/// Richer format used by the bundled UI.
pub fn suyo_json(r: &Rendered) -> Value {
    json!({
        "from": r.from,
        "until": r.until,
        "tz": r.zone.name(),
        "step": r.bucket.to_string(),
        "stepSeconds": r.bucket.nominal(),
        "series": r.series.iter().map(|s| json!({
            "target": s.name,
            "path": s.path,
            "unit": s.unit,
            "agg": s.agg,
            "step": s.bucket.to_string(),
            "stepSeconds": s.bucket.nominal(),
            "style": s.style,
            "datapoints": datapoints(s),
        })).collect::<Vec<_>>(),
        "errors": r.errors.iter().map(|(t, e)| json!({"target": t, "error": e})).collect::<Vec<_>>(),
    })
}

pub fn csv(r: &Rendered) -> String {
    let mut out = String::new();
    for s in &r.series {
        for (t, v) in s.ts.iter().zip(&s.vals) {
            let name = if s.name.contains([',', '"']) {
                format!("\"{}\"", s.name.replace('"', "\"\""))
            } else {
                s.name.clone()
            };
            out.push_str(&format!(
                "{},{},{}\n",
                name,
                fmt_local(*t, r.zone),
                v.map(|x| x.to_string()).unwrap_or_default()
            ));
        }
    }
    out
}
