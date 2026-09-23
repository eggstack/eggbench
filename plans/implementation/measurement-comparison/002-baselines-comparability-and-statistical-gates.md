# Measurement/Comparison M002 — Baselines, Comparability, and Statistical Gates

Status: ready for handoff

Repository baseline: `97b388edcc2b4d54b889ea1604ab6ae56a301a28`

Source roadmap:

- `plans/subsystems/measurement-comparison-roadmap.md` — M002

Closed prerequisites:

- Measurement M001 metric vocabulary and trial normalization;
- Local Runner M003;
- post-M003/M001 qualification corrective C001;
- hosted CI run `35809000437` green on Linux stable, Linux Rust 1.89, macOS stable, and Windows stable.

Controlling ADR:

- `plans/adrs/ADR-0003-trial-level-comparison-and-regression-verdicts.md`

Long-term requirements:

- `plans/000-long-term-specification.md#10-statistical-comparison`
- `plans/000-long-term-specification.md#11-baselines`
- `plans/000-long-term-specification.md#12-evidence-bundle`
- `plans/000-long-term-specification.md#13-environment-and-testbed-model`
- `plans/000-long-term-specification.md#17-library-and-cli-surface`
- `plans/000-long-term-specification.md#21-compatibility-and-versioning`

Primary class: capability/infrastructure.

## 1. Objective

Make two immutable Eggbench evidence bundles comparable under one transparent, versioned policy without mutating either bundle.

M002 must implement:

1. explicit immutable baseline references;
2. bundle identity and baseline alias resolution;
3. candidate/baseline comparability checks;
4. deterministic absolute and relative gate evaluation;
5. deterministic trial-level bootstrap policy v1 for statistical relative gates;
6. per-metric and aggregate comparison dispositions;
7. a versioned standalone comparison receipt;
8. `eggbench compare` with stable JSON/human output.

M002 must not add paired/interleaved scheduling; that remains M003.

## 2. Current repository evidence

At the baseline:

- `TrialMetrics` schema v1 stores one normalized scalar state per requested metric per measured trial;
- `ObservationState` distinguishes observed, missing, and invalid;
- metric unit, direction, intent, aggregation, and provenance are explicit;
- `BundleReader::trial_metrics(trial_id)` safely loads manifest-listed normalized trial evidence;
- `EnvironmentFingerprint` schema v1 classifies fields as comparison-critical, warning-only, or informational;
- plan schema v1 already contains:
  - `Gate::Absolute`;
  - `Gate::RelativeRegression`;
  - `Gate::StatisticalRelative`;
  - `EnvironmentPolicy::{StrictSameTestbed, WarnOnMismatch, CrossTestbedDescriptive}`;
- `BundleManifest` v2 separates `ExecutionStatus` from optional `ComparisonVerdict`;
- completed `.eggb` bundles are immutable;
- no comparison module, receipt schema, baseline alias format, or CLI compare command exists.

## 3. Core invariants

1. A measured trial is the statistical unit.
2. Request count, histogram buckets, and per-request samples never increase comparison sample count.
3. Candidate gates come from the candidate's predeclared resolved plan.
4. Baseline gate declarations do not override candidate policy.
5. Completed source bundles are never modified.
6. Comparison always records the exact candidate and baseline identities it consumed.
7. Practical threshold and uncertainty remain separate.
8. Missing/invalid trial metrics are never imputed.
9. Cross-testbed evidence never silently produces a same-testbed gate verdict.
10. A deterministic seed and comparison-policy version make statistical output reproducible.
11. Relative statistical gating uses only completed, finite, domain-valid trial-level observations.
12. No p-value is computed or reported in policy v1.
13. Paired inference is not invented before pair identities exist.
14. Comparison output remains independently inspectable without a database.

## 4. Non-goals

Do not implement:

- paired/interleaved scheduling;
- adaptive trial extension;
- automatic outlier deletion;
- p-values;
- BCa/bootstrap-t confidence intervals;
- multiple-comparison correction;
- automatic historical-baseline search;
- mutable baseline databases;
- remote comparison services;
- request-level inference;
- a general statistics framework;
- mutation of `manifest.json` in already-finalized bundles;
- security correctness gating.

## 5. New core module and policy version

Add a dependency-light core module such as:

~~~text
crates/eggbench-core/src/comparison.rs
~~~

Define:

~~~text
COMPARISON_RECEIPT_SCHEMA_VERSION = 1
COMPARISON_POLICY_V1 = "eggbench.trial-bootstrap.v1"
BASELINE_ALIAS_SCHEMA_VERSION = 1
~~~

`eggbench-core` remains free of Tokio/process/network dependencies.

Policy v1 is immutable once evidence is emitted. Any semantic change to resampling, interval extraction, degradation orientation, comparability behavior, or aggregate verdict ordering requires a new comparison-policy identifier.

## 6. Immutable bundle identity

Define a stable `BundleIdentity` containing at minimum:

- manifest schema version;
- run ID;
- SHA-256 of the exact finalized `manifest.json` bytes;
- optional subject revision/digest summary for presentation only.

Add a `BundleReader` helper that derives this identity only after normal bundle verification.

Do not use filesystem path as immutable identity.

Two different paths with the same run ID but different manifest digest are different evidence identities and must not be silently treated as aliases.

## 7. Explicit baseline references

Define a typed baseline reference with at least:

~~~text
BaselineReference::Bundle {
  identity,
  path
}

BaselineReference::Alias {
  alias,
  alias_file,
  resolved_identity,
  resolved_path
}
~~~

The comparison receipt stores the resolved immutable identity, not merely the alias string.

## 8. Human-managed baseline alias file

Add a small explicit JSON alias format, e.g. `*.eggbaseline.json`, with:

- schema version;
- human alias name;
- bundle path;
- required manifest SHA-256;
- optional note.

Resolution rules:

1. alias files are read explicitly by path; no global mutable registry in M002;
2. relative bundle paths resolve relative to the alias file;
3. referenced bundle is verified through `BundleReader`;
4. computed manifest digest must equal the alias digest;
5. mismatch fails closed;
6. alias mutation is allowed because the alias is human-managed, but every comparison receipt records what immutable identity it resolved to.

Do not add automatic “latest successful run” aliases.

## 9. Candidate comparison input

The candidate bundle is authoritative for:

- metric requests;
- primary/diagnostic intent;
- gate type/threshold;
- environment policy;
- workload semantics;
- selected workload driver requirements.

The baseline supplies historical observations and baseline provenance.

Add a verified input loader that returns a bounded comparison-ready DTO rather than exposing arbitrary bundle paths.

It should load:

- bundle identity;
- resolved plan;
- environment fingerprint;
- completed trial descriptors/results;
- `TrialMetrics` for measured trials;
- driver descriptors needed for comparability.

Legacy bundles without normalized metrics are readable but not statistically gateable.

## 10. Comparability report v1

Define a typed `ComparabilityReport` with dimension-level outcomes rather than one raw JSON equality test.

At minimum compare:

### Testbed-critical environment

For every comparison-critical field appearing in either fingerprint:

- equal;
- missing candidate;
- missing baseline;
- unequal.

Warning-only mismatches are recorded as warnings and do not independently invalidate same-testbed gating.

Informational fields are recorded only when useful for diagnostics and never gate comparison.

### Workload semantics

Require baseline and candidate workload intent to match for baseline-dependent gating:

- workload kind;
- target role/name semantics;
- concurrency where applicable;
- offered rate where applicable;
- request-count/duration termination semantics;
- time-bounded load mode.

Trial counts may differ.

### Driver semantics

Require compatible selected workload-driver identity for same-method gating:

- canonical driver name;
- adapter version;
- upstream tool/version when known;
- relevant load-mode capabilities.

A driver-version mismatch is a comparability mismatch unless policy v1 explicitly classifies the field as warning-only.

### Topology semantics

Compare resolved service graph/configuration that can influence workload behavior while intentionally excluding candidate subject revision/binary identity.

Subject revisions/digests are expected to differ.

### Metric semantics

For every candidate metric used in baseline comparison, baseline evidence must agree on:

- name;
- unit;
- direction;
- normalized aggregation meaning.

A semantic mismatch makes that metric invalid for gating.

## 11. Environment-policy behavior

Policy v1 must make the three existing plan values observably distinct.

### `StrictSameTestbed`

Any comparison-critical testbed/workload/driver/topology mismatch:

- baseline-dependent primary gates become `Invalid`;
- descriptive point estimates MAY still be included with explicit mismatch provenance;
- aggregate gated verdict becomes `Invalid` when an affected primary metric exists.

### `WarnOnMismatch`

When critical comparability matches, ordinary gating proceeds.

