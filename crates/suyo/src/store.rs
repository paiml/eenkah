//! SQLite storage. Raw points are kept per series; 5-minute and hourly
//! rollups store count/sum/min/max so any consolidation stays exact when a
//! query reads the coarser tables.

use crate::timeutil::{Bucket, Zone};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::HashMap;

pub const ROLLUPS: [i64; 2] = [300, 3600];

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Agg {
    pub cnt: f64,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
    pub last: f64,
}

impl Agg {
    pub fn one(v: f64) -> Agg {
        Agg {
            cnt: 1.0,
            sum: v,
            min: v,
            max: v,
            last: v,
        }
    }
    pub fn merge(&mut self, o: &Agg) {
        if self.cnt == 0.0 {
            *self = *o;
            return;
        }
        self.cnt += o.cnt;
        self.sum += o.sum;
        self.min = self.min.min(o.min);
        self.max = self.max.max(o.max);
        self.last = o.last;
    }
    pub fn value(&self, f: AggFn) -> f64 {
        match f {
            AggFn::Avg => self.sum / self.cnt,
            AggFn::Sum => self.sum,
            AggFn::Min => self.min,
            AggFn::Max => self.max,
            AggFn::Count => self.cnt,
            AggFn::Last => self.last,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AggFn {
    Avg,
    Sum,
    Min,
    Max,
    Count,
    Last,
}

impl AggFn {
    pub fn parse(s: &str) -> Result<AggFn> {
        Ok(
            match s.trim().trim_matches('"').to_ascii_lowercase().as_str() {
                "avg" | "average" | "mean" => AggFn::Avg,
                "sum" | "total" => AggFn::Sum,
                "min" | "minimum" => AggFn::Min,
                "max" | "maximum" => AggFn::Max,
                "count" | "cnt" => AggFn::Count,
                "last" | "current" => AggFn::Last,
                o => bail!("unknown aggregation '{o}' (avg, sum, min, max, count, last)"),
            },
        )
    }
    pub fn name(&self) -> &'static str {
        match self {
            AggFn::Avg => "avg",
            AggFn::Sum => "sum",
            AggFn::Min => "min",
            AggFn::Max => "max",
            AggFn::Count => "count",
            AggFn::Last => "last",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SeriesMeta {
    pub id: i64,
    pub name: String,
    pub unit: String,
    pub agg: AggFn,
    /// Native sampling interval in seconds (0 = unknown).
    pub interval: i64,
    pub description: String,
    pub first: Option<i64>,
    pub last: Option<i64>,
}

/// Optional metadata supplied alongside points on ingest.
#[derive(Clone, Debug, Default)]
pub struct MetaHint {
    pub unit: Option<String>,
    pub agg: Option<AggFn>,
    pub interval: Option<i64>,
    pub description: Option<String>,
}

pub struct Store {
    pub conn: Connection,
    id_cache: HashMap<String, i64>,
}

impl Store {
    pub fn open(path: &str) -> Result<Store> {
        let conn = Connection::open(path).with_context(|| format!("opening database {path}"))?;
        conn.busy_timeout(std::time::Duration::from_secs(30))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA temp_store=MEMORY;
             PRAGMA cache_size=-65536;
             PRAGMA mmap_size=268435456;
             CREATE TABLE IF NOT EXISTS series(
                id INTEGER PRIMARY KEY,
                name TEXT UNIQUE NOT NULL,
                unit TEXT NOT NULL DEFAULT '',
                agg TEXT NOT NULL DEFAULT 'avg',
                interval INTEGER NOT NULL DEFAULT 0,
                description TEXT NOT NULL DEFAULT '',
                first_ts INTEGER,
                last_ts INTEGER);
             CREATE TABLE IF NOT EXISTS points(
                series_id INTEGER NOT NULL, ts INTEGER NOT NULL, value REAL NOT NULL,
                PRIMARY KEY(series_id, ts)) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS rollup_300(
                series_id INTEGER NOT NULL, ts INTEGER NOT NULL,
                cnt INTEGER NOT NULL, sum REAL NOT NULL, min REAL NOT NULL, max REAL NOT NULL, last REAL NOT NULL,
                PRIMARY KEY(series_id, ts)) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS rollup_3600(
                series_id INTEGER NOT NULL, ts INTEGER NOT NULL,
                cnt INTEGER NOT NULL, sum REAL NOT NULL, min REAL NOT NULL, max REAL NOT NULL, last REAL NOT NULL,
                PRIMARY KEY(series_id, ts)) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS dashboards(
                name TEXT PRIMARY KEY, spec TEXT NOT NULL, updated INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS sync_state(
                source TEXT PRIMARY KEY, last_ts INTEGER, rows INTEGER NOT NULL DEFAULT 0, updated INTEGER NOT NULL);",
        )?;
        Ok(Store {
            conn,
            id_cache: HashMap::new(),
        })
    }

    pub fn series(&self) -> Result<Vec<SeriesMeta>> {
        let mut st = self.conn.prepare_cached(
            "SELECT id, name, unit, agg, interval, description, first_ts, last_ts FROM series ORDER BY name",
        )?;
        let rows = st.query_map([], row_meta)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn series_by_name(&self, name: &str) -> Result<Option<SeriesMeta>> {
        let mut st = self.conn.prepare_cached(
            "SELECT id, name, unit, agg, interval, description, first_ts, last_ts FROM series WHERE name=?1",
        )?;
        Ok(st.query_row([name], row_meta).optional()?)
    }

    pub fn point_count(&self, id: i64) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT count(*) FROM points WHERE series_id=?1",
            [id],
            |r| r.get(0),
        )?)
    }

    fn ensure_series(&mut self, name: &str, hint: Option<&MetaHint>) -> Result<i64> {
        if let Some(id) = self.id_cache.get(name) {
            return Ok(*id);
        }
        validate_name(name)?;
        let id: i64 = match self
            .conn
            .query_row("SELECT id FROM series WHERE name=?1", [name], |r| r.get(0))
            .optional()?
        {
            Some(id) => id,
            None => {
                self.conn
                    .execute("INSERT INTO series(name) VALUES (?1)", [name])?;
                self.conn.last_insert_rowid()
            }
        };
        if let Some(h) = hint {
            self.apply_hint(id, h)?;
        }
        self.id_cache.insert(name.to_string(), id);
        Ok(id)
    }

    fn apply_hint(&self, id: i64, h: &MetaHint) -> Result<()> {
        if let Some(u) = &h.unit {
            self.conn
                .execute("UPDATE series SET unit=?2 WHERE id=?1", params![id, u])?;
        }
        if let Some(a) = h.agg {
            self.conn.execute(
                "UPDATE series SET agg=?2 WHERE id=?1",
                params![id, a.name()],
            )?;
        }
        if let Some(i) = h.interval {
            self.conn
                .execute("UPDATE series SET interval=?2 WHERE id=?1", params![id, i])?;
        }
        if let Some(d) = &h.description {
            self.conn.execute(
                "UPDATE series SET description=?2 WHERE id=?1",
                params![id, d],
            )?;
        }
        Ok(())
    }

    pub fn set_meta(&mut self, name: &str, hint: &MetaHint) -> Result<()> {
        let id = self.ensure_series(name, None)?;
        self.apply_hint(id, hint)
    }

    /// Upsert points. `hints` may carry metadata per series name. Rollups for
    /// the touched time ranges are recomputed in the same transaction.
    pub fn write(
        &mut self,
        points: &[(String, i64, f64)],
        hints: &HashMap<String, MetaHint>,
    ) -> Result<usize> {
        if points.is_empty() {
            return Ok(0);
        }
        let mut by_series: HashMap<&str, (i64, i64)> = HashMap::new();
        for (n, ts, _) in points {
            let e = by_series.entry(n.as_str()).or_insert((*ts, *ts));
            e.0 = e.0.min(*ts);
            e.1 = e.1.max(*ts);
        }
        let mut ids = HashMap::new();
        for n in by_series.keys() {
            ids.insert(n.to_string(), self.ensure_series(n, hints.get(*n))?);
        }
        let tx = self.conn.transaction()?;
        let mut written = 0;
        {
            let mut ins = tx.prepare_cached(
                "INSERT OR REPLACE INTO points(series_id, ts, value) VALUES (?1, ?2, ?3)",
            )?;
            for (n, ts, v) in points {
                if !v.is_finite() {
                    continue;
                }
                ins.execute(params![ids[n.as_str()], ts, v])?;
                written += 1;
            }
            for (n, (lo, hi)) in &by_series {
                let id = ids[*n];
                for res in ROLLUPS {
                    let a = lo.div_euclid(res) * res;
                    let b = hi.div_euclid(res) * res + res;
                    tx.execute(
                        &format!(
                            "DELETE FROM rollup_{res} WHERE series_id=?1 AND ts>=?2 AND ts<?3"
                        ),
                        params![id, a, b],
                    )?;
                    tx.execute(
                        &format!(
                            "INSERT INTO rollup_{res}(series_id, ts, cnt, sum, min, max, last)
                             SELECT ?1, g.b, g.c, g.s, g.mn, g.mx, p.value FROM
                               (SELECT (ts/{res})*{res} AS b, count(*) c, sum(value) s, min(value) mn, max(value) mx, max(ts) lt
                                FROM points WHERE series_id=?1 AND ts>=?2 AND ts<?3 GROUP BY b) g
                             JOIN points p ON p.series_id=?1 AND p.ts=g.lt"
                        ),
                        params![id, a, b],
                    )?;
                }
                tx.execute(
                    "UPDATE series SET first_ts=min(coalesce(first_ts, ?2), ?2), last_ts=max(coalesce(last_ts, ?3), ?3) WHERE id=?1",
                    params![id, lo, hi],
                )?;
            }
        }
        tx.commit()?;
        for (n, _) in by_series {
            let id = ids[n];
            let known: i64 =
                self.conn
                    .query_row("SELECT interval FROM series WHERE id=?1", [id], |r| {
                        r.get(0)
                    })?;
            if known == 0 {
                if let Some(iv) = self.detect_interval(id)? {
                    self.conn
                        .execute("UPDATE series SET interval=?2 WHERE id=?1", params![id, iv])?;
                }
            }
        }
        Ok(written)
    }

    /// Median gap of the most recent points.
    fn detect_interval(&self, id: i64) -> Result<Option<i64>> {
        let mut st = self
            .conn
            .prepare_cached("SELECT ts FROM points WHERE series_id=?1 ORDER BY ts DESC LIMIT 64")?;
        let ts: Vec<i64> = st
            .query_map([id], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        if ts.len() < 4 {
            return Ok(None);
        }
        let mut gaps: Vec<i64> = ts
            .windows(2)
            .map(|w| w[0] - w[1])
            .filter(|g| *g > 0)
            .collect();
        gaps.sort_unstable();
        Ok(gaps.get(gaps.len() / 2).copied())
    }

    /// Fetch one series consolidated into `bucket` over [from, until).
    /// Returns one entry per bucket start (None where there is no data).
    pub fn fetch(
        &self,
        id: i64,
        from: i64,
        until: i64,
        bucket: Bucket,
        zone: Zone,
    ) -> Result<Vec<(i64, Option<Agg>)>> {
        let starts = bucket.range(from, until, zone);
        if starts.is_empty() {
            return Ok(vec![]);
        }
        let lo = starts[0];
        let hi = until.max(lo + 1);
        let width = bucket.nominal();
        // Read from the coarsest table that still lines up with bucket edges.
        let whole_hour_zone = zone.offset_secs(lo) % 3600 == 0;
        let table =
            if width >= 3600 && (bucket.is_calendar() || width % 3600 == 0) && whole_hour_zone {
                Some(3600)
            } else if width >= 300 && (bucket.is_calendar() || width % 300 == 0) {
                Some(300)
            } else {
                None
            };
        let mut out: Vec<(i64, Option<Agg>)> = starts.iter().map(|s| (*s, None)).collect();
        let mut idx = 0usize;
        let mut push = |ts: i64, a: Agg| {
            while idx + 1 < out.len() && out[idx + 1].0 <= ts {
                idx += 1;
            }
            if ts < out[idx].0 {
                return;
            }
            match &mut out[idx].1 {
                Some(e) => e.merge(&a),
                slot => *slot = Some(a),
            }
        };
        match table {
            Some(res) => {
                let mut st = self.conn.prepare_cached(&format!(
                    "SELECT ts, cnt, sum, min, max, last FROM rollup_{res} WHERE series_id=?1 AND ts>=?2 AND ts<?3 ORDER BY ts"
                ))?;
                let mut rows = st.query(params![id, lo, hi])?;
                while let Some(r) = rows.next()? {
                    let a = Agg {
                        cnt: r.get::<_, i64>(1)? as f64,
                        sum: r.get(2)?,
                        min: r.get(3)?,
                        max: r.get(4)?,
                        last: r.get(5)?,
                    };
                    push(r.get(0)?, a);
                }
            }
            None => {
                let mut st = self.conn.prepare_cached("SELECT ts, value FROM points WHERE series_id=?1 AND ts>=?2 AND ts<?3 ORDER BY ts")?;
                let mut rows = st.query(params![id, lo, hi])?;
                while let Some(r) = rows.next()? {
                    push(r.get(0)?, Agg::one(r.get(1)?));
                }
            }
        }
        Ok(out)
    }

    pub fn delete_series(&mut self, name: &str) -> Result<bool> {
        let Some(m) = self.series_by_name(name)? else {
            return Ok(false);
        };
        let tx = self.conn.transaction()?;
        for t in ["points", "rollup_300", "rollup_3600"] {
            tx.execute(&format!("DELETE FROM {t} WHERE series_id=?1"), [m.id])?;
        }
        tx.execute("DELETE FROM series WHERE id=?1", [m.id])?;
        tx.commit()?;
        self.id_cache.remove(name);
        Ok(true)
    }

    /// Drop raw points older than `before`; rollups are kept.
    pub fn prune_raw(&self, before: i64) -> Result<usize> {
        Ok(self
            .conn
            .execute("DELETE FROM points WHERE ts < ?1", [before])?)
    }

    pub fn dashboards(&self) -> Result<Vec<(String, String, i64)>> {
        let mut st = self
            .conn
            .prepare("SELECT name, spec, updated FROM dashboards ORDER BY name")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
    pub fn dashboard(&self, name: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT spec FROM dashboards WHERE name=?1", [name], |r| {
                r.get(0)
            })
            .optional()?)
    }
    pub fn put_dashboard(&self, name: &str, spec: &str) -> Result<()> {
        validate_name(name)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO dashboards(name, spec, updated) VALUES (?1, ?2, ?3)",
            params![name, spec, crate::timeutil::now()],
        )?;
        Ok(())
    }
    pub fn delete_dashboard(&self, name: &str) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM dashboards WHERE name=?1", [name])?
            > 0)
    }

    pub fn sync_state(&self, source: &str) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT last_ts FROM sync_state WHERE source=?1",
                [source],
                |r| r.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten())
    }
    pub fn set_sync_state(&self, source: &str, last_ts: Option<i64>, rows: usize) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sync_state(source, last_ts, rows, updated) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(source) DO UPDATE SET last_ts=coalesce(excluded.last_ts, last_ts), rows=rows+excluded.rows, updated=excluded.updated",
            params![source, last_ts, rows as i64, crate::timeutil::now()],
        )?;
        Ok(())
    }
}

