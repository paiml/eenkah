use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;
use suyo::store::{AggFn, MetaHint, Store};
use suyo::timeutil::{Span, Zone};
use suyo::{collect, eval, ingest, presets, render, server, sources, timeutil};

#[derive(Parser)]
#[command(name = "suyo", version, about = "Graphite-style time-series plotting in one binary", long_about = None)]
struct Cli {
    /// SQLite database file [env: SUYO_DB] [default: ~/.local/share/suyo/suyo.db]
    #[arg(long, global = true)]
    db: Option<String>,
    /// Timezone for calendar buckets and naive timestamps (IANA name or "local")
    #[arg(long, global = true, default_value = "local")]
    tz: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the web UI and HTTP API
    Serve {
        #[arg(long, default_value = "127.0.0.1:8080")]
        listen: String,
        /// Sources file (TOML) to sync on startup and periodically
        #[arg(long)]
        sources: Option<String>,
        /// How often to re-sync sources, e.g. 15m, 1h
        #[arg(long, default_value = "15m")]
        sync_every: String,
        /// Sample this host's CPU/memory/load/network at this interval, e.g. 10s
        #[arg(long)]
        collect: Option<String>,
        /// Accept Graphite plaintext over TCP on this address, e.g. 0.0.0.0:2003
        #[arg(long)]
        carbon: Option<String>,
        /// Require this bearer token for writes (ingest, dashboards, metadata)
        #[arg(long, env = "SUYO_WRITE_TOKEN")]
        write_token: Option<String>,
        /// Delete raw points older than this (rollups are kept), e.g. 400d
        #[arg(long)]
        retention: Option<String>,
        /// Serve the built web UI from this directory (e.g. ui/dist) instead of the embedded copy (development)
        #[arg(long)]
        assets: Option<String>,
        #[arg(long, default_value_t = 8)]
        threads: usize,
    },
    /// Pull new rows from every source in a sources file
    Sync {
        #[arg(long)]
        sources: String,
        #[arg(long)]
        full: bool,
    },
    /// Import a CSV file (wide: ts + one column per series; long: --series-col)
    ImportCsv {
        file: String,
        /// Series name prefix, e.g. "finance.budget"
        #[arg(long, default_value = "")]
        prefix: String,
        #[arg(long)]
        ts_col: Option<String>,
        #[arg(long)]
        series_col: Option<String>,
        #[arg(long, default_value = "value")]
        value_col: String,
        /// column=unit, repeatable
        #[arg(long = "unit")]
        units: Vec<String>,
        /// column=avg|sum|min|max|count|last, repeatable
        #[arg(long = "agg")]
        aggs: Vec<String>,
    },
    /// Read Graphite plaintext lines (`path value [ts]`) from stdin
    Ingest,
    /// Evaluate targets and print the result
    Query {
        targets: Vec<String>,
        #[arg(long, default_value = "-1d", allow_hyphen_values = true)]
        from: String,
        #[arg(long, default_value = "now", allow_hyphen_values = true)]
        until: String,
        #[arg(long)]
        step: Option<String>,
        /// csv or json
        #[arg(long, default_value = "csv")]
        format: String,
    },
    /// List series (optionally matching a glob)
    Series { pattern: Option<String> },
    /// Set a series' unit, default aggregation, interval or description
    Meta {
        name: String,
        #[arg(long)]
        unit: Option<String>,
        #[arg(long)]
        agg: Option<String>,
        #[arg(long)]
        interval: Option<String>,
        #[arg(long)]
        description: Option<String>,
    },
    /// Export or import dashboards as JSON
    Dashboard {
        #[command(subcommand)]
        cmd: DashCmd,
    },
}

#[derive(Subcommand)]
enum DashCmd {
    List,
    Export {
        name: String,
    },
    Import {
        file: String,
        #[arg(long)]
        name: Option<String>,
    },
    Delete {
        name: String,
    },
}

