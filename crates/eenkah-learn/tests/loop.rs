#![allow(clippy::type_complexity)]

use chrono::NaiveDate;
use eenkah_store::{derive, import, DataDir, Db};

fn athlete_db(days: i64, seed: u64) -> (tempfile::TempDir, tempfile::TempDir, Db) {
    let data = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let d = eenkah_testkit::athlete(NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(), days, seed);
    eenkah_testkit::write_health_data(files.path(), &d).unwrap();
    let mut db = Db::open(&DataDir::new(data.path()).unwrap()).unwrap();
    db.set_setting("tz", "Europe/Paris").unwrap();
    import::import_path(&mut db, files.path()).unwrap();
    derive::run(&mut db).unwrap();
    (data, files, db)
}

#[test]
fn fc2_learned_model_wins_only_when_it_beats_every_baseline() {
    // The synthetic athlete's HRV drops after long sessions, which only the learned model can see.
    let (_a, _b, mut db) = athlete_db(420, 5);
    let r = eenkah_learn::train(&mut db).unwrap();
    let hrv = &r.targets[0];
    let mae = hrv["mae"].as_object().unwrap();
    let best_baseline = ["Persistence", "Mean7", "Mean60"].iter().map(|k| mae[*k].as_f64().unwrap()).fold(f64::INFINITY, f64::min);
    let champ = hrv["champion"].as_str().unwrap();
    if ["Ridge", "Forest"].contains(&champ) {
        assert!(mae[champ].as_f64().unwrap() < best_baseline);
    } else {
        assert!(mae["Ridge"].as_f64().unwrap() >= best_baseline && mae["Forest"].as_f64().unwrap() >= best_baseline);
    }
    assert!(["Ridge", "Forest"].contains(&champ), "the known signal should be learnable: {hrv}");
}

#[test]
fn fc3_intervals_cover_about_as_stated() {
    let (_a, _b, mut db) = athlete_db(420, 6);
    let r = eenkah_learn::train(&mut db).unwrap();
    for t in &r.targets {
        let c = t["coverage"].as_f64().unwrap();
        assert!((0.7..=0.9).contains(&c), "{t}");
    }
}

#[test]
fn fc6_stale_data_is_refused_not_forecast() {
    // the synthetic athlete ends in 2025, so "today" is far past its newest morning
    let (_a, _b, mut db) = athlete_db(420, 7);
    let r = eenkah_learn::train(&mut db).unwrap();
    for t in &r.targets {
        assert!(t["live"].is_null());
        assert!(t["refused"].as_str().unwrap().starts_with("data too old to forecast"));
    }
}

