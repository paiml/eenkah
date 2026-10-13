use eenkah_store::{DataDir, Db};

#[test]
fn mcp1_only_import_writes() {
    for (name, _, _, read_only, _) in eenkah_mcp::tools() {
        assert_eq!(read_only, name != "eenkah_import", "{name}");
    }
}

#[test]
fn mcp2_mcp_and_http_return_identical_data() {
    let d = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let days = eenkah_testkit::athlete(chrono::NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(), 120, 3);
    eenkah_testkit::write_health_data(files.path(), &days).unwrap();
    let mut db = Db::open(&DataDir::new(d.path()).unwrap()).unwrap();
    eenkah_store::import::import_path(&mut db, files.path()).unwrap();
    eenkah_store::derive::run(&mut db).unwrap();
    let tools = eenkah_mcp::tools();
    let call = |db: &mut Db, n: &str| (tools.iter().find(|t| t.0 == n).unwrap().4)(db, &serde_json::json!({})).unwrap();
    assert_eq!(call(&mut db, "eenkah_today"), eenkah_api::views::today(&db).unwrap());
    assert_eq!(call(&mut db, "eenkah_metric_catalog"), eenkah_api::views::metric_catalog());
    let a = (tools.iter().find(|t| t.0 == "eenkah_activities").unwrap().4)(&mut db, &serde_json::json!({"limit": 5})).unwrap();
    assert_eq!(a, eenkah_api::views::activities(&db, None, 5).unwrap());
}
