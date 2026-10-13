# DOGFOOD: what eenkah proved and found in aprender

Pinned release: `aprender-core = "=0.70.3"` (crates.io), `default-features = false`,
feature `parallel` (`format-signing` dropped 2026-10-12: no model files are written, LRN-3). CLI checked against `apr 0.70.3`.

## M0 probe: the [U] items in EENKAH-SPEC §9

| Need | Result | Evidence |
|---|---|---|
| Regression with regularisation | **Works.** `aprender::prelude::Ridge` is the champion for both forecasts on real data | `crates/eenkah-learn/src/model.rs`, Model view |
| Boosted trees | **Gap G2.** Only `GradientBoostingClassifier`; no regressor | aprender `tree/gradient_boosting.rs` |
| Random forest regression | **Works.** `RandomForestRegressor` (seeded, deterministic) is tried as family 2 | `model.rs` |
| Quantile loss | **Gap G2.** No quantile/pinball loss anywhere | aprender `loss/mod.rs` |
| State-space / exponential smoothing | **Gap G3.** Only ARIMA; it is f64-only and not `Serialize`, so it cannot be saved | aprender `time_series/mod.rs` |
| Conformal intervals | **Gap G1.** None. eenkah carries a 60-line adaptive-conformal stopgap in `eenkah-learn/src/interval.rs` (see "Deviations") | `interval.rs` test `coverage_approaches_target_on_shifting_noise` |
| Signed `.apr` save/load | **Worked** (with gaps G4/G5 below; tampering was detected). No longer used: since 2026-10-12 every model is rebuilt in memory on each run and only results are kept (LRN-3) | test `lrn3_no_model_file_is_stored_and_every_run_records_its_lineage` |
| Embedding with `include_bytes!()` | Not used yet: personal models are rebuilt from the athlete's data on every run, so there is nothing to embed; embedding is for the insight writer (M6) | — |
| `apr inspect` / `apr validate` in CI | **Gap G4**: both reject files written by aprender-core 0.70.3's own `save_signed` | below |
| Fine-tune / quantize / serve the insight writer | Not started (M6) | — |

## Gaps to file in paiml/aprender

Nothing has been filed yet. Filing is an outward action and needs the owner's go-ahead.

| # | Gap | Impact on eenkah | Proposed fix in aprender |
|---|---|---|---|
| G1 | No conformal prediction | FC-3 needs intervals; carried as an isolated stopgap | `aprender::conformal::{SplitConformal, AdaptiveConformal}` over any `Estimator` |
| G2 | No gradient-boosted **regressor**, no quantile loss | LRN-6's second family is a random forest instead | `GradientBoostingRegressor` with `Loss::{Squared, Huber, Quantile(q)}` |
| G3 | No exponential smoothing / Kalman / state-space; ARIMA cannot be persisted | LRN-7 (fit fitness/fatigue time constants) not started | `time_series::{ETS, LocalLevel}` with `Serialize` |
| G4 | `format::save_signed` in aprender-core 0.70.3 writes legacy `APRN` v1; `apr inspect`/`apr validate` 0.70.3 reject it: "Only APR v2 (APR\0) is supported" | Cannot use `apr validate` in CI on eenkah's models | Make `save*` write v2, or keep the CLI reading v1 |
| G5 | `.apr` metadata is not deterministic by default: `created_at` is wall-clock and the metadata maps are `HashMap`s | LRN-5 needed a fixed `created_at` and empty maps | `BTreeMap` for metadata maps; `SaveOptions::deterministic()` |
| G6 | README dependency example is stale (`aprender = "0.35"`, `aprender::linear_regression`, which does not exist); plain `aprender = "0.70"` pulls in the CLI stack by default | First-hour friction | README shows `aprender-core` with `default-features = false` |
| G7 | Estimators are `f32` only | Health data is cast to f32; harmless at these scales | Generic over `Float` |
| G8 | No offline change-point detection | Interval detection inside sessions (Vekta-style) not attempted; the start of a normal-level shift is placed by a 20-line best-split search in `shift.rs` | `PELT` / binary segmentation |
| G9 | `NormalInverseGamma` has no predictive interval: `posterior_predictive()` returns only μₙ, and `credible_interval_mu` is the interval for the **mean**, which narrows as nights accumulate | Model 1 computes the Student-t predictive range from the public posterior parameters (`normal.rs`, ~15 lines) | `posterior_predictive_interval(level)` returning the t interval |
| G10 | `credible_interval_mu(level)` silently uses z = 1.96 (95 %) for any level other than 0.90, 0.95 or 0.99 | Would have given an 80 % range that is really 95 %; not used | Compute the quantile, or return an error for unsupported levels |
| G11 | The streaming drift detectors (`online::drift::{PageHinkley, ADWIN, DDM}`) take only `add_element(error: bool)`, and Page-Hinkley detects increases only | Cannot watch a measured value such as resting heart rate; model 3 uses the window detector `metrics::drift::DriftDetector` instead | `add_value(f64)` and a two-sided Page-Hinkley |
| G12 | `GaussianMixture::score` sums **normalised** responsibilities, so the "log-likelihood" is always 0 | Cannot choose the number of session groups by likelihood/BIC; model 6 uses `KMeans` + `silhouette_score` | Sum the unnormalised weighted densities (log-sum-exp) before normalising |

