//! eenkah: tell an endurance athlete the state of their body and what it means for tomorrow.

use anyhow::{bail, Context, Result};
use chrono::NaiveDate;
use clap::{Parser, Subcommand};
use eenkah_store::{derive, import, DataDir, Db};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "eenkah", version, about = "Your Garmin data, on your machine, with forecasts you can check. Not medical advice.")]
struct Cli {
    /// Data directory [env: EENKAH_HOME] [default: ~/.local/share/eenkah]
    #[arg(long, global = true)]
    data: Option<PathBuf>,
    /// More detail on what Garmin Connect is doing: -v every HTTP request, -vv headers too
    /// (secrets redacted). RUST_LOG overrides
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Download new data from Garmin Connect and import it. With a PATH, import that export
    /// ZIP, folder or FIT/JSON file instead, without going online
    Import {
        path: Option<PathBuf>,
        /// First day to download from Connect (default: setting connect_start, else the day
        /// after the newest imported day's archive start, else one year ago)
        #[arg(long)]
        since: Option<String>,
        /// Stop the Connect download after this many requests (resume by running again)
        #[arg(long)]
        max_requests: Option<u64>,
        /// Print the full report as JSON
        #[arg(long)]
        json: bool,
    },
    /// Sign in to Garmin Connect (once). The password is used once and never stored
    Login {
        /// Account email [env: EENKAH_GARMIN_EMAIL]
        #[arg(long, env = "EENKAH_GARMIN_EMAIL")]
        email: Option<String>,
        /// garmin.com, or garmin.cn for accounts in China
        #[arg(long, default_value = "garmin.com")]
        domain: String,
    },
    /// Forget the Garmin Connect sign-in (deletes the saved tokens)
    Logout,
    /// Start over: delete everything downloaded from Garmin or computed from it. Keeps the
    /// sign-in, marked days, thresholds, settings and dashboards. Shows what goes and asks first
    Reset {
        /// Also sign out and delete marks, thresholds, settings and dashboards: a fresh folder
        #[arg(long)]
        all: bool,
        /// Do not ask for confirmation
        #[arg(long, short)]
        yes: bool,
    },
    /// Rebuild every model from your data on this computer's CPU: forecasts (backtest, promote,
    /// forecast tomorrow), normal ranges, shifts, unusual days and session types. Nothing is stored but results
    Train {
        #[arg(long)]
        json: bool,
    },
    /// Show each forecast's champion model and track record, and the personal models' results
    Model,
    /// Correct a session's type (Easy, Long, Tempo, Intervals), or "clear" to undo. Corrections
    /// always win and teach the session-type model
    SessionType {
        /// Activity id (see the Activities page or `eenkah_activities`)
        id: i64,
        label: String,
    },
    /// Recreate the database from raw/ (marks and threshold overrides are kept)
    Rebuild,
    /// Mark a day: sick, travel, race, altitude, or any word (shown on charts, used by models)
    Mark {
        /// YYYY-MM-DD, "today" or "yesterday"
        day: String,
        kind: String,
        #[arg(long, default_value_t = 1.0)]
        value: f64,
        #[arg(long, default_value = "")]
        note: String,
        /// Mark every day from `day` through this day
        #[arg(long)]
        through: Option<String>,
    },
    /// Set a threshold from a date onwards: lthr, max_hr, rest_hr (bpm). Never rewrites earlier days
    Threshold {
        kind: String,
        value: f64,
        #[arg(long)]
        from: String,
    },
    /// Set a setting: tz (IANA zone), trimp_k (1.92 men, 1.67 women)
    Set { key: String, value: String },
    /// Show what is imported and how fresh it is
    Status,
    /// Run the web UI on 127.0.0.1 (pass --listen 0.0.0.0:8090 to expose it; you are responsible for access control)
    Serve {
        #[arg(long, default_value = eenkah_api::DEFAULT_LISTEN)]
        listen: String,
        #[arg(long, default_value_t = 4)]
        threads: usize,
        /// Serve the web app from this folder (e.g. ui/dist) instead of the embedded copy
        #[arg(long)]
        assets: Option<PathBuf>,
    },
    /// Print the metric catalogue (--markdown for docs/METRICS.md)
    Metrics {
        #[arg(long)]
        markdown: bool,
    },
    /// MCP server on stdio for AI agents (e.g. `claude mcp add eenkah -- eenkah mcp`)
    Mcp,
    /// Today's state in one or two sentences, with the data age
    Brief {
        #[arg(long)]
        json: bool,
    },
    /// Default dashboards: restore one (or all) by name
    Dashboard {
        #[arg(long)]
        reset: Vec<String>,
    },
}

