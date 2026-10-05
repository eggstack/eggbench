# Telemetry adapters (Gregg and bounded Prometheus)

## Purpose

`eggbench-drivers` ships two host-telemetry adapters, both behind optional
cargo features, both implementing the runner's object-safe
[`TelemetryCollector`](../crates/eggbench-runner/src/telemetry.rs#L123) seam:

- `gregg/*` — a trial-synchronized collector that polls a loopback Gregg
  daemon's v2 endpoints. Fixed eight `host_*` metric vocabulary, aggregated by
  the adapter from retained wire snapshots.
- `prometheus_http.rs` — a bounded, mapping-driven collector that scrapes a
  private HTTP endpoint's Prometheus text exposition. The module contains **no
  subject-specific metric names**; a workspace-pinned mapping translates
  owner-defined samples into Eggbench metric names.

The rule that governs both, and that a reviewer must not misread, is:

> **Telemetry observes alongside. It does not measure.**

Workload adapters produce the load; collectors observe it. Neither collector
starts, stops, configures, or measures the subject. Their output is
protocol-neutral raw observations that enter the *same* post-measurement
normalization pipeline as workload output, and attribution back to the
producing collector is recovered through per-observation **producer
overrides**. Neither collector ever writes normalized `TrialMetrics`
(`telemetry.rs:11-14`, `overview.md` "Normalize-once rule").

Gregg's module doc states the ownership split explicitly: "Gregg owns telemetry
collection and wire semantics; Eggbench owns scheduling, aggregation, and
evidence" (`gregg/collect.rs:6-8`).

## Source layout

| File | LOC | Role |
|---|---|---|
| `crates/eggbench-drivers/src/prometheus_http.rs` | 1,744 | Collector, mapping contract, exposition parser, endpoint policy, descriptor, in-crate tests |
| `crates/eggbench-drivers/src/gregg/collect.rs` | 773 | Collector, eight-metric vocabulary, polling/retention/aggregation, provenance |
| `crates/eggbench-drivers/src/gregg/endpoint.rs` | 201 | Loopback endpoint policy + its unit tests |
| `crates/eggbench-drivers/src/gregg/mod.rs` | 75 | Module ownership, version constants, `gregg` descriptor |

Both are feature-gated and registered in the production catalog
(`catalog.rs:55-58`). Neither is linked into a default build
(`gregg/mod.rs:18`).

## Key types

| Type | Location | Notes |
|---|---|---|
| `TelemetryCollector` | `runner/src/telemetry.rs:123-156` | The seam both implement: `source`, `preflight_timing`, `preflight`, `start_trial`, `stop_trial`, `drain` |
| `TelemetryPreflightTiming` | `runner/src/telemetry.rs:114-120` | `BeforeStartup` (default) or `AfterReadiness` |
| `TelemetryOutput` | `runner/src/telemetry.rs:96-104` | `{ artifacts, metrics: Vec<RawMetricObservation>, warnings }` |
| `GreggCollector` | `gregg/collect.rs:78-85` | Holds `eggfetch_core::Client`, validated `GreggEndpoint`, requested metrics, cadence, identity, optional window |
| `GreggEndpoint` | `gregg/endpoint.rs:16-20` | Validated loopback base URL with explicit port; builds `/v2/healthz` and `/v2/status` URLs |
| `RequestedHostMetric` | `gregg/collect.rs:70-75` | Plan-declared `{name, unit}`; unit echoed verbatim so a mismatch normalizes as invalid rather than being silently corrected |
| `PrometheusHttpCollector` | `prometheus_http.rs:33-44` | Holds client, endpoint, binding service/key, mapping + digest, requested metrics, cadence, optional window |
| `PrometheusMappingV1` | `prometheus_http.rs:686-693` | `#[serde(deny_unknown_fields)]` mapping document: `schema_version`, `source: "prometheus"`, `fields` |
| `PrometheusFieldMapping` | `prometheus_http.rs:698-715` | `output_name`, `prometheus_name`, `kind`, `unit`, `aggregation`, exact `labels` selector, `required` |
| `PrometheusMetricKind` | `prometheus_http.rs:720-725` | `Gauge` (aggregated over the window) or `Counter` (trial delta only) |
| `PrometheusAggregation` | `prometheus_http.rs:730-737` | `Mean` / `Max` / `Min`; gauges only |
| `RawMetricObservation` | `core/src/metrics.rs:246-270` | Carries the `producer` / `producer_version` overrides that make combined normalization work |
| `MetricProvenance` | `core/src/metrics.rs:144-158` | The normalized record the override ultimately populates |

## How it works

### 1. Telemetry observes; it never becomes the measurement

A collector's four lifecycle points are ordered by the runner, not by the
adapter, and all of them sit outside the measurement interval:

| Point | Position | Collector obligation |
|---|---|---|
| `preflight` | before managed startup (Gregg) / after readiness (Prometheus) | Prove the backend contract holds; no measurement occurs |
| `start_trial` | before the measurement timer | Open the window; take the baseline snapshot |
| `stop_trial` | **after** the captured elapsed | Close the window, return samples, artifacts, warnings |
| `drain` | cleanup tail | Cancel and join collector-owned tasks; no network wait in Gregg's case |

The `stop_trial` contract states it directly: "Must follow the captured
workload elapsed" (`telemetry.rs:146-148`). This is the load-bearing timing
property. Telemetry overhead — the final scrape, the join of a polling task,
serializing a 256 KiB NDJSON series — lands *outside* the interval the
workload adapter timed. Placing the stop point before the captured elapsed
would fold the collector's own teardown cost into the subject's measured
duration, and the reported elapsed would stop being a property of the subject
alone. Correspondingly, `start_trial` completing before the timer starts keeps
the window's opening scrape out of the measured span.

The polling cadence is therefore the only telemetry cost that can land *inside*
the interval, which is why both adapters bound it and both record it in
evidence (`gregg/collect.rs:268-269` clamps to the daemon's own sample
interval; `prometheus_http.rs:239` carries the plan-selected cadence into the
capability record).

### 2. Producer overrides are how attribution survives one combined call

The runner does not run a second normalization pass for telemetry. It
concatenates workload and telemetry observations into one vector and makes a
single call:

```
let mut combined = metrics;                        // orchestration.rs:3452
combined.extend(telemetry_observations);           // orchestration.rs:3453
let input = NormalizationInput { observations: &combined, producer: &producer, .. };
let mut normalized = normalize_trial_metrics(&input)?;   // orchestration.rs:3466
```

`producer` on that call is the **workload's** descriptor name. Without an
override, a Gregg `host_cpu_percent` observation would be attributed to the
workload driver. The override closes that gap:

1. Both collectors stamp their own source on every observation —
   `producer: Some(GREGG_SOURCE)` (`gregg/collect.rs:668`) and
   `producer: Some(PROMETHEUS_HTTP_SOURCE)` (`prometheus_http.rs:428`).
2. `RawMetricObservation::producer` is an `Option<String>` documented as an
   override: "Lets telemetry observations carry their own producer (for
   example `gregg`) through a combined normalization call without changing the
   `TrialMetrics` v1 output" (`metrics.rs:258-263`).
