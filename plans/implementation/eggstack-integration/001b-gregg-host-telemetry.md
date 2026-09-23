# Eggstack Integration M001b — Gregg Host Telemetry

Status: planned; implementation blocked on External Oracles M001 shared driver crate/catalog

Repository planning baseline: `46ebaa6baa13ec1b74512295a1886a54d2911ace`

Source roadmap:

- `plans/subsystems/eggstack-integration-roadmap.md` — second half of M001

Hard dependency:

- `plans/implementation/external-oracles/001-external-command-driver-substrate.md` must land first because it establishes `crates/eggbench-drivers` and production catalog ownership.

Interface dependency:

- M001a may proceed in parallel after the shared driver crate lands; M001b must not depend on EggServe service semantics or Eggfetch workload metrics beyond using Eggfetch as the HTTP client for Gregg polling.

Closed semantic prerequisites:

- Local Runner M003;
- Measurement M001;
- post-M003/M001 qualification corrective C001.

Controlling ADRs:

- `plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md`
- `plans/adrs/ADR-0004-eggstack-composition-and-independent-oracles.md`

Long-term requirements:

- `plans/000-long-term-specification.md#9-metrics-and-observations`
- `plans/000-long-term-specification.md#13-environment-and-testbed-model`
- `plans/000-long-term-specification.md#14-eggstack-ownership-and-reuse`
- `plans/000-long-term-specification.md#20-resource-and-harness-overhead`

Primary class: capability/infrastructure.

## 1. Objective

Add an optional trial-synchronized telemetry source backed by Gregg without recreating a host-monitor daemon.

M001b must deliver:

1. a generic runner telemetry-collector seam;
2. trial-synchronized telemetry start/stop outside measured workload timing;
3. Gregg v2 endpoint preflight and polling;
4. raw Gregg NDJSON retention;
5. bounded trial-level host metric aggregation;
6. normalization through the existing `TrialMetrics` contract;
7. required/optional telemetry semantics;
8. production catalog/CLI wiring behind an optional feature.

## 2. Re-audited Gregg state

Audit date: 2026-09-23 planning pass.

Current repository evidence:

- Gregg workspace version 1.0.14;
- Rust 1.89;
- `gregg-protocol` is a dependency-light public crate containing versioned JSON wire types;
- universal endpoint: `GET /v2/status`;
- health endpoint: `GET /v2/healthz`;
- v2 schema exposes CPU, memory, optional load/swap/commit, optional CPU frequency, disk I/O, and network telemetry;
- optional values are intentionally absent where unsupported;
- daemon is intended for private-network use and has no TLS/authentication.

Integration seam:

~~~toml
gregg-protocol = "1"
~~~

Use Eggfetch for HTTP transport rather than adding a second HTTP client.

## 3. Invariants

1. Gregg owns telemetry collection and v2 wire semantics.
2. Eggbench never scrapes OS counters remotely when Gregg is configured.
3. Gregg remains optional.
4. Absence of optional Gregg does not disable smaller local environment fingerprinting.
5. Required Gregg telemetry failing preflight prevents measurement.
6. Optional Gregg telemetry failing preflight produces explicit warning/missing evidence, not fabricated zeroes.
7. Telemetry start occurs before the measured workload timer begins.
8. Telemetry stop/final sample occurs after workload elapsed is captured.
9. Telemetry cleanup runs on workload failure/cancellation.
10. Polling cadence is bounded and cannot busy-loop.
11. Raw Gregg responses are retained as bounded evidence.
12. Host metrics use names that cannot be confused with subject-process RSS/CPU.
13. No Gregg metric silently becomes a primary gate unless declared by the candidate plan.
14. Gregg endpoint/identity provenance is recorded without credentials.
15. No public-internet Gregg behavior is introduced in M001b.

## 4. Non-goals

Do not implement:

- management/installation/startup of `greggd`;
- remote authentication/TLS;
- public-internet telemetry;
- push/stream protocol;
- per-process subject CPU/RSS from Gregg;
- process tracing/profiling;
- Linux perf/eBPF;
- Prometheus;
- comparison statistics;
- a second HTTP client;
- automatic vocabulary-v2 promotion of Gregg metrics.

## 5. Feature boundary

After External Oracles M001 establishes `eggbench-drivers`, add:

~~~text
feature: gregg
  -> gregg-protocol
  -> eggfetch-core/standard-http1
~~~

If M001a already added `eggfetch-core`, share the dependency and feature wiring rather than duplicating it.

