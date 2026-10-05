# Core metrics: normalized per-trial metric contract

## Purpose

`crates/eggbench-core/src/metrics.rs` owns three things and nothing else: the
versioned metric vocabulary, the normalized `TrialMetrics` artifact schema, and
a pure function that turns driver-supplied raw observations into deterministic
per-trial evidence. It performs no comparison, emits no verdict, and parses no
protocol-specific output; drivers own parsing, this module owns validation and
normalization ([metrics.rs:19-21](../crates/eggbench-core/src/metrics.rs)).

Two decisions shape everything downstream. **One measured trial is one
statistical observation unit** — request counts inside a trial never increase
the comparison sample count ([metrics.rs:9-10](../crates/eggbench-core/src/metrics.rs)).
And **a measured zero, a missing metric, and an invalid metric are three
distinct states** — missing data is never encoded as `0`, `NaN`, or absence of a
record ([metrics.rs:11-13](../crates/eggbench-core/src/metrics.rs)).

Units, direction, and gate intent come from the resolved plan; driver output
cannot change them, and no implicit unit conversion happens in v1
([metrics.rs:14-16](../crates/eggbench-core/src/metrics.rs)). Normalization
runs after the measured interval ends, so warmup observations can never enter
this path ([metrics.rs:17-18](../crates/eggbench-core/src/metrics.rs)).

## Source layout

| Item | Location | Role |
|---|---|---|
| Vocabulary table | [metrics.rs:345-409](../crates/eggbench-core/src/metrics.rs) | Built-in name → unit/aggregation/direction |
| `normalize_trial_metrics` | [metrics.rs:665](../crates/eggbench-core/src/metrics.rs) | The only constructor of `TrialMetrics` |
| `normalize_one_request` | [metrics.rs:800](../crates/eggbench-core/src/metrics.rs) | Per-request state resolution and check precedence |
| `normalize_histograms` | [metrics.rs:1009](../crates/eggbench-core/src/metrics.rs) | Reference-only histogram staging |
| `normalize_error_counts` | [metrics.rs:1054](../crates/eggbench-core/src/metrics.rs) | Sorted `category → count` map |
| `TrialMetrics::validate` | [metrics.rs:460](../crates/eggbench-core/src/metrics.rs) | Read-path revalidation of any artifact |
| `trial_metrics_path` | [metrics.rs:340](../crates/eggbench-core/src/metrics.rs) | Canonical `trials/NNN/metrics.json` |
| `stage_trial` | [orchestration.rs:3382](../crates/eggbench-runner/src/orchestration.rs) | Staging, telemetry merge, artifact registration |
| `BundleReader::trial_metrics` | [evidence.rs:1264](../crates/eggbench-core/src/evidence.rs) | Manifest-gated load of normalized evidence |

## Key types

`METRIC_VOCABULARY_VERSION = 1` ([metrics.rs:31](../crates/eggbench-core/src/metrics.rs))
and `TRIAL_METRICS_SCHEMA_VERSION = SchemaVersion(1)`
([metrics.rs:34](../crates/eggbench-core/src/metrics.rs)) are separate
version axes; every record carries both. Provenance always carries the fixed
method label `NORMALIZATION_METHOD_V1 = "eggbench-normalize-v1"`
([metrics.rs:37](../crates/eggbench-core/src/metrics.rs)).

| Bound | Value | Line |
|---|---|---|
| `MAX_NORMALIZED_METRICS_PER_TRIAL` | 256 | [metrics.rs:40](../crates/eggbench-core/src/metrics.rs) |
| `MAX_RAW_OBSERVATIONS_PER_INVOCATION` | 256 | [metrics.rs:42](../crates/eggbench-core/src/metrics.rs) |
| `MAX_HISTOGRAM_REFERENCES_PER_TRIAL` | 16 | [metrics.rs:44](../crates/eggbench-core/src/metrics.rs) |
| `MAX_RAW_ARTIFACT_REFS_PER_METRIC` | 8 | [metrics.rs:46](../crates/eggbench-core/src/metrics.rs) |
| `MAX_ERROR_CATEGORIES_PER_TRIAL` | 32 | [metrics.rs:48](../crates/eggbench-core/src/metrics.rs) |
| `MAX_METRIC_WARNINGS_PER_TRIAL` | 16 | [metrics.rs:50](../crates/eggbench-core/src/metrics.rs) |
| `MAX_PROVENANCE_FIELD_LEN` | 128 | [metrics.rs:52](../crates/eggbench-core/src/metrics.rs) |
| `MAX_METRIC_DETAIL_LEN` | 512 | [metrics.rs:54](../crates/eggbench-core/src/metrics.rs) |
| `MAX_HISTOGRAM_FORMAT_LEN` | 64 | [metrics.rs:56](../crates/eggbench-core/src/metrics.rs) |

**Input.** `NormalizationInput`
([metrics.rs:291-310](../crates/eggbench-core/src/metrics.rs)) borrows
everything: `trial_id`, the resolved `&[MetricRequest]`, the terminal
`TrialExecutionStatus`, raw `observations`/`histograms`/`error_counts`, the
`artifact_map` from safe artifact name to staged `ArtifactPath`, and the
call-level `producer`/`producer_version`.

