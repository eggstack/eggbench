# Measurement/Comparison M001 — Metric Vocabulary and Trial Normalization — Closure

Disposition: **closed**
Closed: 2026-09-23
Implementation work verified against plan baseline `42f4696f15a9653ccced7d961ec1e513dd7f840b`, on top of M003 implementation commit `59d53774dc6015e385d705af4dee5f063f4aefff`.
Hosted qualification: not run in this pass; local verification only (see §5).

## 1. Requirement-to-evidence matrix

| Plan § / requirement | Evidence | Outcome |
|---|---|---|
| §5 core `metrics.rs`, `METRIC_VOCABULARY_VERSION = 1`, `TRIAL_METRICS_SCHEMA_VERSION = 1`, no Tokio/process/network in core | `crates/eggbench-core/src/metrics.rs`; `Cargo.toml` unchanged (no new deps) | Pass |
| §6 vocabulary table (throughput, latency_min/mean/p50/p90/p95/p99/p999, error/timeout_rate, bytes_sent/received, cpu_percent, rss_bytes) with units/directions; plan contradiction surfaces inconsistency | `builtin_metric()` + `docs/metrics.md` table; plan-unit/direction contradiction yields `invalid` (unit_mismatch/domain_error), never a silent rewrite | Pass |
| §7 aggregation enum incl. percentile basis points, p99.9 = 9990 without float ambiguity | `Aggregation::{Direct,Minimum,Maximum,Mean,Sum,Rate,Ratio,Percentile{basis_points}}`; `validate()` enforces `1..=10000` | Pass |
| §8 observed/missing/invalid states; zero ≠ missing ≠ invalid; finite only; bounded reasons | `ObservationState`, `MissingReason` (3), `InvalidReason` (6); `0.0` observed test vs `source_not_provided` test | Pass |
| §9 `TrialMetrics` DTO with versions, trial id, observations, histograms, error distribution, warnings; deterministic ordering; one record per request | Sorted by metric name / (metric, path) / category; duplicate requests and duplicate normalized names rejected | Pass |
| §10 typed provenance (producer, version, source field, method label, raw paths); no invented paths; no secrets/command lines | `MetricProvenance`; producer derived from resolved workload driver; empty `raw_artifacts` when nothing retained | Pass |
| §11 histogram references without parsing math; same-trial membership; no claimed ghosts | `HistogramReference`; unknown artifact refs dropped (scalar path marks them invalid); referenced path must be manifest-listed | Pass |
| §12 bounded error distribution, deterministic order, nonnegative counts | `ErrorCategoryCount`, cap 32, sorted; descriptive only | Pass |
| §13 `WorkloadOutput` metric seam (metrics, histograms, error counts); bounded; no protocol fields | `orchestration.rs::WorkloadOutput` extended with `metrics/histograms/error_counts`; structural caps enforced via `normalize_trial_metrics` → evidence-error path | Pass |
| §14 artifact-name → path resolution against same-trial map; unknown refs invalid; no arbitrary paths | `artifact_map` built from staged names; unknown scalar refs → `malformed_source_reference`; drivers never supply bundle paths | Pass |
| §15 normalization algorithm (non-completed → missing; exact-name match; plan unit/direction/intent wins; custom aggregation preserved; deterministic; unrequested never gate-eligible) | `normalize_trial_metrics()`; failed-trial test; unrequested-warning test; no unit conversion | Pass |
| §16 exact unit equality, no implicit conversion | `ms`≠`us`, `ratio`≠`percent` tests; ratio domain `[0,1]` enforced | Pass |
| §17 direction/intent copied from plan; driver cannot change | `NormalizedObservation` copies from `MetricRequest`; vocabulary direction contradiction → `invalid` | Pass |
| §18 `trials/NNN/metrics.json` staged after measurement with `TrialArtifact` role; warmups excluded; descriptor membership | `stage_trial()` stages after `measurement_elapsed_ns` captured; warmup test asserts no `warmups/*/metrics.json` | Pass |
| §19 semantic invalid finalizes; structural errors use mandatory cleanup | invalid-unit test finalizes `Completed`; 300-observation overflow test returns `Evidence` with drain + teardown | Pass |
| §20 synthetic `FakeWorkload` producer (valid/zero/missing/duplicate/nonfinite/unit-mismatch/custom/histogram/error cases) | `metrics_by_invocation`, `histograms_by_invocation`, `error_counts_by_invocation` per-invocation inputs | Pass |
| §21 bounds + validation rejections | Caps 256/256/16/8/32/16 + string caps; duplicate/unsupported-version/nonfinite/bad-basis-points/cross-trial refs rejected | Pass |
| §22 `BundleReader::trial_metrics(trial_id)`; manifest-listed membership; legacy `None` | `evidence.rs::trial_metrics()`; legacy `current-v2.eggb` returns `None`; identity mismatch rejected | Pass |
| §23 M003 coordination: inspect tolerates both shapes; no CLI dependency | CLI untouched by M001; `eggbench inspect` verified against a bundle with and without `metrics.json` | Pass |
| Acceptance 12–15: no comparison verdict/statistics; trial is the unit; manifest v2 + trial-result v1 unchanged | No `ComparisonVerdict` code touched; statistical-unit guard (4 vs 10 000 requests → 1 scalar/trial); manifest v2 reader unchanged | Pass |

