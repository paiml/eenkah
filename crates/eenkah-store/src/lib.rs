//! eenkah-store: one SQLite file (`eenkah.db`) plus a content-addressed archive (`raw/`).
//!
//! suyo's tables live in the same file, so every metric is also a suyo series that the charts,
//! the composer and the Graphite API can read.

#![allow(clippy::type_complexity)]

pub mod derive;
pub mod import;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use eenkah_core::Daily;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

/// Where eenkah keeps its data: `eenkah.db` and `raw/`.
#[derive(Clone, Debug)]
pub struct DataDir {
    pub root: PathBuf,
}

impl DataDir {
    /// `$EENKAH_HOME`, else `$XDG_DATA_HOME/eenkah`, else `~/.local/share/eenkah`. A data folder
    /// from before the rename (`…/seekai`) is moved there the first time.
    pub fn default_location() -> PathBuf {
        if let Ok(p) = std::env::var("EENKAH_HOME") {
            return p.into();
        }
        let base = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".local/share"));
        migrate_location(&base)
    }
    pub fn new(root: impl Into<PathBuf>) -> Result<DataDir> {
        let root = root.into();
        std::fs::create_dir_all(root.join("raw")).with_context(|| format!("creating {}", root.display()))?;
        migrate_db_file(&root)?;
        Ok(DataDir { root })
    }
    pub fn db_path(&self) -> PathBuf {
        self.root.join("eenkah.db")
    }
    pub fn raw_path(&self, sha: &str) -> PathBuf {
        self.root.join("raw").join(&sha[..2]).join(sha)
    }
    pub fn models_dir(&self) -> PathBuf {
        self.root.join("models")
    }
}

/// The product was called seekai before 2026-10-13: its default folder was `<base>/seekai`.
const LEGACY: &str = "seekai";

/// `<base>/eenkah`, moving `<base>/seekai` there first when only the old folder exists.
pub fn migrate_location(base: &Path) -> PathBuf {
    let new = base.join("eenkah");
    let old = base.join(LEGACY);
    if !new.exists() && old.is_dir() && std::fs::rename(&old, &new).is_ok() {
        eprintln!("eenkah: moved your data from {} to {} (seekai is now eenkah)", old.display(), new.display());
    }
    new
}

/// Rename `seekai.db` (and its WAL files) to `eenkah.db` when only the old file exists.
fn migrate_db_file(root: &Path) -> Result<()> {
    let new = root.join("eenkah.db");
    let old = root.join(format!("{LEGACY}.db"));
    if new.exists() || !old.exists() {
        return Ok(());
    }
    for suffix in ["", "-wal", "-shm"] {
        let (o, n) = (root.join(format!("{LEGACY}.db{suffix}")), root.join(format!("eenkah.db{suffix}")));
        if o.exists() {
            std::fs::rename(&o, &n).with_context(|| format!("renaming {} to {}", o.display(), n.display()))?;
        }
    }
    Ok(())
}

/// Series and saved dashboards from before the rename use `seekai.*` names. Idempotent: an old
/// series whose new name already exists is dropped (the new one is current), the rest renamed.
fn migrate_series_names(conn: &Connection) -> Result<()> {
    let old: i64 = conn.query_row("SELECT count(*) FROM series WHERE name LIKE 'seekai.%'", [], |r| r.get(0))?;
    let dash: i64 = conn.query_row("SELECT count(*) FROM dashboards WHERE spec LIKE '%seekai.%'", [], |r| r.get(0))?;
    if old == 0 && dash == 0 {
        return Ok(());
    }
    conn.execute_batch(
        "BEGIN;
         CREATE TEMP TABLE dup AS SELECT o.id FROM series o JOIN series n ON n.name = 'eenkah.' || substr(o.name, 8) WHERE o.name LIKE 'seekai.%';
         DELETE FROM points WHERE series_id IN (SELECT id FROM dup);
         DELETE FROM rollup_300 WHERE series_id IN (SELECT id FROM dup);
         DELETE FROM rollup_3600 WHERE series_id IN (SELECT id FROM dup);
         DELETE FROM series WHERE id IN (SELECT id FROM dup);
         DROP TABLE dup;
         UPDATE series SET name = 'eenkah.' || substr(name, 8) WHERE name LIKE 'seekai.%';
         UPDATE dashboards SET spec = replace(spec, 'seekai.', 'eenkah.') WHERE spec LIKE '%seekai.%';
         COMMIT;",
    )?;
    Ok(())
}

