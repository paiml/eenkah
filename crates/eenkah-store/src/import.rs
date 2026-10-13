//! `eenkah import` and `eenkah rebuild`.
//!
//! Design rules:
//! - no time window: every file is identified by its SHA-256, so a file is read exactly once
//!   no matter when it was downloaded or how long ago the last import ran;
//! - nothing is silently skipped: unsupported files and unread FIT messages are counted in
//!   the report (ING-3);
//! - the database can always be rebuilt from `raw/` (ING-5).

use crate::{sha256, Db};
use anyhow::{Context, Result};
use eenkah_ingest::{classify, parse, Activity, Ctx, Kind, Parsed, Rec, PARSER_VERSION};
use rayon::prelude::*;
use rusqlite::{params, OptionalExtension, Transaction};
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

const BATCH: usize = 256;
const MAX_ERRORS_LISTED: usize = 40;

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub files_seen: usize,
    pub new: usize,
    pub unchanged: usize,
    pub reparsed: usize,
    pub failed: usize,
    pub errors: Vec<(String, String)>,
    /// What supported files turned out to hold.
    pub contents: BTreeMap<String, usize>,
    /// Files recognised but not read, by kind.
    pub not_supported: BTreeMap<String, usize>,
    /// FIT messages present in read files but not used, with counts.
    pub unread_messages: BTreeMap<String, usize>,
    pub coverage: Vec<Coverage>,
}

#[derive(Debug, Serialize)]
pub struct Coverage {
    pub what: String,
    pub first: Option<String>,
    pub last: Option<String>,
    pub days: usize,
    pub missing_days: i64,
}

struct Pending {
    name: String,
    sha: String,
    kind: Kind,
    bytes: Vec<u8>,
    reparse: bool,
}

/// Import a folder (recursively), a ZIP (including ZIPs inside it) or a single file. Idempotent.
pub fn import_path(db: &mut Db, path: &Path) -> Result<Report> {
    let mut rep = Report::default();
    let ctx = Ctx { tz: db.tz() };
    let mut batch: Vec<Pending> = vec![];
    let started = chrono::Utc::now().timestamp();
    {
        let mut sink = |name: &str, kind: Kind, bytes: Vec<u8>, rep: &mut Report, db: &mut Db| -> Result<()> {
            rep.files_seen += 1;
            let sha = sha256(&bytes);
            let known: Option<(u32, String)> = db
                .conn
                .query_row("SELECT parser_version, status FROM raw_file WHERE sha=?1", [&sha], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?;
            match known {
                Some((v, st)) if v == PARSER_VERSION && st == "ok" => rep.unchanged += 1,
                k => {
                    batch.push(Pending {
                        name: name.to_string(),
                        sha,
                        kind,
                        bytes,
                        reparse: k.is_some(),
                    });
                    if batch.len() >= BATCH {
                        flush(db, &ctx, &mut batch, rep)?;
                    }
                }
            }
            Ok(())
        };
        walk(path, &mut |name, kind, read| {
            if !kind.supported() {
                *rep.not_supported.entry(kind.name()).or_default() += 1;
                return Ok(());
            }
            let bytes = read()?;
            sink(name, kind, bytes, &mut rep, db)
        })?;
    }
    flush(db, &ctx, &mut batch, &mut rep)?;
    rep.coverage = coverage(db)?;
    db.conn.execute(
        "INSERT INTO import_run(started, finished, report_json) VALUES (?1, strftime('%s','now'), ?2)",
        params![started, serde_json::to_string(&rep)?],
    )?;
    Ok(rep)
}

type Reader<'a> = &'a mut dyn FnMut() -> Result<Vec<u8>>;

/// Visit every file under `path`, in sorted order, without reading unsupported ones.
fn walk(path: &Path, f: &mut dyn FnMut(&str, Kind, Reader) -> Result<()>) -> Result<()> {
    let meta = std::fs::metadata(path).with_context(|| format!("cannot read {}", path.display()))?;
    if meta.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(path)?.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            walk(&p, f)?;
        }
        return Ok(());
    }
    let name = path.to_string_lossy().to_string();
    if name.to_ascii_lowercase().ends_with(".zip") {
        let file = std::fs::File::open(path)?;
        return walk_zip(&name, zip::ZipArchive::new(file).with_context(|| format!("{name} is not a readable ZIP"))?, f);
    }
    f(&name, classify(&name), &mut || Ok(std::fs::read(path)?))
}