`MetricRequest` is declared in the plan module
([plan.rs:518-529](../crates/eggbench-core/src/plan.rs)): `name`, `unit`,
`direction` (`HigherIsBetter | LowerIsBetter | TargetRange{min,max} |
Informational`, [plan.rs:533-547](../crates/eggbench-core/src/plan.rs)),
`intent` (`Primary | Diagnostic`, [plan.rs:551](../crates/eggbench-core/src/plan.rs)),
`gate: Option<Gate>`. **The gate budget is not copied into `TrialMetrics`** —
only `name`/`unit`/`direction`/`intent` travel
([metrics.rs:737-745](../crates/eggbench-core/src/metrics.rs)), so comparison
budgets still come from the plan, not the metric artifact.

`RawMetricObservation` ([metrics.rs:246-270](../crates/eggbench-core/src/metrics.rs))
is the driver-facing type: `name`, `unit`, `value: f64`, `aggregation`, optional
`source_field`, optional `producer`/`producer_version` override, optional
`raw_artifacts: Vec<String>`. It is "validated, never trusted as evidence" and
is never staged directly. `RawHistogramInput`
([metrics.rs:275-287](../crates/eggbench-core/src/metrics.rs)) carries
`metric`, `artifact_name`, `format`, `unit`, optional `method` — no bucket data
crosses that boundary.

**Output.** `TrialMetrics` ([metrics.rs:223-241](../crates/eggbench-core/src/metrics.rs)):

| Field | Type | Notes |
|---|---|---|
| `schema_version` | `SchemaVersion` | Must equal v1 |
| `vocabulary_version` | `u32` | Must equal v1 |
| `trial_id` | `TrialId` | Cross-checked against the requested id on read |
| `observations` | `Vec<NormalizedObservation>` | One per request, sorted by name |
| `histograms` | `Vec<HistogramReference>` | Sorted by `(metric, path)` |
| `error_distribution` | `Vec<ErrorCategoryCount>` | Sorted by category |
| `warnings` | `Vec<MetricWarning>` | Bounded diagnostics |

`NormalizedObservation` ([metrics.rs:163-178](../crates/eggbench-core/src/metrics.rs))
adds `aggregation` to the four plan-copied fields, plus `state` and
`provenance`. Every serialized type here is `deny_unknown_fields`, so a schema
change is a hard parse failure rather than a silent field drop.
`MetricProvenance` ([metrics.rs:144-158](../crates/eggbench-core/src/metrics.rs))
answers "where did this number come from?": `producer: Name`, optional
`producer_version`, optional `source_field`, the fixed `normalization` label,
and `raw_artifacts: Vec<ArtifactPath>` — already resolved to same-trial staged
paths, never driver-supplied path strings.

`ObservationState` ([metrics.rs:120-139](../crates/eggbench-core/src/metrics.rs))
is internally tagged with `state`, so JSON is `{"state":"observed","value":…}`:
`Observed { value: f64 }` ([metrics.rs:122-125](../crates/eggbench-core/src/metrics.rs)),
`Missing { reason: MissingReason }`
([metrics.rs:127-130](../crates/eggbench-core/src/metrics.rs)),
`Invalid { reason, detail? }`
([metrics.rs:132-138](../crates/eggbench-core/src/metrics.rs)).
`MissingReason` ([metrics.rs:90-97](../crates/eggbench-core/src/metrics.rs)):
`SourceNotProvided`, `UnsupportedByDriver`, `TrialNotCompleted`.
`InvalidReason` ([metrics.rs:102-115](../crates/eggbench-core/src/metrics.rs)):
`NonFinite`, `UnitMismatch`, `DuplicateObservation`, `AggregationMismatch`,
`MalformedSourceReference`, `DomainError`.

**Aggregation.** `Aggregation` ([metrics.rs:64-85](../crates/eggbench-core/src/metrics.rs))
describes what one scalar means inside one trial: `Direct`, `Minimum`, `Maximum`,
`Mean`, `Sum`, `Rate`, `Ratio`, `Percentile { basis_points: u16 }`.

Percentile identity is stored as **basis points of percentile** — `p50 = 5000`,
`p99 = 9900`, `p99.9 = 9990`, range `1..=10000`, enforced in
`Aggregation::validate`
([metrics.rs:441-450](../crates/eggbench-core/src/metrics.rs)). Floating
point cannot represent `99.9` exactly, so a `f64` percentile would make `p99.9`
and a nearby value compare equal or re-serialize differently across platforms.
An integer basis-point code is exact, orderable, and stable in JSON. `0` and
`10001` are both rejected, so "p0" is not expressible.

**Vocabulary v1.** `builtin_metric`
([metrics.rs:345-409](../crates/eggbench-core/src/metrics.rs)) is a literal
`match` over stable names; anything else is custom
([metrics.rs:407](../crates/eggbench-core/src/metrics.rs)).