3. `normalize_one_request` builds provenance per observation: a `None`
   producer takes the call-level producer; a `Some(label)` must parse as a
   valid `Name` or the whole normalization is a structural
   `BundleError::ManifestParse`, and `producer_version` travels *only* with
   its label (`metrics.rs:819-840`).
4. The override does not weaken the duplicate policy. If two observations
   share a metric name, the group is marked invalid regardless of attribution:
   "Cross-producer duplicates attribute to the overriding producer when one is
   present, independent of input order; the invalid marking itself is what
   preserves the collision policy" (`metrics.rs:897-908`). This is what
   `../docs/gregg-telemetry.md:110-112` means by 'a future workload emitting a
   colliding name still fails normalization as a duplicate rather than
   selecting silently.'
5. A raw observation only qualifies if its name exactly matches a resolved
   `MetricRequest` (`metrics.rs:247-248`); observations are grouped by exact
   name (`metrics.rs:688-696`). Telemetry therefore cannot introduce metrics
   the plan did not ask for, and cannot smuggle a name that bypasses the
   vocabulary's unit/aggregation checks.

Warnings follow the same "append, don't rewrite" discipline: telemetry
warnings are extended onto the normalized document after normalization's own
warnings, and the result is re-validated so bound overflow still fails closed
(`orchestration.rs:3467-3470`).

### 3. Staging, and why the source label is not in the path

Per-trial base is `trials/{:03}` (`orchestration.rs:3390`). Telemetry artifacts
stage under a dedicated namespace:

```rust
// orchestration.rs:3429-3432
let path = ArtifactPath::new(format!(
    "{base}/telemetry/{collector_index:02}-{artifact_index:02}-{}",
    artifact.name,
))?;
```

Three properties follow from this, and each is deliberate:

- **Bounded by construction.** Both path segments are ordinals, not labels.
  The source label is dropped outright — `let _ = source;` at
  `orchestration.rs:3447` — so the comment at `orchestration.rs:3427-3428` is
  the whole rationale: "Collector/source labels never enter the path: indices
  keep staging deterministic even for adversarial labels." A driver-supplied
  source string is attacker-influenced in the same way an artifact name is;
  keeping it out of the path means path validity never depends on a label
  being well-behaved, and the path length is fixed regardless of label length.
- **Non-identifying.** The directory records *where in the trial* an artifact
  came from, not *which collector*. The mapping back to a collector is the
  evidence's own provenance document (endpoint, wire schema, crate versions),
  which the adapter controls and validates — not a path segment the bundler
  would have to trust.
