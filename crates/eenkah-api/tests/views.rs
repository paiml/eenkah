use eenkah_api::{seed_dashboards, DASHBOARDS, DEFAULT_LISTEN};
use eenkah_store::{DataDir, Db};

#[test]
fn prv1_binds_to_loopback_by_default() {
    assert!(DEFAULT_LISTEN.starts_with("127.0.0.1:"));
}

#[test]
fn view1_every_view_states_its_question() {
    for (name, spec) in DASHBOARDS {
        let v: serde_json::Value = serde_json::from_str(spec).unwrap();
        assert!(v["title"].as_str().unwrap().ends_with('?'), "{name}");
    }
}

#[test]
fn view2_every_chart_is_a_suyo_chart_with_a_data_view() {
    // charts are drawn only by suyo; each type is one suyo's typed ChartType union declares
    // (every suyo chart has the Data table view)
    let types = include_str!("../../../../suyo/ui/src/lib/types.ts");
    let union = &types[types.find("export type ChartType").unwrap()..];
    let union = &union[..union.find(';').unwrap()];
    for (name, spec) in DASHBOARDS {
        let v: serde_json::Value = serde_json::from_str(spec).unwrap();
        for p in v["panels"].as_array().unwrap() {
            let c = p["chart"].as_str().unwrap_or("line");
            assert!(union.contains(&format!("\"{c}\"")), "{name}: {c}");
        }
    }
}

#[test]
fn cus1_cus2_dashboards_round_trip_and_restore_by_name() {
    let d = tempfile::tempdir().unwrap();
    let db = Db::open(&DataDir::new(d.path()).unwrap()).unwrap();
    assert_eq!(seed_dashboards(&db, &[]).unwrap().len(), DASHBOARDS.len());
    let store = db.suyo().unwrap();
    // the athlete edits and deletes defaults; an upgrade (re-seed) keeps the edit
    store.put_dashboard("today", r#"{"title":"Mine?","panels":[]}"#).unwrap();
    store.delete_dashboard("week").unwrap();
    assert_eq!(seed_dashboards(&db, &[]).unwrap(), vec!["week".to_string()]);
    assert!(store.dashboard("today").unwrap().unwrap().contains("Mine?"));
    // restore by name
    seed_dashboards(&db, &["today"]).unwrap();
    let exported = store.dashboard("today").unwrap().unwrap();
    assert_eq!(exported, DASHBOARDS[0].1);
    // export → import round trip is byte-identical JSON
    store.put_dashboard("copy", &exported).unwrap();
    assert_eq!(store.dashboard("copy").unwrap().unwrap(), exported);
}

#[test]
fn ins3_product_is_complete_without_a_language_model() {
    let d = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let days = eenkah_testkit::athlete(chrono::NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(), 120, 2);
    eenkah_testkit::write_health_data(files.path(), &days).unwrap();
    let mut db = Db::open(&DataDir::new(d.path()).unwrap()).unwrap();
    eenkah_store::import::import_path(&mut db, files.path()).unwrap();
    eenkah_store::derive::run(&mut db).unwrap();
    let t = eenkah_api::views::brief_text(&db).unwrap();
    assert!(t.contains("Not medical advice") && t.contains("Newest data: 1 year ago (2025-04-30)"), "{t}");
    assert!(t.contains("too old to forecast"));
}

#[test]
fn met4_catalogue_is_generated_from_code_and_docs_match() {
    let v = eenkah_api::views::metric_catalog();
    assert_eq!(v.as_array().unwrap().len(), eenkah_core::catalog::METRICS.len());
    let doc = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/METRICS.md")).unwrap();
    assert_eq!(doc, eenkah_api::metrics_markdown(), "docs/METRICS.md is stale: run `eenkah metrics --markdown > docs/METRICS.md`");
}

#[test]
fn mcp3_readme_says_where_health_data_goes() {
    let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../README.md")).unwrap();
    assert!(readme.contains("sends the returned health data to that client's model provider"));
    assert!(readme.contains("eenkah itself sends nothing"));
}

#[test]
fn view4_today_reads_cached_results_and_runs_no_model() {
    let d = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let days = eenkah_testkit::athlete(chrono::NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(), 200, 4);
    eenkah_testkit::write_health_data(files.path(), &days).unwrap();
    let mut db = Db::open(&DataDir::new(d.path()).unwrap()).unwrap();
    eenkah_store::import::import_path(&mut db, files.path()).unwrap();
    eenkah_store::derive::run(&mut db).unwrap();
    eenkah_api::views::today(&db).unwrap();
    eenkah_api::views::brief_text(&db).unwrap();
    eenkah_api::views::personal(&db).unwrap();
    eenkah_api::views::activities(&db, None, 10).unwrap();
    let runs: i64 = db.conn.query_row("SELECT count(*) FROM model_run", [], |r| r.get(0)).unwrap_or(0);
    let results: i64 = db
        .conn
        .query_row("SELECT (SELECT count(*) FROM day_normal) + (SELECT count(*) FROM day_unusual)", [], |r| r.get(0))
        .unwrap_or(0);
    assert_eq!((runs, results), (0, 0), "a page load trained a model");
}
