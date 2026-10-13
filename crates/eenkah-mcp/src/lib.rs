//! `eenkah mcp`: tools for AI agents over stdio. Every tool calls the same functions as the
//! HTTP API and CLI (MCP-2) and every tool except `eenkah_import` is read-only (MCP-1).
//!
//! Connecting an AI client sends what these tools return to that client's model provider.
//! eenkah itself sends nothing (MCP-3).

#![allow(clippy::type_complexity)]

use anyhow::Result;
use async_trait::async_trait;
use eenkah_api::views;
use eenkah_store::{DataDir, Db};
use pmcp::types::capabilities::ServerCapabilities;
use pmcp::types::ToolInfo;
use pmcp::{RequestHandlerExtra, Server, ToolHandler};
use serde_json::{json, Value};

type Call = fn(&mut Db, &Value) -> Result<Value>;

struct Tool {
    name: &'static str,
    description: &'static str,
    schema: fn() -> Value,
    read_only: bool,
    call: Call,
    dir: DataDir,
}

#[async_trait]
impl ToolHandler for Tool {
    async fn handle(&self, args: Value, _extra: RequestHandlerExtra) -> pmcp::Result<Value> {
        let run = || -> Result<Value> {
            let mut db = Db::open(&self.dir)?;
            (self.call)(&mut db, &args)
        };
        run().map_err(|e| pmcp::Error::internal(format!("{e:#}")))
    }
    fn metadata(&self) -> Option<ToolInfo> {
        let mut t = ToolInfo::new(self.name, Some(self.description.to_string()), (self.schema)());
        t.annotations = Some(pmcp::types::ToolAnnotations::new().with_read_only(self.read_only));
        Some(t)
    }
}

fn no_args() -> Value {
    json!({"type": "object", "properties": {}})
}

pub fn tools() -> Vec<(&'static str, &'static str, fn() -> Value, bool, Call)> {
    vec![
        (
            "eenkah_today",
            "Body state on the newest morning (HRV, resting heart rate, sleep, form) against the athlete's own normal, the ranked findings, one-sentence brief and data age.",
            no_args,
            true,
            |db, _| views::today(db),
        ),
        (
            "eenkah_week",
            "This week's training load, sleep, HRV and resting heart rate against last week and the same week last year.",
            no_args,
            true,
            |db, _| views::week(db),
        ),
        (
            "eenkah_forecast",
            "Forecasts for tomorrow morning (HRV, resting heart rate) with 80% interval, the model in use, its drivers, and its backtest track record against the athlete's own baselines.",
            no_args,
            true,
            |db, _| eenkah_learn::model_summary(db),
        ),
        (
            "eenkah_normal",
            "The athlete's own normal range for HRV, resting heart rate, sleep score, Body Battery, overnight stress, breathing rate, blood oxygen and skin temperature on the newest night; shifts in those normals; whether recent nights were unusual (several signals off normal at once) and why; and session types (easy, long, tempo, intervals). Rebuilt from the athlete's data on every import.",
            no_args,
            true,
            |db, _| views::personal(db),
        ),
        (
            "eenkah_activities",
            "Training sessions, newest first, with load, decoupling and session type. Optional: since (YYYY-MM-DD), limit.",
            || json!({"type": "object", "properties": {"since": {"type": "string", "description": "YYYY-MM-DD"}, "limit": {"type": "integer", "default": 20}}}),
            true,
            |db, a| {
                views::activities(
                    db,
                    a["since"].as_str().and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()),
                    a["limit"].as_u64().unwrap_or(20) as usize,
                )
            },
        ),
        (
            "eenkah_activity",
            "One session in detail: summary, thresholds used, load, time in zones, decoupling.",
            || json!({"type": "object", "properties": {"id": {"type": "integer"}}, "required": ["id"]}),
            true,
            |db, a| activity(db, a["id"].as_i64().unwrap_or_default()),
        ),
        (
            "eenkah_query",
            "Evaluate suyo/Graphite expressions over the eenkah.* series (see eenkah_metric_catalog). Returns data, or an SVG chart with format=svg.",
            || json!({"type": "object", "properties": {"targets": {"type": "array", "items": {"type": "string"}}, "from": {"type": "string", "default": "-30d"}, "until": {"type": "string", "default": "now"}, "step": {"type": "string"}, "format": {"type": "string", "enum": ["json", "svg"], "default": "json"}}, "required": ["targets"]}),
            true,
            query,
        ),
        ("eenkah_metric_catalog", "Every eenkah series with unit, formula and source.", no_args, true, |_, _| {
            Ok(views::metric_catalog())
        }),
        (
            "eenkah_import",
            "Download new data from Garmin Connect (when no path is given; needs a prior `eenkah login`), or import a folder, ZIP or file the user names; then recompute metrics and forecasts. The only tool that writes.",
            || json!({"type": "object", "properties": {"path": {"type": "string", "description": "Optional. Omit to download from Garmin Connect."}}}),
            false,
            |db, a| {
                let dir = db.dir.clone();
                let o = eenkah_api::jobs::ImportOpts { path: a["path"].as_str().map(Into::into), ..Default::default() };
                Ok(serde_json::to_value(eenkah_api::jobs::run_import(&dir, db, &o, &mut |_: &eenkah_api::jobs::Update| {})?)?)
            },
        ),
    ]
}

fn activity(db: &mut Db, id: i64) -> Result<Value> {
    let list = views::activities(db, None, 100_000)?;
    let a = list
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["id"].as_i64() == Some(id))
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no activity {id}"))?;
    let mut st = db.conn.prepare("SELECT metric, value FROM activity_metric WHERE activity_id=?1 ORDER BY metric")?;
    let m: serde_json::Map<String, Value> = st.query_map([id], |r| Ok((r.get::<_, String>(0)?, json!(r.get::<_, f64>(1)?))))?.collect::<rusqlite::Result<_>>()?;
    Ok(json!({"activity": a, "metrics": m, "units": {"zone*_s": "seconds", "load": "heart-rate impulse", "decoupling": "%", "thr.*": "bpm"}}))
}

fn query(db: &mut Db, a: &Value) -> Result<Value> {
    let store = db.suyo()?;
    let mut p: Vec<(String, String)> = vec![
        ("from".into(), a["from"].as_str().unwrap_or("-30d").into()),
        ("until".into(), a["until"].as_str().unwrap_or("now").into()),
    ];
    if let Some(s) = a["step"].as_str() {
        p.push(("step".into(), s.into()));
    }
    for t in a["targets"].as_array().into_iter().flatten().filter_map(|t| t.as_str()) {
        p.push(("target".into(), t.into()));
    }
    let params = suyo::render::Params(p);
    let r = suyo::render::run(&store, &params, suyo::timeutil::Zone::Named(db.tz()))?;
    if a["format"].as_str() == Some("svg") {
        return Ok(json!({"svg": suyo::svg::render(&r, &params)}));
    }
    Ok(suyo::render::suyo_json(&r))
}

pub fn run(dir: DataDir) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    rt.block_on(async move {
        let mut b = Server::builder().name("eenkah").version(env!("CARGO_PKG_VERSION")).capabilities(ServerCapabilities::tools_only());
        for (name, description, schema, read_only, call) in tools() {
            b = b.tool(
                name,
                Tool {
                    name,
                    description,
                    schema,
                    read_only,
                    call,
                    dir: dir.clone(),
                },
            );
        }
        b.build()?.run_stdio().await?;
        Ok(())
    })
}