Default drivers build remains telemetry-free.

## 6. Exact dependency provenance

Embed the exact resolved versions from Cargo.lock for:

- gregg-protocol;
- eggfetch-core.

Use the same build-time dependency-version mechanism established by M001a/external substrate.

Driver evidence records:

- Eggbench adapter version;
- gregg-protocol exact version;
- Gregg wire schema version;
- endpoint URL host/port;
- health/status path;
- observed system identity fields from Gregg.

Do not claim daemon binary version: Gregg deliberately does not expose daemon version over the wire.

## 7. Plan/config representation

Do not change ExperimentPlan schema v1.

Use existing constructs:

~~~text
Service {
  name = "gregg"
  kind = Named { service_type = "gregg" }
  lifecycle = External
  config = {
    endpoint = "http://127.0.0.1:11310"
  }
}

TelemetryRequest {
  source = "gregg"
  fields = [...]
  required = true|false
}
~~~

Rules:

- exactly one external named service of type `gregg` is allowed when Gregg telemetry is requested;
- missing or multiple Gregg endpoint services are resolution/preflight errors for required telemetry;
- endpoint config is part of resolved-plan evidence.

## 8. Endpoint policy

M001b accepts loopback HTTP endpoints only:

- `127.0.0.0/8`;
- `::1`;
- localhost only if deterministic resolution remains loopback.

Reject:

- public addresses;
- arbitrary private-LAN addresses;
- HTTPS pretending Gregg supports TLS;
- embedded credentials/userinfo;
- query/fragment endpoint forms.

A later plan may explicitly add authorized private-LAN Gregg endpoints.

This conservative scope matches Gregg's unauthenticated private-network security model.

## 9. Generic telemetry collector seam

Add an object-safe runner interface, e.g.:

~~~text
trait TelemetryCollector: Send {
  fn source(&self) -> &Name;

  fn preflight<'a>(
    &'a mut self,
    context: TelemetryPreflightContext
  ) -> BoxFuture<'a, Result<TelemetryCapability, TelemetryError>>;

  fn start_trial<'a>(
    &'a mut self,
    context: TelemetryTrialContext
  ) -> BoxFuture<'a, Result<(), TelemetryError>>;

  fn stop_trial<'a>(
    &'a mut self,
    context: TelemetryTrialContext
  ) -> BoxFuture<'a, Result<TelemetryOutput, TelemetryError>>;

  fn drain<'a>(...)
}
~~~

Exact names may differ.

Add `TelemetryRegistry` keyed by source name.

## 10. Telemetry output

Define a protocol-neutral output analogous to workload output:

~~~text
TelemetryOutput {
  artifacts: Vec<WorkloadArtifact-compatible artifact>
  metrics: Vec<RawMetricObservation>
  warnings: Vec<...>
}
~~~

Prefer a shared generic artifact type if that avoids unnecessary conversion.

Telemetry-specific metrics feed the same post-measurement normalization pipeline as workload metrics.

Do not let telemetry write normalized `TrialMetrics` JSON itself.

## 11. Runner phase ordering

For each measured trial:

~~~text
telemetry.start_trial
measurement_start = Instant::now()
workload.execute
measurement_elapsed = ...
telemetry.stop_trial
stage workload raw artifacts
stage telemetry raw artifacts
normalize combined metric inputs
stage metrics.json
~~~

Rules:

- telemetry start is outside measured interval;
- telemetry stop is outside measured interval;
- workload measurement duration remains unchanged;
- telemetry stop must be attempted after workload failure/cancellation if start succeeded;
- telemetry stop failure is cleanup/diagnostic evidence and must not overwrite an earlier workload failure;
- telemetry structural evidence failure uses the existing post-start cleanup tail.

Warmups do not produce measured telemetry artifacts unless an explicit diagnostics-only warmup artifact path is added and cannot be mistaken for trial evidence.

## 12. Telemetry preflight

Before managed startup/measurement:

- validate endpoint syntax/policy;
- GET `/v2/healthz`;
- GET `/v2/status`;
- deserialize with `gregg-protocol`;
- validate protocol schema/capabilities;
- capture daemon sampling interval/system identity.

Required telemetry:

- any endpoint/network/schema/health failure prevents startup.

Optional telemetry:

- record a warning;
- disable the collector for the run;
- requested Gregg-derived metrics later normalize as missing rather than zero.

Do not retry indefinitely.

## 13. Gregg polling strategy

