//! Pull-based sources: run a SQL query against any SQLite database and map
//! its columns to series. Wide rows (`ts, a, b, c`) become `prefix.a`,
//! `prefix.b`, ...; long rows (`ts, series, value`) name their own series.
//! Incremental: the query may use `:since`, bound to the last synced time
//! minus `overlap`, so re-syncs are cheap and idempotent.

use crate::store::{sanitize_segment, AggFn, MetaHint, Store};
use crate::timeutil::{self, parse_clock_duration, Span, Zone};
use anyhow::{anyhow, bail, Context, Result};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize, Default)]
pub struct SourcesFile {
    #[serde(default, rename = "source")]
    pub sources: Vec<SourceSpec>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SourceSpec {
    pub name: String,
    pub db: String,
    pub query: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default = "default_ts_col")]
    pub ts_col: String,
    /// Timezone for naive timestamps in the source ("local", "UTC", IANA).
    #[serde(default)]
    pub tz: String,
    /// Native interval for all series (e.g. "1d" for daily summaries).
    pub interval: Option<String>,
    /// How far back to re-read on incremental syncs (default 2d).
    pub overlap: Option<String>,
    /// Unit for "HH:MM:SS" text values: s, min or h (default min).
    pub duration_unit: Option<String>,
    #[serde(default)]
    pub units: HashMap<String, String>,
    #[serde(default)]
    pub agg: HashMap<String, String>,
    #[serde(default)]
    pub descriptions: HashMap<String, String>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn default_ts_col() -> String {
    "ts".into()
}
fn yes() -> bool {
    true
}

pub fn load(path: &str) -> Result<SourcesFile> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
    let f: SourcesFile = toml::from_str(&text).with_context(|| format!("parsing {path}"))?;
    Ok(f)
}

pub fn expand_home(p: &str) -> String {
    match (p.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(h)) => format!("{h}/{rest}"),
        _ => p.to_string(),
    }
}

pub struct SyncReport {
    pub source: String,
    pub rows: usize,
    pub points: usize,
    pub series: usize,
}

pub fn sync_all(store: &mut Store, file: &SourcesFile, full: bool) -> Vec<Result<SyncReport>> {
    file.sources
        .iter()
        .filter(|s| s.enabled)
        .map(|s| sync(store, s, full).with_context(|| format!("source '{}'", s.name)))
        .collect()
}