fn parse_day(s: &str) -> Result<NaiveDate> {
    let today = chrono::Local::now().date_naive();
    Ok(match s {
        "today" => today,
        "yesterday" => today.pred_opt().unwrap(),
        _ => NaiveDate::parse_from_str(s, "%Y-%m-%d").with_context(|| format!("expected YYYY-MM-DD, got '{s}'"))?,
    })
}

/// A stored day ("YYYY-MM-DD") as "3 weeks ago", relative to this computer's date.
fn rel(day: Option<&str>) -> String {
    match day.map(|d| (d, NaiveDate::parse_from_str(d, "%Y-%m-%d"))) {
        Some((_, Ok(d))) => eenkah_core::when::day(d, chrono::Local::now().date_naive()),
        Some((d, Err(_))) => d.to_string(),
        None => "—".into(),
    }
}

fn print_report(r: &import::Report) {
    println!(
        "files: {} seen · {} new · {} unchanged · {} re-read · {} failed",
        r.files_seen, r.new, r.unchanged, r.reparsed, r.failed
    );
    for (k, n) in &r.contents {
        println!("  read     {n:>6}  {k}");
    }
    for (k, n) in &r.not_supported {
        println!("  skipped  {n:>6}  {k}");
    }
    for (name, e) in &r.errors {
        println!("  failed   {name}: {e}");
    }
    if !r.unread_messages.is_empty() {
        let total: usize = r.unread_messages.values().sum();
        println!("  {total} FIT messages of {} types were present but not used (see `eenkah import --json`)", r.unread_messages.len());
    }
    println!("\ncoverage:");
    for c in &r.coverage {
        println!(
            "  {:<22} {} → {}  {:>5} days{}",
            c.what,
            rel(c.first.as_deref()),
            rel(c.last.as_deref()),
            c.days,
            if c.missing_days > 0 { format!(", {} missing", c.missing_days) } else { String::new() }
        );
    }
}

