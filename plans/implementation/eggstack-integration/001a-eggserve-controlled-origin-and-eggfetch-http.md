# Eggstack Integration M001a — EggServe Controlled Origin and Eggfetch Native HTTP Workload

Status: ready; unblocked by External Oracles M001 closure (`plans/closure/external-oracles/001-status.md`)

Repository planning baseline: `852bf2dab6a266cc5043cf0817b429a8073d6339`

Source roadmap:

- `plans/subsystems/eggstack-integration-roadmap.md` — first half of M001

Hard dependency (satisfied):

- `plans/implementation/external-oracles/001-external-command-driver-substrate.md` landed first (closure `plans/closure/external-oracles/001-status.md`, commit `7afa054`) and established `crates/eggbench-drivers` and production catalog ownership.

Closed semantic prerequisites:

- Local Runner M003;
- Measurement M001;
- post-M003/M001 qualification corrective C001.

Controlling ADRs:

- `plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md`
- `plans/adrs/ADR-0004-eggstack-composition-and-independent-oracles.md`

Long-term requirements:

- `plans/000-long-term-specification.md#7-topology-and-service-model`
- `plans/000-long-term-specification.md#8-workload-model`
- `plans/000-long-term-specification.md#14-eggstack-ownership-and-reuse`
- `plans/000-long-term-specification.md#18-crate-architecture`

Primary class: capability/infrastructure.

## 1. Objective

Prove the first real Eggstack-native experiment path without copying HTTP implementation:

~~~text
ExperimentPlan
  |
EggServe named managed origin
  |
runtime HTTP binding
  |
Eggfetch WorkloadExecutor
  |
M002 trial lifecycle
  |
M001 TrialMetrics + raw HDR evidence
  |
immutable .eggb
~~~

M001a owns:

1. a generic named in-process service-adapter seam in the runner;
2. runtime service bindings visible to workload drivers;
3. an EggServe H1 controlled-origin adapter;
4. an Eggfetch H1 native closed-loop workload adapter;
5. normalized/raw HTTP measurement evidence;
6. production catalog/CLI wiring behind an explicit Eggstack feature;
7. one deterministic end-to-end loopback experiment.

Gregg telemetry is deliberately split into M001b.

## 2. Re-audited sibling state

Audit date: 2026-09-23 planning pass.

### Eggfetch

Current repository evidence:

- `eggfetch-core` package version 0.2.0;
- Rust 1.89;
- async-first public client;
- supported H1/H2, experimental H3;
- lean `standard-http1` profile exists;
- own benchmark harness is unpublished and remains Eggfetch-owned.

Eggbench integration seam:

~~~toml
eggfetch-core = {
  version = "0.2",
  default-features = false,
  features = ["standard-http1"]
}
~~~

Do not depend on unpublished `eggfetch-bench`.

### EggServe

Current workspace version: 0.2.1.

The public-boundary document explicitly identifies:

- `eggserve-primitives` as canonical application values;
- `eggserve-server` as the direct generic H1 runtime;
- `eggserve-core` as the compatibility/multiprotocol facade.

M001a should prefer direct leaf crates:

~~~toml
eggserve-primitives = "0.2"
eggserve-server = "0.2"
~~~

H2/H3 are not part of this milestone.

## 3. Invariants

1. Eggfetch owns outbound HTTP semantics.
2. EggServe owns inbound HTTP/runtime semantics.
3. Eggbench owns experiment lifecycle, adapter configuration, metric mapping, and evidence only.
4. No Hyper client/server implementation is created in Eggbench.
5. The controlled origin binds loopback only.
6. Runtime-selected ephemeral ports are recorded as evidence.
7. Named in-process services participate in dependency ordering and reverse teardown exactly like managed command services.
8. In-process service failure cannot bypass M002 cleanup.
9. Workload timing includes HTTP request execution, not metric normalization/evidence serialization.
10. Warmups never become measured trial observations.
11. Open-loop semantics are not claimed until a real open-loop scheduler is implemented.
12. Eggfetch workload errors are visible and categorized; they are never silently retried by Eggbench.
13. The first native workload retains a raw latency histogram.
14. No Gregg dependency is required for this path.
15. Minimal/default Eggbench builds remain free of Eggfetch/EggServe unless feature-selected.

## 4. Non-goals

Do not implement:

- H2/H3 qualification;
- TLS;
- proxy routing;
- Eggress;
- Eggchaos;
- external oha/h2load comparison;
- Gregg telemetry;
- open-loop corrected latency;
- POST/upload workloads;
- arbitrary HTTP scripting;
- production public-network targets;
- security scanning;
- Windows Job Objects;
- a second HTTP stack.

## 5. Feature/dependency boundary

After External Oracles M001 creates `eggbench-drivers`, add:

~~~text
feature: eggstack-http
  -> eggfetch-core/standard-http1
  -> eggserve-primitives
  -> eggserve-server
  -> optional hdrhistogram-compatible dependency if selected
~~~

CLI may expose a matching cargo feature that forwards to `eggbench-drivers/eggstack-http`.

Default features remain empty/minimal.

Add CI feature coverage explicitly; do not make the whole workspace depend on Eggstack HTTP by default if feature isolation can preserve the minimal build.

## 6. Exact dependency provenance

The integration roadmap requires exact version evidence.

At build time, embed the exact versions resolved from the workspace `Cargo.lock` for:

- eggfetch-core;
- eggserve-server;
- eggserve-primitives.

A small build script may parse the lockfile and emit compile-time env values.

Do not hardcode a patch version in runtime evidence independently of the lockfile.

Driver descriptors record:

- canonical adapter name;
- Eggbench adapter version;
- sibling crate name;
- exact resolved sibling version;
- capability set.

## 7. Generic named-service adapter seam

The current runner owns command processes only. Add an object-safe named-service seam, e.g.:

~~~text
trait ManagedServiceAdapter: Send + Sync {
  fn service_type(&self) -> &Name;
  fn start<'a>(
    &'a self,
    request: ServiceStartRequest,
    cancel: CancellationToken
  ) -> BoxFuture<'a, Result<Box<dyn ManagedServiceHandle>, ServiceAdapterError>>;
}

trait ManagedServiceHandle: Send {
  fn bindings(&self) -> RuntimeBindings;
  fn shutdown<'a>(
    &'a mut self,
    grace: Duration
  ) -> BoxFuture<'a, Result<(), String>>;
}
~~~

Exact method names may differ.

Add a `ServiceAdapterRegistry` to runner options or another explicit runner dependency.

Do not make core depend on concrete Eggstack crates.

## 8. Mixed managed-service ownership

Refactor `LocalSession` internals to support:

~~~text
RunningManagedService::Process(...)
RunningManagedService::Adapter(...)
~~~

Requirements:

- dependency order is unchanged;
- reverse teardown is unchanged;
- original failure remains primary;
- every started adapter service is attempted during cleanup;
- adapter shutdown failure becomes existing cleanup evidence;
- command-process behavior/regressions remain unchanged;
- externally managed named services remain unowned.

No service may be represented as process-owned if it is actually in-process.

## 9. Named-service readiness

For M001a:

- `ManagedServiceAdapter::start` returns only after the adapter's own readiness condition is satisfied;
- EggServe uses its typed server readiness handle;
- plan-level `Readiness::Probe` on an in-process named service is rejected unless a compatible probe path is explicitly registered;
- an optional plan-level post-ready delay MAY remain supported outside measurement.

Do not fake a PID for an in-process service.

Lifecycle evidence needs an adapter-service identity rather than OS PID.

## 10. Runtime bindings

Add a protocol-neutral runner-owned runtime binding map.

Representative shape:

~~~text
RuntimeBindings {
  services: BTreeMap<Name, BTreeMap<Name, String>>
}
~~~

For EggServe origin:

~~~text
service "origin":
  "http_url" -> "http://127.0.0.1:<ephemeral>/bench"
~~~

Rules:

- values are non-secret;
- keys are bounded `Name` values;
- bindings are immutable after successful startup;
- workloads only receive bindings after readiness;
- runtime bindings remain available through teardown for final evidence.

## 11. Invocation context integration

Extend `InvocationContext` with a read-only snapshot/reference to runtime bindings.

Every M002 invocation gets the same startup-established bindings.

Qualification fake workloads may ignore the new field.

Do not let workload drivers mutate topology bindings.

## 12. Runtime topology evidence

Add a small versioned runner artifact, e.g.:

~~~text
runtime-topology.json
~~~

containing:

- schema version;
- service identities;
- adapter/process ownership kind;
- non-secret runtime bindings;
- adapter provenance.

Stage it outside measured intervals, preferably in final runner evidence staging from retained session state.

If runtime-topology staging fails after startup, the existing M002 evidence-safety cleanup invariant must still hold.

Update evidence-capacity preflight.

## 13. EggServe origin adapter

Canonical service type:

~~~text
eggserve-origin
~~~

Initial configuration keys in `Service.config`:

- `path` — default `/bench`;
- `body_bytes` — bounded deterministic response body size, default 1024;
- `status` — default 200, initial accepted range 200–599 only if needed by tests.

Bind policy:

- always `127.0.0.1:0` or `[::1]:0`;
- no public bind in M001a;
- actual address comes from EggServe server handle/runtime;
- emit `http_url` binding after readiness.

Response:

- deterministic bytes generated once at startup;
- fixed content length;
- no filesystem/static-file dependency required;
- no timestamp/random body.

## 14. EggServe lifecycle

Use the direct `eggserve-server` public supervision contract:

- start server;
- wait for typed readiness;
- retain shutdown control/completion;
- on cancellation/startup failure, request shutdown;
- on normal teardown, graceful shutdown within plan grace;
- propagate typed completion failures as cleanup diagnostics.

Do not call private/internal EggServe modules.

## 15. Eggfetch workload adapter

Canonical workload driver name:

~~~text
eggfetch-http
~~~

M001a capability matrix:

Supported:

- `Workload::ClosedLoop` with requests;
- `Workload::ClosedLoop` with duration;
- `Workload::FiniteCount`;
- `Workload::TimeBounded { mode: ClosedLoop }`.

Not supported:

- `OpenLoop`;
- `TimeBounded { mode: OpenLoop }`;
- H2/H3;
- proxy/TLS-specific semantics.

Resolution must fail explicitly for unsupported load modes.

## 16. Eggfetch client lifetime

Use one Eggfetch client per workload-executor/run, not one client per request.

Consequences:

- warmups can establish pool/connection state;
- measured trials reflect a warmed run when warmups are configured;
- connection reuse is part of driver method provenance;
- resets do not secretly recreate the client unless a later explicit reset adapter does so.

Document this method in evidence.

## 17. Request semantics

Initial native request:

- HTTP GET;
- target = workload target service's `http_url` runtime binding;
- response body fully consumed;
- no Eggbench-level retries;
- Eggfetch logical retry/redirect features should be omitted by the lean `standard-http1` feature profile;
- transport failure counts as error;
- timeout counts separately;
- non-2xx response counts as an HTTP-status error while still retaining status-category evidence.

Do not follow redirects in M001a.

## 18. Closed-loop scheduling

For concurrency N:

- maintain at most N active requests;
- after one request finishes, the worker issues the next until request count/deadline;
- cancellation stops issuing new requests and allows bounded in-flight completion/cancellation;
- no busy-spin;
- no per-request task leak after invocation.

Finite request count must issue exactly the planned count unless the trial fails/cancels.

Time-bounded mode stops issuance at the deadline.

## 19. Latency measurement

Measure end-to-end request latency from request dispatch until full response body consumption or terminal error.

Do not include post-invocation metric normalization.

Use a bounded HDR-style histogram implementation rather than retaining every latency sample indefinitely.

Preferred:

- a mature Rust HDR histogram crate;
- fixed precision/range documented by the adapter;
- no unbounded vector of request latencies.

Record histogram method and correction policy.

M001a makes **no coordinated-omission correction claim** because only closed-loop load is supported.

## 20. Raw histogram artifact

Retain one same-trial raw histogram artifact, e.g.:

~~~text
latency.hdr
~~~

Requirements:

- documented encoding/format identifier;
- bounded size;
- referenced by normalized latency observations through `RawHistogramInput`;
- digest included in bundle manifest through existing artifact staging.

If the selected histogram library has no stable portable binary encoding suitable for evidence, use a documented bounded textual interval/histogram format rather than an opaque internal serialization.

## 21. Raw metrics emitted

At minimum emit when representable:

- `throughput` / `rps` / Rate;
- `latency_min` / `ms` / Minimum;
- `latency_mean` / `ms` / Mean;
- `latency_p50`;
- `latency_p90`;
- `latency_p95`;
- `latency_p99`;
- `latency_p999`;
- `error_rate` / ratio;
- `timeout_rate` / ratio;
- `bytes_received` / bytes / Sum.

Only metrics requested by the plan become gate-eligible normalized observations under existing M001 logic.

Use source fields such as `eggfetch.completed_requests`, not human prose.

## 22. Error distribution

Populate stable categories such as:

- transport;
- timeout;
- http_3xx;
- http_4xx;
- http_5xx;
- body_read;
- cancelled.

Do not expose unstable full Eggfetch error strings as category identities.

Raw detailed errors, if retained, must be bounded and redaction-safe.

## 23. Workload saturation/method metadata

Retain diagnostic evidence:

- requested concurrency;
- completed requests;
- attempted requests;
- invocation elapsed;
- maximum observed in-flight count;
- client reuse policy;
- Eggfetch exact dependency version;
- H1-only method.

Do not claim offered-rate achievement because M001a is closed-loop only.

## 24. Production catalog integration

With `eggstack-http` enabled, production catalog registers:

- EggServe named service adapter descriptor;
- Eggfetch workload driver descriptor.

Without the feature, catalog remains empty.

`doctor` must show exact adapter/sibling versions and supported load-mode capabilities.

## 25. Production `run` generalization

The current production `run` intentionally stops after resolving an empty catalog.