- **Collision-checked, not shadowed.** The artifact's safe name must still be
  a single path component (`validate_artifact_name`,
  `orchestration.rs:3426`, `orchestration.rs:3485-3486`) and must not already
  exist in the trial's artifact map; a telemetry/workload name collision
  returns `BundleError::InvalidManifest("telemetry/workload artifact name
  collision")` rather than letting one shadow the other's provenance
  reference (`orchestration.rs:3441-3445`). Per-collector artifact count is
  capped at 8 (`orchestration.rs:393`, checked at `orchestration.rs:3422`).

So Gregg's two artifacts land as `trials/NNN/telemetry/00-00-gregg.ndjson` and
`00-01-gregg-provenance.json`, and a lone Prometheus collector's as
`00-00-prometheus-provenance.json` — exactly as
[`gregg-telemetry.md`](../docs/gregg-telemetry.md) documents.

### 4. Gregg: fixed vocabulary, one wire schema, no second HTTP client

Ownership and transport:

- Every request goes through the shared `eggfetch_core::Client`
  (`gregg/collect.rs:79`, constructed at `gregg/collect.rs:146`). There is no
  second HTTP client in the crate; the only dependency edge is
  `eggfetch-core` behind the `gregg` feature
  ([`gregg-telemetry.md`](../docs/gregg-telemetry.md) header block).
- Every payload is parsed into `gregg-protocol` types and then *validated*:
  health snapshot `.validate()` (`gregg/collect.rs:261-265`), and on status
  both an exact `SCHEMA_VERSION_V2` equality check
  (`gregg/collect.rs:456-464`) and `payload.validate()`
  (`gregg/collect.rs:465-467`). Transport success is never mistaken for a
  contract-valid payload.
- Preflight order is health first because readiness gates status
  interpretation (`gregg/collect.rs:253`); a non-`Ready` state is
  `health_unavailable` (`gregg/collect.rs:255-260`). The poll cadence is
  derived from the daemon's own `sample_interval_ms`, clamped to
  `[250ms, 5s]` (`gregg/collect.rs:267-269`) — the adapter never polls faster
  than the backend samples.
- Identity carried into the capability record is hostname / OS / architecture,
  each truncated to 128 characters, with empty values omitted
  (`gregg/collect.rs:271-281`).

Window discipline (`gregg/collect.rs:9-15`): one snapshot immediately after
start — whose failure fails the start, so a trial never measures against a
dead backend (`gregg/collect.rs:304-307`) — then cadenced polling, then one
final snapshot at stop when the budget allows. `stop_inner` cancels and joins
the polling task inside the stop budget, refusing to leave a task running past
its trial (`gregg/collect.rs:331-344`); the final snapshot is only attempted
when more than 100 ms of budget remains, otherwise it degrades to
`gregg_final_snapshot_skipped` (`gregg/collect.rs:348-366`).

Retention is doubly capped — 256 samples **and** 256 KiB of raw series bytes —
and overflow counts as dropped rather than truncating silently
(`gregg/collect.rs:520-532`). Aggregation always runs over exactly the
retained set (`gregg/collect.rs:633-637` plus `deduplicate` at
`gregg/collect.rs:621-631`, which collapses identical
`observed_at_unix_ms` values and is reported as
`gregg_timestamps_deduplicated`). The NDJSON artifact is the exact retained
wire lines in arrival order, with a canonical reserialization fallback when the
response is not a single line (`gregg/collect.rs:470-475`,
`gregg/collect.rs:677-684`).

The eight `host_*` metrics and their aggregation are fixed in code. Names are
a closed `match` (`gregg/collect.rs:174-186`); an unknown name is a plan/config
error, never a silent omission (`gregg/collect.rs:137-144`), and the descriptor
advertises one `Capability::TelemetryField` per name so required-field
resolution can check them (`gregg/mod.rs:57-61`).

| Metric | Wire field | Aggregation | Absent field |
|---|---|---|---|
| `host_cpu_percent` | `v2.cpu.usage_pct` | mean | always present in v2 |
| `host_memory_used_bytes` | `v2.memory.used_bytes` | maximum | always present in v2 |
| `host_memory_percent` | `v2.memory.usage_pct` | maximum | always present in v2 |
| `host_cpu_frequency_hz` | `v2.cpu_frequency_hz` | mean of present values | skipped that snapshot |
| `host_disk_read_bytes_per_sec` | `v2.disk_io.aggregate_read_bytes_per_sec` | mean | skipped that snapshot |
| `host_disk_write_bytes_per_sec` | `v2.disk_io.aggregate_write_bytes_per_sec` | mean | skipped that snapshot |
| `host_network_rx_bytes_per_sec` | `v2.network.aggregate_rx_bytes_per_sec` | mean | skipped that snapshot |
| `host_network_tx_bytes_per_sec` | `v2.network.aggregate_tx_bytes_per_sec` | mean | skipped that snapshot |

Absent optional fields stay missing — "never substitute zero"
(`gregg/collect.rs:648-649`) — and the retained set is empty ⇒ the metric is
simply absent from the output, which normalization then reports as missing.
`u64 → f64` conversion is bounded rather than lossy-silent: values above
2^53-1 saturate to `u32::MAX` instead of losing precision
(`gregg/collect.rs:714-723`).