| Name | Unit | Aggregation | Fixed direction | Line |
|---|---|---|---|---|
| `throughput` | `rps` | `Rate` | higher-is-better | [metrics.rs:347](../crates/eggbench-core/src/metrics.rs) |
| `latency_min` | `ms` | `Minimum` | lower-is-better | [metrics.rs:352](../crates/eggbench-core/src/metrics.rs) |
| `latency_mean` | `ms` | `Mean` | lower-is-better | [metrics.rs:357](../crates/eggbench-core/src/metrics.rs) |
| `latency_p50` | `ms` | `Percentile{5000}` | lower-is-better | [metrics.rs:362](../crates/eggbench-core/src/metrics.rs) |
| `latency_p90` | `ms` | `Percentile{9000}` | lower-is-better | [metrics.rs:367](../crates/eggbench-core/src/metrics.rs) |
| `latency_p95` | `ms` | `Percentile{9500}` | lower-is-better | [metrics.rs:372](../crates/eggbench-core/src/metrics.rs) |
| `latency_p99` | `ms` | `Percentile{9900}` | lower-is-better | [metrics.rs:377](../crates/eggbench-core/src/metrics.rs) |
| `latency_p999` | `ms` | `Percentile{9990}` | lower-is-better | [metrics.rs:382](../crates/eggbench-core/src/metrics.rs) |
| `error_rate`, `timeout_rate` | `ratio` | `Ratio` | lower-is-better | [metrics.rs:387](../crates/eggbench-core/src/metrics.rs) |
| `bytes_sent`, `bytes_received` | `bytes` | `Sum` | none | [metrics.rs:392](../crates/eggbench-core/src/metrics.rs) |
| `cpu_percent` | `percent` | `Mean` | none | [metrics.rs:397](../crates/eggbench-core/src/metrics.rs) |
| `rss_bytes` | `bytes` | `Maximum` | none | [metrics.rs:402](../crates/eggbench-core/src/metrics.rs) |

## How it works

### Call site

`stage_trial` ([orchestration.rs:3382](../crates/eggbench-runner/src/orchestration.rs))
is the only production caller, and its order is deliberate. It stages
`result.json` as `ArtifactRole::TrialResult`
([orchestration.rs:3391-3400](../crates/eggbench-runner/src/orchestration.rs)),
then captures raw artifact **names** *before* `stage_workload_artifacts`
consumes the output, so provenance resolves references without trusting driver
paths ([orchestration.rs:3401-3407](../crates/eggbench-runner/src/orchestration.rs)),
and zips them with the staged paths into `artifact_map`
([orchestration.rs:3411-3415](../crates/eggbench-runner/src/orchestration.rs)).
Telemetry artifacts stage under `trials/NNN/telemetry/II-AA-<name>` with
**index-based** prefixes, so adversarial collector labels never enter a path
([orchestration.rs:3429-3432](../crates/eggbench-runner/src/orchestration.rs));
a safe-name collision with a workload artifact fails closed rather than
shadowing a provenance reference
([orchestration.rs:3441-3445](../crates/eggbench-runner/src/orchestration.rs)).
Workload and telemetry raw observations are merged into one combined list
([orchestration.rs:3452-3453](../crates/eggbench-runner/src/orchestration.rs))
for a single normalization call, after which telemetry warnings are appended,
the artifact is re-`validate()`d so the warning bound still fails closed, and
`metrics.json` is registered as `ArtifactRole::TrialArtifact` /
`Sensitivity::Public`
([orchestration.rs:3466-3480](../crates/eggbench-runner/src/orchestration.rs)).

`WorkloadOutput` ([orchestration.rs:250-262](../crates/eggbench-runner/src/orchestration.rs))
— `artifacts`, `metrics`, `histograms`, `error_counts`, `measurement_elapsed` —
is a raw carrier, not evidence
([orchestration.rs:246-248](../crates/eggbench-runner/src/orchestration.rs)).

### The algorithm

`normalize_trial_metrics` ([metrics.rs:665-783](../crates/eggbench-core/src/metrics.rs)):

1. **Structural gate.** Four hard input-count bounds before anything else —
   requests, raw observations, raw histograms, raw error categories
   ([metrics.rs:668-679](../crates/eggbench-core/src/metrics.rs)). The
   producer label must be a valid `Name`; producer version is length-bounded
   ([metrics.rs:680-686](../crates/eggbench-core/src/metrics.rs)).
2. **Group raw observations by exact name** into
   `BTreeMap<&str, Vec<&RawMetricObservation>>`
   ([metrics.rs:689-706](../crates/eggbench-core/src/metrics.rs)).
   Per-observation name/unit length, `raw_artifacts` count, and `source_field`
   length are hard bounds here. Grouping by name is what makes the later
   duplicate check a length test rather than an O(n²) scan.
3. **Compute `completed`** once:
   `terminal_status == TrialExecutionStatus::Completed`
   ([metrics.rs:708](../crates/eggbench-core/src/metrics.rs)).
