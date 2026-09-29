# Security Qualification M003c — Subject Telemetry and SynVoid Metrics Ingestion

Status: authored; blocked on M003b closure and the SynVoid owner telemetry-contract handoff (upstream plan is READY, not closed).

Research baselines:

- `eggstack/eggbench@1eb1bfed4edbc6c4af4f193fa71dc82235ad927e`
- `dbowm91/synvoid@30e68af8f6e79ce0fe07f0c1871f1d0caa6be6f2`

Prerequisites:

- M003a closed;
- M003b closed;
- `dbowm91/synvoid:plans/eggbench_security_qualification_m003_telemetry_contract.md` closed for live SynVoid proof.

## 1. Objective

Add a reusable target/subject telemetry collector that can observe a versioned loopback Prometheus endpoint during the exact measured trial window.

This closes the gap between:

- Gregg host/testbed telemetry, which must remain host-scoped; and
- subject-owned runtime telemetry such as SynVoid event-loop lag, request-queue timing, active connections, worker memory, body buffering, and CPU-offload saturation.

Eggbench must not parse SynVoid IPC, depend on SynVoid Rust crates, or reinterpret internal target state.

## 2. Research findings

The runner already has the correct lifecycle seam:

- `TelemetryCollector::start_trial` opens before workload timing;
- `stop_trial` closes after workload timing;
- collectors emit raw observations, bounded artifacts, warnings, and identity.

Gregg already proves this seam but exposes host-level fields only.

Current Gregg v2 protocol has no per-process/PID telemetry contract, so Gregg must not be used as a SynVoid-process RSS surrogate.

Current SynVoid worker payloads already contain the desired target metrics:

- `event_loop_lag_ms`;
- `request_queue_time_ms` avg/p50/p95/p99;
- `active_connections`;
- `memory_bytes` and `cpu_percent`;
- per-phase inline CPU timing;
- `body_buffering_bytes_total`;
- offload submissions/timeouts/rejections/fallbacks;
- CPU-worker RSS/task-duration information.

SynVoid also contains a Prometheus exporter that binds loopback, but the M002 qualification config explicitly disables metrics and the required worker-heartbeat fields are not currently a frozen owner-qualified Prometheus surface.

Therefore Eggbench needs a generic Prometheus collector, while SynVoid owns publication/naming/stability of its own metrics.

## 3. Generic Prometheus telemetry source

Add a telemetry source named `prometheus-http` or equivalent.

It must be target-neutral and configured through:

- target service;
- runtime binding key containing the scrape URL;
- workspace-relative metric mapping/contract file;
- bounded poll interval;
- requested fields;
- required/optional semantics.

The collector must accept only loopback/private endpoints under the same confinement model used for security drivers. No public metrics scrape is needed for M003.

## 4. Mapping contract

Do not hardcode SynVoid metric names in the generic collector.

Use a bounded immutable mapping file with content identity, for example:

~~~json
{
  "schema_version": 1,
  "source": "prometheus",
  "fields": [
    {
      "output_name": "subject_event_loop_lag_ms",
      "prometheus_name": "synvoid_event_loop_lag_ms",
      "kind": "gauge",
      "unit": "ms",
      "aggregation": "max"
    }
  ]
}
~~~

Required field semantics:

- `gauge` — aggregate retained samples using explicit mean/max/min policy;
- `counter` — compute non-negative trial delta from a pre-window/start observation to the final observation;
- counter reset during a measured trial yields warning/invalid required observation, never a negative delta;
- unsupported/missing required field invalidates the trial;
- optional fields remain absent with a warning, never fabricated as zero.

Mapping identity is evidence and comparison-critical when mapped metrics gate acceptance.

## 5. Prometheus parser and bounds

Implement only the bounded text exposition needed for numeric scalar samples.

Requirements:

- body byte cap;
- line count cap;
- metric-name/label length caps;
- finite numeric values only;
- reject duplicate ambiguous samples for a field unless the mapping declares an exact low-cardinality label selector;
- reject wildcard label matching;
- no high-cardinality value retention;
- parser errors are explicit;
- raw scrape retention is optional and bounded/redacted; normalized observations are the durable requirement.

Use a small maintained parser crate if it materially reduces parser risk without bloating the dependency graph; otherwise implement a narrow tested parser for the supported subset and document the grammar.

## 6. Trial synchronization

Recommended lifecycle:

1. preflight endpoint and mapping before managed subject workload begins;
2. take a start snapshot immediately before/at `start_trial`;
3. poll at bounded cadence while the trial is active;
4. take a final snapshot in `stop_trial`;
5. aggregate gauges and calculate counter deltas;
6. stop all polling tasks before returning.

The collector must honor cancellation and never outlive runner drain.

Polling overhead must be measured on a local fixture; default cadence should be conservative.

## 7. Normalized initial target metrics

For the SynVoid M003 profile, normalize a bounded initial set such as:

- `subject_event_loop_lag_ms` — max;
- `subject_request_queue_p95_ms` — max or mean of exported p95 snapshots, with the chosen method frozen;
- `subject_active_connections` — max;
- `subject_memory_bytes` — max;
- `subject_cpu_percent` — mean;
- `subject_body_buffering_bytes` — counter delta;
- `subject_offload_submissions` — counter delta;
- `subject_offload_timeouts` — counter delta;
- `subject_offload_rejections` — counter delta;
- `subject_offload_fallbacks` — counter delta;
- `subject_cpu_worker_rss_bytes` — max when owner-exported.

Exact SynVoid names come from the owner mapping contract, not from Eggbench code.

Keep Gregg host metrics separately named `host_*`.

## 8. Gating policy

Initially treat target telemetry as diagnostic unless repeatability evidence supports a gate.

Never freeze resource/event-loop thresholds from one hosted run.

Before a target telemetry metric becomes primary:

- run same-revision repeated qualification;
- record host class and testbed identity;
- demonstrate stable measurement semantics;
- choose an absolute or relative threshold before candidate interpretation.

Counter deltas such as timeout/rejection counts may use an absolute zero gate when the owner contract defines any occurrence as undesirable and repeatability confirms no harness artifact.

## 9. Evidence

Per trial retain:

- collector identity/version;
- endpoint authority without credentials;
- endpoint binding source;
- mapping contract identity/digest;
- poll cadence;
- sample count and missing/parse warning counts;
- aggregation/delta policy;
- normalized observations;
- optional bounded source scrape artifact if enabled.

Do not retain authentication tokens, arbitrary labels, request payloads, or high-cardinality worker/site identifiers.

## 10. SynVoid live proof

Once the upstream SynVoid telemetry contract closes:

- materialize the owner qualification assets with telemetry enabled;
- start the real minimal SynVoid binary;
- verify the metrics endpoint is loopback only;
- execute M003b pooled and pressure/body scenarios;
- collect target telemetry during measured trials;
- show at least one gauge changes plausibly under load;
- show at least one monotonic counter delta path;
- show missing/renamed required metric fails closed;
- retain exact SynVoid SHA, telemetry-contract digest, binary digest, and Eggbench mapping digest.

Do not interpret this as universal production capacity evidence.

## 11. Tests

Required tests:

1. loopback endpoint acceptance/public endpoint rejection;
2. parser numeric/NaN/Inf rejection;
3. duplicate ambiguous metric rejection;
4. label-selector exact-match behavior if supported;
5. gauge mean/max aggregation;
6. counter delta;
7. counter reset handling;
8. required missing metric invalidation;
9. optional missing metric warning;
10. cancellation drains polling task;
11. trial start/stop ordering;
12. artifact/sample bounds;
13. Gregg and subject telemetry names cannot collide;
14. mapping digest change changes method/evidence identity.

## 12. Acceptance criteria

M003c closes only when:

1. a generic subject-neutral Prometheus collector exists;
2. mappings are immutable workspace inputs;
3. trial synchronization is exact and bounded;
4. gauge and monotonic-counter semantics are explicit;
5. target metrics remain distinguishable from Gregg host metrics;
6. live SynVoid telemetry executes through an owner-qualified contract;
7. missing/drifted metrics fail closed when required;
8. no SynVoid Rust/IPC dependency enters Eggbench;
9. polling overhead is bounded and documented;
10. hosted CI plus live SynVoid telemetry proof are green.

## 13. Stop conditions

Stop and re-plan if:

- required SynVoid data is not exposed through an owner-stable endpoint;
- generic ingestion would require scraping authenticated admin HTML/WS internals;
- metric labels require retaining unbounded/high-cardinality identifiers;
- polling materially perturbs the workload and cannot be reduced to an acceptable bounded cadence;
- target and host metrics cannot be kept semantically distinct.

## 14. Handoff

Blocked on M003b and the SynVoid telemetry owner contract for terminal live qualification.

The generic collector may be developed/tested against hermetic fixture endpoints before the SynVoid handoff closes, but M003c must not close on a fake endpoint alone.