fn walk_zip<R: Read + std::io::Seek>(label: &str, mut z: zip::ZipArchive<R>, f: &mut dyn FnMut(&str, Kind, Reader) -> Result<()>) -> Result<()> {
    let mut names: Vec<(String, usize)> = (0..z.len()).filter_map(|i| z.name_for_index(i).map(|n| (n.to_string(), i))).collect();
    names.sort();
    for (n, i) in names {
        if n.ends_with('/') {
            continue;
        }
        let full = format!("{label}!{n}");
        let read_entry = |z: &mut zip::ZipArchive<R>| -> Result<Vec<u8>> {
            let mut e = z.by_index(i)?;
            let mut b = Vec::with_capacity(e.size() as usize);
            e.read_to_end(&mut b)?;
            Ok(b)
        };
        if n.to_ascii_lowercase().ends_with(".zip") {
            let inner = read_entry(&mut z)?;
            walk_zip(&full, zip::ZipArchive::new(std::io::Cursor::new(inner))?, f)?;
            continue;
        }
        f(&full, classify(&n), &mut || read_entry(&mut z))?;
    }
    Ok(())
}

fn flush(db: &mut Db, ctx: &Ctx, batch: &mut Vec<Pending>, rep: &mut Report) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let parsed: Vec<Result<Parsed>> = batch.par_iter().map(|p| parse(p.kind, &p.name, &p.bytes, ctx)).collect();
    let tx = db.conn.transaction()?;
    for (p, res) in batch.iter().zip(parsed) {
        let dest = db.dir.raw_path(&p.sha);
        if !dest.exists() {
            std::fs::create_dir_all(dest.parent().unwrap())?;
            let tmp = dest.with_extension("tmp");
            std::fs::write(&tmp, &p.bytes)?;
            std::fs::rename(&tmp, &dest)?;
        }
        let (status, err, content, unknown) = match res {
            Ok(parsed) => {
                write_recs(&tx, &p.sha, p.kind, &parsed.recs)?;
                for (k, v) in &parsed.unknown {
                    *rep.unread_messages.entry(k.clone()).or_default() += v;
                }
                *rep.contents.entry(parsed.content.clone()).or_default() += 1;
                ("ok", None, parsed.content, serde_json::to_string(&parsed.unknown).ok())
            }
            Err(e) => {
                rep.failed += 1;
                if rep.errors.len() < MAX_ERRORS_LISTED {
                    rep.errors.push((p.name.clone(), format!("{e:#}")));
                }
                ("failed", Some(format!("{e:#}")), String::new(), None)
            }
        };
        if p.reparse {
            rep.reparsed += 1;
        } else if status == "ok" {
            rep.new += 1;
        }
        tx.execute(
            "INSERT INTO raw_file(sha, kind, content, orig_name, size, first_seen, parser_version, status, error, unknown_json)
             VALUES (?1, ?2, ?3, ?4, ?5, strftime('%s','now'), ?6, ?7, ?8, ?9)
             ON CONFLICT(sha) DO UPDATE SET kind=excluded.kind, content=excluded.content, parser_version=excluded.parser_version,
               status=excluded.status, error=excluded.error, unknown_json=excluded.unknown_json",
            params![p.sha, format!("{:?}", p.kind), content, p.name, p.bytes.len() as i64, PARSER_VERSION, status, err, unknown],
        )?;
    }
    tx.commit()?;
    batch.clear();
    Ok(())
}

/// Write one file's records. Re-writing the same file replaces what it wrote before.
fn write_recs(tx: &Transaction, sha: &str, kind: Kind, recs: &[Rec]) -> Result<()> {
    tx.execute("DELETE FROM day_value WHERE sha=?1", [sha])?;
    let mut day = tx.prepare_cached("INSERT OR REPLACE INTO day_value(day, metric, prio, value, text, sha) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")?;
    for r in recs {
        match r {
            Rec::Day { day: d, metric, value, prio } => {
                day.execute(params![d.to_string(), metric, prio, value, None::<String>, sha])?;
            }
            Rec::DayText { day: d, metric, text, prio } => {
                day.execute(params![d.to_string(), metric, prio, None::<f64>, text, sha])?;
            }
            Rec::Activity(a) => upsert_activity(tx, a.as_ref(), sha, kind == Kind::Fit)?,
            Rec::Samples { activity_id, samples } => {
                tx.execute("DELETE FROM activity_sample WHERE activity_id=?1", [activity_id])?;
                tx.execute("DELETE FROM streams_written WHERE activity_id=?1", [activity_id])?;
                let mut ins = tx.prepare_cached("INSERT OR REPLACE INTO activity_sample(activity_id, t, hr, speed, power, cadence, altitude, distance, temp) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)")?;
                for s in samples {
                    ins.execute(params![activity_id, s.t, s.hr, s.speed, s.power, s.cadence, s.altitude, s.distance, s.temp])?;
                }
            }
        }
    }
    Ok(())
}