pub const SCHEMA: &str = "
PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
CREATE TABLE IF NOT EXISTS raw_file(
  sha TEXT PRIMARY KEY, kind TEXT NOT NULL, content TEXT NOT NULL DEFAULT '', orig_name TEXT NOT NULL,
  size INTEGER NOT NULL, first_seen INTEGER NOT NULL, parser_version INTEGER NOT NULL,
  status TEXT NOT NULL, error TEXT, unknown_json TEXT);
CREATE TABLE IF NOT EXISTS day_value(
  day TEXT NOT NULL, metric TEXT NOT NULL, prio INTEGER NOT NULL, value REAL, text TEXT, sha TEXT NOT NULL,
  PRIMARY KEY(day, metric, prio)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS activity(
  id INTEGER PRIMARY KEY, start_utc INTEGER NOT NULL, utc_offset_s INTEGER, sport TEXT, sub_sport TEXT, type_key TEXT,
  name TEXT, duration_s REAL, moving_s REAL, distance_m REAL, ascent_m REAL, avg_hr REAL, max_hr REAL,
  calories REAL, device_load REAL, lthr REAL, max_hr_setting REAL, rest_hr_setting REAL,
  sha_fit TEXT, sha_json TEXT);
CREATE INDEX IF NOT EXISTS activity_start ON activity(start_utc);
CREATE TABLE IF NOT EXISTS activity_sample(
  activity_id INTEGER NOT NULL, t INTEGER NOT NULL, hr REAL, speed REAL, power REAL, cadence REAL,
  altitude REAL, distance REAL, temp REAL, PRIMARY KEY(activity_id, t)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS activity_metric(
  activity_id INTEGER NOT NULL, metric TEXT NOT NULL, value REAL NOT NULL, PRIMARY KEY(activity_id, metric)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS streams_written(activity_id INTEGER PRIMARY KEY);
-- Athlete input. Never derived from raw/, so `rebuild` keeps it.
CREATE TABLE IF NOT EXISTS mark(
  day TEXT NOT NULL, kind TEXT NOT NULL, value REAL NOT NULL DEFAULT 1, note TEXT NOT NULL DEFAULT '',
  created INTEGER NOT NULL, PRIMARY KEY(day, kind));
CREATE TABLE IF NOT EXISTS threshold_override(
  kind TEXT NOT NULL, from_day TEXT NOT NULL, value REAL NOT NULL, note TEXT NOT NULL DEFAULT '',
  PRIMARY KEY(kind, from_day));
CREATE TABLE IF NOT EXISTS setting(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS import_run(id INTEGER PRIMARY KEY, started INTEGER NOT NULL, finished INTEGER, report_json TEXT);
";

pub struct Db {
    pub conn: Connection,
    pub dir: DataDir,
}

impl Db {
    pub fn open(dir: &DataDir) -> Result<Db> {
        let path = dir.db_path();
        // let suyo create its own tables first, in the same file
        drop(suyo::store::Store::open(path.to_str().context("non-UTF-8 data path")?)?);
        let conn = Connection::open(&path)?;
        conn.busy_timeout(std::time::Duration::from_secs(30))?;
        conn.execute_batch(SCHEMA)?;
        migrate_series_names(&conn)?;
        Ok(Db { conn, dir: dir.clone() })
    }

    pub fn suyo(&self) -> Result<suyo::store::Store> {
        suyo::store::Store::open(self.dir.db_path().to_str().context("non-UTF-8 data path")?)
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT value FROM setting WHERE key=?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute("INSERT OR REPLACE INTO setting(key, value) VALUES (?1, ?2)", params![key, value])?;
        Ok(())
    }

    /// The athlete's time zone: the `tz` setting, else this computer's zone.
    pub fn tz(&self) -> chrono_tz::Tz {
        self.setting("tz")
            .ok()
            .flatten()
            .or_else(|| iana_time_zone::get_timezone().ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(chrono_tz::UTC)
    }

    /// One daily metric, best source per day (lowest `prio`).
    pub fn daily(&self, metric: &str) -> Result<Daily> {
        let mut st = self
            .conn
            .prepare_cached("SELECT day, value FROM day_value WHERE metric=?1 AND value IS NOT NULL ORDER BY day, prio DESC")?;
        let mut out = Daily::new();
        for r in st.query_map([metric], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))? {
            let (d, v) = r?;
            if let Ok(d) = NaiveDate::parse_from_str(&d, "%Y-%m-%d") {
                out.insert(d, v); // ascending prio DESC → the lowest prio is written last and wins
            }
        }
        Ok(out)
    }

    pub fn daily_text(&self, metric: &str) -> Result<std::collections::BTreeMap<NaiveDate, String>> {
        let mut st = self
            .conn
            .prepare_cached("SELECT day, text FROM day_value WHERE metric=?1 AND text IS NOT NULL ORDER BY day, prio DESC")?;
        let mut out = std::collections::BTreeMap::new();
        for r in st.query_map([metric], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (d, v) = r?;
            if let Ok(d) = NaiveDate::parse_from_str(&d, "%Y-%m-%d") {
                out.insert(d, v);
            }
        }
        Ok(out)
    }

    pub fn marks(&self, kind: &str) -> Result<Daily> {
        let mut st = self.conn.prepare_cached("SELECT day, value FROM mark WHERE kind=?1")?;
        let mut out = Daily::new();
        for r in st.query_map([kind], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))? {
            let (d, v) = r?;
            if let Ok(d) = NaiveDate::parse_from_str(&d, "%Y-%m-%d") {
                out.insert(d, v);
            }
        }
        Ok(out)
    }

    pub fn put_mark(&self, day: NaiveDate, kind: &str, value: f64, note: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO mark(day, kind, value, note, created) VALUES (?1, ?2, ?3, ?4, strftime('%s','now'))",
            params![day.to_string(), kind, value, note],
        )?;
        Ok(())
    }

    /// Athlete override for a threshold in force on `day` (MET-3: dated, never rewrites history).
    pub fn threshold_override(&self, kind: &str, day: NaiveDate) -> Result<Option<f64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM threshold_override WHERE kind=?1 AND from_day<=?2 ORDER BY from_day DESC LIMIT 1",
                params![kind, day.to_string()],
                |r| r.get(0),
            )
            .optional()?)
    }
}

