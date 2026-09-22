# Measurement/Comparison M001 — Metric Vocabulary and Trial Normalization

Status: ready for handoff

Repository baseline: `42f4696f15a9653ccced7d961ec1e513dd7f840b`

Source roadmap:

- `plans/subsystems/measurement-comparison-roadmap.md` — M001

Closed prerequisites:

- Foundation experiment/evidence M001-M003;
- foundation status/verdict corrective;
- Local Runner M001;
- Local Runner M002;
- M002 evidence-safety corrective C001, implementation `6900212a5997b3d776e824b115d8ad35d36cd431`, hosted CI `35797812233` green.

Controlling ADRs:

- `plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md`
- `plans/adrs/ADR-0003-trial-level-comparison-and-regression-verdicts.md`
- `plans/adrs/ADR-0004-eggstack-composition-and-independent-oracles.md`

Long-term requirements:

- `plans/000-long-term-specification.md#9-metrics-and-observations`
- `plans/000-long-term-specification.md#10-statistical-comparison`
- `plans/000-long-term-specification.md#12-evidence-bundle`
- `plans/000-long-term-specification.md#20-resource-and-harness-overhead`
- `plans/000-long-term-specification.md#21-compatibility-and-versioning`

Primary class: infrastructure/capability.

## 1. Objective

Define the first stable normalized per-trial metric evidence contract and integrate it into the M002 measured-trial staging path without implementing baseline comparison or statistical verdicts.

M001 must establish:

- versioned metric vocabulary metadata;
- stable units and aggregation semantics;
- explicit source/provenance;
- observed vs missing vs invalid values;
- normalized scalar trial summaries;
- raw histogram/distribution references;
- error-category distributions;
- runner staging of normalized metric evidence after the measured interval;
- a deterministic fake metric producer for qualification.

M001 does **not** compare candidate versus baseline and does not emit `ComparisonVerdict`.

## 2. Current repository evidence

At the baseline:

- `ExperimentPlan.metrics` / `ResolvedPlan.metrics` already contain `MetricRequest`:
  - name;
  - unit;
  - direction;
  - primary/diagnostic intent;
  - optional absolute/relative/statistical gate request;
- existing fixtures use examples such as:
  - `latency_p99` with unit `ms`;
  - `throughput` with unit `rps`;
- M002 produces versioned `TrialExecutionResult` v1 with timing/execution state only;
- `TrialDescriptor.artifacts` can reference additional per-trial artifacts;
- `WorkloadOutput` currently carries bounded diagnostic artifacts but no typed metric observations;
- M002 measurement timing stops before workload output staging;
- M002's corrected common cleanup tail makes post-workload evidence/staging failures lifecycle-safe;
- no normalized metric schema or vocabulary version exists;
- no real workload parser exists yet.

## 3. Required invariants

1. One measured trial is one statistical observation unit for later comparison.
2. Request count inside a trial never increases the comparison sample count.
3. Metric name, unit, directionality, aggregation semantics, and source are explicit.
4. A measured zero is distinct from missing data.
5. Missing data is distinct from invalid data.
6. NaN/infinity never enters serialized normalized evidence.
7. Primary/diagnostic intent comes from the predeclared resolved plan, not from driver output.
8. Directionality and gate intent are never inferred after observing candidate values.
9. Normalization occurs after the measured workload interval ends.
10. Raw driver artifacts remain available when retained; normalized summaries reference rather than replace them.
11. No implicit unit conversion occurs in v1.
12. No outlier deletion, imputation, or request-level confidence calculation occurs.
13. Warmup observations cannot become measured metric evidence.
14. Failed/cancelled/timed-out trials may retain diagnostic/raw artifacts but are not silently promoted to valid completed metric observations.
15. M001 emits no comparison artifact and no comparison verdict.
16. Existing manifest v2 and TrialExecutionResult v1 remain readable and unchanged.

## 4. Explicit non-goals

Do not implement:

- baseline selection;
- same-testbed comparison policy enforcement;
- bootstrap resampling;
- confidence intervals;
- pass/fail/inconclusive/invalid aggregate comparison verdicts;
- paired/interleaved schedules;
- automatic unit conversion;
- histogram mathematics;
- raw request-sample retention as normalized observations;
- a real oha/Eggfetch/Gregg parser;
- dashboards;
- arbitrary statistics expressions;
- multiple-comparison correction;
- outlier removal.

These belong to later milestones.

## 5. Core module and versioning

Add a dependency-light core module such as:

~~~text
crates/eggbench-core/src/metrics.rs
~~~

Define explicit versions:

- `METRIC_VOCABULARY_VERSION = 1`;
- `TRIAL_METRICS_SCHEMA_VERSION = 1`.

Do not make Tokio/process/network dependencies enter `eggbench-core`.

The normalized artifact should carry both its schema version and metric-vocabulary version so later vocabulary evolution does not require guessing from field names.

## 6. Stable metric vocabulary v1

Define documented canonical built-in metric names and units for the first adapters.

At minimum reserve/document:

| Metric | Unit | Typical aggregation | Direction |
|---|---|---|---|
| `throughput` | `rps` | rate | higher-is-better |
| `latency_min` | `ms` | minimum | lower-is-better |
| `latency_mean` | `ms` | mean | lower-is-better |
| `latency_p50` | `ms` | p50 | lower-is-better |
| `latency_p90` | `ms` | p90 | lower-is-better |
| `latency_p95` | `ms` | p95 | lower-is-better |
| `latency_p99` | `ms` | p99 | lower-is-better |
| `latency_p999` | `ms` | p99.9 | lower-is-better |
| `error_rate` | `ratio` | ratio | lower-is-better |
| `timeout_rate` | `ratio` | ratio | lower-is-better |
| `bytes_sent` | `bytes` | sum | informational by default |
| `bytes_received` | `bytes` | sum | informational by default |
| `cpu_percent` | `percent` | mean | informational by default |
| `rss_bytes` | `bytes` | maximum or explicit source aggregation | informational by default |

The table is a vocabulary/default semantics reference, not permission to override the plan.

If a plan explicitly declares a different direction for a built-in name, M001 should surface a validation/normalization inconsistency rather than silently rewriting the plan.

Custom metric names remain allowed. They require explicit unit in the existing `MetricRequest` and explicit aggregation/source metadata from the producing adapter.

Do not modify ExperimentPlan schema merely to encode the built-in table.

## 7. Aggregation semantics type

Define a versioned enum suitable for normalized evidence, for example:

- `Direct`;
- `Minimum`;
- `Maximum`;
- `Mean`;
- `Sum`;
- `Rate`;
- `Ratio`;
- `Percentile { basis_points: u16 }`.

The percentile representation must encode p99.9 without floating-point identity ambiguity. Basis points of percentile are acceptable if documented consistently (for example p99.9 = 9990).

This field states **what the scalar means inside one trial**. It does not tell M001 to recompute that scalar from millions of requests.

## 8. Normalized observation state

A normalized requested metric must be structurally one of:

~~~text
observed(value)
missing(reason)
invalid(reason)
~~~

Requirements:

- observed numeric value must be finite;
- zero is a valid observed value when the metric domain permits it;
- missing means no usable value was produced;
- invalid means a value/provenance was produced but violated the normalization contract;
- reasons are stable enums/categories plus optional bounded detail, not arbitrary unbounded strings.

Suggested missing reasons:

- source_not_provided;
- unsupported_by_driver;
- trial_not_completed.

Suggested invalid reasons:

- non_finite;
- unit_mismatch;
- duplicate_observation;
- aggregation_mismatch;
- malformed_source_reference;
- domain_error.

Do not encode missing as `0`, NaN, an empty string, or absence of the entire metric record.

## 9. Normalized trial metric schema

Add a runtime-independent core DTO equivalent to:

~~~text
TrialMetrics {
  schema_version
  vocabulary_version
  trial_id
  observations[]
  histograms[]
  error_distribution
  warnings[]
}
~~~

Each normalized observation includes:

- metric name;
- unit;
- plan-derived direction;
- plan-derived intent;
- aggregation semantics;
- observation state;
- provenance.

