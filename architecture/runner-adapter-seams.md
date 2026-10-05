# Runner adapter seams

## Purpose

`eggbench-runner` performs local side effects. Four of those effects are not
process side effects and are therefore not reachable through argv-direct
spawning. They are object-safe trait contracts the runner resolves by label at
run time, so that no concrete sibling crate is a compile-time dependency of the
runner or of `eggbench-core`:

| Seam | Module | Extension point | Concrete implementation lives in |
|---|---|---|---|
| Managed service | [`service.rs`](../crates/eggbench-runner/src/service.rs) | `ManagedServiceAdapter` | `eggbench-drivers` (Eggstack origin etc.) |
| Trial telemetry | [`telemetry.rs`](../crates/eggbench-runner/src/telemetry.rs) | `TelemetryCollector` | `eggbench-drivers` behind the `gregg` feature |
| Security correctness | [`correctness.rs`](../crates/eggbench-runner/src/correctness.rs) | `CorrectnessExecutor` | `eggbench-drivers` (Eggsec) |
| Lifecycle diagnostics | [`diagnostics.rs`](../crates/eggbench-runner/src/diagnostics.rs) | `DiagnosticExecutor` | `eggbench-drivers` (Eggprobe) |

The shared design constraint is that none of the four may perturb a measurement.
The runner enforces that structurally: adapters start before any traffic, every
telemetry and diagnostic/correctness exchange is bounded and sequenced outside
the measured interval, and only `eggbench_core::metrics` writes normalized
`TrialMetrics`. The seam modules hold the contracts, the registries, the label
functions, and the test doubles; the orchestrator
([`orchestration.rs`](../crates/eggbench-runner/src/orchestration.rs)) and the
session ([`session.rs`](../crates/eggbench-runner/src/session.rs)) hold the
control flow that calls them.

The re-export list in [`lib.rs`](../crates/eggbench-runner/src/lib.rs) is the
authoritative public surface: `lib.rs:50` (correctness), `lib.rs:55`
(diagnostics), `lib.rs:77` (service), `lib.rs:82` (telemetry), plus the
`test_support` block at `lib.rs:89`.

## Source layout

| File | Lines | Contents |
|---|---|---|
| [`service.rs`](../crates/eggbench-runner/src/service.rs) | 290 | `BoxFuture`, `RuntimeBindings`, `ServiceStartRequest`, `ManagedServiceAdapter`, `ManagedServiceHandle`, `ServiceAdapterRegistry`, and the runtime-topology evidence types |
| [`telemetry.rs`](../crates/eggbench-runner/src/telemetry.rs) | 478 | `TelemetryFuture`, preflight/trial contexts, `TelemetryCapability`, `TelemetryError`, `TelemetryOutput`, `TelemetryPreflightTiming`, `TelemetryCollector`, `TelemetryRegistry`, `FakeTelemetryCollector`/`FakeTelemetryHandle` |
| [`correctness.rs`](../crates/eggbench-runner/src/correctness.rs) | 311 | `CorrectnessContext`/`Output`/`Disposition`, `CorrectnessExecutor`, `CorrectnessRegistry`, `CorrectnessExecutionRecord`, three label functions, `FakeCorrectnessExecutor` |
| [`diagnostics.rs`](../crates/eggbench-runner/src/diagnostics.rs) | 348 | `DiagnosticContext`/`Output`/`Disposition`, `DiagnosticExecutor`, `DiagnosticRegistry`, `DiagnosticExecutionRecord`, `DiagnosticsIndex`, `diagnostics_role_label`, `FakeDiagnosticExecutor` |

All four modules are private (`lib.rs:32`, `lib.rs:33`, `lib.rs:41`,
`lib.rs:45`); every type listed above is reachable only through the
`pub use` blocks. Modules with no runtime wiring of their own (`service.rs`) are
pure contract plus evidence types.

## Key types

### Traits

