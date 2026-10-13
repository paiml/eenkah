# One project a week: aprender dogfood backlog

DRAFT · 2026-10-10 · owner: Noah Gift · built from `AGENTS.md` v0.2

## Rules of the cadence

1. A "project" is one user-visible slice that ships in 7 days: tagged release, `install.sh`, one reproducible README claim, one `docs/DOGFOOD.md` entry.
2. Anything bigger is split. Garmin is three weeks, not one.
3. Every week must end with at least one of: an aprender gap filed, or an aprender feature proven in a shipped binary. Zero of both means the week missed its purpose.
4. Kill rule: if the first real user has not used it in the 14 days after ship, archive it.
5. Sprawl guard: incubate in one workspace repo (`paiml/apr-apps` [name is a placeholder]); a project earns its own repo only after it survives rule 4. The portfolio already has 196 repos.

Marks: [V] verified in project docs · [S] stated by you, not checked · [U] unknown, check before starting.

## Sequence

| Wk | Project | What the user gets | aprender surface | Why this week |
|---|---|---|---|---|
| 1 | **suyo 0.1** | Charts for anything over time, installed with one command | Forecast function backed by aprender regression [U] | Nothing is in git yet, and every later project draws its charts here. Also the first shakedown of the template |
| 2 | **Garmin ingest** | FIT files and the official export → one SQLite file → suyo dashboards | `alimentar` data loading [U for FIT-shaped data] | No login needed, so no dependency on Garmin's bot checks |
| 3 | **Garmin MCP** | Ask Claude Code about your own training data | `pmcp` [V]; `aprender-db` [V exists, fit U] | Small once week 2 exists; makes the data useful daily |
| 4 | **Garmin forecast** | Resting heart rate, sleep and load forecasts with intervals | train → quantize → `include_bytes!()` embed [V]; time-series model [U] | Exercises the full lifecycle in one binary; must beat a naive baseline to ship |
| 5 | **apr bench board** | Public page of tokens/sec and time-to-first-token per model, each with a receipt | `apr bench`, `apr profile`, `apr serve` [V] | The throughput table was withdrawn in September 2026 for lack of receipts; this rebuilds it honestly |
| 6 | **Agent proxy** | Local proxy that caches, compresses and routes prompts to `apr serve` or a remote API | `apr serve` OpenAI-compatible API [V]; embeddings for a semantic cache [S] | Every coding session you run becomes a load test of `apr serve` |
| 7 | **Voice training log** | Speak a post-workout note; it is transcribed locally and attached to that day's data | `whisper-apr` [V exists]; `aprender-rag` [V exists] | Second model family in a shipped app; joins weeks 2–4 |
| 8 | **Local search (wigolo slice)** | `search` CLI + MCP over the JSON-API engines, results reranked locally | `CrossEncoder`, `BertEncoder` embedded [S] | Only the YAML connectors and reranker; no scrapers, no accounts, no vault |

## Parked, with the reason

| Idea | Reason | Un-park when |
|---|---|---|
| browser-use port | Obscura already ships an MCP server with click, fill and snapshot tools | A task fails on Obscura's MCP that a custom agent loop would fix |
| Jev-like action model | Jev Ultrafast is a hosted TypeSafe API, not open weights | You decide to train an open operation/target classifier in aprender; that is a strong dogfood project but more than a week |
| wigolo scrapers, accounts, vault | Session cookies for personal portals need a new threat model | Week 8 is in weekly use |
| Garmin Connect login sync | Community login libraries broke in March 2026; needs a browser tier | Weeks 2–4 are in weekly use and the export path proves too slow |

## More candidates (unranked)

- **Race predictor**: finish-time prediction from your own history, with intervals. Same lifecycle as week 4.
- **Workout classifier**: label unlabelled activities (easy, tempo, intervals) from heart-rate shape. Clustering, then a small classifier.
- **Anomaly watch**: flag days where resting heart rate or heart-rate variability leaves your own normal band. Tiny model, high personal value.
- **Notes RAG MCP**: `aprender-rag` over a folder of Markdown and PDFs, served to Claude Code.
- **Commit triage**: classify your own repos' commits (fix, feature, chore) with a fine-tuned small model; charts in suyo.
- **Course demo binary**: one executable with a quantized model embedded, used as the worked example in a Sovereign AI course module.

## Decisions needed before week 1

| # | Decision |
|---|---|
| B1 | One incubator repo, or one repo per project from day one? |
| B2 | Is suyo 0.1 allowed to ship with its plain-JavaScript UI, with the TypeScript port as its own later week? (AGENTS.md D1) |
| B3 | Which three Garmin quantities do you want forecast in week 4? |