/// Logs go to stderr (stdout stays clean for --json and `eenkah mcp`). Garmin Connect steps
/// show by default; -v adds each HTTP request, -vv its headers.
fn init_log(verbose: u8) {
    let connect = ["info", "debug", "trace"][verbose.min(2) as usize];
    env_logger::Builder::new()
        .parse_filters(&format!("warn,eenkah_connect={connect}"))
        .parse_env("RUST_LOG")
        .format(|f, r| {
            use std::io::Write;
            let level = match r.level() {
                log::Level::Error => "error: ",
                log::Level::Warn => "warning: ",
                _ => "",
            };
            writeln!(f, "[{}] {level}{}", chrono::Local::now().format("%H:%M:%S%.3f"), r.args())
        })
        .init();
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_log(cli.verbose);
    let dir = DataDir::new(cli.data.clone().unwrap_or_else(DataDir::default_location))?;
    let mut db = Db::open(&dir)?;
    match cli.cmd {
        Cmd::Import { path, since, max_requests, json } => {
            let opts = eenkah_api::jobs::ImportOpts {
                path,
                since: since.as_deref().map(parse_day).transpose()?,
                max_requests,
            };
            let mut shown = Steps::new(json);
            let res = eenkah_api::jobs::run_import(&dir, &mut db, &opts, &mut |u| shown.show(u));
            shown.close(if res.is_ok() { "✓" } else { "✗ failed" });
            let out = res?;
            if json {
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                if let Some(c) = &out.connect {
                    println!(
                        "connect: {} requests ({} retries, {:.1} MB), {} files, {} new activities, {} items already here{}",
                        c.counters.requests,
                        c.counters.retries,
                        c.counters.bytes as f64 / 1e6,
                        c.files_written,
                        c.activities_new,
                        c.skipped_known,
                        if c.stopped_early { " — stopped at --max-requests; run again to continue" } else { "" }
                    );
                    for e in c.errors.iter().take(20) {
                        println!("  error: {e}");
                    }
                }
                print_report(&out.import);
                println!("\n{}", eenkah_learn::model_text(&db)?);
                let d = &out.derive;
                println!(
                    "\nderived {} series ({} points), {} activities ({} with heart-rate samples) in {:.1}s",
                    d.series_written, d.points_written, d.activities, d.activities_with_hr_samples, out.secs
                );
                let warnings = eenkah_api::jobs::warnings(&out);
                if warnings.is_empty() {
                    println!("\n✓ import complete in {}", duration(out.secs));
                } else {
                    println!("\n⚠ import complete in {}, with problems:", duration(out.secs));
                    for w in warnings {
                        println!("  • {w}");
                    }
                }
            }
        }
        Cmd::Login { email, domain } => {
            let net = eenkah_connect::Net::new();
            let retry = eenkah_connect::Retry::default();
            let mut c = eenkah_connect::Counters::default();
            let email = match email {
                Some(e) => e,
                None => prompt_line("Garmin Connect email: ")?,
            };
            let password = match std::env::var("EENKAH_GARMIN_PASSWORD") {
                Ok(p) => p,
                Err(_) => rpassword::prompt_password("Password (used once, not stored): ")?,
            };
            let tokens = eenkah_connect::login(&net, &retry, &mut c, &domain, &email, &password, &StdinMfa)?;
            // prove the token works before saving it
            let mut client = eenkah_connect::Client {
                t: &net,
                retry,
                tokens,
                tokens_path: None,
                counters: c,
            };
            let me = client.profile().context("signed in, but the Connect API did not accept the token")?;
            client.tokens.save(&tokens_path(&dir))?;
            db.set_setting("connect_login", &serde_json::json!({"id": me.id, "name": me.name, "at": chrono::Utc::now().timestamp()}).to_string())?;
            println!("signed in to Garmin Connect as {}; tokens saved to {} (owner-only).", me.describe(), tokens_path(&dir).display());
            match eenkah_api::jobs::data_account(&db)? {
                Some(owner) if owner.id != me.id => {
                    println!(
                        "\nThe data in this folder came from a different Garmin account, {}. Downloads are paused so the two never mix.",
                        owner.describe()
                    );
                    let p = eenkah_api::jobs::reset_preview(&db, false)?;
                    print_reset_preview(&p);
                    if confirm("Start over with this account?")? {
                        eenkah_api::jobs::reset(&mut db, false)?;
                        println!("Done. Run `eenkah import` to download {}'s data.", me.describe());
                    } else {
                        println!("Nothing changed. Run `eenkah reset` to start over later, or `eenkah login` with {}.", owner.describe());
                    }
                }
                _ => println!("Run `eenkah import` to download."),
            }
        }
        Cmd::Logout => match std::fs::remove_file(tokens_path(&dir)) {
            Ok(()) => println!("signed out: tokens deleted"),
            Err(_) => println!("not signed in"),
        },
        Cmd::Reset { all, yes } => {
            let p = eenkah_api::jobs::reset_preview(&db, all)?;
            print_reset_preview(&p);
            if !yes
                && !confirm(if all {
                    "Delete everything and start fresh?"
                } else {
                    "Delete the downloaded data and start over?"
                })?
            {
                println!("Nothing changed.");
            } else {
                eenkah_api::jobs::reset(&mut db, all)?;
                println!(
                    "Done: this folder is empty{}. Run `eenkah import` to download again.",
                    if all {
                        " and signed out (run `eenkah login` first)"
                    } else {
                        "; sign-in, marks and settings were kept"
                    }
                );
            }
        }
        Cmd::Train { json } => {
            let r = eenkah_learn::train(&mut db)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                println!("{}", eenkah_learn::model_text(&db)?);
            }
        }
        Cmd::Model => println!("{}", eenkah_learn::model_text(&db)?),
        Cmd::SessionType { id, label } => {
            let l = (label != "clear").then_some(label.as_str());
            eenkah_learn::personal::set_label(&mut db, id, l)?;
            match l {
                Some(l) => println!("session {id} is now {l}"),
                None => println!("session {id}: correction removed"),
            }
        }
        Cmd::Rebuild => {
            let r = import::rebuild(&mut db)?;
            derive::run(&mut db)?;
            print_report(&r);
        }
        Cmd::Mark { day, kind, value, note, through } => {
            let a = parse_day(&day)?;
            let b = through.as_deref().map(parse_day).transpose()?.unwrap_or(a);
            if b < a {
                bail!("--through is before the start day");
            }
            for d in a.iter_days().take_while(|d| *d <= b) {
                db.put_mark(d, &kind, value, &note)?;
            }
            derive::run(&mut db)?;
            println!("marked {} day(s) as {kind}", (b - a).num_days() + 1);
        }
        Cmd::Threshold { kind, value, from } => {
            if !["lthr", "max_hr", "rest_hr"].contains(&kind.as_str()) {
                bail!("threshold must be lthr, max_hr or rest_hr");
            }
            db.conn.execute(
                "INSERT OR REPLACE INTO threshold_override(kind, from_day, value) VALUES (?1, ?2, ?3)",
                (&kind, parse_day(&from)?.to_string(), value),
            )?;
            derive::run(&mut db)?;
            println!("{kind} = {value} from {from}");
        }
        Cmd::Set { key, value } => {
            match key.as_str() {
                "tz" => {
                    value.parse::<chrono_tz_check::Tz>().map_err(|_| anyhow::anyhow!("unknown time zone '{value}'"))?;
                }
                "trimp_k" => {
                    value.parse::<f64>().context("trimp_k must be a number")?;
                }
                _ => bail!("unknown setting '{key}' (tz, trimp_k)"),
            }
            db.set_setting(&key, &value)?;
            println!("{key} = {value} (run `eenkah rebuild` to apply to imported data)");
        }
        Cmd::Serve { listen, threads, assets } => {
            drop(db);
            eenkah_api::serve(dir, eenkah_api::ServeOpts { listen, threads, assets })?;
        }
        Cmd::Metrics { markdown } => {
            if markdown {
                print!("{}", eenkah_api::metrics_markdown());
            } else {
                println!("{}", serde_json::to_string_pretty(&eenkah_api::views::metric_catalog())?);
            }
        }
        Cmd::Mcp => {
            drop(db);
            eenkah_mcp::run(dir)?;
        }
        Cmd::Brief { json } => {
            if json {
                println!("{}", serde_json::to_string_pretty(&eenkah_api::views::today(&db)?)?);
            } else {
                println!("{}", eenkah_api::views::brief_text(&db)?);
            }
        }
        Cmd::Dashboard { reset } => {
            let names: Vec<&str> = if reset.iter().any(|r| r == "all") {
                eenkah_api::DASHBOARDS.iter().map(|d| d.0).collect()
            } else {
                reset.iter().map(String::as_str).collect()
            };
            for n in eenkah_api::seed_dashboards(&db, &names)? {
                println!("restored {n}");
            }
        }
        Cmd::Status => {
            println!("data: {}", dir.root.display());
            println!("time zone: {}", db.tz());
            for c in import::coverage(&db)? {
                println!("  {:<22} {} → {}  {:>5} days", c.what, rel(c.first.as_deref()), rel(c.last.as_deref()), c.days);
            }
        }
    }
    Ok(())
}