pub fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(bytes))
}

pub fn path_str(p: &Path) -> String {
    p.to_string_lossy().to_string()
}

/// What the Garmin Connect sync can skip because the archive already holds it (for example
/// files imported earlier from a folder or an export). Read from archived file names and from
/// days that already have values from the wellness FIT files.
pub fn connect_known(db: &Db) -> Result<eenkah_connect::Known> {
    use eenkah_connect::sync::{Daily as K, Ranged as R};
    let mut k = eenkah_connect::Known::default();
    let day = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok();
    let mut st = db.conn.prepare("SELECT orig_name FROM raw_file WHERE status='ok'")?;
    let names: Vec<String> = st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    for n in names {
        let base = n.rsplit(['/', '\\', '!']).next().unwrap_or(&n).to_string();
        let stem = base.trim_end_matches(".json").trim_end_matches(".fit");
        let span = |rest: &str| -> Vec<NaiveDate> {
            match rest.split_once('_') {
                Some((a, b)) => match (day(a), day(b)) {
                    (Some(a), Some(b)) => a.iter_days().take_while(|d| *d <= b).collect(),
                    _ => vec![],
                },
                None => day(rest).into_iter().collect(),
            }
        };
        if let Some(rest) = stem.strip_prefix("sleep_") {
            k.daily.entry(K::Sleep).or_default().extend(day(rest));
        } else if let Some(rest) = stem.strip_prefix("daily_summary_") {
            k.daily.entry(K::Summary).or_default().extend(day(rest));
        } else if let Some(rest) = stem.strip_prefix("rhr_") {
            k.ranged.entry(R::Rhr).or_default().extend(span(rest));
        } else if let Some(rest) = stem.strip_prefix("weight_") {
            k.ranged.entry(R::Weight).or_default().extend(span(rest));
        } else if let Some(rest) = stem.strip_prefix("hrv_") {
            k.ranged.entry(R::Hrv).or_default().extend(span(rest));
        } else if let Some(id) = stem.strip_suffix("_ACTIVITY").and_then(|s| s.parse().ok()) {
            // only the recording counts: a summary JSON alone means the FIT download failed
            k.activities.insert(id);
        }
    }
    let mut st = db
        .conn
        .prepare("SELECT DISTINCT day FROM day_value WHERE (metric='body.hrv' AND prio=0) OR (metric='sleep.score' AND prio=1)")?;
    let fit_days: Vec<String> = st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    k.daily.entry(K::Wellness).or_default().extend(fit_days.iter().filter_map(|s| day(s)));
    Ok(k)
}
