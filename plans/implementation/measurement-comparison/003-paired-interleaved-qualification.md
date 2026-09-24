# Measurement/Comparison M003 — Paired/Interleaved Experiment Qualification

Status: closed (closure
`plans/closure/measurement-comparison/003-status.md`, commit `49a4105`)

Repository baseline: `2e35ab8`

Source roadmap:

- `plans/subsystems/measurement-comparison-roadmap.md` — M003

Closed prerequisites:

- Measurement M001 (metric vocabulary/trial normalization, hosted-qualified);
- Measurement M002 (immutable baselines, comparability, unpaired trial-level
  bootstrap policy v1, `eggbench compare`, exits 6/7/8);
- Local Runner M003 (sequential trial execution, reset hooks, cancellation);
- External Oracles M001+M002 (driver catalog; oracle executors follow the
  shared `InvocationContext.workload`, so per-trial target overrides apply
  to native and external drivers alike).

Controlling ADRs:

- `plans/adrs/ADR-0003-trial-level-comparison-and-regression-verdicts.md`
  (trial is the statistical unit; pair becomes the resampling unit only
  where pair identities exist — they are created here, never invented later)

Long-term requirements:

- `plans/000-long-term-specification.md#10-statistical-comparison`
- `plans/002-long-term-roadmap.md` — Phase 4 (M003)

Primary class: capability/infrastructure.

## 1. Objective

Let one Eggbench bundle contain a drift-controlled paired experiment: two
variant arms (baseline + candidate services, both live for the whole run)
executed under a deterministic alternating schedule, with per-trial arm and
pair identities in evidence, a paired trial-level bootstrap policy, and
descriptive drift diagnostics.

M003 must implement:

1. predeclared paired design in the experiment plan (no post-hoc pairing);
2. deterministic balanced alternating schedule in the runner;
3. per-trial `arm` + `pair_id` evidence (trial-result schema v2);
4. paired bootstrap policy v2 (`eggbench.trial-bootstrap-paired.v1`);
5. drift diagnostics that are descriptive only, never gates;
6. `eggbench compare --paired` with the locked exit-code matrix;
7. methodology documentation.

M003 must not invent pairing for separately-executed bundles, restart
processes to switch arms, support attested-only (unenforced) pairing,
extend trials adaptively, delete outliers, compute p-values, or change any
v1 schema semantics.

## 2. Current repository evidence

At the baseline:

- `ExperimentPlan` schema v1 (`deny_unknown_fields`): single workload
  target, `TrialPolicy.measured` total, no variant concept;
- `ResolvedPlan` schema v1 (`deny_unknown_fields`): single resolved
  workload/subject/topology;
- runner `execute_run` iterates `1..=measured` strictly sequentially;
  executors derive the target exclusively from
  `InvocationContext.workload` + `RuntimeBindings`;
- `TrialExecutionResult` schema v1 (`deny_unknown_fields`): no arm/pair
  fields; version const owned by the runner;