fn table(db: &Db, q: &str) -> Vec<String> {
    let mut st = db.conn.prepare(q).unwrap();
    let n = st.column_count();
    st.query_map([], |r| Ok((0..n).map(|i| format!("{:?}", r.get_ref(i).unwrap())).collect::<Vec<_>>().join("|")))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

#[test]
fn lrn5_same_data_same_seed_same_model_and_results() {
    let (_a, _b, mut db1) = athlete_db(300, 8);
    let (_c, _d, mut db2) = athlete_db(300, 8);
    eenkah_learn::train(&mut db1).unwrap();
    eenkah_learn::train(&mut db2).unwrap();
    for q in [
        "SELECT * FROM forecast WHERE kind='backtest' ORDER BY target, for_day",
        "SELECT target, rows, data_hash, scores_json, champion, coverage, lineage_json FROM model_run ORDER BY id",
        "SELECT * FROM day_normal ORDER BY day, signal",
        "SELECT * FROM shift ORDER BY signal, day",
        "SELECT * FROM day_unusual ORDER BY day",
    ] {
        assert_eq!(table(&db1, q), table(&db2, q), "{q}");
    }
    // the fitted model itself, byte for byte
    let inputs = eenkah_store::derive::inputs(&db1).unwrap();
    let t = eenkah_core::features::build(&inputs, eenkah_store::derive::load_model(&db1));
    let names: Vec<&str> = eenkah_core::features::FEATURES.iter().map(|f| f.0).collect();
    let (_, rows) = eenkah_learn::dataset(&inputs, &t, eenkah_learn::Target::HrvNext);
    for fam in [eenkah_learn::model::Family::Ridge, eenkah_learn::model::Family::Forest] {
        let a = serde_json::to_vec(&eenkah_learn::model::Fitted::fit(fam, &rows, &names, eenkah_learn::SEED).unwrap()).unwrap();
        let b = serde_json::to_vec(&eenkah_learn::model::Fitted::fit(fam, &rows, &names, eenkah_learn::SEED).unwrap()).unwrap();
        assert_eq!(a, b);
    }
}

#[test]
fn lrn3_no_model_file_is_stored_and_every_run_records_its_lineage() {
    let (_a, _b, mut db) = athlete_db(300, 9);
    // files from before LRN-3 changed are removed by the next run
    let legacy = db.dir.models_dir();
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(legacy.join("hrv_next-ridge-0123456789ab.apr"), b"old").unwrap();
    eenkah_learn::train(&mut db).unwrap();
    assert!(!legacy.exists());
    let lineages = table(&db, "SELECT lineage_json FROM model_run");
    assert_eq!(lineages.len(), 2 + eenkah_learn::personal::RUNS.len());
    let l: serde_json::Value = db
        .conn
        .query_row("SELECT lineage_json FROM model_run WHERE target='hrv_next'", [], |r| r.get::<_, String>(0))
        .map(|s| serde_json::from_str(&s).unwrap())
        .unwrap();
    for k in ["data_hash", "first_day", "last_day", "features", "backtest_mae", "seed", "aprender"] {
        assert!(!l[k].is_null(), "lineage lacks {k}: {l}");
    }
}

#[test]
fn lrn4_an_earlier_champion_is_rebuilt_from_the_data_up_to_its_last_day() {
    // the same athlete seen 40 days later: the first 300 days are identical
    let (_a, _b, mut early) = athlete_db(300, 10);
    let (_c, _d, later) = athlete_db(340, 10);
    eenkah_learn::train(&mut early).unwrap();
    let (hash, champion, last_day): (String, String, String) = early
        .conn
        .query_row("SELECT data_hash, champion, json_extract(lineage_json, '$.last_day') FROM model_run WHERE target='hrv_next'", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    let last_day = NaiveDate::parse_from_str(&last_day, "%Y-%m-%d").unwrap();
    let rebuild = |db: &Db| {
        let inputs = eenkah_store::derive::inputs(db).unwrap();
        let t = eenkah_core::features::build(&inputs, eenkah_store::derive::load_model(db));
        let (dates, rows) = eenkah_learn::dataset(&inputs, &t, eenkah_learn::Target::HrvNext);
        let keep = dates.iter().filter(|d| **d <= last_day).count();
        (dates[..keep].to_vec(), rows[..keep].to_vec())
    };
    let (dates, rows) = rebuild(&later);
    assert_eq!(eenkah_learn::data_hash(&dates, &rows), hash, "same data → same lineage hash");
    let (de, re) = rebuild(&early);
    let names: Vec<&str> = eenkah_core::features::FEATURES.iter().map(|f| f.0).collect();
    let fam = eenkah_learn::model::Family::Ridge;
    let a = serde_json::to_vec(&eenkah_learn::model::Fitted::fit(fam, &rows, &names, eenkah_learn::SEED).unwrap()).unwrap();
    let b = serde_json::to_vec(&eenkah_learn::model::Fitted::fit(fam, &re, &names, eenkah_learn::SEED).unwrap()).unwrap();
    assert_eq!(a, b, "the {champion} champion's family refits to the same bytes");
    assert_eq!(dates, de);
}

#[test]
fn nrm2_shf1_unu1_personal_models_run_after_training_and_are_read_back() {
    let (_a, _b, mut db) = athlete_db(420, 12);
    let r = eenkah_learn::train(&mut db).unwrap();
    assert!(r.personal["days"].as_u64().unwrap() > 300, "{}", r.personal);
    let v = eenkah_learn::personal::summary(&db).unwrap();
    let names: Vec<&str> = v["signals"].as_array().unwrap().iter().map(|s| s["signal"].as_str().unwrap()).collect();
    for s in ["hrv", "rhr", "sleep_score"] {
        assert!(names.contains(&s), "{names:?}");
    }
    // NRM-2: about 80 % of nights inside the stated 80 % range
    let cov = db.conn.query_row("SELECT coverage FROM model_run WHERE target='normal'", [], |r| r.get::<_, f64>(0)).unwrap();
    assert!((0.72..=0.88).contains(&cov), "{cov}");
    // the synthetic athlete's levels never change: no shifts; unusual days stay rare
    assert_eq!(v["shifts"].as_array().unwrap().len(), 0, "{}", v["shifts"]);
    let rate = db
        .conn
        .query_row("SELECT json_extract(scores_json, '$.flag_rate') FROM model_run WHERE target='unusual'", [], |r| r.get::<_, f64>(0))
        .unwrap();
    assert!(rate < 0.12, "{rate}");
    // synthetic rides carry no heart-rate samples, so no session type is guessed
    assert_eq!(table(&db, "SELECT * FROM session_type").len(), 0);
    assert!(eenkah_learn::model_text(&db).unwrap().contains("Last night against your normal"));
}

#[test]
fn fc1_fc5_lrn2_live_forecast_is_stored_once_with_interval_drivers_and_cost() {
    // an athlete whose newest morning is yesterday, so a live forecast is allowed
    let data = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let today = chrono::Utc::now().with_timezone(&chrono_tz::Europe::Paris).date_naive();
    let d = eenkah_testkit::athlete(today - chrono::Duration::days(400), 400, 11);
    eenkah_testkit::write_health_data(files.path(), &d).unwrap();
    let mut db = Db::open(&DataDir::new(data.path()).unwrap()).unwrap();
    db.set_setting("tz", "Europe/Paris").unwrap();
    import::import_path(&mut db, files.path()).unwrap();
    derive::run(&mut db).unwrap();
    eenkah_learn::train(&mut db).unwrap();
    let q = "SELECT for_day, value, lo, hi, model_version, data_age_days, drivers_json FROM forecast WHERE kind='live' ORDER BY target";
    let read = |db: &Db| -> Vec<(String, f64, Option<f64>, Option<f64>, String, i64, Option<String>)> {
        db.conn
            .prepare(q)
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    let first = read(&db);
    assert_eq!(first.len(), 2);
    for f in &first {
        assert!(f.2.is_some() && f.3.is_some(), "FC-1: interval stored");
        assert!(f.2.unwrap() <= f.1 && f.1 <= f.3.unwrap());
        assert!(!f.4.is_empty() && f.5 <= 1);
        let drivers: Vec<String> = serde_json::from_str(f.6.as_deref().unwrap()).unwrap();
        assert!(!drivers.is_empty(), "FC-5: drivers in plain words");
    }
    // a second run on the same day never rewrites a stored forecast
    eenkah_learn::train(&mut db).unwrap();
    assert_eq!(first, read(&db));
    let (secs, peak): (f64, Option<i64>) = db
        .conn
        .query_row("SELECT secs, peak_kb FROM model_run ORDER BY id DESC LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert!(secs > 0.0 && peak.unwrap_or(1) > 0, "LRN-2: time and memory recorded");
}
