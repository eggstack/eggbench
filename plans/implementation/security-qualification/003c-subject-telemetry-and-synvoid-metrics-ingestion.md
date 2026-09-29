# Security Qualification M003c — Subject Telemetry and SynVoid Metrics Ingestion

Status: **CLOSING — terminal SynVoid v2 qualification unblocked** (2026-09-29). Generic collector implementation landed at `2742e0e`; the former upstream owner-contract blocker is closed. Remaining work is Eggbench-owned current-head live/hosted qualification and closure evidence.

Research baselines:

- original collector baseline: `eggstack/eggbench@1eb1bfed4edbc6c4af4f193fa71dc82235ad927e`;
- current Eggbench reconciliation baseline:
  `a96407017716b8ca7d18da8237a135b1a49af8cc`;
- original SynVoid research baseline:
  `dbowm91/synvoid@30e68af8f6e79ce0fe07f0c1871f1d0caa6be6f2`;
- closed SynVoid v2 owner implementation:
  `dbowm91/synvoid@739e7ba6f02c5e3f83fe9ff5321b09213182b193`;
- SynVoid current closeout head observed during reconciliation:
  `0dc1f7fb21a5df60e72fc7f2cd60b7cb73bc9f35`.

Prerequisites:

- M003a closed — satisfied;
- M003b closed — satisfied;
- SynVoid M003 telemetry owner contract closed — satisfied under
  `synvoid.eggbench-telemetry.v2`.

SynVoid terminal authority:
`architecture/eggbench_security_qualification_m003_telemetry_corrective_closeout.md`.
The v1 owner contract is withdrawn/unqualified and must not be consumed.

Closed owner mapping SHA-256:
`622f6a13c4353cc7465cce39a57ed86fa0db2fe4114258e6f06226c1748d2d99`.

The SynVoid owner closeout already exercised Eggbench's real
`PrometheusMappingV1` parser/validator and `PrometheusHttpCollector` against
the exact v2 mapping and a live minimal SynVoid endpoint. That proves the
cross-repo interface is compatible; it does **not** substitute for this plan's
Eggbench-owned current-head M003b pressure/body trial evidence, polling/drain
evidence, hosted CI, or terminal closure.

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
      "prometheus_name": "synvoid_subject_event_loop_lag_ms",
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

The closed SynVoid v2 mapping now freezes the initial normalized set:

- `subject_event_loop_lag_ms` — max;
- `subject_request_queue_p95_ms` — max;
- `subject_active_connections` — max;
- `subject_worker_memory_bytes` — max;
- `subject_worker_cpu_percent` — mean;
- `subject_body_buffering_bytes_total` — counter delta, bytes;
- `subject_offload_submissions_total` — counter delta, count;
- `subject_offload_timeouts_total` — counter delta, count;
- `subject_offload_rejections_total` — counter delta, count;
- `subject_offload_fallbacks_total` — counter delta, count;
- `subject_cpu_worker_rss_bytes` — max when owner-exported, optional;
- `subject_worker_metric_resets_total` — counter delta, optional diagnostic.

The Prometheus source names remain owner-authored
`synvoid_subject_*` names supplied by the digest-pinned mapping; Eggbench code
must not hardcode them outside fixtures/tests that explicitly assert the owner
contract.

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

The owner contract is now closed; execute this section rather than waiting on
upstream work.

Use the closed `synvoid.eggbench-telemetry.v2` materializer output and:

- pin an exact proof-bearing SynVoid revision at or after
  `739e7ba6f02c5e3f83fe9ff5321b09213182b193`;
- materialize owner qualification assets with telemetry enabled and verify the
  mapping digest is
  `622f6a13c4353cc7465cce39a57ed86fa0db2fe4114258e6f06226c1748d2d99`;
- start the real minimal SynVoid binary with admin disabled;
- verify the metrics endpoint is loopback only;
- execute M003b pooled and pressure/body scenarios through the normal Eggbench
  runner, not an out-of-tree proof helper;
- collect target telemetry during the exact measured trial windows;
- show at least one required gauge changes plausibly under load;
- show at least one required monotonic counter delta path;
- confirm optional absence remains absent/warned rather than fabricated;
- show missing/renamed required metric and TYPE drift fail closed;
- prove the polling task drains and does not outlive trial/runner teardown;
- retain exact Eggbench SHA, SynVoid SHA, owner-contract digest, mapping digest,
  binary digest, scenario identity, and collector identity.

The SynVoid-side 16-check interop proof is accepted as interface evidence and
need not be duplicated byte-for-byte. This Eggbench proof must instead close
the consumer-side trial/lifecycle/evidence requirements.

Do not interpret the result as universal production capacity evidence.

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

M003c is **unblocked and closing**. M003a-b and the SynVoid v2 owner contract
are closed.

Terminal closure remains Eggbench-owned and requires:

1. current-head real SynVoid v2 live telemetry through the M003b
   pressure/body scenarios;
2. polling/drain and fail-closed drift evidence;
3. normal hosted CI green on the closure source;
4. a closure record that pins the exact Eggbench/SynVoid revisions and
   mapping/contract/binary identities.

The M003d CI portability corrective
(`003d-corrective-eggsec-load-ci-portability-and-unblock.md`) may execute in
parallel and should land before relying on repo-wide hosted CI, because current
HEAD is red for unrelated M003d registration/Windows-lint defects.

M003d terminal profile/live closure remains gated on this M003c terminal proof
and M002 C002 closure.