## 2. Tests/guards run and outcomes

- `cargo fmt --all -- --check` — pass
- `cargo check --workspace --all-targets --locked` — pass
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass
- `EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-features --locked` — pass: **135 passed (13 suites)** — 12 new core metric unit tests + 11 new runner metric integration tests (incl. the statistical-unit guard)
- `cargo +1.89.0 check --workspace --all-targets --locked` — pass
- `cargo tree --locked` — pass; no new dependencies
- `git diff --check` — pass
- CLI end-to-end: `eggbench run smoke.json run.eggb --json` publishes a bundle containing `trials/001/metrics.json` (`missing/source_not_provided` via default fake); `eggbench inspect` verifies and summarizes it.

## 3. Schema/migration/compatibility evidence

- New additive schemas only: metric vocabulary v1, `TrialMetrics` schema v1. Manifest v2 and `TrialExecutionResult` v1 are byte-identical and still readable (legacy `current-v2.eggb` opens, verifies, and yields `trial_metrics() == None`).
- `TrialDescriptor.artifacts` now additionally lists `trials/NNN/metrics.json`; readers that ignore unknown trial artifacts remain compatible.
- Preflight capacity accounts for the extra artifact (`+1` count and `1 024` bytes per measured trial).

## 4. Security and lifecycle evidence

- Normalization runs strictly after the measured interval; `measurement_elapsed_ns` is captured before any metric work.
- Provenance carries no secrets, command lines, or environment dumps.
- Structural metric failures (300-observation overflow proven) return `OrchestrationError::Evidence` through the M002 mandatory drain/teardown tail; no partial bundle is published.
- Failed/cancelled/timed-out trials stage `missing(trial_not_completed)` diagnostics but expose no valid observation a comparison could consume.

## 5. Documentation/operational evidence

- Added `docs/metrics.md` (vocabulary table, states, rules, provenance, layout, statistical unit, boundaries).
- Updated `architecture/core.md` (metrics module ownership), `architecture/runner.md` (M001 normalization section), `docs/evidence-bundle.md` (`metrics.json` layout + legacy behavior), `README.md` (M001 status + metrics doc link).
- Representative JSON captured from a real CLI run (see §2).

## 6. Known limitations

- No comparison, baseline, bootstrap, confidence-interval, or verdict logic (explicitly M002 scope).
- No real workload parser; only `FakeWorkload` produces observations, so production runs currently record `missing(source_not_provided)` until External Oracles/Eggstack drivers land.
- `inspect` lists metric-agnostic summaries; it does not yet display normalized values (M003-tolerated, M002-may-extend).
- Hosted CI (Linux stable, Linux 1.89, macOS, Windows subset) not run in this pass.

## 7. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| None | No unresolved M001 correctness, security, lifecycle, or portability finding | No corrective work required |
| Note | Hosted CI qualification outstanding | Tracked as follow-up; local matrix (fmt/check/clippy/test/MSRV/tree/diff-check) is green |

## 8. Disposition

**Closed.** Metric vocabulary, trial-normalization, staging, reader, synthetic producer, statistical-unit guard, and documentation are landed and regression-tested. Measurement/Comparison M002 (baselines, comparability, statistical gates) is ready for plan authoring against this contract. Manifest v2 and trial-result v1 remain unchanged.
