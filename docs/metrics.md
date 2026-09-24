# Metrics and trial normalization

Measurement M001 defines the first stable normalized per-trial metric
evidence contract. It establishes what a number means, where it came from,
and whether it is usable — without comparing candidate versus baseline and
without emitting a comparison verdict.

## Vocabulary v1

`METRIC_VOCABULARY_VERSION = 1`. Built-in metric names carry default units,
aggregation semantics, and (where defined) direction:

| Metric | Unit | Aggregation | Direction |
|---|---|---|---|
| `throughput` | `rps` | rate | higher-is-better |
| `latency_min` | `ms` | minimum | lower-is-better |
| `latency_mean` | `ms` | mean | lower-is-better |
| `latency_p50` | `ms` | p50 | lower-is-better |
| `latency_p90` | `ms` | p90 | lower-is-better |
| `latency_p95` | `ms` | p95 | lower-is-better |
| `latency_p99` | `ms` | p99 | lower-is-better |
| `latency_p999` | `ms` | p99.9 (basis points 9990) | lower-is-better |
| `error_rate` | `ratio` | ratio | lower-is-better |
| `timeout_rate` | `ratio` | ratio | lower-is-better |
| `bytes_sent` | `bytes` | sum | informational (no fixed direction) |
| `bytes_received` | `bytes` | sum | informational (no fixed direction) |
| `cpu_percent` | `percent` | mean | informational (no fixed direction) |
| `rss_bytes` | `bytes` | maximum | informational (no fixed direction) |

The table is a vocabulary/default-semantics reference, not permission to
override the plan. If a plan declares a contradicting unit or direction for
a built-in name, normalization surfaces an `invalid` observation rather than
rewriting the plan. Custom metric names remain allowed; they require an
explicit unit in the existing `MetricRequest` and explicit aggregation from
the producing adapter.

Percentiles encode as basis points of percentile (`p50 = 5000`, `p99 = 9900`,
`p99.9 = 9990`, range `1..=10000`) so `p99.9` has no floating-point identity
ambiguity.

## Observation states

Every requested metric normalizes to exactly one of:

- `observed(value)` — a finite scalar. Zero is valid where the domain permits.
- `missing(reason)` — `source_not_provided`, `unsupported_by_driver`, or
  `trial_not_completed`. Missing is never `0`, `NaN`, or record absence.
- `invalid(reason, detail?)` — `non_finite`, `unit_mismatch`,
  `duplicate_observation`, `aggregation_mismatch`,
  `malformed_source_reference`, or `domain_error`.

Semantic data problems (a bad benchmark number) become `missing`/`invalid`
inside a valid finalized bundle. Structural problems (unsafe artifact paths,
bound overflows, unserializable evidence) remain orchestration errors on the
mandatory drain/teardown cleanup path.

## Normalization rules

For each measured trial, after the measured interval ends:

1. Stage and validate raw workload artifacts.
2. Non-completed trials emit every requested metric as
   `missing(trial_not_completed)`; partial raw values are never promoted.
3. Match raw observations to resolved requests by exact stable name.
4. No source becomes `missing`; exactly one compatible source becomes
   `observed`; duplicates, unit mismatches, nonfinite values, aggregation
   mismatches, unknown artifact references, and domain errors become
   `invalid`.
5. Copy unit, direction, and intent from the resolved plan — driver output
   cannot change them. No implicit unit conversion (`ms` never accepts `us`,
   `ratio` never accepts `percent`).
6. Emit deterministic ordering: observations sorted by metric name,
   histograms by `(metric, path)`, error categories by name.

Unrequested raw observations never become gate-eligible metrics; they survive
only as retained raw artifacts or a bounded `unrequested_raw_metric`
warning.

## Provenance and distributions

Each observation records producer label, optional producer version, source
field, the `eggbench-normalize-v1` method label, and resolved same-trial
artifact paths. When no raw artifact was retained, provenance says so rather
than inventing a path. Secrets and full command lines never enter
provenance.

Histograms are references, not parsed mathematics: metric name, artifact
path, format label, unit, and optional method. The referenced artifact must
belong to the same trial. Error distributions are bounded `category → count`
maps with deterministic ordering; they are descriptive evidence, not
correctness verdicts.

A raw observation may carry a per-observation `producer`/`producer_version`
override so telemetry observations normalize through a combined call with
their own attribution (for example `gregg`); without the override the
call-level producer applies. Duplicate observations for one requested
metric — from one producer or across producers — still normalize as
invalid rather than selecting silently. See
[Gregg telemetry](gregg-telemetry.md).

## Artifact layout

Each measured trial stages `trials/NNN/metrics.json` with
`ArtifactRole::TrialArtifact` and `Public` sensitivity, listed in the
trial's descriptor artifacts. Warmups never receive `TrialMetrics`.
`BundleReader::trial_metrics(trial_id)` loads normalized metrics from a
verified bundle; pre-M001 bundles return `None`.

## The statistical unit

One measured trial is one observation. A trial that issues 10,000 requests
still contributes exactly one scalar per requested metric. Later comparison
consumes completed-trial scalars plus explicit missing/invalid states; it
must never infer sample count from requests or histogram buckets.

## Boundaries

M001 implements no baseline selection, bootstrap, confidence intervals,
comparison verdicts, paired schedules, unit conversion, histogram
mathematics, or dashboards. Drivers own parsing upstream output into
`WorkloadMetricObservation`; they must never write `TrialMetrics` JSON
themselves.