The artifact must be deterministic in ordering, preferably sorted by metric name.

One requested metric appears at most once.

## 10. Provenance

Define typed provenance sufficient to answer “where did this number come from?”

At minimum:

- producer/driver label;
- producer/upstream version when known;
- source field/key label;
- normalization method/version label;
- referenced raw artifact paths, if any.

When a metric was synthesized from no retained raw artifact, the provenance must say so rather than invent a path.

Do not copy secrets or full command lines into metric provenance.

Driver descriptor inventory remains authoritative for selected adapter versions; normalized provenance may reference the descriptor identity rather than duplicate every capability.

## 11. Histogram/distribution references

M001 does not implement histogram parsing mathematics, but the schema must support raw distribution evidence.

Define a bounded `HistogramReference`/equivalent containing:

- logical metric/distribution name;
- artifact path;
- format/encoding identifier;
- value unit;
- optional correction/method label supplied by the driver.

The referenced artifact must be part of the same trial descriptor/artifact set.

A percentile scalar may reference the histogram it came from.

Do not claim a histogram exists if the driver only supplied summary percentiles.

## 12. Error-category distribution

Add an optional bounded error distribution to the trial metric artifact.

Represent categories as stable `Name -> count` entries with:

- deterministic ordering;
- bounded category count;
- nonnegative integer counts.

This is descriptive trial evidence. M001 does not turn it into a security correctness verdict.

`error_rate` may be separately normalized as a scalar if requested.

## 13. Runner-facing raw metric input

Extend the workload output seam minimally.

A representative runner-side type:

~~~text
WorkloadMetricObservation {
  name
  unit
  value
  aggregation
  source
  raw_artifact_names[]
}
~~~

and:

~~~text
WorkloadOutput {
  artifacts
  metrics
  histograms/error distribution metadata as needed
}
~~~

The raw observation is trusted only as driver output to be validated; it is not the normalized evidence schema.

Keep the API bounded:

- cap metric observations per invocation;
- cap provenance/raw-artifact references;
- cap error categories;
- reject structurally excessive output through the existing M002 evidence-error cleanup path.

Do not add protocol-specific fields to `WorkloadOutput`.

## 14. Artifact-name to artifact-path resolution

A workload observation may refer only to artifacts returned by the same invocation.

The runner must:

1. validate/stage workload artifacts after measurement;
2. map safe returned artifact names to their final trial artifact paths;
3. resolve metric/histogram provenance references against that map;
4. mark unknown references invalid rather than following arbitrary paths.

A driver must never supply an arbitrary bundle-relative path for normalization.

## 15. Normalization algorithm

For each measured trial that entered:

1. inspect `TrialExecutionResult`;
2. stage/validate raw workload artifacts;
3. if trial terminal status is not `Completed`:
   - emit requested metrics as missing with `trial_not_completed`, or omit the metric artifact only if the plan explicitly documents that behavior;
   - do not treat partial raw values as successful primary observations;
4. for a completed trial, match raw observations to `ResolvedPlan.metrics` by exact stable name;
5. for each requested metric:
   - no source -> missing;
   - exactly one compatible source -> observed;
   - duplicate/mismatched/nonfinite source -> invalid;
6. copy unit/direction/intent from the resolved plan;
7. validate aggregation against the built-in vocabulary where defined;
8. preserve custom aggregation for custom metrics;
9. produce deterministic ordered normalized evidence.

Unrequested raw observations must not become gate-eligible normalized metrics. They may remain only in retained raw artifacts or a bounded diagnostics/warnings section.

## 16. Unit policy v1

No implicit conversion.

For a requested metric to be observed:

~~~text
raw unit == requested unit
~~~

using exact normalized unit labels.

Examples:

- `ms` does not silently accept `us`;
- `ratio` does not silently accept `percent`;
- `rps` does not silently accept a count/duration pair unless the driver itself explicitly produced the rate and provenance.

A future unit-conversion policy may be added with a version bump.

## 17. Directionality and intent

The normalized observation copies `MetricDirection` and `MetricIntent` from the resolved plan.

Driver output cannot change them.