/// FIT files are the measurement; the JSON list entry adds the name, type and device load.
/// Each source only fills what the other left empty, so the order of import does not matter.
fn upsert_activity(tx: &Transaction, a: &Activity, sha: &str, from_fit: bool) -> Result<()> {
    let (mine, theirs) = if from_fit { ("excluded.", "") } else { ("", "excluded.") };
    let pick = |c: &str| format!("{c}=COALESCE({mine}{c}, {theirs}{c})");
    let fit_cols = [
        "start_utc",
        "utc_offset_s",
        "sport",
        "sub_sport",
        "duration_s",
        "moving_s",
        "distance_m",
        "ascent_m",
        "avg_hr",
        "max_hr",
        "calories",
        "lthr",
        "max_hr_setting",
        "rest_hr_setting",
    ];
    let json_cols = ["name", "type_key", "device_load"];
    let mut sets: Vec<String> = fit_cols.iter().map(|c| pick(c)).collect();
    sets.extend(json_cols.iter().map(|c| format!("{c}=COALESCE(excluded.{c}, {c})")));
    sets.push(if from_fit { "sha_fit=excluded.sha_fit".into() } else { "sha_json=excluded.sha_json".into() });
    let sql = format!(
        "INSERT INTO activity(id, start_utc, utc_offset_s, sport, sub_sport, type_key, name, duration_s, moving_s, distance_m, ascent_m,
           avg_hr, max_hr, calories, device_load, lthr, max_hr_setting, rest_hr_setting, sha_fit, sha_json)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)
         ON CONFLICT(id) DO UPDATE SET {}",
        sets.join(", ")
    );
    let (sf, sj) = if from_fit { (Some(sha), None) } else { (None, Some(sha)) };
    tx.execute(
        &sql,
        params![
            a.id,
            a.start_utc,
            a.utc_offset_s,
            a.sport,
            a.sub_sport,
            a.type_key,
            a.name,
            a.duration_s,
            a.moving_s,
            a.distance_m,
            a.ascent_m,
            a.avg_hr,
            a.max_hr,
            a.calories,
            a.device_load,
            a.lthr,
            a.max_hr_setting,
            a.rest_hr_setting,
            sf,
            sj
        ],
    )?;
    Ok(())
}

/// ING-5: drop everything derived from `raw/` and re-read every archived file in a fixed
/// order. Marks and threshold overrides are athlete input and are kept.
pub fn rebuild(db: &mut Db) -> Result<Report> {
    db.conn
        .execute_batch("DELETE FROM day_value; DELETE FROM activity; DELETE FROM activity_sample; DELETE FROM activity_metric; DELETE FROM streams_written;")?;
    crate::derive::drop_series(db)?;
    let files: Vec<(String, String)> = {
        let mut st = db.conn.prepare("SELECT sha, orig_name FROM raw_file ORDER BY orig_name, sha")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        rows
    };
    let ctx = Ctx { tz: db.tz() };
    let mut rep = Report::default();
    let mut batch = vec![];
    for (sha, name) in files {
        rep.files_seen += 1;
        let bytes = std::fs::read(db.dir.raw_path(&sha)).with_context(|| format!("archived file {sha} ({name}) is missing from raw/"))?;
        batch.push(Pending {
            kind: classify(&name),
            name,
            sha,
            bytes,
            reparse: true,
        });
        if batch.len() >= BATCH {
            flush(db, &ctx, &mut batch, &mut rep)?;
        }
    }
    flush(db, &ctx, &mut batch, &mut rep)?;
    rep.coverage = coverage(db)?;
    Ok(rep)
}

const COVERED: &[(&str, &str)] = &[
    ("body.hrv", "Overnight HRV"),
    ("body.rhr", "Resting heart rate"),
    ("sleep.hours", "Sleep"),
    ("sleep.score", "Sleep score"),
    ("body.bb_wake", "Body Battery at wake"),
    ("body.skin_temp_dev", "Skin temperature"),
    ("body.weight", "Weight"),
];

/// ING-4: per data type, the date range covered and the days missing inside it.
pub fn coverage(db: &Db) -> Result<Vec<Coverage>> {
    let mut out = vec![];
    for (metric, label) in COVERED {
        let d = db.daily(metric)?;
        let (first, last) = (d.keys().next().copied(), d.keys().next_back().copied());
        let span = match (first, last) {
            (Some(a), Some(b)) => (b - a).num_days() + 1,
            _ => 0,
        };
        out.push(Coverage {
            what: label.to_string(),
            first: first.map(|x| x.to_string()),
            last: last.map(|x| x.to_string()),
            days: d.len(),
            missing_days: span - d.len() as i64,
        });
    }
    let (n, a, b): (i64, Option<i64>, Option<i64>) = db
        .conn
        .query_row("SELECT count(*), min(start_utc), max(start_utc) FROM activity", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    let day = |t: Option<i64>| t.and_then(|t| chrono::DateTime::from_timestamp(t, 0)).map(|d| d.date_naive().to_string());
    out.push(Coverage {
        what: "Activities".into(),
        first: day(a),
        last: day(b),
        days: n as usize,
        missing_days: 0,
    });
    Ok(out)
}