Refactor it to consume the production catalog generically:

1. resolve plan;
2. construct selected service adapter registry;
3. construct selected workload executor;
4. perform existing environment/subject/bundle preparation;
5. `LocalSession::prepare`;
6. `execute_run` with real executor;
7. existing truthful exit/presentation logic.

No adapter name switch belongs in `main.rs`.

When feature-disabled or unsupported, existing code-3 behavior remains.

## 26. Deterministic loopback fixture plan

Add a canonical example/fixture plan:

~~~text
subject: label
service: origin (managed named eggserve-origin)
workload: closed loop -> origin
warmup: >= 1
measured: >= 3 in routine CI, 7 in qualification fixture if runtime permits
metrics:
  throughput
  latency_p99
  error_rate
~~~

Use a small bounded request count/duration so hosted CI remains fast.

## 27. End-to-end acceptance test

A full run must prove:

- EggServe starts and becomes ready;
- runtime ephemeral URL recorded;
- Eggfetch warmup executes;
- measured trials execute;
- no errors on normal loopback;
- raw histogram retained each measured trial;
- `metrics.json` contains observed throughput/latency/error metrics;
- runtime-topology evidence exists;
- server shuts down;
- finalized bundle verifies;
- no managed process/service remains.

## 28. Negative tests

- named service adapter missing;
- duplicate service adapter registration;
- unsupported named-service readiness probe;
- open-loop request rejected before startup;
- target binding missing;
- EggServe startup failure;
- Eggfetch transport failure;
- cancellation during request workload;
- evidence staging failure after service startup still cleans up origin;
- shutdown failure does not overwrite workload failure.

## 29. Methodological guard

A dedicated test compares:

- one run with many requests inside 3 trials;
- one run with few requests inside 3 trials.

Both must expose exactly 3 trial-level observations to Measurement M002.

Raw histogram sample count may differ; statistical sample count may not.

## 30. Cross-platform scope

EggServe/Eggfetch in-process H1 integration should compile and run on:

- Linux;
- macOS;
- Windows.

This path does not depend on managed OS process ownership because the controlled origin is in-process.

Hosted tests should therefore exercise the loopback path on Windows as well.

ARM64-specific code must not be assumed.

## 31. Broad verification

Required:

    cargo fmt --all -- --check
    cargo check --workspace --all-targets --all-features --locked
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-features --locked
    cargo +1.89.0 check --workspace --all-targets --all-features --locked
    cargo tree --locked
    git diff --check

Also qualify a minimal feature-off build to prove Eggfetch/EggServe do not leak into the minimal dependency graph.

## 32. Documentation

Add/update:

- `docs/eggstack-http.md`;
- `architecture/drivers.md`;
- `architecture/runner.md`;
- `docs/trial-orchestration.md`;
- `docs/evidence-bundle.md`;
- README real-run example with feature requirement;
- Eggstack integration roadmap/registry.

## 33. Acceptance criteria

M001a closes only when:

1. runner supports managed named in-process service adapters generically;
2. mixed process/adapter service teardown preserves dependency order and cleanup invariants;
3. runtime bindings are available to workloads and recorded as evidence;
4. EggServe direct H1 public API provides a loopback controlled origin;
5. Eggfetch-core provides the native HTTP workload with no duplicate HTTP implementation;
6. only truthful closed-loop capabilities are advertised;
7. one raw bounded latency histogram is retained per measured trial;
8. core HTTP metrics normalize through existing M001 schema;
9. production CLI can execute the feature-selected real driver path;
10. feature-off production still fails closed with no adapter;
11. end-to-end bundle verifies and origin is shut down;
12. full hosted CI and MSRV are green.

M001 remains open until Gregg telemetry M001b also closes.

## 34. Stop conditions

Stop for planning review if:

- current EggServe direct public APIs cannot provide typed bound-address/readiness/shutdown without private access;
- Eggfetch requires enabling retry/redirect behavior that changes benchmark semantics;
- named in-process services require changing ExperimentPlan schema v1;
- runtime bindings cannot be added without leaking protocol-specific types into core;
- raw histogram support requires unbounded per-request storage;
- feature isolation cannot keep Eggfetch/EggServe out of the minimal build.

## 35. Closure evidence required

Record:

- implementation commits;
- exact resolved Eggfetch/EggServe versions;
- dependency/feature tree on and off;
- named service adapter API;
- mixed service lifecycle evidence;
- runtime-topology JSON example;
- EggServe binding/readiness evidence;
- Eggfetch driver capability matrix;
- representative TrialMetrics + histogram evidence;
- end-to-end loopback bundle tree;
- cancellation/cleanup proof;
- MSRV/hosted CI;
- known limitations;
- unresolved findings/severity;
- disposition.