- `BundleManifest` v2 (no `deny_unknown_fields`): single `subject`;
- comparison policy v1 is explicitly unpaired (`comparison.rs`: "Paired
  inference is not attempted"); receipt schema v1; `InputTrial` carries
  terminal status + metrics only;
- `eggbench compare` takes two bundles (or `--absolute-only`);
- no occurrence of pair/interleave/variant-tag/schedule-trials/drift
  semantics anywhere in the tree (verified by sweep 2026-09-24).

## 3. Core invariants

1. Pair identities are created by the runner schedule, never inferred.
2. Both arms are live managed/external services for the whole run; the
   runner switches which service receives load per trial, never which
   process exists. No process restart machinery is added in M003.
3. A paired trial measures exactly one arm; one pair contributes at most
   one scalar per arm per metric.
4. Arms share workload shape, drivers, topology, and testbed by
   construction (one plan, one bundle); only the workload-target service
   and the declared arm subject identities differ.
5. Arms that do not physically differ fail closed (identical target
   service rejected; attested-only pairing rejected).
6. Missing/invalid arm observations exclude their pair; pairs are never
   imputed or split.
7. Practical threshold and uncertainty remain separate; no p-value.
8. Drift diagnostics are descriptive; they never gate.
9. Completed bundles are never modified; policy v1 and receipt v1
   semantics are untouched (v2 is additive).
10. Unpaired comparison of a paired bundle fails closed per metric
    (invalid with a stable reason) instead of silently applying unpaired
    inference to paired data.

## 4. Non-goals

Do not implement: cross-bundle pairing or pair-by-order heuristics;
managed-subject process switching per trial (arms are services, not the
singular subject); arm-specific service config overlays; adaptive
schedules; `ABBA` or other counterbalanced orders (alternating only in
v1); outlier deletion; p-values; BCa/bootstrap-t; multiple-comparison
correction; automatic trial extension; p-hacking affordances such as
re-pairing or pair-dropping options; security correctness gating.

## 5. Plan schema v2 (additive)

`EXPERIMENT_PLAN_SCHEMA_VERSION` remains 1 for unpaired plans. New const
`EXPERIMENT_PLAN_SCHEMA_VERSION_2 = SchemaVersion(2)`. `from_json` /
`from_toml` / `validate` accept 1 and 2. New field with
`#[serde(default)]`:

```text
ExperimentPlan.paired: Option<PairedDesign>
PairedDesign { baseline: PairedArm, candidate: PairedArm }
PairedArm { service: Name, subject: Subject }
```

Validation (stable categories):

- `paired.is_some()` requires `schema_version == 2`
  (`unsupported_version`-adjacent: use `invalid` "unsupported_version"?
  PlanError has UnsupportedVersion(u32) variant — reuse it).
- top-level `subject` must be `Subject::Label` when paired
  (`contradictory_configuration`): the label names the comparison; the
  physical variants are services. (The runner launches nothing for a
  label subject today — verified in spec.rs.)
- `trials.measured` must be even and >= 2 (`invalid_bound`); pairs =
  measured / 2.
- each arm service must be a declared service (`missing_reference`); arm
  services must differ (`contradictory_configuration`).
- arm `subject` must be `Label` or `External` (`unsupported_option`):
  ManagedCommand arms need launch-or-verify semantics that do not exist;
  variant binaries already resolve through the normal managed-service
  machinery, and arm subjects are provenance declarations, never launched
  or digested by the runner.
- the plan-level workload target must equal one arm's service or a
  neutral entry point? Decision: the plan workload `target` must equal
  the BASELINE arm service (`contradictory_configuration` otherwise), so
  unpaired-minded readers see the control arm as the nominal target.

`to_json`/`to_toml` preserve the plan's own version (no silent upgrade).

## 6. Resolution (schema v2, additive)

`RESOLVED_PLAN_SCHEMA_VERSION` becomes 2 for new writes. `ResolvedPlan`
gains `#[serde(default)] paired: Option<ResolvedPairedDesign>`:

```text
ResolvedPairedDesign {
  baseline: ResolvedPairedArm { service: Name, subject: Subject },
  candidate: ResolvedPairedArm { service: Name, subject: Subject },
  schedule: Name,  // "alternating-baseline-first" (PAIRED_SCHEDULE_V1)
  pairs: u32,
}
```

Resolution re-validates arm services and runs the existing driver
compatibility check (`IncompatibleService`) against BOTH arm services, so
a driver that can only drive one variant fails closed at resolution, not
mid-run. `source_plan_schema_version` records 1 or 2.
`validate_resolved_plan_bytes` accepts resolved schema 1 and 2.

## 7. Runner schedule v1

Schedule: strictly alternating arms starting with baseline
(`PAIRED_SCHEDULE_V1 = "alternating-baseline-first"`). Trial `n`
(1-based): pair `(n+1)/2`, arm baseline when `n` odd, candidate when `n`
even. Warmups alternate arms round-robin starting with baseline and carry
no pair identity (odd warmup counts are legal; the imbalance is
deterministic and documented).

Per measured trial the runner builds the effective workload as the
resolved workload with target replaced by the arm service
(`Workload::with_target`, new core helper). Executors are untouched: they
already derive everything from `InvocationContext.workload`, so native
(Eggfetch) and external (oha/h2load/iperf3) drivers follow arms
automatically — including oracle URL derivation from target bindings.

- `InvocationKind::Measured` gains `arm: Option<TrialArm>` (`None` only
  for legacy/unpaired paths in tests). `derive_seed` namespaces the arm
  so paired trials never share a seed stream with each other.
- `TrialExecutionResult` schema v2: `#[serde(default)] arm:
  Option<TrialArm>`, `#[serde(default)] pair_id: Option<u32>`.
  `TrialArm::{Baseline, Candidate}`, snake_case, owned by core
  (`evidence.rs`) so comparison can read it. Warmup/telemetry-failure
  results in paired runs carry the trial's arm/pair tags (they are still
  that trial's facts).
- Reset policy, cooldowns, timeouts, cancellation, and phase accounting
  apply per trial exactly as today; the schedule changes order only.
- Per-arm subject snapshots: the top-level label subject stages
  `subject.json` as today; each arm subject (Label/External,
  declared-only) stages `subject-arm-baseline.json` /
  `subject-arm-candidate.json` under `ArtifactRole::Subject`.
- Manifest gains `#[serde(default)] paired: Option<PairedRunRecord>`
  (stays manifest v2 — no deny attribute to violate):
  `{ schedule, pairs, baseline_service, candidate_service,
  baseline_subject, candidate_subject }`.

Preflight fails closed when: measured is odd, arm services are not both
in the runtime topology bindings, or cancellation precedes scheduling.

## 8. Paired comparison policy v2

New policy `COMPARISON_POLICY_V2 = "eggbench.trial-bootstrap-paired.v1"`,
method string `"paired-trial-bootstrap-percentile-95"`. Receipt schema v2:
all v1 fields unchanged plus `#[serde(default)] paired:
Option<PairedComparisonSection>`:

```text
PairedComparisonSection {
  schedule, pairs_complete, pairs_excluded: Vec<ExcludedPair { pair_id, reason }>,
  drift: DriftDiagnostics { pair_effects: Vec<f64>, first_half_mean, second_half_mean, trend },
  statistical_method, resamples, base_seed,
}
```

Entry: `compare_paired(input: &ComparisonInput, options:
&ComparisonOptions) -> ComparisonReceipt`. The single bundle supplies both
sides: candidate trials = candidate-arm trials, baseline trials =
baseline-arm trials. Receipt `candidate_identity == baseline_identity`
(same bundle); `baseline_reference` is `Bundle` with the same identity and
path; comparability is evaluated with the input on both sides (testbed,
workload, driver, topology match by construction; arm subject identities
are recorded as expected-differing provenance, never mismatches) plus the
required `StrictSameTestbed`-compatible outcome under any plan policy —
a paired bundle is inherently same-testbed evidence, but the candidate
policy is still recorded truthfully.

Per metric (gates come from the one plan, as in M002):

- complete pair = both arms `Completed` + both `Observed` + both finite
  + both domain-valid (strictly positive for relative gates). Anything
  else excludes the pair with a stable reason; pairs are never split.
- required complete pairs: `max(plan.min_trials, 5)` for
  `StatisticalRelative`; recommendation 7+ stays diagnostic.
- paired bootstrap: per-pair oriented log-difference `d_i`
  (lower-is-better: `ln(candidate) - ln(baseline)`; higher-is-better:
  negated); 10,000 resamples of pairs with replacement via the owned
  SplitMix64; mean per resample; `exp() - 1` transform; documented
  percentile interval at 95% with the exact v1 quantile indexing.
- verdicts reuse v1 rules (lower > threshold fail; upper <= threshold
  pass; crossing inconclusive; short/degenerate evidence invalid).
- `RelativeRegression` (non-statistical) on paired input: geometric means
  over complete-pair arm values only (pairs with both arms present), same
  orientation math as v1. `Absolute` on paired input: INVALID —
  arm-agnostic averaging would mix variants; the stable reason directs to
  paired comparison or an unpaired run.
- drift diagnostics per metric with a relative gate: pair effects
  `e_i = oriented ratio - 1` in execution order; first/second half means
  (second half may be one pair longer when odd — documented); `trend` is
  the sign of `second_half_mean - first_half_mean` (`up`/`down`/`flat`
  with exact-zero `flat`). Descriptive only.

Seed: `FNV(manifest_sha256 | POLICY_V2 | metric)` (single bundle — no
baseline digest); explicit `--seed` still overrides; per-metric seeds
recorded as in v1. No OS randomness.

Unpaired `compare` with a paired candidate bundle: every metric evaluates
Invalid with reason `paired-evidence-requires-paired-comparison`
(absolute-only mode included). Aggregate follows v1 precedence to
Invalid. This is additive behavior change affecting only new paired
bundles; all v1 bundles compare exactly as before.

## 9. CLI surface (additive)

- `eggbench compare --paired <bundle.eggb>` (conflicts with
  baseline/candidate/alias/absolute-only; `--output`, `--seed`, `--json`,
  `--quiet` as today). Same exit matrix: 0 pass/descriptive, 6 fail, 7
  inconclusive, 8 invalid, 5 evidence/IO.
- `eggbench doctor` on a paired plan reports the paired design (schedule,
  pairs, arm services) in the payload; resolution failures keep exit 3.
- No `run` flags: pairing is predeclared in the plan.

## 10. Comparability, testbed, and topology notes

- Environment fingerprint is captured once per bundle (testbed-level) and
  covers both arms; no per-trial fingerprint is added.
- Topology comparison naturally matches (one resolved topology); the two
  arm services are both present in it.
- Driver compatibility is proven at resolution for both arms; the receipt
  records the single driver identity with a both-arms-checked note.
- Subject revision/digest differences between arms are expected and
  recorded in the paired manifest record + receipt arm provenance, never
  surfaced as comparability mismatches.

## 11. Property and fixture verification

- schedule: alternation, pair ids, warmup round-robin (incl. odd warmup),
  effective target per trial, arm-namespaced seeds differ for the two
  trials of a pair;
- validation matrix: odd measured; equal arm services; unknown arm
  service; non-label top subject; ManagedCommand arm subject; v1 plan
  with `paired` key rejected at parse; v2 round-trip preserves version;
- resolution matrix: driver incompatible with one arm fails;
  source_plan_schema_version recorded;
- result v1 still parses (defaults); result v2 round-trips;
- paired bootstrap fixtures: clear regression fail (both directions),
  non-regression pass, threshold crossing inconclusive, short pairs
  invalid, zero/negative invalid, unsplit pairs (one arm missing excludes
  the pair; aggregate counts prove no imputation);
- determinism: same bundle + seed → byte-equivalent receipt; derived
  seeds stable; metric order irrelevant;
- drift fixtures: monotone drift yields correct trend sign and half
  means; drift never changes a gate verdict (same verdict with/without
  drift section);
- unpaired-compare-of-paired-bundle invalid matrix (incl. absolute-only);
- golden receipts v2: paired pass / fail / inconclusive / invalid /
  drift-present;
- runner scheduling test with a recording stub executor (per-trial target
  + arm + pair assertions); e2e paired `run` + `compare --paired` at CLI
  level only if the existing test harness supports live execution without
  new fixtures (no new external fixtures may be invented — reuse);
- full matrices: fmt, check, clippy pedantic (features on/off), tests
  all-features + default-features, MSRV 1.89 check+test, `cargo tree`
  (no new dependencies — SplitMix64/FNV are already owned), `git
  diff --check`.

## 12. Documentation

- new `docs/paired-experiments.md`: methodology (why interleaving
  controls drift, schedule definition, predeclared-design rule,
  pairing/inference rules, drift interpretation, limitations incl.
  managed-subject switching and external-arm future work);
- update `docs/comparison.md` (policy v2 math + worked paired example),
  `docs/cli.md` (`--paired`, doctor paired payload),
  `architecture/core.md` + `architecture/runner.md` (schedule, result
  v2), `docs/eggstack-http.md` if arm-service examples touch it (no),
  README compare example;
- roadmap/registry on closure.

## 13. Acceptance criteria

M003 closes only when: paired plans validate under schema v2 with the
full negative matrix; resolution proves both-arm driver compatibility;
runner alternation/pairing/warmup behavior is stub-proven with arm
namespaced seeds; result v1/v2 interop holds; paired bootstrap verdicts
match fixtures with byte-stable determinism; drift is present,
correct-signed, and never gating; unpaired compare of paired bundles is
invalid with a stable reason; golden v2 receipts exist; CLI matrix
(`--paired`, exits 6/7/8, doctor payload) is locked by tests; docs
describe the methodology honestly including limitations; broad
verification is green incl. MSRV; no new dependencies.

Closing M003 unblocks the first security/performance policies that consume
paired verdicts and qualifies interleaved methodology for Eggstack
subjects.

## 14. Stop conditions

Stop for planning review if: honest pairing requires mutating finalized
bundles; alternating requires process restart machinery; executors need
per-arm code changes (they must not — target override rides
`InvocationContext.workload`); policy v2 needs anything beyond owned
SplitMix64/FNV; receipt v2 breaks any v1 golden; plan v2 breaks any v1
fixture parse; or `--paired` needs new exit codes.

## 15. Closure evidence required

Implementation commits; schedule definition; schema versions touched
(plan v2, resolved v2, result v2, receipt v2, manifest v2+paired record)
with interop proof; policy identifier + exact paired math; seed
derivation; validation/resolution negative matrices; scheduling proof;
bootstrap/drift fixtures; determinism evidence; CLI/exit matrix; golden
receipts; dependency tree; Rust 1.89 result; hosted CI status (not run
locally — record as follow-up); known limitations; unresolved
findings/severity; disposition.

## 16. Planning review checklist (§registry)

1. spec/terminology refs correct (M003 roadmap §, ADR-0003 trial unit);
2. hard deps closed (M002, runner M003, oracles substrate);
3. no ownership duplication (executors untouched; services owned by
   runner; subjects by plan);
4. timed/untimed phases explicit (warmup untimed/untagged; measured timed
   per trial as today);
5. experimental unit explicit (pair is the resampling unit; trial stays
   the measurement unit);
6. status/verdict not conflated (terminal status per trial; verdicts only
   in receipts);
7. lifecycle/cancellation explicit (per-trial as today; arm switch is a
   target selection, not a lifecycle event);
8. filesystem/env/platform truthful (artifact paths listed; no new
   platform demands);
9. security/correctness effects explicit (none — no security semantics);
10. schema/version effects explicit (§5–8, §13);
11. closure evidence sufficient (fixtures + stub scheduling proof + live
    ground truth where the harness allows — no invented external/served
    fixtures).