fn default_db() -> String {
    if let Ok(p) = std::env::var("SUYO_DB") {
        return p;
    }
    let base =
        std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| sources::expand_home("~/.local/share"));
    let dir = format!("{base}/suyo");
    let _ = std::fs::create_dir_all(&dir);
    format!("{dir}/suyo.db")
}

fn seed_defaults(store: &Store) -> Result<()> {
    if store.dashboard("demo")?.is_none() {
        store.put_dashboard("demo", presets::DEMO_DASHBOARD)?;
    }
    if store.dashboard("system")?.is_none() {
        store.put_dashboard("system", presets::SYSTEM_DASHBOARD)?;
    }
    Ok(())
}

fn report(results: Vec<Result<sources::SyncReport>>) {
    for r in results {
        match r {
            Ok(r) => eprintln!(
                "  {:<22} {:>9} rows  {:>10} points  {:>3} series",
                r.source, r.rows, r.points, r.series
            ),
            Err(e) => eprintln!("  error: {e:#}"),
        }
    }
}

fn kv(list: &[String]) -> Result<Vec<(String, String)>> {
    list.iter()
        .map(|s| {
            s.split_once('=')
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .with_context(|| format!("expected column=value, got '{s}'"))
        })
        .collect()
}

/// Write to stdout, exiting quietly if the reader went away (`| head`).
fn emit(s: &str) {
    use std::io::Write;
    if std::io::stdout().lock().write_all(s.as_bytes()).is_err() {
        std::process::exit(0);
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let db = cli.db.clone().unwrap_or_else(default_db);
    let zone = Zone::parse(&cli.tz)?;
    match cli.cmd {
        Cmd::Serve {
            listen,
            sources: src,
            sync_every,
            collect: coll,
            carbon,
            write_token,
            retention,
            assets,
            threads,
        } => {
            let store = Store::open(&db)?;
            seed_defaults(&store)?;
            eprintln!("database: {db}");
            if let Some(path) = src {
                let every =
                    Duration::from_secs(Span::parse(&sync_every)?.approx_secs().max(10) as u64);
                let db2 = db.clone();
                let file = sources::load(&path)?;
                eprintln!(
                    "syncing {} source(s) from {path} every {sync_every}",
                    file.sources.len()
                );
                std::thread::spawn(move || loop {
                    match Store::open(&db2) {
                        Ok(mut st) => report(sources::sync_all(&mut st, &file, false)),
                        Err(e) => eprintln!("sync: {e:#}"),
                    }
                    std::thread::sleep(every);
                });
            }
            if let Some(c) = coll {
                collect::spawn(
                    db.clone(),
                    Duration::from_secs(Span::parse(&c)?.approx_secs().max(1) as u64),
                );
            }
            if let Some(addr) = carbon {
                ingest::spawn_carbon(addr, db.clone())?;
            }
            if let Some(r) = retention {
                let keep = Span::parse(&r)?.approx_secs();
                let db2 = db.clone();
                std::thread::spawn(move || loop {
                    if let Ok(st) = Store::open(&db2) {
                        match st.prune_raw(timeutil::now() - keep) {
                            Ok(n) if n > 0 => eprintln!("retention: pruned {n} raw points"),
                            Err(e) => eprintln!("retention: {e:#}"),
                            _ => {}
                        }
                    }
                    std::thread::sleep(Duration::from_secs(3600));
                });
            }
            let app = Arc::new(server::App {
                db,
                zone,
                assets: assets.map(Into::into),
                write_token,
                base: String::new(),
                brand: None,
                nav: vec![],
            });
            server::serve(app, &listen, threads)
        }
        Cmd::Sync {
            sources: path,
            full,
        } => {
            let file = sources::load(&path)?;
            let mut store = Store::open(&db)?;
            report(sources::sync_all(&mut store, &file, full));
            Ok(())
        }
        Cmd::ImportCsv {
            file,
            prefix,
            ts_col,
            series_col,
            value_col,
            units,
            aggs,
        } => {
            let mut hints: HashMap<String, MetaHint> = HashMap::new();
            for (k, u) in kv(&units)? {
                hints.entry(k).or_default().unit = Some(u);
            }
            for (k, a) in kv(&aggs)? {
                hints.entry(k).or_default().agg = Some(AggFn::parse(&a)?);
            }
            let mut store = Store::open(&db)?;
            let opts = sources::CsvOpts {
                path: file,
                prefix,
                ts_col,
                series_col,
                value_col,
                tz: zone,
                hints,
            };
            let (rows, pts) = sources::import_csv(&mut store, &opts)?;
            eprintln!("imported {rows} rows, {pts} points");
            Ok(())
        }
        Cmd::Ingest => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            let pts = ingest::parse_plaintext(&s)?;
            let n = Store::open(&db)?.write(&pts, &HashMap::new())?;
            eprintln!("wrote {n} points");
            Ok(())
        }
        Cmd::Query {
            targets,
            from,
            until,
            step,
            format,
        } => {
            let store = Store::open(&db)?;
            let mut params = vec![("from".into(), from), ("until".into(), until)];
            if let Some(s) = step {
                params.push(("step".into(), s));
            }
            params.extend(targets.into_iter().map(|t| ("target".to_string(), t)));
            let r = render::run(&store, &render::Params(params), zone)?;
            for (t, e) in &r.errors {
                eprintln!("error in {t}: {e}");
            }
            let out = match format.as_str() {
                "json" => serde_json::to_string_pretty(&render::suyo_json(&r))? + "\n",
                _ => render::csv(&r),
            };
            emit(&out);
            Ok(())
        }
        Cmd::Series { pattern } => {
            let store = Store::open(&db)?;
            let index = store.series()?;
            let list: Vec<_> = match &pattern {
                Some(p) => eval::match_paths(p, &index)?.into_iter().cloned().collect(),
                None => index,
            };
            let mut out = String::new();
            for m in list {
                let range = match (m.first, m.last) {
                    (Some(a), Some(b)) => format!(
                        "{} → {}",
                        timeutil::fmt_local(a, zone),
                        timeutil::fmt_local(b, zone)
                    ),
                    _ => "empty".into(),
                };
                out += &format!(
                    "{:<44} {:<8} {:<5} {}\n",
                    m.name,
                    m.unit,
                    m.agg.name(),
                    range
                );
            }
            emit(&out);
            Ok(())
        }
        Cmd::Meta {
            name,
            unit,
            agg,
            interval,
            description,
        } => {
            let hint = MetaHint {
                unit,
                agg: agg.as_deref().map(AggFn::parse).transpose()?,
                interval: interval
                    .as_deref()
                    .map(|s| Span::parse(s).map(|x| x.approx_secs()))
                    .transpose()?,
                description,
            };
            Store::open(&db)?.set_meta(&name, &hint)?;
            Ok(())
        }
        Cmd::Dashboard { cmd } => {
            let store = Store::open(&db)?;
            match cmd {
                DashCmd::List => {
                    for (n, _, _) in store.dashboards()? {
                        println!("{n}");
                    }
                }
                DashCmd::Export { name } => println!(
                    "{}",
                    store
                        .dashboard(&name)?
                        .with_context(|| format!("no dashboard '{name}'"))?
                ),
                DashCmd::Import { file, name } => {
                    let text = std::fs::read_to_string(&file)?;
                    let _: serde_json::Value =
                        serde_json::from_str(&text).context("dashboard must be JSON")?;
                    let name = name.unwrap_or_else(|| {
                        std::path::Path::new(&file)
                            .file_stem()
                            .unwrap()
                            .to_string_lossy()
                            .to_string()
                    });
                    store.put_dashboard(&name, &text)?;
                    eprintln!("saved dashboard '{name}'");
                }
                DashCmd::Delete { name } => {
                    store.delete_dashboard(&name)?;
                }
            }
            Ok(())
        }
    }
}
