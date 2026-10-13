//! Write paths: JSON, Graphite plaintext (`path value timestamp`), and a
//! carbon-compatible TCP listener so existing Graphite senders work as-is.

use crate::store::{AggFn, MetaHint, Store};
use crate::timeutil::{self, Zone};
use anyhow::{anyhow, bail, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::time::{Duration, Instant};

pub type Batch = (Vec<(String, i64, f64)>, HashMap<String, MetaHint>);

/// Timestamps may be epoch seconds, epoch milliseconds or a date string.
pub fn parse_ts(v: &Value, zone: Zone) -> Result<i64> {
    match v {
        Value::Number(n) => {
            let f = n.as_f64().ok_or_else(|| anyhow!("bad timestamp"))?;
            Ok(if f > 1e11 {
                (f / 1000.0) as i64
            } else {
                f as i64
            })
        }
        Value::String(s) => timeutil::parse_time(s, timeutil::now(), zone, false),
        Value::Null => Ok(timeutil::now()),
        _ => bail!("bad timestamp {v}"),
    }
}

fn hint_from(o: &serde_json::Map<String, Value>) -> Result<Option<MetaHint>> {
    let unit = o.get("unit").and_then(|v| v.as_str()).map(String::from);
    let agg = o
        .get("agg")
        .and_then(|v| v.as_str())
        .map(AggFn::parse)
        .transpose()?;
    let interval = o
        .get("interval")
        .and_then(|v| v.as_str())
        .map(|s| timeutil::Span::parse(s).map(|sp| sp.approx_secs()))
        .transpose()?;
    let description = o
        .get("description")
        .and_then(|v| v.as_str())
        .map(String::from);
    Ok(
        (unit.is_some() || agg.is_some() || interval.is_some() || description.is_some()).then_some(
            MetaHint {
                unit,
                agg,
                interval,
                description,
            },
        ),
    )
}

/// Accepts any of:
/// `{"series": "a.b", "ts": 1700000000, "value": 1}`
/// `{"series": "a.b", "points": [[ts, value], ...], "unit": "bpm", "agg": "sum"}`
/// or an array of those objects.
pub fn parse_json(body: &str, zone: Zone) -> Result<Batch> {
    let v: Value = serde_json::from_str(body)?;
    let items = match v {
        Value::Array(a) => a,
        o @ Value::Object(_) => vec![o],
        _ => bail!("expected a JSON object or array"),
    };
    let mut pts = Vec::new();
    let mut hints = HashMap::new();
    for it in items {
        let o = it
            .as_object()
            .ok_or_else(|| anyhow!("each item must be an object"))?;
        let name = o
            .get("series")
            .or_else(|| o.get("name"))
            .or_else(|| o.get("target"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("item missing \"series\""))?
            .to_string();
        crate::store::validate_name(&name)?;
        if let Some(h) = hint_from(o)? {
            hints.insert(name.clone(), h);
        }
        if let Some(points) = o
            .get("points")
            .or_else(|| o.get("datapoints"))
            .and_then(|v| v.as_array())
        {
            for p in points {
                let arr = p
                    .as_array()
                    .ok_or_else(|| anyhow!("points must be [ts, value] pairs"))?;
                if arr.len() != 2 {
                    bail!("points must be [ts, value] pairs");
                }
                // Accept Graphite's [value, ts] order when the first item is clearly not a timestamp.
                let (t, val) = if arr[1].as_f64().is_some_and(|x| x > 1e9)
                    && arr[0].as_f64().is_some_and(|x| x < 1e9)
                {
                    (&arr[1], &arr[0])
                } else {
                    (&arr[0], &arr[1])
                };
                if let Some(x) = val.as_f64() {
                    pts.push((name.clone(), parse_ts(t, zone)?, x));
                }
            }
        } else {
            let x = o
                .get("value")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| anyhow!("item for {name} missing numeric \"value\""))?;
            let t = parse_ts(
                o.get("ts")
                    .or_else(|| o.get("timestamp"))
                    .unwrap_or(&Value::Null),
                zone,
            )?;
            pts.push((name, t, x));
        }
    }
    Ok((pts, hints))
}

/// Graphite plaintext: `metric.path value [timestamp]` per line.
pub fn parse_plaintext_line(line: &str) -> Result<Option<(String, i64, f64)>> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let mut it = line.split_whitespace();
    let name = it.next().unwrap().to_string();
    crate::store::validate_name(&name)?;
    let value: f64 = it
        .next()
        .ok_or_else(|| anyhow!("missing value in '{line}'"))?
        .parse()
        .map_err(|_| anyhow!("bad value in '{line}'"))?;
    let ts = match it.next() {
        Some(t) if t != "-1" && t != "N" => {
            let f: f64 = t
                .parse()
                .map_err(|_| anyhow!("bad timestamp in '{line}'"))?;
            if f > 1e11 {
                (f / 1000.0) as i64
            } else {
                f as i64
            }
        }
        _ => timeutil::now(),
    };
    Ok(Some((name, ts, value)))
}

