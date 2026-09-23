# Measurement/Comparison M002 — Baselines, Comparability, and Statistical Gates — Closure

Disposition: **closed**
Closed: 2026-09-23
Implementation commit: `80ff6d1` (feat(comparison): close Measurement M002 baselines, comparability, statistical gates), on top of planning baseline `97b388edcc2b4d54b889ea1604ab6ae56a301a28`.
Hosted qualification: not run in this pass; local verification only (see §5). Four-lane hosted CI remains required before release qualification claims.

## 1. Requirement-to-evidence matrix

| Plan § / requirement | Evidence | Outcome |
|---|---|---|
| §5 core `comparison.rs`, `COMPARISON_RECEIPT_SCHEMA_VERSION = 1`, `COMPARISON_POLICY_V1 = "eggbench.trial-bootstrap.v1"`, `BASELINE_ALIAS_SCHEMA_VERSION = 1`; core stays Tokio/process/network-free | `crates/eggbench-core/src/comparison.rs`; bounded sync `std::fs` reads only; no new dependencies (`cargo tree` unchanged) | Pass |
| §6 stable `BundleIdentity` (manifest version, run ID, SHA-256 of exact `manifest.json` bytes, subject revision/digest presentation-only); derived only after verification; path never identity | `load_comparison_input()` verifies first, then hashes manifest bytes; two paths/same run ID with different digests compare as different identities | Pass |
| §7 typed `BaselineReference::Bundle/Alias` with resolved identity, not alias string | Receipt fixtures `comparison-*.json` record resolved identities | Pass |
| §8 human-managed `*.eggbaseline.json` (version, alias, path, digest, note); explicit path reads; relative resolves vs alias file; verified open; digest equality else fail-closed; no latest-run aliases | `load_baseline_alias()`; CLI test `compare_alias_resolves_and_digest_mismatch_fails` (tampered digest → code 2, bundles untouched) | Pass |
| §9 candidate authoritative for requests/gates/policy; verified input loader returns bounded DTO (identity, resolved plan, env, trial descriptors/results, `TrialMetrics`, driver descriptors); legacy readable but not gateable | `ComparisonInput`/`InputTrial`; legacy `None` metrics → excluded `no_normalized_metrics` | Pass |
| §10 typed `ComparabilityReport`: critical env fields (equal/missing/unqual), warning-only as warnings, informational never gating; workload semantics (trial counts excluded); driver identity; topology excluding subject identity; per-metric semantics | `evaluate_comparability()`; unit tests for each dimension; golden `comparability` blocks | Pass |
| §11 strict/warn/descriptive observably distinct | Strict mismatch → `Invalid` + estimates retained; warn → `Descriptive` + warnings, verdict absent; cross → always `Descriptive`; absolute gates still evaluate; dedicated unit tests | Pass |
| §12 trial selection: only completed + observed; missing/invalid counted with reasons; exact trial ID lists; one scalar per trial; 4-vs-100k-requests guard | `select_trials()`; statistical-unit test (16 histograms + 100k error counts → still 7 samples) | Pass |
| §13 absolute gates: baseline-free; arithmetic mean; orientation; insufficient → invalid; informational/target-range unsupported | `evaluate_absolute()`; unit tests incl. `target_range_absolute_unsupported` | Pass |
| §14 non-statistical relative: baseline required; strictly positive; geometric means via mean(log); oriented degradation; allowance/10_000; invalid on domain/direction | `evaluate_relative()` relative arm; monotonicity property tests both directions | Pass |
| §15 statistical policy v1: min evidence `max(plan.min_trials, 5)` (7+ diagnostic warning); unpaired 10,000 resamples; 95% percentile interval with documented integer quantile indexing; verdict lower>threshold fail / upper≤threshold pass / crossing inconclusive; no p-value | `bootstrap_interval()`; unit tests pass/fail/inconclusive/insufficient/zero; method label `unpaired-trial-bootstrap-percentile-95` | Pass |
| §16 deterministic SplitMix64 owned by policy; explicit `--seed` or derived base seed (digests + policy via FNV-1a) + per-metric seed; both recorded; no OS randomness | `SplitMix64`, `derive_base_seed`, `derive_metric_seed`; byte-equivalence test; `base_seed` + `effective_seed` in receipts | Pass |
| §17 receipt schema v1 with identities, policy, seed, trial IDs, estimates, interval, threshold, verdict | `ComparisonReceipt`/`MetricComparison`; golden fixtures assert full shape | Pass |
| §18 aggregate conservative precedence; diagnostics excluded; descriptive never counted; absent when no gate-eligible verdict | `aggregate()`; precedence covered by pass/fail/inconclusive/invalid/descriptive tests | Pass |
| §19 no bundle mutation; receipt via JSON stdout or `--output`; human summary via CLI discipline; manifest `comparison_verdict` untouched | CLI tests hash manifest bytes before/after compare; `execute_run` untouched | Pass |
| §20 CLI `compare <baseline> <candidate>`, `--absolute-only`, `--alias`, `--output`, `--seed`, `--json/--quiet`; no production resample reduction; additive exits 6/7/8 (descriptive/no-verdict 0, I/O 5); codes 0–5 untouched | `commands/compare.rs`, `ExitCode::{ComparisonFail,ComparisonInconclusive,ComparisonInvalid}`; locked in `cli.rs` + `binary_exit_codes.rs` | Pass |
| §21 `CliOutput::Compare` additive payload (summary + full receipt + receipt path); no ANSI in machine output | `envelope.rs` (receipt boxed for variant-size lint) | Pass |
| §22 safe `BundleReader` helpers; manifest-verified path-safe reads; legacy inspectable with actionable invalid reasons | `load_role_json` via `open_artifact` + role lookup; `ArtifactPath::to_path_buf` widened to `pub(crate)` (same-crate only) | Pass |
| Acceptance 1–17 | All hold; notably trial-count sample semantics, threshold/interval separation, identities/policy/seed/trial-ID recording, precedence, no pass-masquerade, stable JSON + exits, no mutation, no p-value/pairs | Pass |

