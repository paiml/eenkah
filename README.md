# eenkah

Your Garmin data, in one file on your own machine, answering three questions:

1. **How is my week going?**
2. **What happens if I go hard tomorrow?**
3. **Where am I heading?**

Small models (aprender, CPU only) are rebuilt from your own days after every import, in
about a second; no model file is kept. Forecasts are used only when they beat your own
baselines, and every forecast shows its interval and track record. Personal models show
your normal range for each body signal, when that normal has moved, nights that were
unusual for you, and the type of each session.
No GPU, no cloud service, no account beyond your Garmin one. **Not medical advice.**
Not affiliated with Garmin.

## Build and run (local use)

It needs Rust 1.91+ and Node 20+.

```sh
make            # lists the targets; nothing runs without one
make build      # builds the web apps (React + TypeScript) and the release binary, assets embedded
make run        # build, then serve http://127.0.0.1:8090
```

`install.sh` is for **production** installs only. It verifies, builds, installs the binary
and, with `--service`, a systemd service plus a daily import timer
(`./install.sh --help`).

## Use

```sh
eenkah login                       # once: Garmin Connect email, password (used once, never stored), MFA code if asked
eenkah import                      # download only what is new from Garmin Connect, then import, compute, forecast
eenkah serve                       # Today, Week, Trends, Activities, Model, Data
eenkah brief                       # today in one or two sentences, with the data age
eenkah mark 2026-09-14 sick --through 2026-09-19   # days the watch cannot explain
eenkah threshold lthr 163 --from 2026-01-01         # dated; never rewrites earlier days
eenkah session-type 20737988224 Tempo               # correct a session's type ("clear" to undo)
eenkah model                                        # forecasts, normal ranges, shifts, unusual nights, session types
```

You can also download from the web UI (Data → Download new data). **Retrain now** (Data or
Model page, or `eenkah train`) recomputes metrics and rebuilds every model from the data
already here. Models are never saved, but their results are, in `eenkah.db`, so a restart
shows the same forecasts, ranges and session types without retraining.

- **Have a Garmin "Export your data" ZIP, or a folder of Garmin FIT/JSON files?**
  `eenkah import <path>` reads it offline. Later downloads skip everything it already holds.

How the download stays fast:

- Each day is checked against the archive **before** any request.
- There are no fixed sleeps; eenkah backs off only when Garmin answers 429 or 5xx.
- Resting heart rate and weight are fetched as date ranges.
- Hydration is skipped.
- Nothing depends on file times.

A day older than two days is never downloaded twice. A daily run makes about 10 requests.
`--max-requests N` caps a long first backfill; run the import again to continue.

Every file is stored once under its SHA-256 in `raw/`. `eenkah rebuild` recreates the
database from that archive.

## Privacy

eenkah talks to the network only when you run `eenkah login` or an import from Garmin
Connect. It then contacts Garmin's servers the way the Connect mobile app does: a password
sign-in goes to `sso.garmin.com` and is traded for a renewable OAuth2 token at
`diauth.garmin.com`. Add `-v` (or `-vv`) to any command to see each request; passwords
and tokens are never logged.

Tokens are kept in `<data>/connect/tokens.json`, readable only by you. `eenkah logout`
deletes them. The web server listens on 127.0.0.1 only unless you pass `--listen`.

A data folder holds one Garmin account's data. If you sign in as a different account,
downloads pause until you choose: **Start over** on the Data page (or `eenkah reset`)
replaces the data with the new account's, keeping your sign-in, marked days, thresholds,
settings and dashboards. `eenkah reset --all` leaves the folder as if eenkah had never run.

## AI agents (MCP)

```sh
claude mcp add eenkah -- eenkah mcp
```

Tools: `eenkah_today`, `eenkah_week`, `eenkah_forecast`, `eenkah_normal`, `eenkah_activities`,
`eenkah_activity`, `eenkah_query`, `eenkah_metric_catalog`, and `eenkah_import` (the only
one that writes; with no path it downloads from Garmin Connect).

**Privacy:** connecting an AI client sends the returned health data to that client's model provider. eenkah itself sends nothing.

## Reproducible claim

Measured on a 2026 laptop with 3.7 years of one athlete's Garmin files (13,944 files):

- Importing takes **15 s**.
- Training and walk-forward backtesting both forecasts takes **10 s**.
- A daily Connect download takes about 10 requests.

All three commands (`eenkah import <folder>`, `eenkah train`, `eenkah import`) print their
timings, so you can check this on your own data.

## Layout

| Path | Job |
|---|---|
| `crates/eenkah-core` | Metric formulas, features, findings. Pure functions |
| `crates/eenkah-ingest` | Garmin JSON and FIT → typed records |
| `crates/eenkah-connect` | Garmin Connect sign-in and incremental download |
| `crates/eenkah-store` | `eenkah.db`, `raw/` archive, import, rebuild, derived series |
| `crates/eenkah-learn` | aprender models rebuilt on every run: forecasts (walk-forward backtest, promotion), normal ranges, shifts, unusual nights, session types |
| `crates/eenkah-api` | HTTP API, import job, embedded web app; suyo mounted at `/suyo` |
| `crates/eenkah-mcp` | MCP tools (pmcp) over the same calls |
| `crates/eenkah-cli` | The `eenkah` binary |
| `crates/eenkah-testkit` | Synthetic athlete; no real personal data in fixtures |
| `crates/suyo` | suyo, the charting library and server, vendored here until it has its own repository |
| `ui/` | The web app: React + TypeScript (strict), charts from suyo's React library |
| `dashboards/` | Default views as editable suyo dashboard JSON |

Development:

- `make dev` runs the API plus Vite with hot reload on :5173.
- `make test` runs the Rust tests and the TypeScript type checks.
- `make verify` runs everything CI checks: format, clippy, tests and type checks.

See `docs/METRICS.md`, `docs/DATA.md` and `docs/DOGFOOD.md`.

## Licence

MIT. See [LICENSE](LICENSE).
