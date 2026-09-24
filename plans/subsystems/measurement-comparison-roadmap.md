# Measurement and Comparison Roadmap

Status: closed — M001 qualified; M002 hosted-qualified; M003 closed/hosted-qualified by post-M003 hosted qualification corrective C002 (run 36029547565)

Long-term references:

- plans/000-long-term-specification.md — metrics, statistical comparison, baselines
- plans/001-terminology-and-domain-model.md
- plans/002-long-term-roadmap.md — Phase 4

Related ADRs:

- plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md
- plans/adrs/ADR-0003-trial-level-comparison-and-regression-verdicts.md

## 1. Purpose and ownership boundary

This subsystem owns normalized metric definitions, trial summaries, baseline references, testbed comparability, effect estimates, uncertainty intervals, practical thresholds, and aggregate verdicts.

It does not own traffic generation or target-specific security correctness semantics.

## 2. Invariants

- Trial is the default statistical unit.
- Request count is not silently treated as independent run count.
- Units and directionality are explicit.
- Practical threshold is independent of confidence.
- Comparison policy is versioned.
- Missing or invalid trials are never silently imputed.
- Cross-testbed gating is disabled by default.
- Primary metrics are declared in the experiment before candidate evidence is interpreted.

## 3. Non-goals

No general statistics framework, automatic outlier deletion, p-value leaderboard, arbitrary benchmark ranking, or automatic adaptive trial extension in the initial release.

## 4. Target architecture

~~~text
TrialResult[]
   |
normalize metric families
   |
comparability check
   |
baseline pairing
   |
effect + bootstrap interval
   |
metric gate verdicts
   |
run verdict + report
~~~

## 5. Dependency graph

~~~text
Foundation schemas
   |
Local trial evidence
   |
M001 Metric normalization
   |
M002 Baseline/comparability/gates
   |
M003 Paired/interleaved qualification
~~~

## 6. Milestones

### M001 — Metric vocabulary and trial normalization

Define stable units, directionality, source metadata, normalized trial summaries, latency histogram references, error distributions, resource metrics, and raw-versus-normalized provenance.

The metric model must distinguish a missing value from a measured zero.

### M002 — Baselines, comparability, and statistical gates

Implement immutable baseline references, same-testbed policy, absolute and relative gates, deterministic trial-level bootstrap policy v1, practical thresholds, and pass/fail/inconclusive/invalid aggregation.

Comparison receipts must store seed, policy version, trial identities, effect estimate, interval, threshold, and verdict.

### M003 — Paired/interleaved experiment qualification

Add pair identities and deterministic balanced candidate/baseline schedules where the runner can execute both variants, drift diagnostics, and methodology documentation.

Initial implementation should prefer simple balanced schedules over an adaptive experimental-design engine.

## 7. Verification strategy

Use synthetic distributions with known outcomes, deterministic bootstrap fixtures, randomized property tests for monotonicity and directionality, insufficient-sample tests, zero/domain errors, paired/unpaired fixtures, same/cross-testbed fixtures, and golden comparison reports.

A dedicated test must prove that increasing request count inside fixed trials does not change the comparison sample count.

## 8. Risks and decision points

- Tail percentiles are noisy; more request samples do not substitute for more independent trials.
- Ratios require domain checks around zero and negative metrics.
- Testbed equality cannot be one raw JSON hash; policy must classify fields.
- Multiple primary metrics may require an explicit aggregate rule rather than informal interpretation.

## 9. Completion definition

The roadmap closes when Eggbench can compare two immutable evidence bundles under one transparent versioned policy without overstating confidence.

## 10. Milestone status

### M001 — Metric vocabulary and trial normalization

Status: closed for qualification. Implementation plan: `plans/implementation/measurement-comparison/001-metric-vocabulary-and-trial-normalization.md`. Historical closure: `plans/closure/measurement-comparison/001-status.md`. Qualification: `plans/closure/post-m003-m001-qualification-corrective/001-status.md` (hosted CI run `35808371805`, four lanes green; metric code untouched by the corrective). Vocabulary v1, `TrialMetrics` schema v1, post-measurement staging, `BundleReader::trial_metrics`, synthetic producer, and the trial-as-statistical-unit guard are landed and hosted-qualified.

### M002 — Baselines, comparability, and statistical gates

Status: hosted-qualified by C002 (hosted run `36029547565`, four lanes green). Implementation commit `80ff6d1`, historical closure record at `plans/closure/measurement-comparison/002-status.md`. Immutable bundle identities, digest-pinned aliases, three observably distinct environment policies, deterministic trial-level bootstrap policy v1, per-metric/aggregate verdicts, standalone receipts, `eggbench compare` with additive exits 6/7/8, and golden fixtures are landed, regression-tested, and hosted-qualified. Corrective qualification evidence: `plans/closure/post-m003-hosted-qualification-corrective/002-status.md`.

### M003 — Paired/interleaved experiment qualification

Status: closed/hosted-qualified by C002 (hosted run `36029547565`, four lanes green). Implementation commit `49a4105`, historical closure record at `plans/closure/measurement-comparison/003-status.md`. Predeclared paired designs (plan schema v2), deterministic alternating runner schedule with arm/pair evidence, paired trial-level bootstrap policy v2, descriptive drift diagnostics, `eggbench compare --paired`, and methodology documentation are landed, locally regression-tested with v1 behavior proven unchanged, and hosted-qualified. C001 fixed the original portability defects at `808c35f`; rerun `36017662684` exposed `eggbench-drivers` stable-Clippy debt and stopped per plan; C002 (`a8fbcea` + `0e32ff0`) cleared the debt and supplied the final qualification — see `plans/closure/post-m003-hosted-qualification-corrective/002-status.md`.