fn tokens_path(dir: &DataDir) -> PathBuf {
    eenkah_api::jobs::tokens_path(dir)
}

fn prompt_line(q: &str) -> Result<String> {
    use std::io::Write;
    print!("{q}");
    std::io::stdout().flush()?;
    let mut s = String::new();
    std::io::stdin().read_line(&mut s)?;
    Ok(s.trim().to_string())
}

fn duration(secs: f64) -> String {
    let s = secs.round() as u64;
    if s < 60 {
        format!("{secs:.1}s")
    } else {
        format!("{}m {:02}s", s / 60, s % 60)
    }
}

/// Import progress on stderr: one numbered line per step, and a live progress bar while
/// downloading (redrawn in place on a terminal, every 30 days otherwise).
struct Steps {
    quiet: bool,
    tty: bool,
    step: Option<(eenkah_api::jobs::Phase, std::time::Instant)>,
    bar_open: bool,
    last_printed: u64,
}

impl Steps {
    fn new(quiet: bool) -> Steps {
        use std::io::IsTerminal;
        Steps {
            quiet,
            tty: std::io::stderr().is_terminal(),
            step: None,
            bar_open: false,
            last_printed: 0,
        }
    }

    fn end_bar(&mut self) {
        if self.bar_open {
            eprintln!();
            self.bar_open = false;
        }
    }