### 5. Gregg endpoint policy — the security boundary

Gregg serves telemetry **unauthenticated on a private network**
(`gregg/endpoint.rs:3-4`). The endpoint belongs to an external, unmanaged
process whose lifecycle Eggbench never starts or installs, so this boundary
cannot be fixed by adding a credential: *any* process on the same network
segment can read — and, if the daemon accepts writes, perturb — host counters
that end up in a qualification artifact.

The mitigation shrinks the reachable surface to one host. The module accepts
loopback HTTP only (`gregg/endpoint.rs:1-7`):

- `http://` scheme is mandatory; `https://` and any userinfo (`@`) are rejected
  (`gregg/endpoint.rs:57-60`). No credential form is accepted, so there is
  nothing to leak.
- Query and fragment forms are rejected; only an empty or `/` path is allowed,
  so the collector cannot be aimed at an arbitrary daemon path
  (`gregg/endpoint.rs:62-73`). An explicit numeric port is required
  (`gregg/endpoint.rs:87-112`).
- IP literals must satisfy `is_loopback()` — `127.0.0.0/8` and `::1` — or the
  host is rejected as `endpoint_not_loopback` (`gregg/endpoint.rs:137-141`).
  This is strictly *narrower* than the Prometheus policy: RFC1918 and IPv6 ULA
  addresses are rejected here even though they are equally "private-network".
- The one name-form exception is `localhost`, and it is not trusted by label:
  it is resolved and *every* resolved address must be loopback
  (`gregg/endpoint.rs:117-136`).

The validation cannot be bypassed through the collector. `GreggCollector::new`
routes every configured string through `validate_endpoint` before constructing
anything (`gregg/collect.rs:134-136`), and only the resulting `GreggEndpoint` is
used — `health_url()`/`status_url()` concatenate two `const` paths onto the
already-validated base URL (`gregg/endpoint.rs:25-33`), so no raw string can
re-enter a request and configuration cannot redirect the collector to another
daemon. The policy is exercised directly by the in-crate tests
(`gregg/endpoint.rs:149-164` accepts; `167-177` non-loopback rejects; `180-200`
malformed rejects).

What this boundary does *not* do is authenticate the daemon: a different process
on the same loopback interface can bind the port first and serve fabricated
counters. The mitigation is reachability, not identity — narrow by design, and
loopback is the only zone where "any local process" is a bounded, reviewable
population.

### 6. Prometheus: the mapping is the indirection

`prometheus_http.rs` contains no subject-specific metric names. Every output
name arrives from a workspace-pinned mapping document
(`prometheus_http.rs:3-5`), and the split is the design:

- **The parser** knows the Prometheus text format and nothing about any
  subject. `parse_exposition` (`prometheus_http.rs:798-870`) walks lines,
  ignores comments, and emits only `output_name → value` pairs for samples that
  match a mapping entry. It has no subject vocabulary to leak.
- **The mapping** is the workspace's reviewed, hashed declaration of what this
  owner's exposition means in Eggbench terms. It is validated before use
  (`validate_mapping`, `prometheus_http.rs:757-789`) and identified in evidence
  by both a reference path and a 64-hex SHA-256 digest
  (`prometheus_http.rs:231`, `prometheus_http.rs:456-457`). The digest is
  also part of the capability identity, so two collectors with different
  mappings are never conflated in a run record
  (`prometheus_http.rs:1657-1691`).

What stops an arbitrary scrape target from smuggling in unbounded or misleading
metric names is a chain, not one check:

| Control | Rule | Cite |
|---|---|---|
| Output namespace | every `output_name` starts `subject_` and passes `Name::new`; the `host_` prefix is unreachable, so a mapping cannot shadow or rename a Gregg host metric | `prometheus_http.rs:769-771`, `1693-1703` |
| Count and length | ≤64 fields, names ≤128 B, ≤16 labels per field, label values ≤256 B with no control characters, duplicate output names rejected | `prometheus_http.rs:677`, `673`, `761-763`, `774-786`, `775` |
| Label selection | exact equality with the declared selector, never a wildcard; an unlabelled field refuses any labelled sample, and a second match is `sample_ambiguous_duplicate` | `prometheus_http.rs:840-845`, `849-853` |
| Type metadata | a declared `# TYPE` must agree with the mapping's `kind`; a duplicate declaration is rejected | `prometheus_http.rs:815-825`, `858-868` |
| Aggregation | explicit, never inferred: a gauge requires an aggregation, a counter forbids one | `prometheus_http.rs:766-768`, `604-632` |
| Requested set | only plan-requested fields are retained; a name/unit mismatch is `collector_fields_invalid` | `prometheus_http.rs:110-117`, `103-109` |
| Requiredness | a required field that is absent, lost to a dropped snapshot, or reset invalidates the trial observation set | `prometheus_http.rs:373-387`, `440-445` |

Two asymmetry decisions are worth naming because they are easy to misread from
the code shape:

- **Gauges and counters use different windows.** The start snapshot is stored
  separately from the sample ring (`prometheus_http.rs:260-263`). Counter
  deltas are measured from that pre-window baseline
  (`prometheus_http.rs:400-409`), while gauge aggregation ranges only over the
  in-window samples (`prometheus_http.rs:388-392`). A counter is therefore
  "work performed during the measured interval" and a gauge is "behaviour
  observed inside it".
- **Retention direction differs from Gregg.** `push_snapshot` evicts the
  *oldest* entry when the 512-slot ring is full, counting the drop
  (`prometheus_http.rs:589-592`), so a long trial keeps its most recent window.
  Gregg's `push_sample` rejects the *incoming* sample once its caps are hit
  (`gregg/collect.rs:524-529`), keeping the earliest retained set. Both count
  their drops into evidence.

Preflight here retries on a bounded cadence until the required contract is
observable, because the scrape endpoint is a listener of a subject the run
manages and may not be up at the first probe
(`prometheus_http.rs:175-223`, `prometheus_http.rs:482-487`). It is still
strictly before warmup and every measured trial, and it returns the last
observed failure as the reported reason (`prometheus_http.rs:192-194`).

### 7. Machine output schemas

| Adapter | Artifact | Contents |
|---|---|---|
| Gregg | `gregg.ndjson` (`gregg/collect.rs:33`) | Exact retained v2 wire lines, arrival order (`gregg/collect.rs:677-684`) |
| Gregg | `gregg-provenance.json` (`gregg/collect.rs:35`) | Endpoint host/port, both paths, `SCHEMA_VERSION_V2`, `gregg-protocol` + adapter versions, system identity, cadence, sample/drop/poll-error counts (`gregg/collect.rs:686-709`) |
| Prometheus | `prometheus-provenance.json` (`prometheus_http.rs:467-471`) | `schema_version: 1`, `exposition_format: "prometheus-text-scalar-v1"`, endpoint authority, binding service/key, mapping ref + digest, cadence, sample/poll-error/missing-field/dropped counts, and the per-field aggregation table (`prometheus_http.rs:446-464`) |

The descriptor's own machine-output slot is `None` for both
(`gregg/mod.rs:70`, `prometheus_http.rs:1075`); the schemas above live in the
per-trial artifacts.

### 8. Default-selection interaction

Both adapters are `DriverCategory::Telemetry` and both can be registered at
once (`catalog.rs:55-58`). Exactly one of them must hold the single default
slot, or selection becomes ambiguous and fails under
[core-driver-resolution.md](core-driver-resolution.md)'s "single marked
default" rule. The arbitration is compile-time, not ordering-dependent:

- `gregg` is unconditionally `default: true` (`gregg/mod.rs:72`).
- `prometheus-http` is `default: !cfg!(feature = "gregg")`
  (`prometheus_http.rs:1077`).

So: `prometheus-http` alone ⇒ Prometheus is the single default. Both features ⇒
`gregg` is default and Prometheus is not, which is a single marked default in
every build. `!cfg!(feature = ...)` also means the value is fixed at compile
time and cannot drift with registration order or runtime selection — a reviewer
can verify it by reading the feature table alone, without a test.
[`drivers-catalog-features.md`](drivers-catalog-features.md) records the same
interaction from the catalog side.