Gregg serves cached snapshots and owns native sampling.

Eggbench should not poll faster than the daemon can meaningfully update.

Initial rule:

~~~text
poll_interval = clamp(
  daemon.sample_interval_ms,
  min = 250ms,
  max = 5s
)
~~~

If protocol field naming differs, use the validated v2 snapshot sample interval.

During a measured trial:

- take one snapshot immediately after telemetry start;
- poll at the bounded cadence;
- take a final snapshot during stop when practical;
- deduplicate identical `observed_at_unix_ms` snapshots in retained trial series.

No sub-250ms default polling.

## 14. Cancellation

Polling runs in an owned task tied to the trial collector.

On cancellation/workload completion:

- signal polling task to stop;
- join it with a bounded timeout;
- attempt final snapshot only within the stop budget;
- return captured samples even if final snapshot fails;
- leave no detached polling task.

A telemetry task leak is a closure blocker.

## 15. Raw telemetry artifact

Retain per measured trial:

~~~text
gregg.ndjson
~~~

Each line is the exact validated v2 status JSON or a canonical reserialization of the typed payload.

Choose one and document it.

Preferred:

- retain exact response bytes for source provenance;
- parse/validate separately;
- bound total artifact bytes;
- record dropped/truncated sample count if the cap is reached.

If truncation occurs, derived metrics may still be valid only if the retained/internal sample set used for aggregation is complete and bounded; otherwise mark affected metrics invalid.

## 16. Host metric naming

Do not reuse process-specific metric names ambiguously.

Initial custom names:

- `host_cpu_percent`;
- `host_memory_used_bytes`;
- `host_memory_percent`;
- `host_cpu_frequency_hz`;
- `host_disk_read_bytes_per_sec`;
- `host_disk_write_bytes_per_sec`;
- `host_network_rx_bytes_per_sec`;
- `host_network_tx_bytes_per_sec`.

These remain custom metrics under metric vocabulary v1.

Do not bump metric vocabulary merely to reserve them in M001b.

Plans requesting them must declare unit/direction/intent explicitly.

## 17. Aggregation semantics

Trial-level aggregation:

- CPU percent: arithmetic mean;
- memory used bytes: maximum;
- memory percent: maximum;
- CPU frequency: arithmetic mean of present values;
- disk read/write byte rate: arithmetic mean;
- network rx/tx byte rate: arithmetic mean.

Missing optional source fields:

- omitted from that metric's sample set;
- if no valid samples remain, metric is missing/unsupported;
- never substitute zero unless Gregg explicitly measured zero.

All arithmetic must reject nonfinite values.

## 18. Gregg capabilities

Respect v2 capability/optional-field semantics.

Examples:

- unsupported load/swap/frequency/network data is absence, not failure;
- capability says supported but required field repeatedly absent -> warning/invalid source depending on requested field;
- unknown additive fields are ignored by the Gregg protocol crate.

Do not recreate Gregg validation rules manually.

## 19. Error categories

Stable telemetry errors:

- endpoint_invalid;
- endpoint_not_loopback;
- health_unavailable;
- status_unavailable;
- schema_unsupported;
- payload_invalid;
- polling_failed;
- polling_timeout;
- collector_cancelled;
- artifact_bound_exceeded.

Keep raw HTTP/Gregg errors as bounded detail.

## 20. Metric-source collision policy

The existing normalization logic treats duplicate raw observations for one requested metric as invalid.

M001b must preserve that.

Gregg custom metric names are intentionally distinct from workload metrics.

If a future workload emits `host_cpu_percent` too, the duplicate must become invalid rather than selecting one silently.

## 21. Trial metric provenance

For Gregg-derived observations record:

- producer: `gregg`;
- producer version: protocol/adaptor version provenance;
- source field, e.g. `v2.cpu.usage_pct`;
- normalization method;
- raw artifact reference: `gregg.ndjson`.

No daemon version is fabricated.

## 22. CLI/doctor integration

With `gregg` feature enabled:

- production catalog registers telemetry descriptor;
- `doctor` validates config and reports descriptor capability;
- `doctor` MAY perform health/status preflight only if current CLI semantics explicitly permit network preflight; otherwise leave live probing to run preflight and document the distinction;
- `run` constructs telemetry collector from resolved plan.

Feature-disabled telemetry request fails explicitly at resolution.

## 23. Gregg test server

Do not require a real `greggd` binary for routine tests.

Add a deterministic local HTTP fixture using an existing EggServe test/server seam if M001a has landed, or a tiny test-only server owned by tests.