    /// Finish the current step with `mark` and its duration.
    fn close(&mut self, mark: &str) {
        if self.quiet {
            return;
        }
        self.end_bar();
        if let Some((_, t)) = self.step.take() {
            eprintln!("      {mark} {}", duration(t.elapsed().as_secs_f64()));
        }
    }

    fn show(&mut self, u: &eenkah_api::jobs::Update) {
        use eenkah_api::jobs::{Phase, Update};
        if self.quiet {
            return;
        }
        match u {
            Update::Phase(p) => {
                self.close("✓");
                let n = Phase::ALL.iter().position(|x| x == p).unwrap_or(0) + 1;
                eprintln!("[{n}/{}] {}", Phase::ALL.len(), p.label());
                self.step = Some((*p, std::time::Instant::now()));
            }
            Update::Note(m) => {
                self.end_bar();
                eprintln!("      {m}");
            }
            Update::Download(p) => {
                let pct = if p.total > 0 { p.done as f64 / p.total as f64 } else { 0.0 };
                let bar = if p.total > 0 {
                    let w = 24;
                    let full = (pct * w as f64).round() as usize;
                    format!("[{}{}] {:>3.0}% ", "█".repeat(full), "·".repeat(w - full), pct * 100.0)
                } else {
                    String::new()
                };
                let count = if p.total > 0 { format!("{}/{}", p.done, p.total) } else { p.done.to_string() };
                let empty = if p.days_empty > 0 { format!(" · {} empty days", p.days_empty) } else { String::new() };
                let line = format!(
                    "      {:<10} {bar}{count} · {} · {} requests · {:.1} MB{empty}",
                    p.step,
                    p.item,
                    p.counters.requests,
                    p.counters.bytes as f64 / 1e6
                );
                if self.tty {
                    eprint!("\r\x1b[K{line}");
                    self.bar_open = true;
                } else if p.done == p.total || p.done < self.last_printed || p.done >= self.last_printed + 30 {
                    eprintln!("{line}");
                    self.last_printed = p.done;
                }
            }
        }
    }
}

fn print_reset_preview(p: &eenkah_api::jobs::ResetPreview) {
    println!(
        "\nThis deletes {} files ({:.1} MB): {} days of data and {} activities{}.",
        p.files,
        p.bytes as f64 / 1e6,
        p.days,
        p.activities,
        p.account.as_ref().map(|a| format!(", downloaded from {}", a.describe())).unwrap_or_default()
    );
    for d in &p.deletes {
        println!("  ✕ {d}");
    }
    for k in &p.keeps {
        println!("  ✓ keeps {k}");
    }
}

/// Ask a yes/no question on the terminal; "no" when there is no terminal to ask on.
fn confirm(q: &str) -> Result<bool> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        println!("{q} Not asking: no terminal. Pass --yes to `eenkah reset` to do it anyway.");
        return Ok(false);
    }
    Ok(matches!(prompt_line(&format!("{q} [y/N] "))?.to_lowercase().as_str(), "y" | "yes"))
}

struct StdinMfa;

impl eenkah_connect::Prompt for StdinMfa {
    fn mfa_code(&self) -> Result<String> {
        prompt_line("Garmin MFA code: ")
    }
}

mod chrono_tz_check {
    pub use chrono_tz::Tz;
}