4. **Iterate the plan, not the driver output.** Duplicate requested names are a
   hard error ([metrics.rs:714-718](../crates/eggbench-core/src/metrics.rs)) —
   a plan defect, not driver data. Output cardinality therefore equals request
   cardinality exactly.
5. **Non-completed trials short-circuit**: every request becomes
   `Missing { TrialNotCompleted }` with call-level provenance, and *any*
   supplied raw value is discarded rather than partially promoted
   ([metrics.rs:719-736](../crates/eggbench-core/src/metrics.rs)).
6. **Completed trials** delegate to `normalize_one_request`
   ([metrics.rs:800-1006](../crates/eggbench-core/src/metrics.rs)).
7. **Sort observations by name** ([metrics.rs:749](../crates/eggbench-core/src/metrics.rs)),
   normalize histograms and error counts, then derive warnings from the raw
   group keys: a name no request claimed becomes `unrequested_raw_metric`; a
   claimed name with a group larger than one becomes `duplicate_raw_observation`
   ([metrics.rs:754-770](../crates/eggbench-core/src/metrics.rs)). The
   unrequested branch wins when both apply.
8. **Final `validate()`** before returning
   ([metrics.rs:781](../crates/eggbench-core/src/metrics.rs)), so the
   constructor and the artifact validator agree by construction.

### Check precedence inside one request

`normalize_one_request` short-circuits in a fixed order, and the order is part
of the contract: the reason recorded for a request is always the *first* failing
check.

| Order | Condition | Result | Line |
|---|---|---|---|
| 1 | No raw group for this name | `Missing{SourceNotProvided}` | [metrics.rs:842-850](../crates/eggbench-core/src/metrics.rs) |
| 2 | Built-in name, plan unit ≠ vocabulary unit | `Invalid{UnitMismatch}` | [metrics.rs:855-875](../crates/eggbench-core/src/metrics.rs) |
| 3 | Built-in name, plan direction contradicts vocabulary | `Invalid{DomainError}` | [metrics.rs:876-894](../crates/eggbench-core/src/metrics.rs) |
| 4 | Group length ≠ 1 | `Invalid{DuplicateObservation}` | [metrics.rs:897-921](../crates/eggbench-core/src/metrics.rs) |
| 5 | Raw aggregation outside basis-point range | hard `BundleError` | [metrics.rs:924](../crates/eggbench-core/src/metrics.rs) |
| 6 | Raw unit ≠ plan unit | `Invalid{UnitMismatch}` | [metrics.rs:925-939](../crates/eggbench-core/src/metrics.rs) |
| 7 | Raw value non-finite | `Invalid{NonFinite}` | [metrics.rs:940-950](../crates/eggbench-core/src/metrics.rs) |
| 8 | Ratio metric, value outside `[0,1]` | `Invalid{DomainError}` | [metrics.rs:951-964](../crates/eggbench-core/src/metrics.rs) |
| 9 | Built-in name, raw aggregation contradicts vocabulary | `Invalid{AggregationMismatch}` | [metrics.rs:965-981](../crates/eggbench-core/src/metrics.rs) |
| 10 | Any `raw_artifacts` name not in the same-trial map | `Invalid{MalformedSourceReference}` | [metrics.rs:983-999](../crates/eggbench-core/src/metrics.rs) |
| 11 | Otherwise | `Observed { value }` | [metrics.rs:1000-1005](../crates/eggbench-core/src/metrics.rs) |

The "otherwise" branch records the **raw** aggregation, not the vocabulary
aggregation. Vocabulary aggregation reaches the artifact only through
`request_aggregation_fallback` ([metrics.rs:789-796](../crates/eggbench-core/src/metrics.rs)),
the structural placeholder used by every `missing` and `invalid` state.

### The plan-contradiction check

[`docs/metrics.md`](../docs/metrics.md) promises that a plan declaring a
contradicting unit or direction for a built-in name surfaces as `invalid`
rather than being rewritten. The mechanism: the check reads
`builtin_metric(request.name)` and compares the plan's declared unit against the
table ([metrics.rs:854-855](../crates/eggbench-core/src/metrics.rs)),
returning `Invalid { UnitMismatch, detail: "plan unit X contradicts vocabulary
unit Y" }` on mismatch. The plan's unit is still copied into the observation, so
the artifact records what was asked for alongside the verdict that the ask was
incoherent. Direction goes through `direction_matches`
([metrics.rs:412-423](../crates/eggbench-core/src/metrics.rs)), which accepts
only an exact `HigherIsBetter`/`LowerIsBetter` pair and treats `TargetRange` and
`Informational` as contradictions of a fixed direction
([metrics.rs:876-894](../crates/eggbench-core/src/metrics.rs)).

Both checks sit *after* the missing-source short-circuit: a built-in metric with
no raw observation normalizes to `Missing{SourceNotProvided}` and the vocabulary
contradiction is never reported. **The invalid verdict requires evidence that a
source existed.** Direction mismatch borrowing `DomainError` is deliberate but
lossy — it shares a tag with a ratio of `-0.2`, so a consumer must read `detail`
to distinguish a plan defect from a bad value. It is the only case where
`detail` is load-bearing.

