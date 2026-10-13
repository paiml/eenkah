# suyo

Graphite-style time-series plotting in one Rust binary: a SQLite store with rollups, the Graphite
`/render` API with ~80 functions, and a themed web UI with dashboards. Nothing else to install.

suyo is **generic**. It stores named series like `host.myhost.cpu.percent` or
`app.api.requests` and plots them. Feed it from SQLite databases, CSV files, HTTP JSON or Graphite
plaintext (see "Adding data" below).

---

## How to run it

| What | Where |
|---|---|
| Database | `~/.local/share/suyo/suyo.db` (default, override with `--db` or `SUYO_DB`) |
| Sources file (optional) | e.g. `~/.config/suyo/sources.toml` — SQLite queries suyo keeps in sync |

```sh
make build                                   # UI + release binary (UI embedded)
./target/release/suyo serve                  # http://127.0.0.1:8080
./target/release/suyo serve --sources ~/.config/suyo/sources.toml --collect 10s
```

- With `--sources`, suyo syncs new rows at startup and then every 15 minutes
  (`--sync-every 1h` to change). Syncs are incremental, so they take milliseconds.
- `--collect 10s` also charts this computer's CPU, memory, load and network.
- Stop it with Ctrl+C. `suyo sync --sources <file>` forces a sync without the server.

| Page | URL |
|---|---|
| Demo dashboard (every chart type, generated data) | http://127.0.0.1:8080/d/demo |
| All dashboards | http://127.0.0.1:8080/d |
| Composer (ad-hoc queries) | http://127.0.0.1:8080/ |
| Function reference | http://127.0.0.1:8080/docs |

Export dashboards you edited before deleting a database (`suyo dashboard export <name> > file.json`).

---

## Using the UI

- **Range picker** (top right) sets the time range for every panel; drag across a chart to zoom in.
  Panels with their own range show a badge (for example "this year").
- **Hover** shows a crosshair synced across panels. Click legend entries to hide series.
- Every panel has **Data** (table view), **CSV** and **SVG** buttons. Click a panel title to open
  it in the composer.
- **Composer**: type a target (Tab autocompletes series names), wrap it in functions with **ƒ**,
  pick a chart type, then **Save to dashboard**. The URL is shareable.
- **Edit** on a dashboard lets you move, resize and delete panels, or edit the whole dashboard as JSON.
- The theme picker (auto, light, dark, paper, nord, midnight) is in the header.

---

## Queries

Targets are Graphite expressions. Paths support globs (`*`, `{a,b}`, `**` for any depth); functions
nest. A few examples:

```text
movingAverage(host.*.cpu.percent, '10min')
alias(summarize(app.orders.count, '1mon', 'sum'), 'Orders per month')
timeShift(app.orders.count, '1y')                    # same series a year back, dashed
group(host.*.cpu.user, host.*.cpu.system, host.*.cpu.iowait)
asPercent(app.api.errors, app.api.requests)         # error rate
highestAverage(host.*.cpu.percent, 3)
```

Time ranges accept Graphite (`-7d`, `-1mon`, `14:00_20260101`), Grafana (`now-1M/M`, `now/y`),
`today`, `yesterday` and dates. `step=1d|1w|1mon|1y` forces calendar buckets in your timezone.

Try queries from the shell:

```sh
./target/release/suyo query 'movingAverage(host.*.cpu.percent,"10min")' --from -1d
```

The HTTP API is Graphite-compatible, so Grafana's Graphite data source also works against it:

```text
/render?target=...&from=-30d&format=json|csv|svg|html|suyo
/metrics/find?query=host.*
```

---

## Adding data

| Method | Command |
|---|---|
| Any SQLite database | Add a `[[source]]` to a sources TOML: `name`, `db`, `prefix`, and a `query` returning `ts` plus value columns, with `:since` for incremental reads (optional `units`, `agg`, `interval`, `tz`) |
| CSV | `suyo import-csv file.csv --prefix my.thing` (wide: a `ts` column + one column per series) |
| HTTP JSON | `curl -X POST localhost:8080/api/ingest -d '[{"series":"a.b","ts":1700000000,"value":3}]'` |
| Graphite plaintext | `echo "a.b 3 $(date +%s)" \| suyo ingest`, or `serve --carbon 0.0.0.0:2003` for TCP |
| This host | `serve --collect 10s` → `host.<hostname>.*` |

Set units and aggregation per series (used for rollups, axis labels and `summarize` defaults):

```sh
suyo meta my.thing.requests --unit req --agg sum
```

---

## Command reference

```text
suyo serve        Web UI + API (--listen, --sources, --sync-every, --collect, --carbon,
                  --write-token, --retention, --assets ui/dist to serve a UI build from disk)
suyo sync         Pull new rows from a sources file (--full to re-read everything)
suyo import-csv   Import a CSV file
suyo ingest       Graphite plaintext from stdin
suyo query        Evaluate targets and print them
suyo series       List series matching a glob
suyo meta         Set unit / aggregation / interval / description for a series
suyo dashboard    list | export | import | delete
```

Global options: `--db <file>` and `--tz <IANA zone>` (default: system timezone).

If you ever expose the server beyond localhost, set `--write-token` (or `SUYO_WRITE_TOKEN`) so
only you can ingest data or edit dashboards.

---

## Development

Requires a Rust toolchain and Node 20+ (for the UI only).

```sh
make              # list targets
make build        # build the UI (ui/dist) and the release binary with the UI embedded
make test         # cargo test + TypeScript typecheck
make run          # build, then serve on http://127.0.0.1:8080 (try /d/demo)
make dev          # Vite dev server with hot reload; proxies /render, /api, /metrics to :8080
```

For UI work, run `make run` (or any `suyo serve` on 127.0.0.1:8080) in one terminal and
`make dev` in another, then open the URL Vite prints.
`suyo serve --assets ui/dist` serves a UI build from disk instead of the embedded copy.

The binary embeds `ui/dist` at compile time (`build.rs`). Without a UI build, `cargo build`
still succeeds and serves a placeholder page that says to run `make build`; the API works either way.

Layout: `src/` holds the server (store, parser, eval, render, ingest, sources, presets);
`ui/` holds the web UI, written in React + TypeScript (strict) and built with Vite.
`ui/src/lib/` is a reusable library (typed API client, `<Chart>`, `<Panel>`, `<Dashboard>`,
`<RangePicker>`, themes) and `ui/src/app/` is suyo's own single-page app built on it.
