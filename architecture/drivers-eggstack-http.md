# Eggstack-native HTTP adapters (`eggbench-drivers::eggstack`)

## Purpose

`crates/eggbench-drivers/src/eggstack/` is the first real Eggstack-native
experiment path (`mod.rs:1-23`): an `ExperimentPlan` names an
`eggserve-origin` managed service, the service publishes a runtime `http_url`
binding, and the `eggfetch-http` workload driver reads that binding and issues
requests through it. Three adapters live here:

| Adapter | Runner seam | Source | Role |
|---|---|---|---|
| `EggfetchWorkload` | `WorkloadExecutor` | [fetch.rs](../crates/eggbench-drivers/src/eggstack/fetch.rs) | Outbound HTTP: closed-loop load, HTTP-corpus load, per-invocation latency/error evidence |
| `EggServeOriginAdapter` | `ManagedServiceAdapter` | [origin.rs](../crates/eggbench-drivers/src/eggstack/origin.rs) | Inbound HTTP: one deterministic route on loopback with an ephemeral port |
| `HttpCorpusExecutor` | `CorrectnessExecutor` | [http_corpus.rs](../crates/eggbench-drivers/src/eggstack/http_corpus.rs) | Status-only fixed-corpus checks outside any measured interval |

The division of labour is the module's point: Eggfetch owns outbound HTTP
semantics, EggServe owns inbound HTTP/runtime semantics, Eggbench owns
lifecycle, scheduling, latency measurement, metric mapping, and evidence
(`fetch.rs:23-25`, `origin.rs:12-15`, `mod.rs:15-18`). No Hyper client or server
implementation is created here.