`aggregation_matches` ([metrics.rs:429-434](../crates/eggbench-core/src/metrics.rs))
special-cases percentiles: **any** `Percentile` satisfies **any** expected
`Percentile`, because "the percentile identity is driver-supplied precision, not
a unit conversion" ([metrics.rs:426-428](../crates/eggbench-core/src/metrics.rs)).
The vocabulary's basis points are therefore advisory on the raw path: a driver
claiming `latency_p99` with `basis_points: 5000` is accepted, recorded as
`Observed`, and the artifact then shows `latency_p99` with a p50 aggregation.

### Custom metric names

A name absent from `builtin_metric` gets no vocabulary check at all. Such a
request requires, from the producing adapter: an explicit `unit` in the
`MetricRequest`, because the field is mandatory
([plan.rs:522](../crates/eggbench-core/src/plan.rs)) and there is no default to
infer; an explicit `aggregation` on the raw observation, because the recorded
aggregation is copied from raw with no fallback
([metrics.rs:1003](../crates/eggbench-core/src/metrics.rs)); exact unit
equality with the plan
([metrics.rs:925](../crates/eggbench-core/src/metrics.rs)); and a
plan-declared direction. Two consequences: no ratio domain check applies —
`is_ratio_metric` ([metrics.rs:581-583](../crates/eggbench-core/src/metrics.rs))
hardcodes exactly `error_rate` and `timeout_rate` — and a custom metric with no
source records `Aggregation::Direct` as a purely structural placeholder.

### Producer overrides

Two closures build provenance: `base_provenance` uses the call-level producer
([metrics.rs:807-814](../crates/eggbench-core/src/metrics.rs));
`observation_provenance` honours the per-observation override
([metrics.rs:819-840](../crates/eggbench-core/src/metrics.rs)). When
`raw.producer` is `Some(label)` the override becomes the producer and
`raw.producer_version` travels with it; when `None`, the call-level values apply.
This lets telemetry observations carry their own attribution through a single
combined normalization call without adding a v2 `TrialMetrics` field
([metrics.rs:258-262](../crates/eggbench-core/src/metrics.rs)). An override
label that is not a valid `Name` is a **structural** hard error, not an `invalid`
observation ([metrics.rs:825-829](../crates/eggbench-core/src/metrics.rs)); a
version override without a label is silently ignored.

For a duplicate group, provenance picks the first observation carrying an
override, else the first group member
([metrics.rs:901-908](../crates/eggbench-core/src/metrics.rs)) — order
independent, so the collision diagnostic is stable. It is a pointer to the
collision, not a selection: the state stays `Invalid{DuplicateObservation}`
([metrics.rs:909-921](../crates/eggbench-core/src/metrics.rs)). Cross-producer
duplicates are deliberately not resolved by preference, because in a combined
call workload and telemetry legitimately both claimed the name and there is no
principled winner.

### Histograms and error counts

`normalize_histograms` ([metrics.rs:1009-1051](../crates/eggbench-core/src/metrics.rs))
builds references — metric, path, format, unit, optional method — and reads no
bucket data. Name/format/method lengths are hard bounds
([metrics.rs:1014-1027](../crates/eggbench-core/src/metrics.rs)). An
`artifact_name` absent from the same-trial map causes the reference to be
**silently dropped**, with no warning
([metrics.rs:1030-1032](../crates/eggbench-core/src/metrics.rs)); the code
comment states the asymmetry explicitly — "Unknown references are dropped with no
claim; the scalar path marks unknown refs invalid explicitly. Histograms are
references only" ([metrics.rs:1028-1029](../crates/eggbench-core/src/metrics.rs)).
A typo in a histogram artifact name is invisible, while the same typo on a
scalar makes the observation invalid. `source_metric` — the documented link from
a percentile scalar back to its histogram — is always `None` from this function
([metrics.rs:1043](../crates/eggbench-core/src/metrics.rs)): part of the v1
schema with no v1 producer. Output is sorted by `(metric, path)`
([metrics.rs:1046-1049](../crates/eggbench-core/src/metrics.rs)).

`normalize_error_counts` ([metrics.rs:1054-1069](../crates/eggbench-core/src/metrics.rs))
inserts into a `BTreeMap`, which both sorts and deduplicates; **last write wins**
for a repeated category, noted as deliberate because counts are descriptive
([metrics.rs:1062-1063](../crates/eggbench-core/src/metrics.rs)). Unlike
scalars, duplicate error categories are not surfaced as invalid.

### Reading back