| Trait | Receiver | Object safety mechanism | Shared? |
|---|---|---|---|
| `ManagedServiceAdapter` (`service.rs:150`) | `&self`, `Send + Sync` | `start` returns `BoxFuture<'_, …>` (`service.rs:23`, `service.rs:155`); no type parameters, no `Self` in the return — the started handle is `Box<dyn ManagedServiceHandle>` | `Arc<dyn …>` in the registry (`service.rs:174`) |
| `ManagedServiceHandle` (`service.rs:163`) | `&self` / `&mut self`, `Send` only | `bindings` returns `RuntimeBindings` **by value**, `shutdown` returns `BoxFuture<'_, …>` | Owned by the session, moved into teardown |
| `TelemetryCollector` (`telemetry.rs:123`) | `&mut self`, `Send` | Four methods returning `TelemetryFuture<'_, …>` (`telemetry.rs:25`); one defaulted method `preflight_timing` (`telemetry.rs:130`) | `Box<dyn …>`, single owner (`telemetry.rs:161`) |
| `CorrectnessExecutor` (`correctness.rs:98`) | `&mut self`, `Send` | `execute<'a>(&'a mut self, …) -> Pin<Box<dyn Future<…> + Send + 'a>>` (`correctness.rs:103`); the lifetime is bound to the borrow, so the method stays monomorphic | `Arc<Mutex<Box<dyn …>>>` (`correctness.rs:118`) |
| `DiagnosticExecutor` (`diagnostics.rs:195`) | `&mut self`, `Send` | Same shape as correctness (`diagnostics.rs:200`) | `Arc<Mutex<Box<dyn …>>>` (`diagnostics.rs:215`) |

The asymmetry is deliberate: an adapter is started once and must stay callable
while other services start, so it is shared behind `Arc` and must be `Sync`.
Collectors and executors are sequential, so they take `&mut self` and need only
`Send`.

### Registries

| Registry | Key | Duplicate handling | Look up | Empty means |
|---|---|---|---|---|
| `ServiceAdapterRegistry` (`service.rs:173`) | `service_type()` label, `BTreeMap` | Rejected (`service.rs:188`); empty label also rejected | `get(&str) -> Option<&Arc<…>>` (`service.rs:202`) | No named service type is implemented |
| `TelemetryRegistry` (`telemetry.rs:160`) | `source()` label, insertion-ordered `Vec` | Rejected (`telemetry.rs:175`) | `get_mut` (`telemetry.rs:185`) | No telemetry source is collected |
| `CorrectnessRegistry` (`correctness.rs:117`) | `source()` label, `BTreeMap` | Last registration silently replaces | `lookup` (`correctness.rs:139`) | Requesting a check fails closed at preflight |
| `DiagnosticRegistry` (`diagnostics.rs:214`) | `source()` label, `BTreeMap` | Last registration silently replaces | `lookup` (`diagnostics.rs:236`) | Requesting a diagnostic fails closed at preflight |

