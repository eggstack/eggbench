# Measurement M003 — Paired/Interleaved Experiment Qualification — Closure

Disposition: **closed**
Closed: 2026-09-24
Implementation commit: (this commit) on top of planning baseline `2e35ab8`
(plus authored plan
`plans/implementation/measurement-comparison/003-paired-interleaved-qualification.md`).
Hosted qualification: not run in this pass; local verification only. The
four-lane hosted CI remains required before release qualification claims.

## 1. Requirement-to-evidence matrix

| Plan § / requirement | Evidence | Outcome |
|---|---|---|
| §1 predeclared paired design, alternating schedule, arm+pair evidence, paired bootstrap, drift diagnostics, `compare --paired`, methodology docs | Plan schema v2, runner schedule, result v2, policy v2, `docs/paired-experiments.md` | Pass |
| §1 no post-hoc pairing, no restart machinery, no attested pairing, no adaptive trials, no p-values, no v1 semantic change | Rejection paths + unchanged v1 goldens (byte-stable) | Pass |
| §3.1 pair identities created by schedule, never inferred | `paired_assignment` in runner; `select_pairs` joins only tagged trials | Pass |
| §3.2 both arms live, switch = target override, no restart | `effective_workload` via `Workload::with_target`; executors untouched | Pass |
| §3.3 one trial measures one arm | `select_pairs` classifies per arm; duplicates excluded | Pass |
| §3.4 shared shape/drivers/topology/testbed | One plan, one bundle; resolution proves both-arm driver compatibility | Pass |
| §3.5 physically identical arms fail closed | Distinct-service validation; managed arm subjects rejected | Pass |
| §3.6 pairs never imputed or split | `pair_incomplete` whole-pair exclusion; unsplit-pairs test | Pass |
| §3.7–3.8 threshold/uncertainty separate; no p-value; drift never gates | Receipt fields; drift-descriptive test (pass verdict with Up drift) | Pass |
| §3.9 bundles immutable; v1/v1 untouched | manifest bytes unchanged in e2e; v1 golden receipts byte-stable | Pass |
| §3.10 unpaired compare of paired bundle fails closed | `paired_evidence_requires_paired_comparison`, exit 8 e2e | Pass |
| §5 plan schema v2 validation matrix | 10 plan unit tests (round-trip, version guard, 6 fail-closed cases, `with_target`) | Pass |
| §6 both-arm driver compatibility at resolution | `IncompatibleService` retype test; schedule/pairs/source-version test | Pass |
| §7 alternating schedule, warmup round-robin, arm tags, arm seeds, arm snapshots, manifest record, preflight | Runner stub tests (alternation, targets, tags, seeds, odd warmup, odd-count preflight); e2e manifest + snapshot asserts | Pass |
| §8 paired policy v2 math, verdicts, seeds, invalid paths | 16 comparison unit tests + 3 v2 goldens; determinism + seed tests | Pass |
| §9 CLI `--paired`, exits 6/7/8, doctor payload, no run flags | 5 CLI e2e tests (fail/pass/invalid/usage/doctor) | Pass |
| §10 testbed/topology/driver notes | Single fingerprint; self-comparability; both-arms-checked resolution | Pass |

Planned-but-adjusted (recorded, not hidden):
- Warmup arm alternation is round-robin from baseline; odd warmup counts
  leave a deterministic documented imbalance (locked by test).
- `compare_paired` takes the bundle path for `BaselineReference::Bundle`
  provenance (inputs carry no path).
- Paired `RelativeRegression` has no minimum-pair requirement, mirroring
  v1's count-free non-statistical gate; `StatisticalRelative` requires
  `max(min_trials, 5)` complete pairs.
- `evaluate_comparability(input, input)` is reused for the paired
  comparability report (matching by construction); arm differences live
  in the paired section as provenance, never mismatches.
- The v1 `compare()` emits receipt schema v1 (not v2) so v1 golden bytes
  prove the v1 path is untouched; only paired receipts carry schema v2.

No stop condition (§14) triggered: no bundle mutation was needed;
alternating needed no restart machinery; executors needed no per-arm
changes (target override rides `InvocationContext.workload` as required);
policy v2 uses only owned SplitMix64/FNV; no v1 golden changed; plan v2
broke no v1 fixture parse; no new exit codes were needed.

## 2. Tests/guards run and outcomes