## Proved in a shipped binary (measured on the owner's data, 2026-10-10)

- Import: 13,944 Garmin files (2023-01-12 → 2026-10-09) in 14.6 s, 0 failures, every skipped file counted.
- Training: walk-forward backtest of 5 families × 2 targets over ~1,240 days in 9.7 s on 16 cores (137 s on one core). Peak memory and time are stored per run.
- Both learned Ridge models beat every personal baseline in backtest, by small margins:
  - HRV: mean absolute error 0.125 ln-ms vs 0.131 for the best baseline.
  - Resting HR: 2.016 bpm vs 2.039 bpm.
  - The adaptive intervals covered 80 % of outcomes at a stated 80 %.
- Day attribution was checked against two independent sources:
  - HRV from `HRV_STATUS.fit` equals the Sleep JSON's `avgOvernightHrv` on all 359 overlapping days.
  - Sleep scores agree on 1,318 days.

## Personal models (2026-10-12, measured on the owner's data: 328 nights, 139 rides)

All four are rebuilt from scratch after every import; nothing trained is stored.

| Model | aprender | Rebuild time (48 threads / 1 thread) | Result on the owner's data |
|---|---|---|---|
| 1. Normal ranges, 8 signals | `NormalInverseGamma` | 0.8 ms / 0.9 ms | 81 % of nights inside the stated 80 % ranges (HRV 76 %, resting HR 77 %, sleep score 86 %) |
| 3. Shifts in the normal | `metrics::drift::DriftDetector` | 1.0 ms / 1.0 ms | 12 shifts in a year across 8 signals, e.g. resting HR 54 → 51 bpm from late July 2026 |
| 2. Unusual nights | `IsolationForest` + `LocalOutlierFactor`, retrained weekly, walk-forward | 15 ms / 130 ms | 14 of 261 scored nights (5.4 %); no sick marks yet to check lead time |
| 6. Session types | `KMeans` (k = 2–5 by `silhouette_score`), `RandomForestClassifier` once there are 15 corrections | 1.7 ms / 1.5 ms | 5 groups (silhouette 0.39): 50 Easy, 57 Steady, 17 Long, 15 Intervals; 11 runs too few to group |

Full `eenkah train` (two forecasts with walk-forward backtests plus the four personal models): 0.8 s on 48 threads, 3.0 s on one.

## Deviations from EENKAH-SPEC, decided by the owner or forced by the above

| Spec | What happened | Why |
|---|---|---|
| §4 T4 parked | **Built** (`eenkah-connect`): `eenkah login` signs in the way the Connect mobile app does (SSO JSON sign-in, MFA, renewable DI OAuth2 token). `eenkah import` downloads only what is new. Plain HTTP only: no CAPTCHA solving, no TLS spoofing | Owner decision 2026-10-10 |
| AGENTS.md references (§3, §4, §8, D4, invariant 6) | `AGENTS.md` in this repo is the weekly backlog; EENKAH-SPEC is treated as the rules | Owner decision |
| §3 `ui/` React + TypeScript | Done. `ui/` is a strict React + TypeScript app, and suyo's web UI was ported to React + TypeScript; eenkah uses its library from source. No vanilla JS remains | Owner decision 2026-10-10 |
| §9 "never worked around inside eenkah" | `interval.rs` (G1) is a stopgap, isolated behind one type | No interval = no forecast under design principle 3; replace when aprender ships G1 |
| §9 "never worked around inside eenkah" | `normal.rs` computes the predictive range from `NormalInverseGamma`'s posterior (G9); `shift.rs` places a shift's start by best split (G8) | Small, isolated, each named in its module doc; replace when aprender ships them |
| LRN-3, LRN-4 | Reworded: models are rebuilt on every run and never stored; an earlier champion is restored by rebuilding it from the data up to its last day | Owner decision 2026-10-12: training takes under a second, so stored model files added nothing |
| FC-4 | Fixed 2026-10-12: a session's max-HR and resting-HR fallbacks used every session, including later ones, so loads and zones changed as data arrived. They now use only what was known by the session (`derive.rs`); test `fc4_a_sessions_load_does_not_change_when_later_sessions_arrive` | Found while testing LRN-4 |
| §6 suyo "write no new chart code" | Three chart types were added **to suyo**: `streams` (Vekta multi-stream with a selected-window stats table), `quadrant` and `status` (traffic lights) | Owner chose these Vekta ideas; built where the spec puts charts |
