use eenkah_api::jobs::{self, JobState};
use eenkah_store::{import, DataDir, Db};
use std::sync::{Arc, Mutex};

fn athlete() -> (tempfile::TempDir, tempfile::TempDir, DataDir, Db) {
    let data = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let days = eenkah_testkit::athlete(chrono::NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(), 300, 3);
    eenkah_testkit::write_health_data(files.path(), &days).unwrap();
    let dir = DataDir::new(data.path()).unwrap();
    let mut db = Db::open(&dir).unwrap();
    db.set_setting("tz", "Europe/Paris").unwrap();
    import::import_path(&mut db, files.path()).unwrap();
    (data, files, dir, db)
}

#[test]
fn lrn2_retrain_rebuilds_every_model_from_stored_data_and_results_survive_a_restart() {
    let (_a, _b, dir, mut db) = athlete();
    let job: jobs::Job = Arc::new(Mutex::new(JobState::default()));
    let r = jobs::retrain_unless_running(&job, &mut db).unwrap();
    assert_eq!(r["train"].as_array().unwrap().len(), 2);
    assert!(r["personal"]["days"].as_u64().unwrap() > 200, "{r}");
    let before = (eenkah_learn::model_summary(&db).unwrap(), eenkah_learn::personal::summary(&db).unwrap());
    drop(db);
    // a server restart opens the same file: every result is read back, nothing is retrained
    let db = Db::open(&dir).unwrap();
    let runs: i64 = db.conn.query_row("SELECT count(*) FROM model_run", [], |r| r.get(0)).unwrap();
    assert_eq!((eenkah_learn::model_summary(&db).unwrap(), eenkah_learn::personal::summary(&db).unwrap()), before);
    let after: i64 = db.conn.query_row("SELECT count(*) FROM model_run", [], |r| r.get(0)).unwrap();
    assert_eq!(runs, after);
}

#[test]
fn retrain_is_refused_while_an_import_runs() {
    let (_a, _b, _dir, mut db) = athlete();
    let job: jobs::Job = Arc::new(Mutex::new(JobState { running: true, ..Default::default() }));
    let e = jobs::retrain_unless_running(&job, &mut db).unwrap_err();
    assert!(format!("{e}").contains("an import is running"));
}