When critical comparability mismatches:

- compute descriptive effects where mathematically valid;
- suppress baseline-dependent gate verdicts rather than presenting them as same-testbed evidence;
- emit explicit warnings;
- aggregate verdict is absent when no other gate can independently produce a verdict.

Absolute candidate-only gates may still be evaluated.

### `CrossTestbedDescriptive`

Baseline-dependent effects are always descriptive even if fingerprints happen to match.

No relative/statistical gate verdict is emitted.

Absolute candidate-only gates may still be evaluated.

This policy must never use `Pass` to imply same-testbed regression qualification for a deliberately descriptive cross-testbed comparison.

## 12. Trial selection

For each metric:

- include only measured trials with `TrialExecutionStatus::Completed`;
- require normalized `ObservationState::Observed`;
- missing and invalid observations are counted and reported, not imputed;
- maintain exact candidate/baseline trial ID lists in the receipt;
- retain excluded trial IDs and reasons.

One trial contributes at most one scalar for one metric.

A dedicated invariant test must prove that 4 requests/trial versus 100,000 requests/trial still yields the same comparison sample count when measured trial count is unchanged.

## 13. Absolute gate semantics

For `Gate::Absolute { value }`:

- baseline is not required;
- candidate estimate is the arithmetic mean of valid completed trial-level scalar observations under policy v1;
- `LowerIsBetter`: pass when estimate <= value, fail otherwise;
- `HigherIsBetter`: pass when estimate >= value, fail otherwise;
- insufficient/no valid candidate observations -> invalid;
- `Informational` cannot gate;
- `TargetRange` + `Gate::Absolute` is unsupported in policy v1 and resolves to invalid with a stable reason because the existing gate schema supplies only one scalar threshold while the direction already owns two bounds.

Do not silently reinterpret `Gate::Absolute` for target-range metrics.

## 14. Non-statistical relative gate semantics

For `Gate::RelativeRegression { allowance }`:

- baseline required;
- all included baseline/candidate observations must be strictly positive;
- compute each side's geometric mean via mean(log(value));
- derive oriented degradation:
  - lower-is-better: `candidate / baseline - 1`;
  - higher-is-better: `baseline / candidate - 1`;
- threshold is `allowance / 10_000`;
- degradation <= threshold -> pass;
- degradation > threshold -> fail;
- zero/negative/domain failure -> invalid;
- target-range/informational direction -> invalid in v1.

The receipt stores both raw ratio orientation and user-facing degradation percentage.

## 15. Statistical relative policy v1

For `Gate::StatisticalRelative { allowance, min_trials }`:

### Minimum evidence

Required valid observations per side:

~~~text
max(plan.min_trials, 5)
~~~

Policy v1 recommended qualification remains 7+ per side but recommendation is diagnostic, not a hidden failure condition.

### Bootstrap

Unpaired only in M002.

For each of 10,000 resamples:

1. sample candidate trial values with replacement to candidate sample length;
2. sample baseline trial values with replacement to baseline sample length;
3. compute each side's mean log value;
4. compute oriented degradation in log space;
5. transform to ratio/degradation space.

No request-level resampling.

### Confidence interval

Use a documented percentile-bootstrap interval at 95%.

Specify exact deterministic quantile indexing in code/documentation; do not delegate semantics to an opaque statistics library.

### Verdict

- lower bound > threshold -> fail;
- upper bound <= threshold -> pass;
- interval crosses threshold -> inconclusive;
- insufficient samples, nonpositive values, comparability violation under strict mode, missing evidence, or unsupported direction -> invalid.

No p-value.

## 16. Deterministic resampling RNG

Implement a tiny documented deterministic RNG owned by comparison policy v1, or use an existing dependency only if its algorithm/version is explicitly frozen by the policy.

Preferred: a small internal fixed algorithm such as SplitMix64 solely for bootstrap index generation.

Seed rules:

- `ComparisonOptions` may accept an explicit seed;
- CLI may expose `--seed`;
- when absent, derive a deterministic base seed from:
  - candidate manifest digest;
  - baseline manifest digest;
  - comparison policy identifier;
- derive a stable per-metric seed from the base seed + metric name.

Record both base seed and effective metric seed(s).

Do not use OS randomness in default comparison.

## 17. Comparison receipt schema v1

Define a standalone immutable-by-content receipt:

~~~text
ComparisonReceipt {
  schema_version
  policy_id
  created_by_version
  candidate_identity
  baseline_reference?
  baseline_identity?
  environment_policy
  comparability
  seed
  metrics[]
  aggregate_verdict?
  warnings[]
}
~~~

Each metric receipt includes at minimum:

- name/unit/direction/intent;
- gate declaration;
- candidate trial IDs included/excluded;
- baseline trial IDs included/excluded;
- candidate estimate;
- baseline estimate if applicable;
- effect/degradation;
- confidence interval if applicable;
- practical threshold;
- statistical method;
- resample count;
- effective seed;
- gate disposition;
- invalid/descriptive reason where applicable.

## 18. Aggregate verdict

Aggregate only candidate primary metrics with actual gate verdicts.

Conservative precedence:

1. any `Invalid` -> aggregate `Invalid`;
2. else any `Fail` -> `Fail`;
3. else any `Inconclusive` -> `Inconclusive`;
4. else one or more gated primary metrics and all pass -> `Pass`;
5. no gate-eligible primary verdict -> aggregate verdict absent.

Diagnostic metrics never affect aggregate verdict.

Descriptive baseline effects under `WarnOnMismatch`/`CrossTestbedDescriptive` do not silently count as passing gates.

## 19. Immutability and output location

`eggbench compare` MUST NOT modify baseline or candidate `.eggb` directories.

M002 therefore emits the comparison receipt separately:

- JSON stdout in machine mode;
- optional explicit `--output <comparison.json>` file;
- human summary on stderr/stdout according to existing CLI discipline.

The existing optional manifest `comparison_verdict` field remains reserved for future run-time comparison performed before bundle finalization. M002 offline comparison does not mutate it.

Do not add a new comparison bundle container unless implementation evidence proves a standalone versioned JSON receipt insufficient.

## 20. CLI surface

Add:

~~~text
eggbench compare <baseline.eggb> <candidate.eggb>
eggbench compare --absolute-only <candidate.eggb>
eggbench compare --alias <baseline.eggbaseline.json> <candidate.eggb>
~~~

Optional:

~~~text
--output <comparison.json>
--seed <u64>
--json
--quiet
~~~

Do not expose a production option to reduce final bootstrap resample count below policy v1's 10,000. Unit/property tests may configure a smaller internal count.

Exit semantics:

- aggregate Pass or descriptive/no-verdict successful comparison -> 0;
- aggregate Fail -> a new documented comparison-fail exit code only if adding it does not collide with the locked M003 matrix; otherwise return 0 with machine verdict and defer CI-policy exit mapping to a follow-up;
- aggregate Inconclusive/Invalid must have explicit documented behavior;
- evidence/open/receipt I/O failures retain code 5.

Because the existing exit-code matrix is already a compatibility surface, do not repurpose codes 1–5. If CI-oriented comparison exit statuses require new codes, define additive codes in this plan and lock them with binary tests.

Preferred additive mapping:

- 6 comparison fail;
- 7 comparison inconclusive;
- 8 comparison invalid.

Descriptive/no-verdict remains 0.

## 21. CLI machine output

Add a `CliOutput::Compare` payload or an equivalent schema-v1 additive result containing:

- receipt summary;
- aggregate verdict;
- candidate/baseline identities;
- comparability status;
- output receipt path if written.

The full receipt may be included in JSON output if bounded and stable, or written separately with a summary in the envelope.

Do not serialize ANSI/human prose into machine output.

## 22. Reader helpers and compatibility

Add safe `BundleReader` helpers for comparison inputs:

- resolved plan;
- environment fingerprint;
- bundle identity;
- measured trial metric inventory.

All reads must remain manifest-verified and path-safe.

Legacy bundles:

- can be inspected;
- can participate only in comparisons for which all required normalized evidence is present;
- otherwise yield actionable invalid/unsupported comparison reasons, not panics.

## 23. Property and fixture verification

Required focused tests:

### Determinism

- same bundles + same policy + same seed -> byte-equivalent receipt excluding explicitly documented creation timestamp if any;
- derived seed is stable;
- metric iteration order does not change results.

Prefer omitting wall-clock creation timestamps from the canonical receipt if they harm reproducibility.

### Clear outcomes

Synthetic fixtures for:

- clear lower-is-better regression -> fail;
- clear higher-is-better regression -> fail;
- clear non-regression -> pass;
- threshold crossing -> inconclusive;
- insufficient trials -> invalid;
- zero/negative relative values -> invalid.

