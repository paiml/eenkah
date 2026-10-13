# Rules not yet tested, with the reason

`cargo xtask trace` accepts a rule ID without a test only if it is listed here.
Remove a line as soon as the rule has a test.

| ID | Reason and milestone |
|---|---|
| INS-1 | Insight writer (M6) not started; the product runs on template sentences (INS-3) |
| INS-2 | M6: sentence verifier |
| INS-4 | M6: there is no free-text input anywhere today; enforced by construction, test comes with the writer |
| INS-5 | M6 |
| INS-7 | M6 |
| INS-8 | M6: fine-tuning and pass-rate publication |
| LRN-1 | Interrupt-safety is by construction (temp file + rename, one transaction per target); needs a kill-mid-run test |
| LRN-7 | Needs a state-space model (aprender gap G3) |
| LRN-8 | Covered by PRV-2 once that test exists |
| MET-2 | Worked examples exist as `*_by_hand` unit tests; METRICS.md does not show them yet |
| PRV-2 | Needs a network-namespace test runner (`unshare -n`) in `make verify` |
| PRV-3 | Weather is not built; nothing calls out |
| PRV-4 | `eenkah export` / `eenkah purge` not built |