Field-level capability checking follows the same asymmetry, but in the opposite
direction. Gregg advertises eight concrete `TelemetryField` capabilities, so a
required field is verified at resolution (`resolved.rs:622-641`). The
Prometheus descriptor advertises an **empty** capability set
(`prometheus_http.rs:1073`) because its output names are owner-defined and
cannot be enumerated. The resolver therefore skips field-capability validation
for that one source by name: "The generic Prometheus collector derives fields
from a workspace-pinned mapping loaded by the CLI. Its descriptor cannot
enumerate owner-defined output names" — `if telemetry.source.as_str() ==
"prometheus-http" { continue; }` (`resolved.rs:615-621`).

The practical consequence: requiredness for `prometheus-http` is **not** checked
at resolution. It is enforced later, inside the collector, as
`required_metric_missing` at preflight (`prometheus_http.rs:196-208`) and
`required_metric_invalid` at stop (`prometheus_http.rs:440-445`). A plan that
requests a required field no mapping declares therefore fails during
preflight, not during `doctor`.

## Invariants

| Invariant | Enforced at |
|---|---|
| Telemetry writes no normalized `TrialMetrics` | Single combined call, `orchestration.rs:3452-3466`; `telemetry.rs:11-14` |
| Telemetry windows never overlap the measurement interval | Runner ordering, `telemetry.rs:140-152` |
| Observations attribute to the collector that produced them | Per-observation override, `metrics.rs:819-840`; stamped at `gregg/collect.rs:668`, `prometheus_http.rs:428` |
| Cross-producer name collisions stay invalid | `metrics.rs:897-908` |
| No driver-supplied label reaches a staged path | `orchestration.rs:3427-3432`, `3447` |
| Telemetry artifact names are safe components and collision-checked | `orchestration.rs:3426`, `3441-3445`, `3485-3486` |
| Exactly one Telemetry default per build | `gregg/mod.rs:72` + `prometheus_http.rs:1077` |
| Duplicate collector sources rejected at registration | `telemetry.rs:175-182` |
| Gregg: loopback-only endpoints, no TLS/userinfo/query/fragment | `gregg/endpoint.rs:56-142` |
| Gregg: every payload validated by `gregg-protocol` | `gregg/collect.rs:261-265`, `456-467` |
| Gregg: aggregation runs over exactly the retained set | `gregg/collect.rs:520-532`, `633-637` |
| Gregg: absent optional field never becomes zero | `gregg/collect.rs:648-649` |
| Prometheus: output names confined to `subject_` | `prometheus_http.rs:769-771` |
| Prometheus: explicit label selector, ambiguity rejected | `prometheus_http.rs:840-845`, `849-853` |
| Prometheus: required gap or counter reset fails the trial | `prometheus_http.rs:373-387`, `440-445` |
| Both: no polling task outlives its trial or its drain | `gregg/collect.rs:229-239`, `331-344`; `prometheus_http.rs:299-316`, `506-528` |
| Both: error details and warning details are bounded | `telemetry.rs:88`; `gregg/collect.rs:534-539` |

## Failure model

`TelemetryError` is a stable category plus a detail truncated to 512
characters (`telemetry.rs:63-79`). Categories are the machine-readable surface;
details are diagnostic.

| Category | Adapter | Meaning |
|---|---|---|
| `endpoint_invalid` | both | URL shape rejected by policy, or the transport refused the URL |
| `endpoint_not_loopback` | Gregg | Host is not loopback (`gregg/endpoint.rs:139`) |
| `endpoint_not_private` / `endpoint_unresolvable` | Prometheus | Public target, or a `localhost` that does not resolve loopback-only (`prometheus_http.rs:927-942`) |
| `collector_config_invalid` / `collector_fields_invalid` | Prometheus | Bad digest, cadence, or requested name/unit set (`prometheus_http.rs:96`, `108`) |
| `health_unavailable` | Gregg | Health transport, status, or body failure (`gregg/collect.rs:256`, `392-406`) |
| `status_unavailable` | Gregg | Status transport, non-2xx, or unreadable body (`gregg/collect.rs:438-452`) |
| `payload_invalid` | Gregg | JSON rejected or `gregg-protocol` validation failed |
| `schema_unsupported` | Gregg | Health envelope unparseable, or status schema ≠ `SCHEMA_VERSION_V2` |
| `scrape_unavailable` | Prometheus | Transport failure, timeout, non-2xx, unreadable body |
| `required_metric_missing` | Prometheus | Required mapped field absent at preflight |
| `required_metric_invalid` | Prometheus | Required field gapped, missing, or counter-reset during the trial |
| `polling_failed` / `polling_timeout` | both | Window misuse, task join failure, or a task that would not stop/abort in budget |
| `preflight_timeout` / `collector_cancelled` | both | Preflight bound expired; cancellation observed |

Exposition-parser categories propagate verbatim as telemetry error categories
(`prometheus_http.rs:161-162`), so the strict bounds are individually
diagnosable: `scrape_too_large`, `scrape_not_utf8`, `scrape_line_bound`,
`sample_type_invalid`, `sample_type_duplicate`, `sample_type_mismatch`,
`sample_value_invalid`, `sample_value_non_finite`, `sample_invalid`,
`sample_name_invalid`, `sample_labels_invalid`, `sample_ambiguous_duplicate`,
`counter_value_negative`, plus the mapping categories.

Required versus optional is decided by the plan, not the adapter. A required
source that fails preflight prevents measurement; an optional one is disabled
for the run with an explicit `telemetry_disabled` warning and its metrics
normalize as missing (`orchestration.rs:500-506`, `resolved.rs:642-647`).

Degradation rules differ per failure, deliberately:

- A **failed final snapshot** is a warning, not a trial failure, for both
  adapters (`prometheus_http.rs:326-336`, `gregg/collect.rs:350-360`).
- A **failed mid-window poll** only increments a counter and surfaces as a
  warning (`prometheus_http.rs:549`, `gregg/collect.rs:507-511`). For
  Prometheus, a *required* field that was gapped by any poll error, missing
  sample, or dropped snapshot escalates at stop
  (`prometheus_http.rs:373-387`).
- A **failed stop** is cleanup-attached and does not rewrite the primary
  workload failure cause ([`gregg-telemetry.md`](../docs/gregg-telemetry.md)).

## Boundaries

**What these adapters own:** cadence, window bookkeeping, bounded retention,
aggregation semantics, evidence documents, and the endpoint policy for their own
transport. **What they do not own:** the subject's lifecycle, the load, the
measurement, and the normalized output document.

| Boundary | Owner |
|---|---|
| Trial phase order and measurement interval | `runner/src/orchestration.rs` |
| Normalization, vocabulary, unit/aggregation checks, collision policy | `core/src/metrics.rs` |
| Artifact path shape, safety, collision, sensitivity label | `runner/src/orchestration.rs:3390`, `runner/src/orchestration.rs:3426-3445` |
| `greggd` process lifecycle, installation, remote OS counters | outside Eggbench entirely ([`gregg-telemetry.md`](../docs/gregg-telemetry.md) "Ownership") |
| Owner exposition meaning and the pinned mapping bytes | the workspace, not this crate |
| HTTP transport bytes on the wire | `eggfetch-core`; no second client |

Sensitivity is uniform: telemetry artifacts stage as
`Sensitivity::Redacted` with role `TrialArtifact`
(`orchestration.rs:3433-3439`), which is why the endpoint policies must keep
credentials out of the URL in the first place.

## Tests and qualification

In-crate unit tests (both files carry their own; nothing is feature-external):

| Test | Location | Proves |
|---|---|---|
| Loopback accept / non-loopback reject / malformed reject | `gregg/endpoint.rs:149`, `167`, `180` | The endpoint policy, exhaustively over the accept and reject sets |
| `exposition_accepts_scalar_samples_and_ignores_comments` | `prometheus_http.rs:1276` | Comment/metadata tolerance, scalar extraction |
| `exposition_rejects_non_finite_and_duplicate_ambiguous_samples` | `prometheus_http.rs:1286` | `NaN` rejected; label fan-out rejected as ambiguous |
| `exact_low_cardinality_selector_accepts_only_matching_label` | `prometheus_http.rs:1301` | Selector is exact, not best-effort |
| `exposition_parser_handles_escaped_and_spaced_label_values` | `prometheus_http.rs:1315` | Quoted-label parsing |
| `exposition_rejects_timestamp_and_bounds_large_samples` | `prometheus_http.rs:1366` | Timestamped samples rejected; `MAX_SCRAPE_BYTES` enforced |
| `exposition_type_metadata_must_match_the_mapping` | `prometheus_http.rs:1378` | `# TYPE` cross-check |
| `endpoint_requires_private_dns_results` | `prometheus_http.rs:1329` | TLS refused, public IP refused, loopback accepted |
| `endpoint_policy_rejects_credentials_queries_and_public_names` | `prometheus_http.rs:1728` | The Prometheus policy is narrower than "any private address" — and ULA/private LAN *is* accepted |
| `gauges_use_explicit_mean_max_or_min_aggregation` | `prometheus_http.rs:1342` | Aggregation is applied, not inferred |
| `counter_delta_is_nonnegative_and_reset_is_invalid` | `prometheus_http.rs:1358` | Monotonicity, reset, empty and absent-baseline cases |
| `mapping_cannot_publish_a_host_named_output` | `prometheus_http.rs:1694` | `host_` namespace reserved against mappings |
| `exposition_line_and_sample_ring_bounds_are_enforced` | `prometheus_http.rs:1706` | Line bound; 512-slot ring with counted drops (600 pushes ⇒ 88 dropped) |
| `collector_synchronizes_trial_and_emits_gauge_and_counter_delta` | `prometheus_http.rs:1389` | Full lifecycle over a live loopback origin; drain clears the window |
| `required_missing_field_fails_preflight_and_optional_field_stays_missing` | `prometheus_http.rs:1466` | Required/optional split, both halves |
| `drain_cancels_polling_and_the_task_never_outlives_the_window` | `prometheus_http.rs:1531` | No scrape after drain; a drained collector cannot be stopped |
| `type_drift_on_a_required_sample_fails_preflight_closed` | `prometheus_http.rs:1572` | Type drift fails closed, not degraded |
| `required_counter_reset_during_the_trial_fails_closed` | `prometheus_http.rs:1591` | A counter restart is not a negative delta |
| `required_sample_disappearing_mid_trial_fails_closed` | `prometheus_http.rs:1611` | Required-field loss mid-window fails closed |
| `polling_is_bounded_by_the_declared_cadence` | `prometheus_http.rs:1630` | Scrape count stays inside the declared cadence — the interference bound |
| `mapping_digest_is_capability_and_provenance_identity` | `prometheus_http.rs:1657` | Digest reaches both the capability record and the provenance document |