- `cargo fmt --all -- --check` — pass
- `cargo check --workspace --all-targets --locked` — pass
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass (pedantic-clean)
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — pass (feature-off clean)
- `EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-targets --all-features --locked` — pass: **335 passed (18 suites), 0 failed**, incl. 10 plan-validation tests, 2 resolution tests, 2 evidence interop tests, 16 paired comparison unit tests + 3 v2 goldens, 3 runner scheduling tests, 5 CLI paired e2e tests
- Feature-off `cargo test --workspace --all-targets --locked` — pass: **295 passed (18 suites), 0 failed**
- `cargo +1.89.0 check --workspace --all-targets --all-features --locked` — pass
- `cargo +1.89.0 test --workspace --all-targets --all-features --locked` — pass: 335 passed (18 suites), 0 failed
- `cargo tree --locked` — zero new dependencies (paired bootstrap reuses owned SplitMix64/FNV; `getrandom`/`fastrand` are pre-existing tempfile/uuid transitives)
- `git diff --check` — pass
- Live e2e: real paired `run` (12 trials, fake workload) → verified bundle with arm/pair tags, arm snapshots, manifest record → `compare --paired` exits 6/0 and unpaired `compare` exits 8, all with byte-identical source bundles

## 3. Schema/migration/compatibility evidence

- Plan schema v2 (adds `paired`); v1 plans validate exactly as before; v1+paired fails closed at the version guard (`UnsupportedVersion(1)`).
- Resolved schema v2 (adds `paired`); readers accept 1|2; new writes are v2; fixture `sample-resolved-plan.json` updated.
- Trial-result schema v2 (adds defaulted `arm`/`pair_id`); v1 evidence parses with absent tags; warmup records keep their own v1.
- Manifest stays v2 (no deny attribute) with additive optional `paired` record; legacy v1 view maps to `None`.
- Receipt schema v2 (adds `paired` section); v1 receipts emit schema v1 with byte-stable goldens; v1 goldens unchanged; 3 v2 goldens added.
- Comparison policies: v1 `eggbench.trial-bootstrap.v1` immutable (percentile-extraction refactor proven byte-identical by goldens); v2 `eggbench.trial-bootstrap-paired.v1` additive.
- CLI: additive `--paired` flag; additive `paired` doctor payload (skipped when absent, so unpaired payloads are byte-identical); exit matrix unchanged (6/7/8 reused).
- Behavior change (new bundles only): unpaired `compare` of a paired bundle yields per-metric Invalid (`paired_evidence_requires_paired_comparison`); absolute gates over paired evidence are Invalid (`absolute_gate_unsupported_for_paired_evidence`).

## 4. Security and lifecycle evidence

- No new process, network, credential, or privilege surface: arm switching
  is a workload-target selection, not a lifecycle event.
- Reset policy, cooldowns, timeouts, cancellation, and phase accounting
  apply per trial exactly as before; the schedule changes order only.
- Arm subjects are declared-only snapshots (Label/External); the runner
  never launches or digests them, and managed-command arms fail closed at
  plan validation and at bundle preparation.
- `FakeWorkload` gains invocation target/seed recording (test-only).

## 5. Documentation/operational evidence

- Added `docs/paired-experiments.md` (why-interleave methodology,
  predeclared-design rule, schedule definition, inference rules, worked
  example, drift interpretation, limitations).
- Updated `docs/comparison.md` (policy v2 section), `docs/cli.md`
  (`--paired`, invalid-mixing rule, doctor payload),
  `docs/experiment-plan.md` (schema v2), `architecture/core.md`,
  `architecture/runner.md`, README (M003 section).

## 6. Known limitations

- Alternating order only; no ABBA/counterbalanced or adaptive schedules.
- Arms must be co-runnable services; externally-switched variants with no
  runner-observable difference are rejected rather than recorded.
- No arm-specific service config overlays; no managed-subject switching.
- Bundle-level environment fingerprint only; no per-trial fingerprint.
- Cross-bundle pairing (including pair-by-order heuristics) unsupported
  by design.
- Qualification harness gained a fake Service descriptor (test-only) so
  service-declaring plans resolve deterministically.
- Hosted four-lane CI not run in this pass.

## 7. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| None | No unresolved M003 correctness, security, lifecycle, or portability finding | No corrective work required |
| Note | Hosted CI qualification outstanding (Linux stable, Linux 1.89, macOS, Windows) | Tracked as follow-up; local matrix is green incl. MSRV 1.89 tests |

## 8. Disposition

**Closed.** Paired/interleaved qualification is landed: predeclared v2
designs, deterministic alternating execution with arm/pair evidence,
paired trial-level bootstrap with honest guards, descriptive drift
diagnostics, CLI surface on the locked exit matrix, and methodology
documentation — all regression-tested with v1 behavior proven unchanged.
M003 unblocks the first security/performance policies that consume paired
verdicts and qualifies interleaved methodology for Eggstack subjects.