### Directionality monotonicity

Property tests:

- worsening candidate values cannot improve degradation for lower-is-better;
- decreasing candidate throughput cannot improve degradation for higher-is-better;
- increasing allowed threshold cannot turn a pass into fail.

### Comparability

- exact critical match;
- missing critical field;
- critical mismatch;
- warning-only mismatch;
- driver version mismatch;
- workload shape mismatch;
- subject digest mismatch alone does not invalidate;
- strict/warn/descriptive environment-policy behavior.

### Baselines

- explicit path;
- alias resolution;
- moved alias resolving same digest;
- alias digest mismatch;
- candidate-only absolute gate.

### Statistical unit

- request count changes do not change bootstrap sample count;
- histogram size changes do not change sample count.

## 24. Golden receipt fixtures

Add versioned golden comparison receipts for at least:

- pass;
- fail;
- inconclusive;
- invalid comparability;
- descriptive cross-testbed;
- absolute-only.

Golden tests should normalize filesystem paths or use path-independent identities so fixtures are portable.

## 25. Performance and numerical safety

- 10,000 resamples over ordinary 5–100 trial vectors should remain small and synchronous; do not introduce an async/statistics runtime;
- check every arithmetic result for finiteness;
- reject overflow/nonfinite transformed effects;
- avoid repeated allocation inside the innermost bootstrap loop where straightforward;
- no SIMD/native dependency required.

## 26. Documentation

Add/update:

- `docs/comparison.md`;
- `docs/baselines.md`;
- `architecture/core.md`;
- `docs/cli.md`;
- README compare examples;
- evidence documentation for standalone comparison receipts;
- roadmap/registry on closure.

Document the exact policy-v1 math with worked examples.

## 27. Broad verification

Required:

    cargo fmt --all -- --check
    cargo check --workspace --all-targets --locked
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-features --locked
    cargo +1.89.0 check --workspace --all-targets --locked
    cargo +1.89.0 test -p eggbench-core --all-features --locked
    cargo tree --locked
    git diff --check

Hosted CI must be green on Linux stable, Linux Rust 1.89, macOS stable, and Windows stable.

## 28. Acceptance criteria

M002 closes only when:

1. two verified immutable bundles can be compared without mutation;
2. immutable bundle identities use manifest digest + run identity;
3. explicit baseline path and human-managed digest-pinned alias are supported;
4. candidate-only absolute gates are supported;
5. comparability is typed and field-aware rather than raw JSON equality;
6. strict/warn/descriptive environment policies are observably distinct;
7. relative degradation orientation is correct for higher/lower metrics;
8. policy v1 uses deterministic unpaired trial-level bootstrap with 95% percentile interval and 10,000 final resamples;
9. comparison sample count is trial count, never request count;
10. practical threshold and interval are both recorded;
11. receipt records identities, policy, seed, trial IDs, estimates, interval, threshold, and verdict;
12. aggregate verdict obeys conservative precedence;
13. cross-testbed descriptive evidence never masquerades as pass;
14. `eggbench compare` has stable JSON and binary exit behavior;
15. no completed source bundle is modified;
16. no p-value or paired inference is implemented;
17. full hosted qualification is green.

Closing M002 unblocks Measurement M003 paired/interleaved qualification and the first security/performance policies that consume aggregate comparison verdicts.

## 29. Stop conditions

Stop for planning review if:

- honest comparison requires mutating an existing `.eggb`;
- baseline aliasing requires a global mutable database;
- comparison requires changing TrialMetrics v1 semantics;
- policy v1 requires paired identities before M003;
- an existing plan Gate cannot be interpreted without changing plan schema v1;
- numerical behavior requires a heavy opaque statistics dependency;
- adding compare exit statuses would require repurposing existing codes 0–5.

## 30. Closure evidence required

Record:

- implementation commits;
- comparison policy identifier and exact math;
- receipt schema/example;
- bundle identity algorithm;
- baseline alias example;
- comparability matrix;
- synthetic pass/fail/inconclusive/invalid fixture results;
- deterministic bootstrap reproduction evidence;
- statistical-unit guard;
- CLI compare JSON/human/exit-code matrix;
- proof baseline/candidate bundles remain byte-unchanged;
- dependency tree;
- Rust 1.89 result;
- hosted CI run ID;
- known limitations;
- unresolved findings/severity;
- disposition.