pub fn parse_plaintext(body: &str) -> Result<Vec<(String, i64, f64)>> {
    let mut out = Vec::new();
    for (i, line) in body.lines().enumerate() {
        match parse_plaintext_line(line) {
            Ok(Some(p)) => out.push(p),
            Ok(None) => {}
            Err(e) => bail!("line {}: {e}", i + 1),
        }
    }
    Ok(out)
}

/// Carbon plaintext listener. Each connection buffers and flushes every
/// second or 5000 points.
pub fn spawn_carbon(addr: String, db: String) -> Result<()> {
    let listener = TcpListener::bind(&addr)?;
    eprintln!("carbon plaintext listener on {addr}");
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let db = db.clone();
            std::thread::spawn(move || {
                let Ok(mut store) = Store::open(&db) else {
                    return;
                };
                let _ = conn.set_read_timeout(Some(Duration::from_secs(1)));
                let mut reader = BufReader::new(conn);
                let mut buf = Vec::new();
                let mut last = Instant::now();
                let mut line = String::new();
                loop {
                    line.clear();
                    let done = match reader.read_line(&mut line) {
                        Ok(0) => true,
                        Ok(_) => {
                            match parse_plaintext_line(&line) {
                                Ok(Some(p)) => buf.push(p),
                                Ok(None) => {}
                                Err(e) => eprintln!("carbon: {e}"),
                            }
                            false
                        }
                        Err(e)
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) =>
                        {
                            false
                        }
                        Err(_) => true,
                    };
                    if !buf.is_empty()
                        && (done || buf.len() >= 5000 || last.elapsed() > Duration::from_secs(1))
                    {
                        if let Err(e) = store.write(&buf, &HashMap::new()) {
                            eprintln!("carbon write failed: {e:#}");
                        }
                        buf.clear();
                        last = Instant::now();
                    }
                    if done {
                        break;
                    }
                }
            });
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_shapes() {
        let z = Zone::parse("UTC").unwrap();
        let (p, h) = parse_json(r#"{"series":"a.b","ts":1700000000,"value":2}"#, z).unwrap();
        assert_eq!(p, vec![("a.b".into(), 1700000000, 2.0)]);
        assert!(h.is_empty());
        let (p, h) = parse_json(r#"[{"series":"x.y","unit":"bpm","agg":"max","points":[[1700000000000, 1],[1700000060, 2]]}]"#, z).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].1, 1700000000);
        assert_eq!(h["x.y"].unit.as_deref(), Some("bpm"));
        let (p, _) = parse_json(r#"{"series":"g.r","datapoints":[[5, 1700000000]]}"#, z).unwrap();
        assert_eq!(p[0], ("g.r".into(), 1700000000, 5.0));
        let (p, _) = parse_json(r#"{"series":"d.t","ts":"2026-01-01","value":3}"#, z).unwrap();
        assert_eq!(p[0].1, 1767225600);
        assert!(parse_json(r#"{"series":"bad name","value":1}"#, z).is_err());
    }

    #[test]
    fn plaintext() {
        let p = parse_plaintext(
            "# comment\nhost.a.cpu 12.5 1700000000\n\nhost.a.mem 3 1700000000000\n",
        )
        .unwrap();
        assert_eq!(
            p,
            vec![
                ("host.a.cpu".into(), 1700000000, 12.5),
                ("host.a.mem".into(), 1700000000, 3.0)
            ]
        );
        assert!(parse_plaintext("x.y notanumber 1").is_err());
    }
}