For built-in vocabulary metrics, validate that the plan's declared unit/direction is consistent with the vocabulary where the vocabulary defines a direction.

If custom metrics use `Informational` direction with a primary gate, retain existing plan validation rules; do not invent new gate semantics in M001.

## 18. Staging into M002 evidence

For each measured trial, stage a deterministic artifact such as:

~~~text
trials/001/metrics.json
~~~

with:

- `ArtifactRole::TrialArtifact`;
- public/redacted sensitivity according to the normalized schema (normally Public if it contains no secrets);
- inclusion in the corresponding `TrialDescriptor.artifacts`.

Normalization/staging is outside the measured interval.

Warmups may retain raw artifacts but must not receive `TrialMetrics` artifacts that later comparison could mistake for measured evidence.

## 19. Failure and evidence semantics

Semantic metric problems such as missing value, unit mismatch, duplicate source, or nonfinite input should normally produce `missing`/`invalid` metric states inside a valid finalized bundle, not abort execution.

Structural evidence problems such as:

- unsafe artifact path;
- artifact bound overflow;
- serialization failure;
- metric observation count over hard safety cap;

remain evidence errors and use the corrected M002 mandatory cleanup path.

This distinction is important: bad benchmark data is evidence that later comparison can mark invalid; unsafe/unrepresentable evidence is an orchestration error.

## 20. Synthetic qualification producer

Extend `FakeWorkload` or test support to return deterministic metric observations.

Required synthetic cases:

- valid throughput;
- valid latency p99;
- observed zero;
- missing requested metric;
- duplicate metric;
- nonfinite metric;
- unit mismatch;
- custom metric;
- histogram reference to retained artifact;
- histogram reference to nonexistent artifact;
- error-category distribution.

No real network generator is needed.

## 21. Core validation and boundedness

Set explicit safety bounds, e.g. conservative caps for:

- normalized metrics per trial;
- raw metric observations per invocation;
- histogram references;
- raw artifact references per metric;
- error categories;
- warning entries/string lengths.

Use existing bundle artifact bounds in addition to schema-local limits.

Validation must reject:

- duplicate normalized metric names;
- invalid trial IDs;
- unsupported schema/vocabulary version;
- nonfinite observed values;
- invalid percentile basis points;
- raw references outside the trial's staged artifacts.

## 22. Public read/inspection API

Add helpers so later comparison and CLI can load normalized metrics from a verified `BundleReader` without reconstructing paths unsafely.

A shape equivalent to:

~~~text
BundleReader::trial_metrics(trial_id) -> Option<TrialMetrics>
~~~

is acceptable if it verifies that the metrics artifact is manifest-listed and belongs to that trial.

Do not expose an API that blindly opens a caller-supplied relative path.

Legacy/pre-M001 bundles simply have no normalized metrics artifact.

## 23. Interaction with M003

M003 and Measurement M001 are independent but may be implemented in either order.

Coordination rule:

- M003 `inspect` must tolerate bundles both with and without `metrics.json`;
- if Measurement M001 lands first, M003 may display basic normalized metric presence/values without performing comparison;
- Measurement M001 must not depend on the new CLI crate;
- avoid semantic edits to M002 orchestration other than adding post-measurement metric normalization/staging.

If both agents touch `runner/lib.rs`, workspace `Cargo.toml`, or orchestration tests, rebase rather than duplicating modules.

## 24. Interaction with future drivers

External Oracles/Eggstack drivers will own parsing upstream outputs into `WorkloadMetricObservation` and retaining raw output.

Measurement M001 owns:

- validation;
- normalization;
- vocabulary;
- typed trial metric artifact.

Drivers must not write `TrialMetrics` JSON themselves.

This prevents each driver from inventing its own normalized evidence schema.

## 25. Interaction with Comparison M002

Closing this milestone should make Measurement/Comparison M002 ready for plan authoring.

M002 comparison will consume:

- only completed measured trial identities;
- normalized observed values;
- explicit missing/invalid states;
- environment fingerprints;
- predeclared metric gates.

It must not infer sample count from requests/histogram buckets.

Do not implement any of that statistical logic in M001.