Runner-side, `FakeTelemetryCollector` and `FakeTelemetryHandle`
(`telemetry.rs:220-234`, `398-429`) record phase calls and inject artificial
start/stop latency; they exist so phase ordering and *timing exclusion* are
proven through the seam rather than asserted in prose. See
[runner-adapter-seams.md](runner-adapter-seams.md) and
[`trial-orchestration.md`](../docs/trial-orchestration.md).

Qualification assets exercise the Prometheus path end to end as an **optional**
source: `qualification/synvoid/v2/scenarios/telemetry-optional-c32.json` binds a
`subject-metrics` target on `http_url` with
`"mapping_ref": "telemetry/telemetry-mapping.json"`, a pinned
`mapping_sha256`, and `"poll_interval_ms": "200"` — inside the code's
`[100ms, 60s]` constructor bound (`prometheus_http.rs:679-681`, `92-97`).

Catalog-level coverage: `production_catalog_matches_feature`
(`catalog.rs:143-202`) pins the registered name set per feature state and
asserts the `gregg` descriptor is default (`catalog.rs:175-181`); the
registration lines for both adapters are `catalog.rs:55-58`.

## Review focus

1. **Stop after the captured elapsed must not drift.** It is a runner
   ordering contract (`telemetry.rs:146-148`) that the adapters reinforce by
   doing all expensive work inside `stop_trial`. Pre-computing in
   `start_trial`, or letting teardown into the timed span, converts observation
   overhead into measured duration.