## 2. Tests/guards run and outcomes

- `cargo fmt --all -- --check` — pass
- `cargo check --workspace --all-targets --locked` — pass
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass (pedantic-clean)
- `EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-features --locked` — pass: **215 passed (18 suites)** — 24 new core comparison tests (incl. determinism, monotonicity, comparability matrix, statistical-unit guard, 6 golden fixtures) + 7 new CLI compare tests + 2 new binary exit-code tests
- `cargo +1.89.0 check --workspace --all-targets --locked` — pass
- `cargo +1.89.0 test -p eggbench-core --all-features --locked` — pass: 63 passed
- `cargo tree --locked` — pass; no new dependencies
- `git diff --check` — pass
- CLI end-to-end: qualification-built bundles compared through `Command::Compare` (pass→0, 30% regression→6, straddling variance→7, 3-trial evidence→8, absolute-only→0, alias→same-as-path, tampered alias→2, missing bundle→5); manifest bytes byte-identical before/after.

## 3. Schema/migration/compatibility evidence

- New additive schemas only: receipt v1, alias v1, policy id v1. Manifest v2, `TrialMetrics` v1, `ResolvedPlan` v1, plan v1 untouched.
- `CliOutput` gains an additive `Compare` variant; envelope schema version stays 1; exit codes 0–5 semantics unchanged.
- Only same-crate visibility change: `ArtifactPath::to_path_buf` private → `pub(crate)`.

## 4. Security and lifecycle evidence

- Comparison performs no spawning, networking, or shelling; reads are manifest-verified, path-safe, and bounded (8 MiB manifest cap, 256-metric receipt cap).
- Alias digest mismatch fails closed before any comparison math; no bundle is ever opened for writing.
- Secrets never enter receipts (identities carry digests, not paths/credentials beyond the supplied reference strings).

## 5. Documentation/operational evidence

- Added `docs/comparison.md` (exact policy-v1 math with worked 30%/1% examples), `docs/baselines.md` (identity, alias format, policies, receipts).
- Updated `architecture/core.md` (comparison ownership), `docs/cli.md` (compare usage + codes 6/7/8), `docs/evidence-bundle.md` (standalone receipts), `README.md` (compare examples + doc links).
- Six versioned golden receipts at `crates/eggbench-core/tests/golden/comparison-{pass,fail,inconclusive,invalid-comparability,descriptive-cross-testbed,absolute-only}.json`, hand-verified (degradations 0.01/0.30/straddling CI/invalid/descriptive/absolute).

## 6. Known limitations

- No paired/interleaved scheduling (explicitly M003 scope); bootstrap is unpaired only.
- No p-values, BCa/bootstrap-t intervals, multiple-comparison correction, or adaptive trial extension (explicit non-goals).
- Alias files are single-path references; no historical-baseline search or mutable baseline database.
- Hosted four-lane CI not run in this pass.
- `hostname`-style equal informational fields are listed in comparability testbed output (diagnostic, never gating).

## 7. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| None | No unresolved M002 correctness, security, lifecycle, or portability finding | No corrective work required |
| Note | Hosted CI qualification outstanding (Linux stable, Linux 1.89, macOS, Windows) | Tracked as follow-up; local matrix is green incl. MSRV core tests |

## 8. Disposition

**Closed.** Immutable bundle identities, digest-pinned aliases, typed comparability with three observably distinct environment policies, deterministic trial-level bootstrap policy v1, per-metric and aggregate verdicts, standalone receipts, `eggbench compare` with stable JSON and additive exits 6/7/8, golden fixtures, and documentation are landed and regression-tested.

**Unblocks:** Measurement M003 paired/interleaved qualification and the first security/performance policies that consume aggregate comparison verdicts.
