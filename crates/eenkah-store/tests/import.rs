use chrono::NaiveDate;
use eenkah_store::{derive, import, DataDir, Db};

fn setup() -> (tempfile::TempDir, tempfile::TempDir, Db) {
    let data = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let days = eenkah_testkit::athlete(NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(), 150, 1);
    eenkah_testkit::write_health_data(files.path(), &days).unwrap();
    let db = Db::open(&DataDir::new(data.path()).unwrap()).unwrap();
    db.set_setting("tz", "Europe/Paris").unwrap();
    (data, files, db)
}

fn dump(db: &Db, sql: &str) -> String {
    let mut st = db.conn.prepare(sql).unwrap();
    let n = st.column_count();
    let rows = st.query_map([], |r| Ok((0..n).map(|i| format!("{:?}", r.get_ref(i).unwrap())).collect::<Vec<_>>().join("|"))).unwrap();
    rows.map(|r| r.unwrap()).collect::<Vec<_>>().join("\n")
}

#[test]
fn ing1_import_is_idempotent() {
    let (_d, files, mut db) = setup();
    let r1 = import::import_path(&mut db, files.path()).unwrap();
    assert!(r1.new > 300, "{r1:?}");
    let before = dump(&db, "SELECT * FROM day_value ORDER BY day, metric, prio");
    let r2 = import::import_path(&mut db, files.path()).unwrap();
    assert_eq!((r2.new, r2.reparsed, r2.unchanged), (0, 0, r1.new));
    assert_eq!(before, dump(&db, "SELECT * FROM day_value ORDER BY day, metric, prio"));
}

#[test]
fn ing2_every_row_names_its_file_and_the_file_is_archived() {
    let (_d, files, mut db) = setup();
    import::import_path(&mut db, files.path()).unwrap();
    let orphans: i64 = db
        .conn
        .query_row("SELECT count(*) FROM day_value WHERE sha NOT IN (SELECT sha FROM raw_file)", [], |r| r.get(0))
        .unwrap();
    assert_eq!(orphans, 0);
    let shas: Vec<String> = db.conn.prepare("SELECT sha FROM raw_file").unwrap().query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
    for s in shas {
        let b = std::fs::read(db.dir.raw_path(&s)).unwrap();
        assert_eq!(eenkah_store::sha256(&b), s);
    }
}

#[test]
fn ing3_unknown_files_are_reported_not_dropped() {
    let (_d, files, mut db) = setup();
    std::fs::write(files.path().join("notes.txt"), "hello").unwrap();
    std::fs::write(files.path().join("Sleep/sleep_broken.json"), "{oops").unwrap();
    let r = import::import_path(&mut db, files.path()).unwrap();
    assert_eq!(r.not_supported.get("Unknown"), Some(&1));
    assert_eq!(r.failed, 1);
    assert!(r.errors[0].0.ends_with("sleep_broken.json"));
}

#[test]
fn ing4_coverage_reports_range_and_gaps() {
    let (_d, files, mut db) = setup();
    std::fs::remove_file(files.path().join("RHR/rhr_2025-02-01.json")).unwrap();
    let r = import::import_path(&mut db, files.path()).unwrap();
    let rhr = r.coverage.iter().find(|c| c.what == "Resting heart rate").unwrap();
    assert_eq!((rhr.first.as_deref(), rhr.missing_days), (Some("2025-01-01"), 1));
}

#[test]
fn ing5_rebuild_yields_identical_tables() {
    let (_d, files, mut db) = setup();
    import::import_path(&mut db, files.path()).unwrap();
    derive::run(&mut db).unwrap();
    let q = [
        "SELECT * FROM day_value ORDER BY day, metric, prio",
        "SELECT * FROM activity ORDER BY id",
        "SELECT * FROM activity_metric ORDER BY activity_id, metric",
        "SELECT s.name, p.ts, p.value FROM points p JOIN series s ON s.id = p.series_id WHERE s.name LIKE 'eenkah.%' ORDER BY s.name, p.ts",
    ];
    let before: Vec<String> = q.iter().map(|s| dump(&db, s)).collect();
    import::rebuild(&mut db).unwrap();
    derive::run(&mut db).unwrap();
    for (s, b) in q.iter().zip(before) {
        assert_eq!(b, dump(&db, s), "differs after rebuild: {s}");
    }
}