`BundleReader::trial_metrics` ([evidence.rs:1264-1294](../crates/eggbench-core/src/evidence.rs))
resolves the trial descriptor, computes the canonical path itself
([evidence.rs:1273](../crates/eggbench-core/src/evidence.rs)), and returns
`Ok(None)` unless the path is listed in that descriptor's artifacts
([evidence.rs:1274-1276](../crates/eggbench-core/src/evidence.rs)). It opens
through `open_artifact` (manifest-listed, symlink-checked), bounds the byte
count ([evidence.rs:1281](../crates/eggbench-core/src/evidence.rs)), verifies
`metrics.trial_id` equals the requested id
([evidence.rs:1286-1291](../crates/eggbench-core/src/evidence.rs)), and
re-runs `validate()` ([evidence.rs:1292](../crates/eggbench-core/src/evidence.rs)).

That last step is why comparison and CLI surfaces do not reconstruct paths: the
artifact is verified against the same validator the writer used, identity is
cross-checked against the caller's `TrialId`, and a bundle whose metrics file is
not manifest-listed for that trial yields `None` rather than a partially trusted
read. Pre-M001 bundles read as `None` rather than erroring.

## Invariants

- Output cardinality equals request cardinality. Every requested metric yields
  exactly one `NormalizedObservation`; unrequested raw names become only warnings.
- Exactly one raw observation is required for `observed`. Two or more is `invalid`,
  never a silent pick ([metrics.rs:897-921](../crates/eggbench-core/src/metrics.rs)).
- `unit`, `direction`, and `intent` on the artifact are byte-copies of the
  resolved plan ([metrics.rs:737-741](../crates/eggbench-core/src/metrics.rs)).
- No unit conversion: `ms` never accepts `us`, `ratio` never accepts `percent`
  ([metrics.rs:925](../crates/eggbench-core/src/metrics.rs)).
- `Observed` implies a finite value. `NaN`/infinity is `invalid` at write time
  ([metrics.rs:940-950](../crates/eggbench-core/src/metrics.rs)) and a hard
  validation failure at read time
  ([metrics.rs:553-559](../crates/eggbench-core/src/metrics.rs)).
- A non-completed trial yields only `Missing{TrialNotCompleted}`; partial raw
  data is never promoted ([metrics.rs:719-736](../crates/eggbench-core/src/metrics.rs)).
- Ordering is total and enforced twice: observations strictly ascending by name,
  histograms by `(metric, path)`, error categories by name
  ([metrics.rs:486-526](../crates/eggbench-core/src/metrics.rs)).
- Provenance `raw_artifacts` holds only same-trial staged `ArtifactPath` values
  resolved through the map, never driver-supplied strings
  ([metrics.rs:985-988](../crates/eggbench-core/src/metrics.rs)); an empty
  list is the honest "nothing retained" answer.
- Bounds are checked on write *and* on read: `to_json_bytes` validates first
  ([metrics.rs:537-541](../crates/eggbench-core/src/metrics.rs)) and the read
  path validates again after deserialization.
- Zero is a legal observed value; only the domain check separates it from a
  violation ([metrics.rs:560-565](../crates/eggbench-core/src/metrics.rs)).

## Failure model

The single organizing distinction: a **semantic** data problem becomes an
`observed`/`missing`/`invalid` record inside a *valid* bundle, while a
**structural** bound violation returns a hard `BundleError`
([metrics.rs:655-663](../crates/eggbench-core/src/metrics.rs)). The runner
turns that hard error into the mandatory evidence-error path — drain and
teardown still run — rather than a silently truncated artifact.

Semantic rows are the precedence table above. Two are not reachable from it: an
unrequested raw name produces only an `unrequested_raw_metric` warning and no
observation ([metrics.rs:755-761](../crates/eggbench-core/src/metrics.rs)),
and a histogram artifact miss is dropped with no record at all
([metrics.rs:1030-1032](../crates/eggbench-core/src/metrics.rs)).

| Structural condition | Error | Line |
|---|---|---|
| Any input count over its bound | `BoundExceeded` | [metrics.rs:668-679](../crates/eggbench-core/src/metrics.rs) |
| Raw percentile basis points outside `1..=10000` | `ManifestParse` | [metrics.rs:441-450](../crates/eggbench-core/src/metrics.rs) |
| Duplicate name in the plan's requests | `ManifestParse` | [metrics.rs:714-718](../crates/eggbench-core/src/metrics.rs) |
| Producer or per-observation override label not a valid `Name` | `ManifestParse` | [metrics.rs:680-681](../crates/eggbench-core/src/metrics.rs) |
| Provenance / warning / detail / histogram string over bound | `BoundExceeded` or `InvalidManifest` | [metrics.rs:590-652](../crates/eggbench-core/src/metrics.rs) |
| Warning count reaches the cap | `BoundExceeded` | [metrics.rs:1077-1078](../crates/eggbench-core/src/metrics.rs) |
| Artifact fails `validate()` on read, or identity mismatch | `ManifestParse` | [evidence.rs:1286-1292](../crates/eggbench-core/src/evidence.rs) |

Two asymmetries decide whether a bug is loud or silent. **Histogram reference
misses are silent; scalar reference misses are loud** — same artifact map, same
typo, opposite visibility. And **driver data can hard-fail the run**: the raw
observation, histogram, and error-category caps
([metrics.rs:42-48](../crates/eggbench-core/src/metrics.rs)) are checked
before any semantic interpretation, so a driver emitting 257 observations
produces a `BundleError` and takes the cleanup path rather than an artifact full
of invalid records. Input-size overflow is treated as structural even though
driver output causes it.