fn row_meta(r: &rusqlite::Row) -> rusqlite::Result<SeriesMeta> {
    let agg: String = r.get(3)?;
    Ok(SeriesMeta {
        id: r.get(0)?,
        name: r.get(1)?,
        unit: r.get(2)?,
        agg: AggFn::parse(&agg).unwrap_or(AggFn::Avg),
        interval: r.get(4)?,
        description: r.get(5)?,
        first: r.get(6)?,
        last: r.get(7)?,
    })
}

/// Series and dashboard names: dotted paths of [A-Za-z0-9_-:] segments.
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 255 {
        bail!("name must be 1-255 characters");
    }
    for seg in name.split('.') {
        if seg.is_empty()
            || !seg
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':' | '@' | '%'))
        {
            bail!("invalid name '{name}': use dotted segments of letters, digits, _ - : @ %");
        }
    }
    Ok(())
}

/// Turn arbitrary text (a CSV header, a sport name) into a valid segment.
pub fn sanitize_segment(s: &str) -> String {
    let mut out: String = s
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        "unnamed".into()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeutil::Bucket;

    #[test]
    fn write_fetch_rollup_roundtrip() {
        let mut s = Store::open(":memory:").unwrap();
        let utc = Zone::parse("UTC").unwrap();
        let pts: Vec<_> = (0..7200)
            .map(|i| ("a.b".to_string(), i * 60, (i % 10) as f64))
            .collect();
        s.write(&pts, &HashMap::new()).unwrap();
        let m = s.series_by_name("a.b").unwrap().unwrap();
        assert_eq!(m.interval, 60);
        let raw = s
            .fetch(m.id, 0, 3600, Bucket::parse("1min").unwrap(), utc)
            .unwrap();
        assert_eq!(raw.len(), 60);
        let hourly = s
            .fetch(m.id, 0, 86400 * 5, Bucket::parse("1h").unwrap(), utc)
            .unwrap();
        assert_eq!(hourly[0].1.unwrap().cnt, 60.0);
        assert_eq!(hourly[0].1.unwrap().value(AggFn::Avg), 4.5);
        let daily = s
            .fetch(m.id, 0, 86400 * 5, Bucket::parse("1d").unwrap(), utc)
            .unwrap();
        assert_eq!(daily.len(), 5);
        assert_eq!(daily[0].1.unwrap().cnt, 1440.0);
        assert_eq!(daily[0].1.unwrap().max, 9.0);
        assert_eq!(daily[4].1.unwrap().cnt, 1440.0);
        let more = s
            .fetch(m.id, 0, 86400 * 6, Bucket::parse("1d").unwrap(), utc)
            .unwrap();
        assert!(more[5].1.is_none());
        // Re-writing a point replaces it and the rollup follows.
        s.write(&[("a.b".to_string(), 0, 100.0)], &HashMap::new())
            .unwrap();
        let hourly = s
            .fetch(m.id, 0, 3600, Bucket::parse("1h").unwrap(), utc)
            .unwrap();
        assert_eq!(hourly[0].1.unwrap().max, 100.0);
        assert_eq!(hourly[0].1.unwrap().cnt, 60.0);
    }

    #[test]
    fn names() {
        assert!(validate_name("health.daily.rhr").is_ok());
        assert!(validate_name("bad..name").is_err());
        assert!(validate_name("bad name").is_err());
        assert_eq!(sanitize_segment("Heart Rate (bpm)"), "heart_rate_bpm");
    }
}