pub fn sync(store: &mut Store, spec: &SourceSpec, full: bool) -> Result<SyncReport> {
    let zone = Zone::parse(&spec.tz)?;
    let db = expand_home(&spec.db);
    if !Path::new(&db).exists() {
        bail!("database not found: {db}");
    }
    let src = Connection::open_with_flags(
        &db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    src.busy_timeout(std::time::Duration::from_secs(30))?;
    let overlap = Span::parse(spec.overlap.as_deref().unwrap_or("2d"))?.approx_secs();
    let since_ts = if full {
        None
    } else {
        store.sync_state(&spec.name)?.map(|t| t - overlap)
    };
    let since = since_ts
        .map(|t| zone.to_local(t).format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "0000-01-01 00:00:00".into());
    let interval = spec
        .interval
        .as_deref()
        .map(Span::parse)
        .transpose()?
        .map(|s| s.approx_secs());
    let dur_div = match spec.duration_unit.as_deref().unwrap_or("min") {
        "s" | "sec" => 1.0,
        "min" | "m" => 60.0,
        "h" | "hour" => 3600.0,
        o => bail!("duration_unit must be s, min or h (got {o})"),
    };

    let mut st = src.prepare(&spec.query).context("preparing query")?;
    let cols: Vec<String> = st.column_names().iter().map(|s| s.to_string()).collect();
    let ts_idx = cols.iter().position(|c| *c == spec.ts_col).ok_or_else(|| {
        anyhow!(
            "query has no '{}' column (columns: {})",
            spec.ts_col,
            cols.join(", ")
        )
    })?;
    let long = cols
        .iter()
        .position(|c| c == "series")
        .zip(cols.iter().position(|c| c == "value"));
    let value_cols: Vec<usize> = (0..cols.len()).filter(|i| *i != ts_idx).collect();

    let name_for = |raw: &str| -> String {
        let tail = raw
            .split('.')
            .map(sanitize_segment)
            .collect::<Vec<_>>()
            .join(".");
        if spec.prefix.is_empty() {
            tail
        } else {
            format!("{}.{}", spec.prefix, tail)
        }
    };
    let hint_for = |key: &str| -> Result<MetaHint> {
        let last = key.rsplit('.').next().unwrap_or(key);
        let pick = |m: &HashMap<String, String>| {
            m.get(key)
                .or_else(|| m.get(last))
                .or_else(|| m.get("*"))
                .cloned()
        };
        Ok(MetaHint {
            unit: pick(&spec.units),
            agg: pick(&spec.agg).map(|a| AggFn::parse(&a)).transpose()?,
            interval,
            description: pick(&spec.descriptions),
        })
    };

    let mut rows = if spec.query.contains(":since") {
        st.query(rusqlite::named_params! {":since": since})?
    } else {
        st.query([])?
    };
    let mut buf: Vec<(String, i64, f64)> = Vec::new();
    let mut hints: HashMap<String, MetaHint> = HashMap::new();
    let mut names_cache: HashMap<String, String> = HashMap::new();
    let (mut nrows, mut npoints) = (0usize, 0usize);
    let mut max_ts: Option<i64> = None;

    let flush = |buf: &mut Vec<(String, i64, f64)>,
                 hints: &HashMap<String, MetaHint>,
                 store: &mut Store|
     -> Result<usize> {
        let n = store.write(buf, hints)?;
        buf.clear();
        Ok(n)
    };

    while let Some(r) = rows.next()? {
        nrows += 1;
        let Some(ts) = ts_value(r.get_ref(ts_idx)?, zone) else {
            continue;
        };
        max_ts = Some(max_ts.map_or(ts, |m: i64| m.max(ts)));
        let mut emit = |key: &str, v: ValueRef| -> Result<()> {
            let Some(x) = num_value(v, dur_div) else {
                return Ok(());
            };
            let name = match names_cache.get(key) {
                Some(n) => n.clone(),
                None => {
                    let n = name_for(key);
                    crate::store::validate_name(&n)?;
                    hints.insert(n.clone(), hint_for(key)?);
                    names_cache.insert(key.to_string(), n.clone());
                    n
                }
            };
            buf.push((name, ts, x));
            Ok(())
        };
        match long {
            Some((si, vi)) => {
                let key = match r.get_ref(si)? {
                    ValueRef::Text(t) => String::from_utf8_lossy(t).to_string(),
                    ValueRef::Null => continue,
                    o => format!("{:?}", o),
                };
                emit(&key, r.get_ref(vi)?)?;
            }
            None => {
                for &i in &value_cols {
                    emit(&cols[i], r.get_ref(i)?)?;
                }
            }
        }
        if buf.len() >= 200_000 {
            npoints += flush(&mut buf, &hints, store)?;
        }
    }
    npoints += flush(&mut buf, &hints, store)?;
    store.set_sync_state(&spec.name, max_ts, nrows)?;
    Ok(SyncReport {
        source: spec.name.clone(),
        rows: nrows,
        points: npoints,
        series: names_cache.len(),
    })
}

fn ts_value(v: ValueRef, zone: Zone) -> Option<i64> {
    match v {
        ValueRef::Integer(i) => Some(if i > 100_000_000_000 { i / 1000 } else { i }),
        ValueRef::Real(f) => Some(if f > 1e11 {
            (f / 1000.0) as i64
        } else {
            f as i64
        }),
        ValueRef::Text(t) => {
            let s = std::str::from_utf8(t).ok()?;
            timeutil::parse_naive(s)
                .map(|n| zone.from_local(n))
                .or_else(|| timeutil::parse_absolute(s, zone))
        }
        _ => None,
    }
}

fn num_value(v: ValueRef, dur_div: f64) -> Option<f64> {
    match v {
        ValueRef::Integer(i) => Some(i as f64),
        ValueRef::Real(f) => Some(f).filter(|f| f.is_finite()),
        ValueRef::Text(t) => {
            let s = std::str::from_utf8(t).ok()?.trim();
            s.parse::<f64>()
                .ok()
                .or_else(|| parse_clock_duration(s).map(|secs| secs / dur_div))
        }
        _ => None,
    }
}

/// Import a CSV file. Wide by default (`ts` column + one series per numeric
/// column); with `series_col` set, rows are `ts, <series_col>, <value_col>`.
pub struct CsvOpts {
    pub path: String,
    pub prefix: String,
    pub ts_col: Option<String>,
    pub series_col: Option<String>,
    pub value_col: String,
    pub tz: Zone,
    pub hints: HashMap<String, MetaHint>,
}

pub fn import_csv(store: &mut Store, o: &CsvOpts) -> Result<(usize, usize)> {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_path(&o.path)
        .with_context(|| format!("opening {}", o.path))?;
    let headers: Vec<String> = rdr.headers()?.iter().map(String::from).collect();
    let ts_idx = match &o.ts_col {
        Some(c) => headers
            .iter()
            .position(|h| h == c)
            .ok_or_else(|| anyhow!("no column '{c}' (columns: {})", headers.join(", ")))?,
        None => headers
            .iter()
            .position(|h| {
                matches!(
                    h.to_ascii_lowercase().as_str(),
                    "ts" | "time" | "timestamp" | "date" | "day" | "datetime"
                )
            })
            .unwrap_or(0),
    };
    let series_idx = o
        .series_col
        .as_ref()
        .map(|c| {
            headers
                .iter()
                .position(|h| h == c)
                .ok_or_else(|| anyhow!("no column '{c}'"))
        })
        .transpose()?;
    let value_idx = match series_idx {
        Some(_) => Some(
            headers
                .iter()
                .position(|h| *h == o.value_col)
                .ok_or_else(|| anyhow!("no column '{}'", o.value_col))?,
        ),
        None => None,
    };
    let name = |raw: &str| {
        let seg = raw
            .split('.')
            .map(sanitize_segment)
            .collect::<Vec<_>>()
            .join(".");
        if o.prefix.is_empty() {
            seg
        } else {
            format!("{}.{}", o.prefix, seg)
        }
    };
    let mut hints = HashMap::new();
    for (k, h) in &o.hints {
        hints.insert(name(k), h.clone());
    }
    let mut buf = Vec::new();
    let (mut rows, mut pts) = (0, 0);
    for rec in rdr.records() {
        let rec = rec?;
        rows += 1;
        let Some(raw_ts) = rec.get(ts_idx) else {
            continue;
        };
        let ts = match raw_ts.parse::<f64>() {
            Ok(f) if f > 1e8 => {
                if f > 1e11 {
                    (f / 1000.0) as i64
                } else {
                    f as i64
                }
            }
            _ => match timeutil::parse_naive(raw_ts)
                .map(|n| o.tz.from_local(n))
                .or_else(|| timeutil::parse_absolute(raw_ts, o.tz))
            {
                Some(t) => t,
                None => continue,
            },
        };
        match (series_idx, value_idx) {
            (Some(si), Some(vi)) => {
                if let (Some(s), Some(Ok(v))) = (rec.get(si), rec.get(vi).map(|v| v.parse::<f64>()))
                {
                    buf.push((name(s), ts, v));
                }
            }
            _ => {
                for (i, h) in headers.iter().enumerate() {
                    if i == ts_idx {
                        continue;
                    }
                    if let Some(Ok(v)) = rec.get(i).map(|v| v.parse::<f64>()) {
                        buf.push((name(h), ts, v));
                    }
                }
            }
        }
        if buf.len() >= 200_000 {
            pts += store.write(&buf, &hints)?;
            buf.clear();
        }
    }
    pts += store.write(&buf, &hints)?;
    Ok((rows, pts))
}