## Boundaries

- **No comparison, no verdict, no statistics.** No baseline selection, bootstrap,
  confidence interval, or gate evaluation. `Gate` never enters this module.
- **No unit conversion.** Unit equality is the whole check
  ([metrics.rs:925](../crates/eggbench-core/src/metrics.rs)).
- **No histogram mathematics.** References only; M001 reads no buckets
  ([metrics.rs:180](../crates/eggbench-core/src/metrics.rs)).
- **No protocol parsing.** Drivers own producing `RawMetricObservation` from
  upstream output and must never write `TrialMetrics` JSON themselves
  ([../docs/metrics.md:121-124](../docs/metrics.md)).
- **No request-count weighting.** Trial scalars are never recomputed from raw
  requests ([metrics.rs:60-61](../crates/eggbench-core/src/metrics.rs)).
- **No path construction from driver input.** `trial_metrics_path` derives from
  `TrialId` alone ([metrics.rs:340-342](../crates/eggbench-core/src/metrics.rs)).
- **The 256 raw-observation cap is shared with telemetry**, which merges into
  the same slice before normalization
  ([orchestration.rs:3452-3453](../crates/eggbench-runner/src/orchestration.rs)).
- **Warmups never reach this path** — only `stage_trial` calls the normalizer, and
  only measured trials are staged.
- **Provenance cannot leak secrets or command lines.** The field set is a bounded
  producer label, bounded optional version, bounded optional source field, fixed
  method label, and resolved paths
  ([metrics.rs:144-158](../crates/eggbench-core/src/metrics.rs)), with every
  string bounded in `MetricProvenance::validate`
  ([metrics.rs:590-612](../crates/eggbench-core/src/metrics.rs)).

### Normalize-once, verified

No driver writes `TrialMetrics`. Every reference to the type outside
`eggbench-core` is a doc comment or a raw-input reference:
`external/common.rs:101` uses the name to justify an `f64` cast helper and
returns `f64` ([common.rs:112-114](../crates/eggbench-drivers/src/external/common.rs));
`eggsec.rs:22` and `eggprobe.rs:19` state their results never enter
`TrialMetrics` and never satisfy a `MetricRequest`; `eggbench-runner` produces
`RawMetricObservation` in `WorkloadOutput`
([orchestration.rs:255](../crates/eggbench-runner/src/orchestration.rs)) and
serializes only at
[orchestration.rs:3471](../crates/eggbench-runner/src/orchestration.rs). The
single writer is `core::metrics`; the single path constructor is
`trial_metrics_path` ([metrics.rs:340](../crates/eggbench-core/src/metrics.rs)).

## Tests and qualification

Unit tests ([metrics.rs:1089-1575](../crates/eggbench-core/src/metrics.rs))
cover version constants, the happy path, zero-versus-missing
([1166](../crates/eggbench-core/src/metrics.rs)), `NonFinite` and
`UnitMismatch` ([1204](../crates/eggbench-core/src/metrics.rs)), duplicates
([1251](../crates/eggbench-core/src/metrics.rs)), the producer override
([1293](../crates/eggbench-core/src/metrics.rs)), cross-producer duplicates
([1330](../crates/eggbench-core/src/metrics.rs)), a structural override
violation ([1379](../crates/eggbench-core/src/metrics.rs)), name sorting
([1404](../crates/eggbench-core/src/metrics.rs)), basis-point bounds
([1444](../crates/eggbench-core/src/metrics.rs)), failed-trial missing
([1457](../crates/eggbench-core/src/metrics.rs)), an unknown raw reference
([1491](../crates/eggbench-core/src/metrics.rs)), duplicate normalized names
rejected by the validator ([1521](../crates/eggbench-core/src/metrics.rs)),
and error-category ordering ([1550](../crates/eggbench-core/src/metrics.rs)).

Integration tests in
[`crates/eggbench-runner/tests/metrics.rs`](../crates/eggbench-runner/tests/metrics.rs)
qualify staging and the bundle contract:

| Test | Line | Pins |
|---|---|---|
| `measured_trial_stages_metrics_json_with_observed_values` | [142](../crates/eggbench-runner/tests/metrics.rs) | End-to-end measured trial |
| `warmup_does_not_receive_trial_metrics` | [204](../crates/eggbench-runner/tests/metrics.rs) | Warmup exclusion |
| `failed_trial_marks_metrics_missing_but_still_stages` | [260](../crates/eggbench-runner/tests/metrics.rs) | Missing state still stages |
| `semantic_invalid_metric_still_finalizes_bundle` | [301](../crates/eggbench-runner/tests/metrics.rs) | Semantic bucket finalizes |
| `structural_metric_overflow_uses_evidence_error_cleanup_path` | [352](../crates/eggbench-runner/tests/metrics.rs) | Structural bucket takes cleanup path |
| `histogram_reference_resolves_to_same_trial_artifact` | [400](../crates/eggbench-runner/tests/metrics.rs) | Reference resolution |
| `unknown_histogram_reference_is_dropped_without_claim` | [480](../crates/eggbench-runner/tests/metrics.rs) | Silent drop, deliberately |
| `unrequested_raw_metric_never_becomes_gate_eligible` | [528](../crates/eggbench-runner/tests/metrics.rs) | Warning, not observation |
| `trial_is_the_statistical_unit_not_requests` | [583](../crates/eggbench-runner/tests/metrics.rs) | One scalar per trial |
| `legacy_bundle_without_metrics_returns_none` | [639](../crates/eggbench-runner/tests/metrics.rs) | Pre-M001 compatibility |
| `trial_metrics_path_is_stable` | [658](../crates/eggbench-runner/tests/metrics.rs) | `trials/NNN/metrics.json` |

The two tests that most directly qualify the failure model are
`semantic_invalid_metric_still_finalizes_bundle`
([301](../crates/eggbench-runner/tests/metrics.rs)) and
`structural_metric_overflow_uses_evidence_error_cleanup_path`
([352](../crates/eggbench-runner/tests/metrics.rs)): together they prove the
semantic/structural split reaches the drain and teardown path rather than
existing only as a doc claim.

Not covered by any test found in this module or the runner suite: the
plan-unit-contradiction branch ([metrics.rs:855](../crates/eggbench-core/src/metrics.rs)),
the plan-direction-contradiction branch
([metrics.rs:876](../crates/eggbench-core/src/metrics.rs)), the ratio
`DomainError` branch ([metrics.rs:951](../crates/eggbench-core/src/metrics.rs)),
`AggregationMismatch`, the `UnsupportedByDriver` variant, and the
`TrialMetrics::validate` ratio-domain read check
([metrics.rs:560](../crates/eggbench-core/src/metrics.rs)).

## Review focus

- **Precedence is the contract.** Reordering checks changes the reason recorded
  for a request, and reasons are stable categories that comparison branches on
  ([comparison.rs:3837](../crates/eggbench-core/src/comparison.rs)).
- **The vocabulary is a validator, not a defaults table.** Nothing fills a
  missing unit, direction, or aggregation from it except
  `request_aggregation_fallback` ([metrics.rs:789-796](../crates/eggbench-core/src/metrics.rs)).
  Anything that starts defaulting semantics from the vocabulary widens driver
  influence over meaning.
- **The percentile wildcard**
  ([metrics.rs:431](../crates/eggbench-core/src/metrics.rs)) is the one place
  the vocabulary is intentionally not enforced. If a consumer needs
  `latency_p99` to be p99, the basis points must be checked elsewhere.
- **`is_ratio_metric` is a name list, not an aggregation check**
  ([metrics.rs:581-583](../crates/eggbench-core/src/metrics.rs)). Extending
  ratio semantics to custom names means changing this predicate and the
  parallel read-path check together, or the two can disagree.
- **Direction mismatch borrowing `DomainError`**
  ([metrics.rs:885](../crates/eggbench-core/src/metrics.rs)) forces every
  consumer that cares to parse `detail`. It earns a dedicated reason in a future
  vocabulary version.
- **The silent histogram drop**
  ([metrics.rs:1030-1032](../crates/eggbench-core/src/metrics.rs)) is a
  documented choice, not an oversight, but it is the least observable failure in
  the module. A bounded warning would cost one of 16 slots.
- **`source_metric` has no producer**
  ([metrics.rs:1043](../crates/eggbench-core/src/metrics.rs)). Either the
  runner populates it later or the field is v1 headroom that should be
  documented as such.
- **Double validation in the writer**
  ([orchestration.rs:3470-3471](../crates/eggbench-runner/src/orchestration.rs))
  is deliberate: `to_json_bytes` validates internally, and the runner must
  validate *after* appending telemetry warnings.

## Related

- [overview.md](overview.md) — module map entry for `metrics.rs` and the
  normalize-once cross-cutting rule.
- [core.md](core.md) — how M001 sits inside `eggbench-core`.
- [core-plan-schema.md](core-plan-schema.md) — `MetricRequest`,
  `MetricDirection`, `MetricIntent`, and `Gate` origins.
- [core-driver-resolution.md](core-driver-resolution.md) — `ResolvedPlan`, source
  of the request list normalization iterates.
- [core-evidence-bundle.md](core-evidence-bundle.md) — manifest roles and
  `BundleReader` read rules.
- [core-comparison.md](core-comparison.md) — the consumer of these states.
- [runner.md](runner.md) — the `M001 trial normalization` staging section.
- [../docs/metrics.md](../docs/metrics.md) — the canonical contract document.
- [../docs/gregg-telemetry.md](../docs/gregg-telemetry.md) — the producer-override
  use case.
- [../docs/security-qualification.md](../docs/security-qualification.md) —
  `host_*` / `subject_*` namespace attribution.