2. **Attribution is the fragile part, not collection.** A collector that
   forgets `producer` has its metrics normalize as the *workload's*, with no
   type error. Per adapter, confirm every emitted observation carries its own
   source label (`gregg/collect.rs:668`, `prometheus_http.rs:428`) through the
   override chain at `metrics.rs:819-840`.
3. **Gregg's security claim is loopback reachability, not authentication.**
   Confirm `check_loopback` is the only path from configuration to a request
   URL. The residual risk stands: a squatter on the same loopback port is not
   detectable by this design.
4. **Provenance inconsistency (verified, pre-existing).** The descriptor
   declares `upstream_name: "prometheus-text-exposition"` /
   `upstream_version: Some("bounded-v1")` as hand-written literals
   (`prometheus_http.rs:1070-1071`), not lockfile-derived, unlike Gregg
   (`gregg/mod.rs:65-66`), while the evidence the same collector writes stamps
   `"transport_version": env!("EGGBENCH_EGGFETCH_CORE_VERSION")`
   (`prometheus_http.rs:451`), which *is* lockfile-derived. `hdrhistogram` is
   absent from the build script's target table (`build.rs:13-24`). The evidence
   adds a third distinct label,
   `"exposition_format": "prometheus-text-scalar-v1"`
   (`prometheus_http.rs:452`), matching neither the descriptor's upstream name
   nor any resolved crate version. A reader comparing `resolved_plan` upstream
   provenance against `trials/NNN/telemetry/*/prometheus-provenance.json`
   therefore finds two differently-derived version statements for one adapter.
   Already recorded in
   [drivers-catalog-features.md](drivers-catalog-features.md) "Known gaps".
5. **Requiredness for `prometheus-http` is not checked at resolution.**
   `resolved.rs:615-621` skips field-capability validation for that source by
   name, so a required-but-unmapped field fails at preflight rather than at
   `doctor`. The fail-closed guarantee for this adapter lives entirely in the
   collector's `required_metric_missing` / `required_metric_invalid` paths.
6. **The single-default rule is compile-time.** Compare `gregg/mod.rs:72` with
   `prometheus_http.rs:1077` and confirm exactly one `default: true` in every
   feature state. Do not rely on registration order (`catalog.rs:51-67`): there
   is no tie-breaking loop, ambiguity fails.
7. **Retention direction differs**: oldest evicted for Prometheus, newest
   rejected for Gregg. Both count drops into evidence; confirm the divergence is
   intentional per adapter rather than assuming a shared policy.
8. **Path construction.** Confirm the source label stays out of the staged path
   and that a telemetry artifact name colliding with a workload artifact still
   fails the bundle rather than shadowing it.
9. **Doc claims to re-check.** Two statements in
   [`gregg-telemetry.md`](../docs/gregg-telemetry.md) are not backed by the
   current implementation; see the report accompanying this document.

## Related

- [overview.md](overview.md) — system shape, feature matrix, cross-cutting
  contracts
- [core-driver-resolution.md](core-driver-resolution.md) — single-marked-default
  selection, required/optional telemetry at resolution
- [core-metrics.md](core-metrics.md) — the normalization pipeline these
  observations enter
- [runner.md](runner.md) — the four lifecycle points and the timing contract
- [runner-adapter-seams.md](runner-adapter-seams.md) — the `TelemetryCollector`
  seam, registry, and test doubles
- [runner-orchestration.md](runner-orchestration.md) — phase schedule, staging,
  cleanup boundary
- [drivers.md](drivers.md) — catalog ownership and feature isolation
- [drivers-catalog-features.md](drivers-catalog-features.md) — per-feature
  registration matrix, descriptor defaults, known gaps
- [`../docs/gregg-telemetry.md`](../docs/gregg-telemetry.md) — canonical Gregg
  telemetry contract
- [`../docs/metrics.md`](../docs/metrics.md) — raw observation and provenance
  contract
- [`../docs/driver-capabilities.md`](../docs/driver-capabilities.md) —
  `TelemetryField` capability matrix
- [`../docs/trial-orchestration.md`](../docs/trial-orchestration.md) — measured
  interval and per-trial phase order
- [`../crates/eggbench-runner/src/telemetry.rs`](../crates/eggbench-runner/src/telemetry.rs) —
  the seam itself
- [`../crates/eggbench-drivers/src/catalog.rs`](../crates/eggbench-drivers/src/catalog.rs) —
  production registration