## 26. Focused tests

### Schema

- round-trip v1 normalized trial metrics;
- unsupported schema/vocabulary version;
- deterministic observation ordering;
- zero observed distinct from missing;
- NaN/inf rejected/invalid;
- duplicate normalized names rejected;
- percentile aggregation boundaries.

### Normalization

- valid built-in throughput;
- valid latency p99;
- missing source;
- duplicate source;
- unit mismatch;
- aggregation mismatch;
- custom metric;
- plan direction/intent wins over driver metadata;
- unrequested raw metric not gate-eligible;
- failed/cancelled/timed-out trial not treated as valid completed metric evidence.

### Provenance/artifacts

- raw artifact reference resolves to same-trial staged artifact;
- unknown artifact reference invalid;
- cross-trial/arbitrary path impossible;
- histogram reference round-trip;
- raw artifact digest remains verified through BundleReader.

### Error distribution

- deterministic category ordering;
- zero/nonzero counts;
- category-count bound.

### Runner integration

- measured trial gets `metrics.json`;
- warmup does not;
- artifact included in TrialDescriptor;
- metric staging occurs after measured elapsed is captured;
- structural metric evidence error still drains/tears down;
- semantic invalid metric still finalizes bundle;
- corrected phase-vector equality remains green.

### Statistical-unit guard

A dedicated test constructs trials with different request counts but the same number of measured trials and proves the normalized dataset exposes exactly one scalar observation per requested metric per completed trial, never one statistical row per request.

## 27. Broad verification

Required:

    cargo fmt --all -- --check
    cargo check --workspace --all-targets --locked
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    cargo test --workspace --all-features --locked
    cargo +1.89.0 check --workspace --all-targets --locked
    cargo tree --locked
    git diff --check

Hosted CI must remain green on Linux stable, Linux Rust 1.89, macOS stable, and Windows supported subset.

## 28. Documentation

Add/update:

- `docs/metrics.md`;
- `architecture/core.md`;
- `architecture/runner.md`;
- `docs/evidence-bundle.md`;
- README status;
- measurement roadmap/registry on closure.

Include a table of built-in metric names/units/aggregation semantics and clearly distinguish normalized trial summaries from raw request distributions.

## 29. Acceptance criteria

M001 closes only when:

1. metric vocabulary and trial metric schemas are explicitly versioned;
2. normalized metrics distinguish observed zero, missing, and invalid;
3. units/direction/intent/aggregation/source are explicit;
4. raw histogram/distribution references are supported without pretending to parse them;
5. error-category distributions are bounded and typed;
6. `WorkloadOutput` can carry protocol-neutral metric observations;
7. measured trials stage deterministic `metrics.json` evidence after measurement;
8. warmups cannot become measured metric evidence;
9. semantic metric invalidity does not crash/abort an otherwise representable run;
10. structural evidence errors still obey M002 mandatory cleanup;
11. BundleReader can safely load normalized trial metrics;
12. no comparison verdict/statistics are implemented;
13. trial—not request—is demonstrably the exposed comparison unit;
14. manifest v2 and TrialExecutionResult v1 remain unchanged;
15. full hosted qualification is green.

## 30. Stop conditions

Stop for planning review if:

- normalized metric evidence requires changing manifest v2;
- a correct design requires changing TrialExecutionResult v1;
- normalization needs protocol-specific parsing inside core/runner;
- unit conversion becomes necessary for first real adapters;
- raw histograms must be parsed to close M001;
- implementation begins statistical comparison or baseline selection;
- adding metric output breaks M002 cleanup/timing invariants.

## 31. Closure evidence required

Record:

- implementation commits;
- metric vocabulary/version table;
- TrialMetrics representative JSON;
- observed/missing/invalid examples;
- WorkloadOutput metric seam;
- normalized artifact tree;
- histogram/provenance example;
- error distribution example;
- request-count/statistical-unit guard;
- semantic-invalid versus structural-error behavior;
- BundleReader loading evidence;
- dependency tree;
- MSRV result;
- hosted CI run ID;
- known limitations;
- unresolved findings/severity;
- disposition.