Only `ServiceAdapterRegistry` and `TelemetryRegistry` reject duplicates. The two
one-shot registries cannot report a registration error (their `register` returns
`()`), so a double registration is a silent overwrite — see
[Review focus](#review-focus).

### Evidence and output types

| Type | Shape | Notes |
|---|---|---|
| `RuntimeBindings` (`service.rs:32`) | `BTreeMap<service, BTreeMap<key, value>>` | Keys/values validated on insert (`service.rs:55`): bounded `Name` service and key, no `/` or whitespace in keys, ≤4096 bytes and no control characters in values, ≤64 keys per service |
| `RuntimeTopology` (`service.rs:262`) | schema version + ordered `services` + `binding_consumptions` | `deny_unknown_fields`; version `RUNTIME_TOPOLOGY_SCHEMA_VERSION = 2` (`service.rs:256`) |
| `ServiceTopologyEntry` (`service.rs:242`) | `identity`, `ownership`, optional `service_type`, `bindings` | `ServiceOwnership` is `Process`/`Adapter`/`External` (`service.rs:230`) |
| `BindingConsumption` (`service.rs:275`) | consumer, destination kind, destination, source service/key, value SHA-256, `value_in_public_topology` | Payload-free by construction: only digests cross |
| `TelemetryOutput` (`telemetry.rs:97`) | `artifacts: Vec<WorkloadArtifact>`, `metrics: Vec<RawMetricObservation>`, `warnings: Vec<MetricWarning>` | `WorkloadArtifact` is reused verbatim from the workload seam, which is what lets telemetry share the staging and normalization path |
| `CorrectnessOutput` (`correctness.rs:78`) | disposition, `sanitized_result` bytes, producer/version, executable + scope SHA-256, evaluated cases, successful bypasses | Sanitized bytes are staged verbatim as `security/<id>.json` |
| `DiagnosticOutput` (`diagnostics.rs:60`) | `raw_report` bytes, disposition, producer/version, executable SHA-256, machine schema, report status, per-probe statuses, warnings, skip reason | Report bytes are staged as `diagnostics/<phase>/<id>.json` |
| `DiagnosticsIndex` (`diagnostics.rs:124`) | schema v1, driver, adapter/executable provenance, ≤64 executions | `validate_contract` (`diagnostics.rs:150`) pins driver and machine schema |

## How it works

### Managed services

`LocalSession` walks one unified launch order over processes and adapters and
tears it down in reverse; the only difference is the `RunningManagedService`
variant (`session.rs:217`). For an adapter entry the session resolves the
adapter by service-type label from `RunnerOptions.service_adapters`
(`session.rs:56`, `session.rs:630`), and rejects the run with
`RunnerError::UnsupportedService` when no adapter is registered
(`session.rs:635`). The same rejection exists at plan-resolution time, so a
named service without an implementation never reaches startup
(`spec.rs:439`, `spec.rs:444`).

`adapter.start` is awaited under the run's cancellation token, and its `String`
error is mapped to `SpawnFailed` (`session.rs:653`). Because `start` returns
only after adapter-owned readiness, the session captures
`handle.bindings()` immediately (`session.rs:664`), merges plan-declared
`http_url`/static bindings into it with `merge_checked`, and on conflict shuts
the handle down and fails with `UnsupportedService` (`session.rs:679`). A
successful merge is then folded into the session-wide binding map, so the
snapshot is complete before the `Ready` lifecycle event is pushed
(`session.rs:605`).

Plan-level readiness on an in-process service is nearly meaningless and is
rejected rather than approximated: `Readiness::Probe` returns
`RunnerError::UnsupportedProbe` because no PID exists to probe
(`session.rs:718`), while `Readiness::Delay` is still honored
(`session.rs:709`). An adapter therefore carries no PID anywhere: the
`Stopping` event is pushed with `None` (`session.rs:755`) and the topology entry
is `ServiceOwnership::Adapter` (`session.rs:407`).

Teardown calls `handle.shutdown(grace)` with the grace captured from the plan at
start time (`session.rs:756`), so the same allowance applies to the error path
as to the normal path. The session keeps `spawn_plan`, `bindings`, and
`binding_consumptions` after `running` is drained, which is what allows
`runtime_topology()` (`session.rs:393`) to be called after teardown. Adapter
entries prefer still-live bindings and fall back to the retained startup map
(`session.rs:410`); after teardown only the retained map is present.
`stage_runtime_topology` writes `lifecycle/runtime-topology.json` with role
`runtime-topology` and `Sensitivity::Redacted` (`bundle.rs:52`, `bundle.rs:59`),
and orchestration calls it after the teardown phase and after any
lifecycle-evidence staging error, so a topology staging failure cannot rewrite
the primary cause (`orchestration.rs:1651`).

Every invocation receives the same bindings value: the context is built from
`session.runtime_bindings()` and cloned (`orchestration.rs:1030`,
`orchestration.rs:1190`, `orchestration.rs:1850`). Nothing on the seam can write
back into the session, because `bindings()` hands out a copy.

### Telemetry

`TelemetryCollector` has exactly four lifecycle points. Their placement is the
contract, and the orchestrator comments mark each one:

| Step | Where | Guarantee |
|---|---|---|
| `preflight` | Before managed startup for collectors declaring `BeforeStartup` (`orchestration.rs:838`), after readiness for `AfterReadiness` (`orchestration.rs:951`) | Backend is validated before any workload traffic; both points precede every warmup (`telemetry.rs:106`) |
| `start_trial` | Immediately before the measurement timer, per measured trial (`orchestration.rs:1135`) | Window is open before any timed work; a failure fails the trial with zero measured elapsed (`orchestration.rs:1158`) |
| `stop_trial` | After the captured elapsed (`orchestration.rs:1204`), and also after a workload failure (`orchestration.rs:1314`) | The window is always closed; partial samples with warnings are preferred over failing (`telemetry.rs:147`) |
| `drain` | After workload drain, before teardown (`orchestration.rs:1517`) | Collector-owned tasks are released while services are still alive |

`preflight_timing` (`telemetry.rs:115`, defaulted at `telemetry.rs:130`) exists
because a collector's endpoint may be provisioned either outside the run or by
the run's own services; the orchestrator skips any collector whose declared
timing does not match the current probe point (`orchestration.rs:2008`). A
required source that is unregistered or fails preflight at the
`BeforeStartup` point aborts the run before startup
(`orchestration.rs:1995`, `orchestration.rs:2029`); at the `AfterReadiness`
point a required failure becomes the run's primary failure
(`orchestration.rs:965`). Optional gaps are retained as structured
`telemetry_disabled` warnings instead (`orchestration.rs:504`).

Outputs re-enter the workload path. In `stage_trial`, telemetry artifacts are
staged under the trial base as `telemetry/<collector:02>-<artifact:02>-<name>`
(`orchestration.rs:3430`), with the source label deliberately excluded from the
path so an adversarial label cannot influence it (`orchestration.rs:3427`), a
single-component name check (`orchestration.rs:3486`), and a name-collision
rejection that also protects the workload artifact map
(`orchestration.rs:3441`). Telemetry observations are then appended to the
workload observations and normalized in a single `normalize_trial_metrics` call
(`orchestration.rs:3452`, `orchestration.rs:3466`), with telemetry warnings
appended after the normalizer's own warnings and re-validated
(`orchestration.rs:3469`). Attribution is per-observation: `RawMetricObservation`
carries an optional `producer`/`producer_version` override
([`metrics.rs:246`](../crates/eggbench-core/src/metrics.rs),
`metrics.rs:263`) honored only alongside `producer` (`metrics.rs:264`), so
attribute metrics from one collector are attributed to that collector while the
trial-level provenance remains the workload driver
(`orchestration.rs:3454`). A collector cannot write `TrialMetrics` itself; it
returns raw observations.

Error surfaces are bounded by construction: `TelemetryError::new` truncates
detail to `MAX_TELEMETRY_DETAIL_LEN = 512` characters (`telemetry.rs:76`,
`telemetry.rs:88`), and every exchange is wrapped in the `telemetry` timeout
name, defaulting to the measurement timeout (`orchestration.rs:1925`).

### Correctness

`CorrectnessExecutor::execute` is a single bounded call
(`correctness.rs:103`). `CorrectnessContext` (`correctness.rs:35`) is fully
resolved by the runner — identity, source, target, requested test family,
predeclared bypass allowance, concurrency, timeouts, the read-only bindings
snapshot, a child cancellation token (`orchestration.rs:2391`) — plus an
optional `http_corpus_request` when the executor also serves the HTTP corpus
check (`correctness.rs:59`). Executors return typed provenance alongside the
sanitized bytes; the runner, not the executor, decides staging and indexing.

The disposition split is the load-bearing semantic. `CorrectnessDisposition` has
only `Pass` and `Fail` (`correctness.rs:69`): an allowance-exceeding observation
is a valid negative result that lets the run continue into warmups and measured
trials. Anything untrustworthy — process, policy, schema, timeout — is an
operational error surfaced as a `FailureCategory` and follows the phase-failure
taxonomy with mandatory cleanup (`correctness.rs:8`). One-shot executions run
after readiness, after pre-workload diagnostics, and before warmups
(`orchestration.rs:989`, `orchestration.rs:1008`).

Three label functions carry the "this is not a benchmark metric" claim into
evidence: `correctness_timing_label()` (`correctness.rs:166`),
`security_operation_label()` returning `waf --json --bypass`
(`correctness.rs:182`), and `security_role_label()` returning the manifest role
`security` (`correctness.rs:176`). Per-check artifacts are indexed by
`CorrectnessExecutionRecord` (`correctness.rs:189`), which records disposition,
allowance versus observed bypasses, and the digest of the staged bytes.

### Diagnostics

`DiagnosticExecutor::execute` is the same shape (`diagnostics.rs:200`), but
`DiagnosticContext` also carries `phase` and `required` (`diagnostics.rs:30`,
`diagnostics.rs:35`), so one executor can serve both lifecycle slots. Two slots
exist and both are outside measurement: pre-workload after readiness and before
warmups (`orchestration.rs:970`), and post-workload after workload drain and
before teardown, while services are still alive (`orchestration.rs:1568`).

`DiagnosticDisposition` is four-valued (`diagnostics.rs:47`): `Negative` is a
valid report with a negative outcome and is explicitly not a process failure;
`Skipped` covers the case where cancellation arrived before the post slot; only
`Failed` is operational. `DiagnosticsIndex::validate_contract`
(`diagnostics.rs:150`) additionally requires the canonical driver name, a
non-empty adapter version, machine schema `0.3`, and at most 64 executions
(`diagnostics.rs:169`–`diagnostics.rs:174`). Its one relaxation: when every
record is `skipped`, empty version and digest fields are accepted because no
producer provenance exists (`diagnostics.rs:161`).

### Timing summary

| Order | Phase | Seam activity | Seam reference |
|---|---|---|---|
| 1 | plan resolution | Named services without a registered adapter are rejected | `spec.rs:444` |
| 2 | pre-start | `preflight` for `BeforeStartup` collectors | `orchestration.rs:838` |
| 3 | pre-start | Diagnostic and correctness source presence checks | `orchestration.rs:857`, `orchestration.rs:867` |
| 4 | managed startup | `adapter.start` → adapter-owned readiness; bindings captured | `session.rs:653`, `session.rs:664` |
| 5 | post-readiness | `preflight` for `AfterReadiness` collectors | `orchestration.rs:951` |
| 6 | pre-workload | Diagnostic `execute` | `orchestration.rs:970` |
| 7 | correctness | `execute` | `orchestration.rs:989` |
| 8 | warmups | none | `orchestration.rs:1008` |
| 9 | per measured trial | `start_trial` → measured invocation → captured elapsed → `stop_trial` | `orchestration.rs:1135`, `orchestration.rs:1204` |
| 10 | post-trial | workload drain, then telemetry `drain` | `orchestration.rs:1451`, `orchestration.rs:1517` |
| 11 | post-workload | Diagnostic `execute` (services still up) | `orchestration.rs:1568` |
| 12 | teardown | reverse-order `handle.shutdown(grace)` | `session.rs:756` |
| 13 | after teardown | `lifecycle/runtime-topology.json` staged from retained state | `orchestration.rs:1651`, `bundle.rs:59` |
| 14 | finalization | diagnostics index staged after all per-diagnostic reports | `orchestration.rs:1655` |

## Invariants

1. **No seam call occurs inside a measured interval.** The measured interval
   contains exactly one workload invocation; `start_trial` precedes it and
   `stop_trial` follows the captured elapsed. Diagnostics and correctness run
   only in steps 6–7 and 11.
2. **No seam writes normalized `TrialMetrics`.** Telemetry returns
   `RawMetricObservation` values that pass through `normalize_trial_metrics`
   (`orchestration.rs:3466`); diagnostics and correctness write their own
   artifact families and never touch the trial metrics path.
3. **Adapters are never process-owned.** Ownership comes from the launch-kind
   entry, not from the implementation (`session.rs:407`); adapter lifecycle
   events carry no PID.
4. **Bindings are startup-established, read-only, and uniform.** The map is
   complete before the first `Ready` event (`session.rs:605`) and every
   invocation context receives a clone of it (`orchestration.rs:1850`);
   `bindings()` returns by value, so a driver cannot mutate session state
   (`service.rs:165`).
5. **Registration is explicit.** Every seam is a caller-populated registry on
   the options/arguments; an empty registry is a valid state that causes
   fail-closed rejection when a plan asks for the missing capability, never a
   silent fallback.
6. **Evidence is written after the work that produced it.** Topology is staged
   after teardown, the diagnostics index after its per-diagnostic reports, and
   all trial artifacts after the measured interval.
7. **Every one-shot execution is bounded.** Telemetry exchanges use the
   `telemetry` timeout name; diagnostics and correctness use the request's own
   `timeout_ms` with a child cancellation token
   (`orchestration.rs:2176`, `orchestration.rs:2393`).
8. **Staged paths cannot be influenced by extension-provided labels.** Telemetry
   collector labels are excluded from artifact paths (`orchestration.rs:3427`),
   and artifact names must be single safe components (`orchestration.rs:3486`).

## Failure model

| Seam | In-band outcome (run continues) | Out-of-band outcome (run fails) | Error carrier |
|---|---|---|---|
| Service | none — a started service either serves or is torn down | Unregistered type, start failure, binding conflict, plan-level probe on an adapter | `RunnerError::UnsupportedService` / `SpawnFailed` / `UnsupportedProbe` (`session.rs:635`, `session.rs:679`, `session.rs:718`) |
| Telemetry | `Positive`-style partial samples with warnings; optional source disabled with a `telemetry_disabled` warning | Required source unregistered or failing preflight; `start_trial` failure; `stop_trial` failure after a successful trial; drain failure | `TelemetryError` (category + bounded detail); `FailureCategory::TelemetryFailed` (`orchestration.rs:283`) |
| Correctness | `CorrectnessDisposition::Fail` — bypasses over allowance; run proceeds to warmups and trials | Operational/process/policy/schema failure | `FailureCategory::CorrectnessFailed` (`correctness.rs:15`) |
| Diagnostics | `Negative` for optional requests; `Skipped` on cancellation | Operational failure; required negative after a `Completed` workload sets `Invalid` | `FailureCategory`; `DiagnosticDisposition::Failed` (`diagnostics.rs:53`) |

Precedence rules that the orchestrator encodes rather than the seams: a workload
failure stays primary over a subsequent telemetry stop failure
(`orchestration.rs:1311`); a telemetry stop failure after a successful trial is
primary with cleanup attached (`orchestration.rs:1269`); a telemetry drain
failure follows the same precedence as workload drain
(`orchestration.rs:1544`); earlier failures are never masked by post-workload
diagnostics (`orchestration.rs:2117`). Every terminal exit still routes through
the single cleanup boundary, so cleanup problems attach as secondary
diagnostics rather than replacing the primary cause.

## Boundaries

Deliberately not seams:

- **Normalization.** `eggbench_core::metrics` owns `TrialMetrics`. Collectors
  and executors return raw observations or opaque sanitized bytes only.
- **Comparison.** Bundle finalization leaves the comparison verdict absent
  ([runner.md](runner.md)); there is no comparison extension point here.
- **Route selection, fault injection, and metric requests.** These are
  capability descriptors resolved by core, not runtime traits the runner calls.
- **Subprocess launching.** Adapters are in-process; the runner's process
  ownership seam (`PlatformAdapter`) is a different contract in
  [`platform.rs`](../crates/eggbench-runner/src/platform.rs).

Sibling neutrality is the stated design goal, and it holds for the trait
surfaces — the context structs carry runner-owned or core-owned types only
(`RuntimeBindings`, `CancellationToken`, `RunId`), and both modules say so
explicitly (`correctness.rs:18`, `diagnostics.rs:9`). Two places weaken that in
practice: the diagnostics index contract pins the literal driver name and
machine schema (`diagnostics.rs:169`, `diagnostics.rs:173`), and the
correctness module ships WAF-specific vocabulary and an audited operation label
(`correctness.rs:44`, `correctness.rs:182`). Those are evidence-contract
constants, not type coupling, but they are Eggstack-shaped and would need
revision to add a second diagnostic driver or a non-WAF correctness source.

## Tests and qualification

The seam doubles are exported for exactly this purpose through
`test_support` (`lib.rs:89`): `FakeTelemetryCollector`/`FakeTelemetryHandle`,
`FakeCorrectnessExecutor`, `FakeDiagnosticExecutor`, plus `FakeWorkload` for the
adjacent workload seam.

| Property under test | Mechanism |
|---|---|
| Phase ordering and timing exclusion | `FakeTelemetryCollector::with_start_delay`/`with_stop_delay` (`telemetry.rs:249`, `telemetry.rs:256`) make the collector slow enough that a leak into the timer would be visible; `telemetry_window_stays_outside_measured_elapsed` compares the staged elapsed against the fake's sleeps |
| Shared observable call log | `FakeTelemetryHandle::wrap` (`telemetry.rs:401`) returns a handle plus a `HandleProxy` that forwards into an `Arc<Mutex<…>>`, so a test can assert recorded starts/stops after registration |
| Failure precedence | Per-trial injection (`with_start_failure`, `with_stop_failure`, `with_drain_failure`, `telemetry.rs:263`, `telemetry.rs:270`, `telemetry.rs:284`) covers "stop failure after success", "workload failure plus stop failure", and drain precedence |
| Fail-closed registration | Duplicate-source rejection (`telemetry.rs:175`), missing required collector, and required preflight failure all abort before any measurement |
| Normalization sharing | Combined workload and telemetry observations are normalized in one pass, proving the producer override path |
| Correctness dispositions | `FakeCorrectnessExecutor` separates `fail_ids` (bypass over allowance → `Fail`) from `invalid_ids` (operational error) (`correctness.rs:265`, `correctness.rs:271`) |
| Diagnostic slots and outcomes | `FakeDiagnosticExecutor` records `phases` and separates `negative_ids` from `fail_ids` (`diagnostics.rs:310`, `diagnostics.rs:314`) |

Adapter-service behavior is exercised through
[`tests/lifecycle.rs`](../crates/eggbench-runner/tests/lifecycle.rs) and the
orchestration/CLI tests, not through a dedicated adapter test file; there is no
`tests/service.rs`, `tests/correctness.rs`, or `tests/diagnostics.rs`.

## Review focus

1. **Duplicate-registration asymmetry.** `ServiceAdapterRegistry::register` and
   `TelemetryRegistry::register` reject duplicates
   (`service.rs:188`, `telemetry.rs:175`); `CorrectnessRegistry::register` and
   `DiagnosticRegistry::register` return `()` and overwrite
   (`correctness.rs:130`, `diagnostics.rs:227`). A double-registered executor
   silently replaces the first, and a typo in a source label degrades to
   "no such executor" at preflight instead of "already registered".
2. **The 512-character telemetry bound is advisory.** `TelemetryError` fields
   are public, so truncation is enforced only through `TelemetryError::new`
   (`telemetry.rs:76`). A collector using a struct literal can exceed the
   documented bound.
3. **Sibling-neutrality constants.** The `eggprobe`/`0.3` pin in
   `DiagnosticsIndex::validate_contract` and the `waf --json --bypass` label are
   cross-sibling constants living in the neutral module.
4. **`RuntimeBindings::is_empty` semantics.** It reports empty when every service
   map is empty, including when service keys exist with no values
   (`service.rs:45`).
5. **Failure-category reuse in the doubles.** `FakeDiagnosticExecutor` returns
   `FailureCategory::WorkloadFailed` for a diagnostic operational failure
   (`diagnostics.rs:312`); whether the orchestrator ever surfaces a diagnostic
   failure under a diagnostic-specific category is worth confirming.
6. **Positional telemetry artifact paths.** Two `{index:02}` slots plus the
   artifact name (`orchestration.rs:3430`) are the only collision defense; the
   widths are not overflow-checked against a count above 99.
7. **Registration key stability.** All four keys are strings compared by value,
   and only two of the four registries enforce that the key is a well-formed
   `Name`; a service-type or source label with unusual characters is otherwise
   accepted.

## Related

- [overview.md](overview.md) — system shape, cross-cutting contracts, module map
- [runner.md](runner.md) — session ownership, phase order, and the two seam
  sections this document expands
- [runner-orchestration.md](runner-orchestration.md) — the phase coordinator that
  sequences every seam call
- [runner-process-lifecycle.md](runner-process-lifecycle.md) — the process
  ownership seam the service seam sits beside
- [runner-evidence-prep.md](runner-evidence-prep.md) — pre-run evidence and
  bundle staging helpers
- [core-metrics.md](core-metrics.md) — `RawMetricObservation`, the per-observation
  producer override, and `TrialMetrics`
- [evidence.md](evidence.md) — artifact roles, paths, and manifest-last
  publication
- [../docs/eggstack-http.md](../docs/eggstack-http.md) — the concrete in-process
  service adapter
- [../docs/gregg-telemetry.md](../docs/gregg-telemetry.md) — the concrete
  telemetry collector
- [../docs/security-qualification.md](../docs/security-qualification.md) —
  correctness checks and security evidence
- [../docs/metrics.md](../docs/metrics.md) — the normalize-once rule