The fixture must emit:

- ready health;
- valid v2 status sequence;
- warming/invalid cases;
- malformed JSON;
- delayed response;
- optional fields absent;
- repeated observed timestamp;
- changing CPU/memory/network rates.

Production code still uses Gregg wire types.

## 24. Optional/required regression tests

Required:

- required Gregg unavailable -> pre-start failure;
- optional Gregg unavailable -> run proceeds + warning + missing metrics;
- malformed v2 payload;
- unsupported schema;
- endpoint non-loopback rejected;
- no credentials accepted;
- empty optional field remains missing;
- valid zero rate remains observed zero;
- duplicate timestamps deduplicated;
- polling stops after cancellation.

## 25. Timing regression tests

Use an injected delayed telemetry collector to prove:

- telemetry start delay does not inflate `measurement_elapsed_ns`;
- telemetry stop delay does not inflate `measurement_elapsed_ns`;
- telemetry failure after workload does not rewrite measured elapsed;
- request count/statistical trial count invariant remains unchanged.

## 26. Cleanup composition tests

Cases:

- workload failure + telemetry stop failure;
- workload cancellation + telemetry stop timeout;
- telemetry artifact staging failure after workload;
- telemetry drain failure + service teardown.

Original workload/evidence failure precedence must remain consistent with M002 cleanup rules.

## 27. Cross-platform scope

Gregg v2 is cross-platform.

M001b code must compile/test on:

- Linux;
- macOS;
- Windows.

Because M001b only talks to a loopback HTTP daemon fixture, it does not depend on managed process support on Windows.

## 28. Resource-interference guard

Telemetry itself perturbs the system.

Record:

- poll interval;
- sample count;
- response bytes;
- collector elapsed outside workload timer where applicable.

Do not poll more frequently than configured policy.

Add a test proving no more than the expected bounded poll count occurs for a short deterministic trial.

## 29. Broad verification

Required:

    cargo fmt --all -- --check
    cargo check --workspace --all-targets --all-features --locked
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-features --locked
    cargo +1.89.0 check --workspace --all-targets --all-features --locked
    cargo tree --locked
    git diff --check

Also run a minimal feature-off build proving Gregg dependencies are absent.

Hosted four-lane CI must be green.

## 30. Documentation

Add/update:

- `docs/gregg-telemetry.md`;
- `architecture/drivers.md`;
- `architecture/runner.md`;
- `docs/trial-orchestration.md`;
- `docs/metrics.md`;
- `docs/evidence-bundle.md`;
- README;
- Eggstack roadmap/registry.

Include the exact host metric names and aggregation methods.

## 31. Acceptance criteria

M001b closes only when:

1. runner has a generic telemetry collector lifecycle;
2. telemetry start/stop are outside measured workload elapsed;
3. cleanup executes on failure/cancellation;
4. Gregg v2 health/status are parsed through `gregg-protocol`;
5. only loopback Gregg endpoints are accepted;
6. required/optional semantics are truthful;
7. polling cadence is bounded by daemon cadence policy;
8. raw Gregg NDJSON is retained and bounded;
9. host metrics use unambiguous custom names and explicit aggregation;
10. normalized TrialMetrics include requested Gregg metrics with source provenance;
11. missing optional Gregg fields remain missing;
12. no telemetry task leaks;
13. feature-off minimal build excludes Gregg;
14. hosted CI/MSRV are green.

Closing M001b together with M001a closes Eggstack Integration M001.

## 32. Stop conditions

Stop for planning review if:

- gregg-protocol v2 cannot deserialize current daemon output without private/internal types;
- live telemetry requires changing ExperimentPlan schema v1;
- telemetry cannot be placed outside measured timing without redesigning M002 phase semantics;
- combining workload + telemetry raw metrics requires changing TrialMetrics v1;
- safe Gregg use requires non-loopback unauthenticated endpoints;
- polling overhead requires a push protocol to meet the milestone.

## 33. Closure evidence required

Record:

- implementation commits;
- exact gregg-protocol/Eggfetch versions;
- telemetry trait/registry API;
- phase-order trace proving timing exclusion;
- required vs optional failure examples;
- representative Gregg NDJSON;
- normalized host metric examples;
- sample cadence/count evidence;
- cancellation/task cleanup proof;
- dependency tree feature-on/off;
- Rust 1.89 result;
- hosted CI run ID;
- known limitations;
- unresolved findings/severity;
- disposition.