This document covers the HTTP adapters only. The listener-free
`EggstackPathDialer` they plug into is in
[drivers-network-path.md](drivers-network-path.md), summarized in
[drivers.md](drivers.md#listener-free-path-ownership); the runner-side seams are
in [runner-orchestration.md](runner-orchestration.md) and
[runner-adapter-seams.md](runner-adapter-seams.md). The canonical user-facing
description is [../docs/eggstack-http.md](../docs/eggstack-http.md). Everything
here is behind the `eggstack-http` feature (`lib.rs:16-17`); a default build
links none of it.

## Source layout

| File | Lines | Role |
|---|---|---|
| `crates/eggbench-drivers/src/eggstack/fetch.rs` | 1,520 | Workload executor, closed-loop scheduler, HTTP-corpus load, raw metric construction, method/histogram evidence |
| `crates/eggbench-drivers/src/eggstack/origin.rs` | 265 | Config parsing, route discipline, loopback bind, `RuntimeBindings` publication, shutdown |
| `crates/eggbench-drivers/src/eggstack/http_corpus.rs` | 202 | Serial correctness execution, corpus identity, sanitized `HttpCorpusCheckResultV1` |
| `crates/eggbench-drivers/src/eggstack/mod.rs` | 178 | Module ownership, version constants, descriptor construction, adapter registry |
| `crates/eggbench-drivers/src/eggstack/path/` | — | `eggstack-path` only: dialer, route lowering, fault wrapping, path evidence |

Feature composition (`crates/eggbench-drivers/Cargo.toml:11-25`):
`eggstack-http` pulls `eggfetch-core`, `eggserve-primitives`, `eggserve-server`,
`hdrhistogram`; `eggstack-path` implies `eggstack-http` and adds
`eggress-outbound`/`eggress-uri`/`eggress-core`/`eggchaos-core`. The compiled
Eggfetch profile is `standard-http1` + `advanced-routing`
(`Cargo.toml:37-40`), which is why logical retry/redirect support does not
exist (`fetch.rs:10-12`).

## Key types

### Adapter types

| Type | File | Notes |
|---|---|---|
| `EggfetchWorkload` | `fetch.rs:83-87` | Holds exactly one `eggfetch_core::Client` and an optional `Arc<EggstackPathDialer>`; no per-request client field |
| `RunPlan` | `fetch.rs:512-520` | `Count{total, concurrency}` or `Deadline{deadline, concurrency}` — the only two lowerings accepted |
| `RequestOutcome` | `fetch.rs:500-509` | `latency: Option<Duration>`, `status: Option<u16>`, `bytes: u64`, `error: Option<&'static str>` |
| `Folded` | `fetch.rs:1018-1024` | Histogram + category totals + `completed`/`timeouts`/`bytes_received` |
| `HttpCorpusShared` / `HttpCorpusOutcome` | `fetch.rs:89-112` | Shared worker state for the corpus campaign; per-case result with `expected_match: Option<bool>` |
| `EggServeOriginAdapter` | `origin.rs:55-58` | Stateless `#[derive(Default)]`; all state is per-start |
| `OriginHandle` | `origin.rs:83-86` | `Option<ServerHandle>` (taken on shutdown) plus published `RuntimeBindings` |
| `HttpCorpusExecutor` | `http_corpus.rs:20-22` | Unit struct; no client is retained between checks |

### Retained version constants

No patch version is hardcoded in source (`mod.rs:46-47`): `EGGSTACK_ADAPTER_VERSION`
is `env!("CARGO_PKG_VERSION")` (`mod.rs:44`), while `EGGFETCH_CORE_VERSION`
(`mod.rs:48`), `EGGSERVE_SERVER_VERSION` (`mod.rs:50`), and
`EGGSERVE_PRIMITIVES_VERSION` (`mod.rs:52`) come from
`env!("EGGBENCH_*")` values the crate build script extracts from `Cargo.lock`.
These constants are the only place exact sibling versions are retained: they
populate `upstream_version` in descriptors and `eggfetch_core_version` in
method evidence (`fetch.rs:380`, `fetch.rs:1272`, `http_corpus.rs:174`). The
origin adapter deliberately keeps versions out of bindings — "the bindings map
... carries connection facts only" (`origin.rs:259-260`).

### Measurement and bound constants

| Constant | Value | Line | Applied at |
|---|---|---|---|
| `LATENCY_HISTOGRAM_ARTIFACT` / `METHOD_ARTIFACT` | `latency.hdr` / `eggfetch-method.json` | `fetch.rs:50`, `fetch.rs:52` | Staged per invocation |
| `HISTOGRAM_FORMAT` / `HISTOGRAM_UNIT` | `hdrhistogram-v2` / `us` | `fetch.rs:54`, `fetch.rs:58` | `RawHistogramInput` |
| `HISTOGRAM_LOW_US` / `HIGH_US` / `SIGFIG` | 1 / 60,000,000 / 3 | `fetch.rs:60`, `62`, `64` | Samples clamp; larger values saturate |
| `REQUEST_TIMEOUT_SECS` | 30 | `fetch.rs:67` | Per request, inside the runner safety deadline |
| `MAX_RESPONSE_BODY_BYTES` | 8 MiB | `fetch.rs:68` and `http_corpus.rs:18` (duplicated) | `max_decoded_body_size` |
| Stable error categories | `transport`, `timeout`, `http_3xx`, `http_4xx`, `http_5xx`, `body_read`, `cancelled` | `fetch.rs:72-78` | Never an Eggfetch error string |
| `HTTP_CORPUS_SOURCE` / `HTTP_CORPUS_SEMANTIC_VERSION` | `eggbench-http-corpus` / `….v1` | `http_corpus.rs:15`, `17` | `source()`; `adapter_semantic_version`, `producer` |
| `MAX_ORIGIN_BODY_BYTES` / `ORIGIN_BODY_FILL` | 1 MiB / `0x42` | `origin.rs:48`, `origin.rs:50` | `body_bytes` bound; deterministic fill |
| `UNCONFIGURED_ROUTE_STATUS` | 501 | `origin.rs:52` | Every target other than the configured route |

## How it works

### The client-ownership invariant

**One `EggfetchWorkload` owns one `eggfetch_core::Client` for the whole run.**
That is the single design decision everything else in this module protects,
because it is what makes a measured trial reflect a warm pool instead of a cold
one.

What the code does to make it true:

| Mechanism | Line |
|---|---|
| The client is a struct field, not a local; created once in `new()` or `with_path_dialer()` (the dialer is installed on that same client) | `fetch.rs:83-87`, `450-456`, `461-468` |
| The runner holds `&mut E` for the whole run, and `eggfetch_workload()` is "one client bound to a workload executor for a whole run" | `orchestration.rs:749-752`, `mod.rs:170-178` |
| `execute_inner` passes a borrow of that field into an `Arc<Shared>` that every worker clones | `fetch.rs:572-579`, `825-834`, `836-838` |
| `worker` never constructs a client; the corpus campaign likewise receives `&self.client` | `fetch.rs:860-893`, `917-920`, `682-694` |
| `drain` is a deliberate no-op — "connection pool state is Eggfetch-owned and needs no explicit close" — so pool state survives into the next trial | `fetch.rs:530-537` |
| The only per-request timer is a timeout (`Timeout::from_secs(30)`), which does not evict pooled connections | `fetch.rs:919-920` |

`method_evidence` records the policy as
`"client_reuse_policy": "one client per executor for the whole run"`
(`fetch.rs:1275`) and `debug_struct` prints driver + `eggfetch_core` version
(`fetch.rs:490-497`).

What would break it: constructing a client per request or per trial; creating a
new `EggfetchWorkload` inside `execute`; adding per-invocation pool teardown to
`drain`; or giving the per-request path its own transport so measured traffic
uses a different pool from warmup traffic. One *deliberate* exception already
exists: `HttpConnectionPolicy::FreshPerRequest` builds a fresh
`eggfetch_core::Client::new()` per request (`fetch.rs:215-218`) — owner-authored,
plan-pinned (`plan.rs:454-459`), and recorded in evidence as
`"physical_connection_contract": "new client per request; ..."` (`fetch.rs:388`),
so it is a declared contract rather than a silent regression.

### The `WorkloadExecutor` seam and load modeling

`impl WorkloadExecutor for EggfetchWorkload` (`fetch.rs:522-554`) provides
`execute` (`fetch.rs:523-528`), `drain` (`fetch.rs:530-537`), and a
feature-gated `run_evidence` (`fetch.rs:539-553`). `execute` forwards to
`execute_inner`, which dispatches `Workload::HttpCorpus` first
(`fetch.rs:561-563`) and otherwise lowers the plan through `run_plan`
(`fetch.rs:718-783`) — the whole mapping:

| `Workload` variant | Lowering | Line |
|---|---|---|
| `ClosedLoop` + `requests` | `RunPlan::Count` | `fetch.rs:728-734` |
| `ClosedLoop` + `duration_ms` | `RunPlan::Deadline` | `fetch.rs:735-741` |
| `ClosedLoop` with both/neither | `WorkloadFailed` | `fetch.rs:742` |
| `FiniteCount` | `RunPlan::Count` | `fetch.rs:745-755` |
| `TimeBounded`, `mode == ClosedLoop` | `RunPlan::Deadline`; absent concurrency becomes 1 | `fetch.rs:756-777` |
| `TimeBounded`, open mode | `WorkloadFailed` | `fetch.rs:763-765` |
| `OpenLoop`, `SemanticReplay`, `HttpCorpus` | `WorkloadFailed` | `fetch.rs:778-781` |

Open-loop fails twice on purpose: at plan resolution, because the descriptor
advertises only `LoadMode::ClosedLoop` (`mod.rs:93-95`), and again here as
defense in depth before any request is issued (`fetch.rs:713-717`).

There is no offered-rate clock anywhere in the module. `RunPlan::Deadline`
lowers `remaining` to `u64::MAX` and lets workers exit on the deadline
(`fetch.rs:815-821`, `866-870`); `RunPlan::Count` issues exactly `total`
requests because the counter is claimed with `fetch_update` and the loop breaks
once it cannot decrement (`fetch.rs:876-889`, which also carries the MSRV note
about the `fetch_update`/`try_update` rename). Concurrency is the worker count,
`concurrency.max(1)` (`fetch.rs:836`), so at most `concurrency` requests are in
flight; the observed maximum is tracked with `fetch_max` (`fetch.rs:908-909`,
`182-185`) and reported.

Per request: HTTP GET, no redirect following, no Eggbench retries, a 30 s
per-request timeout on every phase (`fetch.rs:223-232`, `919-920`), an 8 MiB
decoded-body cap, and full-body consumption (`fetch.rs:958-982`). Cancellation
is raced with `tokio::select!` at both the send and the body-read stage
(`fetch.rs:931-941`, `958-966`) so teardown never waits on a hung request, and
all worker tasks are joined before return (`fetch.rs:840-844`). Latency spans
dispatch to full-body consumption, computed at `fetch.rs:983` after the body is
read; a non-2xx response still yields timing and byte evidence and is *also*
classified as an error category (`fetch.rs:986-991`).

### The measured interval, and per-trial targeting

`context.measurement.finish(elapsed)` runs at `fetch.rs:581` and `fetch.rs:696`,
before the output builders, so the interval covers request execution only —
metric mapping and evidence serialization are outside it, matching the runner's
contract (`orchestration.rs:246-248`). One asymmetry: the closed-loop `started`
stamp precedes worker spawn (`fetch.rs:571`) while the corpus stamp follows
corpus load and permutation (`fetch.rs:681`).

There is no per-arm code in this module. `execute_inner` reads the target from
the `Workload` it was handed — `binding_url(&context, target)` at
`fetch.rs:565`, target taken from the plan variant at `fetch.rs:729`,
`fetch.rs:750`, or `fetch.rs:762` — and looks up that service's `http_url`
binding (`fetch.rs:793-800`). Arm switching therefore arrives as data: the
runner computes an effective workload per invocation
(`orchestration.rs:3369` `effective_workload`) and puts it in
`InvocationContext.workload` (`orchestration.rs:1847`); for a paired run that is
`Workload::with_target(arm service)` (`plan.rs:1367`), which preserves
concurrency, request count, and duration byte-for-byte. Warmups alternate arms
round-robin and carry no pair id (`orchestration.rs:1018-1024`). Missing binding
⇒ `WorkloadFailed` with no request issued (`fetch.rs:799`). See
[runner-orchestration.md](runner-orchestration.md).

### Raw observations, not normalized metrics

The adapter's only outputs are the four `WorkloadOutput` fields
(`fetch.rs:1197-1214`, `fetch.rs:425-444`). It never writes a `metrics.json`
or any normalized trial metric: `metrics` holds `RawMetricObservation` values
that the runner normalizes after the measured interval
(`orchestration.rs:250-262`). `raw()` leaves `producer`/`producer_version` as
`None` (`fetch.rs:1231-1232`); driver identity comes from the descriptor, not
from the metric.

Closed-loop path (`fetch.rs:1066-1148`). `rate(count) = count / attempted`
(`fetch.rs:1074`):

| Name | Unit | Aggregation | `source_field` |
|---|---|---|---|
| `throughput` | `rps` | `Rate` | `eggfetch.responses_received` |
| `error_rate` | `ratio` | `Ratio` | `eggfetch.error_counts` |
| `timeout_rate` | `ratio` | `Ratio` | `eggfetch.timeouts` |
| `bytes_received` | `bytes` | `Sum` | `eggfetch.bytes_received` |
| `latency_min`, `latency_mean` | `ms` | `Minimum`, `Mean` | `eggfetch.latency_histogram` |
| `latency_p50/p90/p95/p99/p999` | `ms` | `Percentile{basis_points}` | `eggfetch.latency_histogram` |

All latency metrics carry `raw_artifacts = ["latency.hdr"]`
(`fetch.rs:1115`). `throughput` counts every response that reached full-body
consumption, including 3xx/4xx/5xx — it is responses received, not successful
responses. `error_rate` and `timeout_rate` are suppressed entirely when nothing
was attempted (`fetch.rs:1162`).

HTTP-corpus path (`fetch.rs:333-377`) reports a deliberately different set
because it must not conflate an expected block with a transport failure:

| Name | Unit | Aggregation | Denominator |
|---|---|---|---|
| `throughput` | `rps` | `Rate` | elapsed |
| `transport_error_rate` | `ratio` | `Ratio` | `planned_count.max(1)` (`fetch.rs:332`) |
| `expected_outcome_mismatch_rate` | `ratio` | `Ratio` | `planned_count.max(1)` |
| `expected_outcome_mismatches` | `count` | `Sum` | — |
| `bytes_received` | `bytes` | `Sum` | — |

In this path an HTTP status is never an error category. A 403 the owner expected
is `transport_error: None, expected_match: Some(true)`; an unexpected one is
`Some(false)` and increments `mismatches` (`fetch.rs:274-281`, `319-321`) — the
distinction the in-file test
`pooled_post_preserves_body_and_expected_403_is_not_transport_error`
(`fetch.rs:1425-1451`) exists to pin. `count_f64` converts counts
losslessly-as-practical, saturating at `u32::MAX` rather than losing precision
silently (`fetch.rs:1061-1063`).

### The HTTP-corpus workload path

`Workload::HttpCorpus` (schema v10) reuses the same executor for a finite
security-load campaign (`plan.rs:422-438`). `execute_http_corpus`
(`fetch.rs:610-710`):

| Step | Line |
|---|---|
| Confine the target binding (`confine_target_url`), reduce it to a bare origin, reject `@` in the authority | `fetch.rs:626-629`, `114-125` |
| Load the digest-pinned corpus and materialize case bodies; `File` bodies are read from the corpus body root | `fetch.rs:630-643` |
| Expand the exact finite multiset (`count` repeats per case id) and reject a `default_headers` entry that contradicts a case header | `fetch.rs:644-667` |
| Require `context.schedule_seed` (absent ⇒ fail) — the runner's *schedule* seed, stable across paired arms for the same pair ordinal (`orchestration.rs:116-118`, `3323-3326`) | `fetch.rs:668-670` |
| Permute with a seeded xorshift, recording both the planned-schedule digest and the realized issue-order digest | `fetch.rs:132-140`, `672-680` |
| Run `concurrency` workers over the shared schedule, per-request timeouts derived from the remaining budget, 8 MiB body cap | `fetch.rs:143-199`, `219-233` |
| Emit `build_http_corpus_output` with `workload: "http_corpus"` and `method_evidence_schema: 2` | `fetch.rs:285-445`, `381-382` |

Default headers are applied only when the case does not already define that
name, case-insensitively (`fetch.rs:234-246`).

### The controlled origin

`start_origin` (`origin.rs:174-265`) is the whole lifecycle:

| Step | Line | Behavior |
|---|---|---|
| Parse and bound config | `origin.rs:117-172` | `path` must start with `/`, be ≤256 bytes, visible ASCII, no query/fragment/whitespace; `body_bytes` ∈ `0..=1 MiB`; `status` ∈ `200..=599` |
| Build the service fn | `origin.rs:186-210` | Exact `head().target().path()` match returns the configured status + fixed body; every other target returns 501 with an empty body |
| Bind | `origin.rs:212-214` | `SocketAddr::new(Ipv4Addr::LOCALHOST, 0)` — fixed in code, never read from plan config |
| Determinism | `origin.rs:215-224` | `DatePolicy::Suppress` so responses carry no timestamp |
| Start | `origin.rs:229-236` | Raced against the cancellation token; a pre-bind cancel is an error |
| Readiness | `origin.rs:237-244` | Adapter-owned: the returned handle's `local_addr()` is already listening, so handle existence *is* readiness; the accept loop drains the backlog independently |
| Post-checks | `origin.rs:240-249` | A non-loopback bind is shut down and fails; a cancel observed during startup fails |
| Publish bindings | `origin.rs:250-258` | `http_url` = `http://{addr}{route_path}`, `bound_addr`, `bound_port` |

The defaults are `path` `/bench`, `body_bytes` 1024, `status` 200
(`origin.rs:42-46`), matching
[../docs/eggstack-http.md](../docs/eggstack-http.md#controlled-origin).
Readiness is adapter-owned because the plan configures no probe for an
in-process service: both worked examples carry `"readiness": null`
(`eggstack-path.json:12`, `eggstack-diagnostics.json:12`).

Bindings are plain connection facts and pass `RuntimeBindings::insert`'s own
bounds (≤4096 bytes, no control characters, ≤64 keys per service,
`service.rs:55-77`); they are immutable after startup and re-recorded in
`lifecycle/runtime-topology.json` after teardown. The adapter registers exactly
one service type, so `eggstack_service_adapters()` (`mod.rs:162-168`) starts
from an empty registry and cannot duplicate. `shutdown` takes the handle, calls
`handle.shutdown()`, then waits up to the grace allowance, naming
`EGGSERVE_SERVER_VERSION` in the error (`origin.rs:93-107`).

**Why the ephemeral port is safe for comparison.** It is selected at runtime, so
it never appears in the resolved plan and therefore never enters
`compare_topology`, which compares resolved topologies
(`comparison.rs:3463-3466`) plus a subject-kind summary. Where a runtime fact
*is* present on both sides, the environment fingerprint class decides:
`comparison_critical` sets `critical_mismatch`, `warning_only` becomes a
`WarningMismatch` plus an `environment_warning_mismatch` warning, and
`informational` is skipped with no report row at all
(`comparison.rs:3033-3056`, `2338-2348`). None of that can hold for a per-run
port. The same split governs path evidence, where dial counts, hop histograms,
connection ordinals, and "transient socket/port identity (never staged)" are
excluded from identity
([core-comparison.md](core-comparison.md#5-why-network-path-configuration-is-critical-but-runtime-facts-are-not)).
`mod.rs:19-20` states the intent: "Runtime-selected ephemeral ports are recorded
as evidence."

### The fixed-corpus correctness executor

`HttpCorpusExecutor` implements `CorrectnessExecutor`
(`http_corpus.rs:24-35`) and runs *serially* — a `for` loop over `corpus.cases`
(`http_corpus.rs:70-164`), outside every measured interval. It creates its own
client per check (`http_corpus.rs:65`), correctly sharing no pool with the
workload. Per case: the case JSON is digested (`http_corpus.rs:74-76`), the
timeout is `min(case_timeout_ms, remaining)` (`http_corpus.rs:89`), the method
is parsed (`http_corpus.rs:90-91`), a `File` body is re-verified against its
content-tree identity before use (`http_corpus.rs:111-120`), the body is
consumed but never inspected (`http_corpus.rs:147-150`), and the disposition is
`Pass`/`Fail` purely on `case.expectation.matches(status)`
(`http_corpus.rs:151-155`). The contract is validated and counted before the
output is built (`http_corpus.rs:177-180`).

Correctness findings and latency evidence are different products:

| | Correctness check | Latency measurement |
|---|---|---|
| Output | `HttpCorpusCheckResultV1` as `sanitized_result` | `latency.hdr` + `RawHistogramInput` |
| Recorded facts | case id, per-case digest, expectation, observed status, disposition, reason | histogram samples, error categories, percentiles |
| Timing | none; `case_timeout_ms` only bounds a case | dispatch-to-full-body microseconds |
| Run position | outside measured intervals | inside the measured interval |
| Producer | `"eggfetch"` + `"<semantic_version>:<eggfetch_core_version>"` (`http_corpus.rs:181-197`) | none stamped on raw observations |

### Descriptor construction

| Function | Line | Category | Notable fields |
|---|---|---|---|
| `eggfetch_http_descriptor` | `mod.rs:88-119` | `Workload` | Capabilities: H1, `LoadMode::ClosedLoop`, `HttpCorpus`, plus `NetworkPath` under `eggstack-path` (`mod.rs:90-98`); `default: true` |
| `eggserve_origin_descriptor` | `mod.rs:59-77` | `Service` | Capability: H1 only; `external_process: false`; `default: true` |
| `http_corpus_descriptor` | `mod.rs:134-152` | `Correctness` | `SecurityCheck{http_observable}`; `machine_output_schema: Some(SchemaVersion(1))`; `adapter_version` is the *semantic* version, not `EGGSTACK_ADAPTER_VERSION`; `default: false` |
| `eggstack_descriptors` | `mod.rs:123-129` | — | Returns the three above in a fixed order |
| `eggstack_service_adapters` | `mod.rs:162-168` | — | Fresh `ServiceAdapterRegistry` + the origin adapter |

`eggfetch-http` declares itself compatible with `eggserve-origin` and with
`command` (`mod.rs:99-105`): a managed command subject that publishes a static
`http_url` binding is drivable, and a missing binding still fails closed at
execution time. `supported_platforms` is empty everywhere (`mod.rs:71`,
`mod.rs:112`, `mod.rs:146`). Registration of these descriptors into the
production catalog is not in this module — see
[drivers-catalog-features.md](drivers-catalog-features.md) and
[drivers.md](drivers.md#production-catalog-ownership-and-feature-isolation).

### Per-method evidence and bounded diagnostics

Each measured invocation stages at most two artifacts (`fetch.rs:1197-1209`):
`latency.hdr` (`application/x-hdrhistogram-v2`) and `eggfetch-method.json`
(`application/json`). The method object (`fetch.rs:1250-1302`) is counts and
scalars only: driver name, `eggfetch_core` version, method/version, the
client-reuse policy string, schedule mode, requested concurrency,
attempted/completed counts, error-category and status-code maps, elapsed ms,
observed max in-flight, per-request timeout, and the histogram
bounds/saturation/omission policy.

Byte bounds, and why this evidence is diagnostic rather than a payload channel:

| Bound | Line |
|---|---|
| 8 MiB decoded response body per request (defined twice) | `fetch.rs:68`, `fetch.rs:233`; `http_corpus.rs:18`, `http_corpus.rs:103` |
| 1 MiB origin body; 256-byte origin path | `origin.rs:48`, `origin.rs:159-161` |
| Artifact staging bounds are the runner's, not the driver's; `WorkloadArtifact.name` must be one safe path component | `orchestration.rs:251`, `145-146` |
| `"request_payloads_retained": false`, plus a test asserting a POST body never reaches the staged evidence | `fetch.rs:400`, `fetch.rs:1501` |

Under `eggstack-path`, two more bounded objects are added: a `network_path` key
inside `eggfetch-method.json` (dial/failure/hop/wrapping/ordinal facts plus
`faults_active`), from `path/dialer.rs:264` `invocation_delta` against a
`begin_invocation` snapshot taken at `fetch.rs:566-570` and inserted at
`fetch.rs:582-598`, `1295-1300`; and a run-level `network-path.json` (schema v1,
`Sensitivity::Redacted`) from `path/dialer.rs:276` via
`RunEvidenceArtifact::from_contract` at `fetch.rs:539-553`. The 128 KiB cap
`NETWORK_PATH_EVIDENCE_MAX_BYTES` (`path/evidence.rs:23`) is enforced on the
run-level artifact at load (`path/evidence.rs:461-463`); the per-invocation
`network_path` object is not separately byte-bounded in `fetch.rs` — its size is
bounded by the dialer's diagnostic shape, not by an explicit check here. Neither
object creates a normalized metric.

## Invariants

1. **One client per run.** `EggfetchWorkload.client` is created once
   (`fetch.rs:450-456`, `461-468`), never rebuilt per request or trial, and
   `drain` does not close the pool (`fetch.rs:530-537`). Only the explicit,
   plan-pinned `FreshPerRequest` policy creates clients per request
   (`fetch.rs:215-218`, recorded at `fetch.rs:388`).
2. **Closed loop only.** Offered-rate load is rejected at resolution (missing
   `LoadMode::OpenLoop` capability, `mod.rs:93-95`) and again before any request
   (`fetch.rs:763-765`, `778-780`).
3. **Eggbench owns measurement, not protocol semantics.** Retries, redirects,
   and keep-alive are Eggfetch's; Eggbench only times dispatch→full body
   (`fetch.rs:983`, and `fetch.rs:277` for a corpus case) and maps outcomes to
   categories (`fetch.rs:986-991`).
4. **The origin never binds a public address**, is byte-deterministic, and is
   adapter-ready: `127.0.0.1:0` in code with a post-hoc non-loopback refusal
   (`origin.rs:214`, `240-244`); fixed path/status/`0x42` body with a suppressed
   `Date` header and 501 for every other target (`origin.rs:190-208`,
   `215-224`); handle existence plus a listening `local_addr()` is readiness
   (`origin.rs:237-239`).
5. **Adapter failure cannot bypass cleanup.** Every error path out of
   `start_origin` shuts the server down and waits before returning
   (`origin.rs:240-249`).
6. **No payload in evidence.** Bodies are counted, never copied; the corpus
   object records `request_payloads_retained: false` (`fetch.rs:400`) and
   statuses only (`http_corpus.rs:156-163`).
7. **No raw Eggfetch error string becomes an identity** — failures collapse to
   the seven stable categories (`fetch.rs:72-78`).
8. **No normalized metric is written by the driver.** Only
   `RawMetricObservation`, `RawHistogramInput`, `error_counts`, and bounded
   artifacts (`fetch.rs:425-444`, `1197-1214`).
9. **Targets are confined.** Both HTTP entry points run `confine_target_url`
   before use (`fetch.rs:627-628`, `http_corpus.rs:47-48`).
10. **Corpus correctness is status-only.** A completed body is consumed and
    discarded; only `expectation.matches(status)` decides
    (`http_corpus.rs:146-155`).

## Failure model

`FailureCategory` is the runner's redaction-safe enum
(`orchestration.rs:264-293`); this module returns `WorkloadFailed` and, for the
correctness executor, `CorrectnessFailed` and `Cancelled`.

| Condition | Category | Line |
|---|---|---|
| Missing/unusable `http_url` binding | `WorkloadFailed`, no request issued | `fetch.rs:799` |
| Unlowerable workload (open-loop, replay, both `requests` and `duration_ms`, corpus in the non-corpus path) | `WorkloadFailed` | `fetch.rs:742`, `763-765`, `778-781` |
| Absent `schedule_seed` for a corpus invocation | `WorkloadFailed` | `fetch.rs:668-670` |
| Corpus digest/body/confined-URL/header-conflict failure | `WorkloadFailed` | `fetch.rs:627-643`, `652-665` |
| Origin config out of bounds | `Err(String)` from `start` → `StartupFailed` | `origin.rs:124-145`, `155-172` |
| Origin non-loopback bind, or cancel before readiness | `Err(String)` after shutdown | `origin.rs:240-249` |
| Origin does not stop within grace | `Err(String)` naming `EGGSERVE_SERVER_VERSION` | `origin.rs:101-105` |
| Correctness: no corpus request, bad binding, corpus load failure, bad method, unverified `File` body, contract validation failure | `CorrectnessFailed` | `http_corpus.rs:39-64`, `90-95`, `111-120`, `177-179` |
| Correctness: cancellation | `Cancelled` | `http_corpus.rs:72`, `128`, `148` |

Per-request failures are *not* invocation failures: they become
`RequestOutcome.error` values mapped to stable categories — build/transport →
`transport` (`fetch.rs:921-927`, `944-955`), timeouts via
`is_timeout_failure`/`is_timeout_error` (`fetch.rs:1000-1013`) → `timeout`,
body-read failure → `body_read` (`fetch.rs:969-979`), cancellation →
`cancelled`, status class → `http_3xx`/`http_4xx`/`http_5xx`
(`fetch.rs:986-991`). Two consequences change how a result reads: an invocation
cancelled mid-flight can return `attempted > 0` where every outcome is
`cancelled` with no latency sample (`fetch.rs:900-907`), so `latency_*` metrics
are absent rather than zero; and in the correctness executor a transport failure
or timeout marks the *case* `Invalid` and execution continues
(`http_corpus.rs:79-88`, `131-145`) while the disposition is decided solely by
`failed == 0` (`http_corpus.rs:188-192`) — so a run where every case timed out
reports `Pass` with a full `invalid` column, and callers must read validity and
pass/fail as separate dimensions.

## Boundaries

**Owns:** closed-loop scheduling and worker lifetime; request dispatch and
full-body consumption; latency and error-category extraction; the HTTP-corpus
permutation and its two digests; origin route discipline, loopback bind,
bindings, and shutdown; serial corpus correctness; descriptor construction; the
per-invocation method/histogram artifacts.

**Does not own:** the network path itself (route lowering, fault wrapping,
`network-path.json` — [drivers-network-path.md](drivers-network-path.md));
catalog registration and feature forwarding
([drivers-catalog-features.md](drivers-catalog-features.md)); normalization,
trial identity, and comparison ([core-comparison.md](core-comparison.md));
metric semantics for the `http_corpus` workload and the `eggsec-load` owner path
([../docs/experiment-plan.md](../docs/experiment-plan.md#schema-v10-http_corpus));
semantic replay, which the executor explicitly rejects (`fetch.rs:778-780`);
process supervision, which `ResetPolicy::Service` does not imply
([drivers.md](drivers.md#listener-free-path-ownership)).

## Tests and qualification

| Test | Line | Pins |
|---|---|---|
| `pooled_post_preserves_body_and_expected_403_is_not_transport_error` | `fetch.rs:1425-1451` | Pooled reuse = 1 physical connection for 2 requests; a 403 is not a transport error; `max_in_flight == 1`; method line and body survive keep-alive |
| `fresh_per_request_uses_distinct_connections_and_mismatch_is_separate` | `fetch.rs:1453-1503` | `FreshPerRequest` = 2 connections; `transport_error_rate == 0.0` while `expected_outcome_mismatch_rate == 1.0`; staged method evidence contains no request payload |
| `deterministic_shuffle_is_stable_and_trial_seed_sensitive` | `fetch.rs:1505-1519` | Same seed ⇒ same permutation; a different seed changes it |

The test origin is a hand-rolled loopback H1 server inside
`http_corpus_load_tests` (`fetch.rs:1311-1378`), so the tests need no
`eggserve-server` instance. Integration coverage lives outside this module:
`tests/eggstack.rs` constructs `InvocationContext` directly for the executor
(`eggstack.rs:394-402`) and `tests/eggstack_path.rs` does the same with a dialer
and a seed (`eggstack_path.rs:328-336`) — those pin the context shape this
adapter depends on. There is no in-file test for the closed-loop path, none for
`EggServeOriginAdapter`, and none for descriptor construction. Scripted
qualification is in `scripts/qualification/`; the checked-in SynVoid profile
records why a managed command subject cannot consume this adapter's ephemeral
port and emulates the origin contract instead
([qualification/synvoid/v1/README.md](../qualification/synvoid/v1/README.md)).

## Review focus

1. **`FreshPerRequest` bypasses the path dialer** (`fetch.rs:215-218`). The
   fresh client is `eggfetch_core::Client::new()` with no dialer, so under
   `eggstack-path` a corpus invocation with `fresh_per_request` would not reach
   the target through the resolved Eggress route and would carry no Eggchaos
   wrapper — while `method_evidence` still records a `connection_policy`
   (`fetch.rs:387-388`). Whether plan validation rejects
   `http_corpus` + `network_path` + `fresh_per_request` was not confirmed here.
2. **Empty histograms are still staged** (`fetch.rs:1169-1178`, `1198-1203`).
   The closed-loop builder emits `latency.hdr` and a `RawHistogramInput` even
   with no sample recorded; the corpus builder omits both when empty
   (`fetch.rs:408-414`, `428-438`).
3. **Correctness `Pass` ignores invalid cases** (`http_corpus.rs:180`,
   `188-192`): `failed == 0` is the only gate on disposition, so a fully
   timed-out corpus passes.
4. **`executable_sha256` is a digest of a string** (`http_corpus.rs:181-186`) —
   SHA-256 over `"<HTTP_CORPUS_SEMANTIC_VERSION>:<EGGFETCH_CORE_VERSION>"`, not
   over any executable.
5. **`successful_bypasses` is set from HTTP status mismatches**
   (`http_corpus.rs:200`) — correct for `http_observable` expectations, but the
   name belongs to a security vocabulary and invites misreading.
6. **Dead digest** (`fetch.rs:415-424`): a case-id digest is computed into
   `_case_ids_digest` and discarded, never reaching evidence.
7. **Duplicated bound** (`fetch.rs:68` and `http_corpus.rs:18`): the 8 MiB body
   cap is defined twice and must not drift.
8. **The `fetch_update` deprecation shim** (`fetch.rs:876-886`) is MSRV-bound to
   Rust 1.89; a compiler bump should drop the `#[allow(deprecated)]` and rename
   together.
9. **Permutation portability** (`fetch.rs:137`): `usize::try_from(state)`
   truncates on 32-bit targets, making `realized_schedule_sha256`
   platform-dependent there.
10. **`mod.rs:121`** documents `eggstack_descriptors` as returning "stable name
    order"; the actual order is `eggfetch-http`, `eggserve-origin`,
    `eggbench-http-corpus` — role order, not name order.
11. **The `network_path` object is not byte-bounded in this module**
    (`fetch.rs:1295-1300`); the 128 KiB cap applies to the run-level artifact
    only (`path/evidence.rs:23`, `461-463`).
12. **`EGGSERVE_PRIMITIVES_VERSION`** (`mod.rs:52`) is re-exported from
    `lib.rs:38` but, unlike the other two sibling versions, no descriptor or
    evidence object in this module references it.

## Related

- [overview.md](overview.md) — module map and artifact chain
- [drivers.md](drivers.md) — catalog ownership, feature isolation, and the
  listener-free path ownership this workload plugs into
- [drivers-network-path.md](drivers-network-path.md) — the dialer, route
  lowering, fault wrapping, path evidence
- [drivers-catalog-features.md](drivers-catalog-features.md) — how these
  descriptors reach the production catalog under feature gates
- [runner-orchestration.md](runner-orchestration.md) — M003 paired schedule, arm
  assignment, per-trial workload override
- [runner-adapter-seams.md](runner-adapter-seams.md) — `WorkloadExecutor`,
  `ManagedServiceAdapter`, `CorrectnessExecutor` as seams
- [core-comparison.md](core-comparison.md) — comparability dimensions and why
  runtime facts are not comparison-critical
- [../docs/eggstack-http.md](../docs/eggstack-http.md) — canonical user docs;
  [../docs/driver-capabilities.md](../docs/driver-capabilities.md) — capability
  matrix incl. the client-per-run statement
- [../docs/experiment-plan.md](../docs/experiment-plan.md#schema-v10-http_corpus)
  — the `http_corpus` workload schema;
  [../docs/security-qualification.md](../docs/security-qualification.md) — corpus
  checks and the mismatch metric
- [../examples/eggstack-path.json](../examples/eggstack-path.json),
  [../examples/eggstack-diagnostics.json](../examples/eggstack-diagnostics.json)
  — worked plans these adapters serve