#[test]
fn ing6_sleep_belongs_to_the_morning_it_ends() {
    let (_d, files, mut db) = setup();
    import::import_path(&mut db, files.path()).unwrap();
    let first: String = db.conn.query_row("SELECT min(day) FROM day_value WHERE metric='sleep.hours'", [], |r| r.get(0)).unwrap();
    assert_eq!(first, "2025-01-01");
}

#[test]
fn met1_derived_series_have_no_invented_days() {
    let (_d, files, mut db) = setup();
    std::fs::remove_file(files.path().join("Sleep/sleep_2025-03-03.json")).unwrap();
    import::import_path(&mut db, files.path()).unwrap();
    derive::run(&mut db).unwrap();
    let ts = derive::local_midnight(chrono_tz::Europe::Paris, NaiveDate::from_ymd_opt(2025, 3, 3).unwrap());
    let n: i64 = db
        .conn
        .query_row(
            "SELECT count(*) FROM points p JOIN series s ON s.id=p.series_id WHERE s.name='eenkah.body.hrv' AND p.ts=?1",
            [ts],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);
}

#[test]
fn met3_threshold_override_is_dated_and_never_rewrites_earlier_days() {
    let (_d, files, mut db) = setup();
    import::import_path(&mut db, files.path()).unwrap();
    derive::run(&mut db).unwrap();
    let load = |db: &Db| -> Vec<(i64, f64)> {
        db.conn
            .prepare("SELECT activity_id, value FROM activity_metric WHERE metric='load' ORDER BY activity_id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    let before = load(&db);
    db.conn
        .execute("INSERT INTO threshold_override(kind, from_day, value) VALUES ('max_hr', '2025-04-01', 200)", [])
        .unwrap();
    derive::run(&mut db).unwrap();
    let after = load(&db);
    let starts: std::collections::HashMap<i64, i64> = db
        .conn
        .prepare("SELECT id, start_utc FROM activity")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    let cut = chrono::NaiveDate::from_ymd_opt(2025, 4, 1).unwrap().and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();
    for ((id, a), (_, b)) in before.iter().zip(&after) {
        if starts[id] < cut - 86400 {
            assert_eq!(a, b, "activity {id} before the override changed");
        }
    }
    assert!(before.iter().zip(&after).any(|(a, b)| a.1 != b.1), "override had no effect after its date");
}

#[test]
fn view3_marks_become_a_series() {
    let (_d, files, mut db) = setup();
    import::import_path(&mut db, files.path()).unwrap();
    db.put_mark(chrono::NaiveDate::from_ymd_opt(2025, 2, 3).unwrap(), "sick", 1.0, "").unwrap();
    derive::run(&mut db).unwrap();
    assert!(db.suyo().unwrap().series_by_name("eenkah.mark.sick").unwrap().is_some());
}

#[test]
fn met5_athlete_series_work_next_to_eenkah_series() {
    // a professional adds lactate through suyo ingest; it is queryable like any eenkah series
    let (_d, files, mut db) = setup();
    import::import_path(&mut db, files.path()).unwrap();
    derive::run(&mut db).unwrap();
    let mut s = db.suyo().unwrap();
    let ts = derive::local_midnight(chrono_tz::Europe::Paris, NaiveDate::from_ymd_opt(2025, 3, 1).unwrap());
    s.write(&[("lab.lactate".into(), ts, 2.1)], &Default::default()).unwrap();
    let p = suyo::render::Params(vec![
        ("target".into(), "group(lab.lactate, eenkah.body.rhr)".into()),
        ("from".into(), "2025-02-25".into()),
        ("until".into(), "2025-03-05".into()),
    ]);
    let r = suyo::render::run(&s, &p, suyo::timeutil::Zone::Named(chrono_tz::Europe::Paris)).unwrap();
    assert_eq!(r.series.len(), 2);
}

#[test]
fn connect_sync_skips_what_the_archive_already_has() {
    // a day already imported (e.g. from a folder or an export) is never requested again
    let (_d, files, mut db) = setup();
    import::import_path(&mut db, files.path()).unwrap();
    let k = eenkah_store::connect_known(&db).unwrap();
    use eenkah_connect::sync::{Daily, Ranged};
    let day = NaiveDate::from_ymd_opt(2025, 2, 1).unwrap();
    assert!(k.daily[&Daily::Sleep].contains(&day));
    assert!(k.ranged[&Ranged::Rhr].contains(&day));
    // a summary alone is not enough: the recording is fetched until its FIT file is here
    assert!(!k.activities.contains(&10_001));
    db.conn
        .execute(
            "INSERT INTO raw_file VALUES ('feed', 'fit', 'activity', 'Activities/10001_ACTIVITY.fit', 1, 0, 1, 'ok', NULL, NULL)",
            [],
        )
        .unwrap();
    assert!(eenkah_store::connect_known(&db).unwrap().activities.contains(&10_001));
}

#[test]
fn fc4_a_sessions_load_does_not_change_when_later_sessions_arrive() {
    // max heart rate and resting heart rate fall back to estimates from the data; those may
    // use only what was known by the session's day
    let build = |n: i64| {
        let data = tempfile::tempdir().unwrap();
        let files = tempfile::tempdir().unwrap();
        let mut days = eenkah_testkit::athlete(NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(), n, 2);
        if n > 100 {
            // a much harder session after day 100 raises the observed maximum heart rate
            days[120].session = Some((9.0 * 60.0, 60.0, 170.0));
        }
        eenkah_testkit::write_health_data(files.path(), &days).unwrap();
        let mut db = Db::open(&DataDir::new(data.path()).unwrap()).unwrap();
        db.set_setting("tz", "Europe/Paris").unwrap();
        import::import_path(&mut db, files.path()).unwrap();
        derive::run(&mut db).unwrap();
        (data, files, db)
    };
    let (_a, _b, early) = build(100);
    let (_c, _d, later) = build(150);
    let q = "SELECT m.activity_id, m.metric, m.value FROM activity_metric m JOIN activity a ON a.id=m.activity_id \
             WHERE a.start_utc < (SELECT max(start_utc) FROM activity) ORDER BY 1, 2";
    let e = dump(&early, q);
    let l = dump(&later, q);
    assert!(!e.is_empty());
    assert!(l.starts_with(&e), "metrics of the first 100 days changed when later sessions arrived");
}

#[test]
fn a_seekai_data_folder_becomes_an_eenkah_one_with_its_series_and_dashboards() {
    // the folder: <base>/seekai → <base>/eenkah, once
    let base = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(base.path().join("seekai/raw")).unwrap();
    std::fs::write(base.path().join("seekai/connect-tokens"), b"kept").unwrap();
    let root = seekai_dir_moved(base.path());
    assert_eq!(root, base.path().join("eenkah"));
    assert!(!base.path().join("seekai").exists());
    assert_eq!(std::fs::read(root.join("connect-tokens")).unwrap(), b"kept");

    // the database: seekai.db → eenkah.db; series and saved dashboards renamed
    let old = root.join("seekai.db");
    {
        let mut s = suyo::store::Store::open(old.to_str().unwrap()).unwrap();
        let hints = std::collections::HashMap::new();
        s.write(&[("seekai.body.rhr".to_string(), 86400, 52.0)], &hints).unwrap();
        drop(s);
        let c = rusqlite::Connection::open(&old).unwrap();
        c.execute(
            "INSERT INTO dashboards(name, spec, updated) VALUES ('today', ?1, 0)",
            [r#"{"panels":[{"targets":["alias(seekai.body.rhr, \"RHR\")"]}]}"#],
        )
        .unwrap();
    }
    let db = Db::open(&DataDir::new(&root).unwrap()).unwrap();
    assert!(root.join("eenkah.db").exists() && !old.exists());
    let one = |q: &str| db.conn.query_row(q, [], |r| r.get::<_, String>(0)).unwrap();
    assert_eq!(one("SELECT group_concat(name) FROM series"), "eenkah.body.rhr");
    assert_eq!(db.conn.query_row("SELECT count(*) FROM points", [], |r| r.get::<_, i64>(0)).unwrap(), 1, "the series keeps its points");
    assert!(one("SELECT spec FROM dashboards").contains("alias(eenkah.body.rhr"));
    // running again changes nothing
    drop(db);
    let db = Db::open(&DataDir::new(&root).unwrap()).unwrap();
    assert_eq!(db.conn.query_row("SELECT group_concat(name) FROM series", [], |r| r.get::<_, String>(0)).unwrap(), "eenkah.body.rhr");
}

fn seekai_dir_moved(base: &std::path::Path) -> std::path::PathBuf {
    let a = eenkah_store::migrate_location(base);
    assert_eq!(eenkah_store::migrate_location(base), a, "idempotent");
    a
}
