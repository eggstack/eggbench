# Local Runner M002 — Warmup, Measured Trial, Cooldown, and Reset Orchestration

Status: closed

Repository baseline: `67d3522c8a36d8d5e1ac1e826e7c02aefad0c894`

Source roadmap:

- `plans/subsystems/local-runner-lifecycle-roadmap.md` — M002

Predecessor closure and corrective evidence:

- `plans/closure/local-runner-lifecycle/001-status.md`
- `plans/closure/local-runner-lifecycle/001-errata.md`
- `plans/closure/foundation-experiment-evidence-post-closure-corrective/001-status.md`
- `plans/closure/local-runner-lifecycle-post-closure-corrective/001-status.md`

Controlling ADRs:

- `plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md`
- `plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md`
- `plans/adrs/ADR-0003-trial-level-comparison-and-regression-verdicts.md`
- `plans/adrs/ADR-0005-local-first-execution-and-remote-provider-boundary.md`

Long-term requirements:

- `plans/000-long-term-specification.md#5-canonical-execution-model`
- `plans/000-long-term-specification.md#8-workload-model`
- `plans/000-long-term-specification.md#9-metrics-and-observations`
- `plans/000-long-term-specification.md#12-evidence-bundle`
- `plans/000-long-term-specification.md#20-resource-and-harness-overhead`
- `plans/001-terminology-and-domain-model.md#5-phase`
- `plans/001-terminology-and-domain-model.md#6-trial`
- `plans/001-terminology-and-domain-model.md#7-warmup`
- `plans/001-terminology-and-domain-model.md#8-cooldown`
- `plans/001-terminology-and-domain-model.md#9-reset`

Primary class: capability/infrastructure.

## 1. Objective

Add the local experiment phase orchestrator on top of the closed M001 process/session boundary.

M002 must execute a resolved experiment through:

~~~text
startup + readiness
    |
warmup invocation(s)
    |
measured trial 1
    |
reset + cooldown
    |
measured trial 2
    |
...
    |
workload drain
    |
service teardown
    |
final evidence
~~~

using one host monotonic clock, deterministic phase ordering, bounded/cancellable workload and reset hooks, per-trial evidence, and truthful execution status.

The milestone uses a deterministic fake workload and fake reset hooks. It does not add Eggfetch, oha, HTTP, statistics, comparison, environment collection, or CLI behavior.

## 2. Why M002 is ready

The prerequisites are now closed.

M001 provides:

- preflighted argv-direct managed processes;
- qualified Linux/macOS process-group ownership;
- explicit unsupported Windows managed execution;
- dependency-ordered startup/readiness;
- bounded stdout/stderr draining;
- cancellation during startup/readiness;
- reverse teardown with primary-failure preservation;
- hermetic process environment;
- filesystem-resolved cwd confinement;
- explicit executable resolution.

Foundation corrective C001 provides:

- manifest v2;
- `ExecutionStatus` separated from `ComparisonVerdict`;
- explicit manifest-v1 read compatibility;
- lifecycle-only evidence as completed execution with no comparison verdict.

The post-closure runner corrective provides hosted Linux/macOS/Windows qualification evidence.

No unresolved hard dependency remains for trial orchestration.

## 3. Required invariants

M002 must preserve all predecessor invariants and additionally establish:

1. setup, startup, readiness, warmup, reset, cooldown, drain, teardown, serialization, hashing, and report generation are outside the measured trial interval;
2. the measured interval begins immediately before invoking the workload for a measured trial and ends immediately after that workload invocation returns, errors, times out, or is cancelled;
3. warmups are never represented as measured trials;
4. the number of requests/operations inside a trial is not treated as the statistical sample count;
5. one completed measured repetition maps to one stable `TrialId`;
6. phase timing uses one host monotonic clock and persists only run-relative offsets/durations, never raw `Instant` values;
7. cancellation stops new workload promptly but does not skip required drain/teardown cleanup;
8. reset/cooldown occurs only between measured trials, never after the final measured trial;
9. reset behavior is explicit and cannot silently become a no-op;
10. workload or reset failure stops subsequent measured trials;
11. already completed measured trial evidence is retained after a later failure/cancellation;
12. M002 emits no comparison verdict;
13. a completed M002 run means orchestration completed, not that performance passed;
14. runner-owned evidence writing occurs after the measurement timer stops;
15. all phase/workload/reset operations have bounded execution semantics;
16. LocalSession remains the process owner; M002 must not create a second process-supervision implementation.

## 4. Explicit non-goals

Do not add:

- Eggfetch;
- EggServe;
- Eggress;
- Gregg;
- Eggchaos;
- EggReplay;
- Eggprobe;
- Eggsec;
- oha, h2load, iperf3, netem, or PATH-based external binary discovery;
- normalized performance metrics;
- HDR histogram processing;
- baseline selection;
- bootstrap statistics;
- comparison verdict calculation;
- environment/testbed collection beyond synthetic test fixtures;
- CLI commands;
- remote execution;
- database/history indexing;
- Windows Job Object support;
- a general workflow engine;
- automatic adaptive trial extension;
- outlier removal.

## 5. Current implementation evidence

At the baseline:

- `LocalSession::prepare` performs pre-spawn plan/platform/filesystem/secret/executable checks.
- `LocalSession::startup` starts and readies all managed processes.
- `LocalSession::shutdown` performs mandatory reverse-order cleanup.
- `LocalSession::run` currently performs a simple startup-readiness-shutdown lifecycle pass and is explicitly documented as the M001 convenience path.
- M001 lifecycle events are process-oriented (`Spawned`, `Ready`, `Stopping`, `Stopped`) and have session-relative monotonic millisecond offsets.
- `TrialPolicy` already carries:
  - measured trial count;
  - warmup count;
  - optional cooldown;
  - reset policy;
  - named phase timeouts.
- `BundleManifest` v2 already carries stable `TrialDescriptor` entries and requires each descriptor to reference one `ArtifactRole::TrialResult` artifact.
- no typed trial-result artifact schema exists yet;
- no workload-execution interface exists yet;
- no reset-hook interface exists yet;
- no trial phase scheduler exists yet;
- no measurement/comparison code exists yet.

## 6. Architecture added by M002

Introduce one higher-level orchestration layer above `LocalSession`.

Conceptually:

~~~text
ResolvedPlan
    |
LocalSession --------------------+
    |                            |
    |                    process lifecycle
    |
RunCoordinator
    |
    +--> WorkloadExecutor
    |
    +--> ResetRegistry
    |
    +--> monotonic phase clock
    |
    +--> BundleWriter
    |
    v
RunOutcome + finalized .eggb
~~~

Exact type names may differ. Preserve the ownership boundaries.

`LocalSession` must remain usable independently for lifecycle tests and future embedding.

## 7. Runner-facing workload contract

Define a narrow object-safe asynchronous workload execution seam in `eggbench-runner`.

A shape equivalent to the following is expected:

~~~text
WorkloadExecutor
  execute(invocation context) -> workload execution outcome
  drain(drain context) -> cleanup outcome
~~~

The interface must support a mutable/stateful executor because future drivers may own connection pools, child generators, counters, or raw-output handles.

The invocation context must contain only runner-level information needed to execute one repetition, such as:

- experiment/run identity if available;
- invocation kind:
  - warmup with ordinal;
  - measured trial with `TrialId`;
- resolved workload intent;
- deterministic seed or derived invocation seed where applicable;
- cancellation context;
- bounded deadline/timeout information;
- run-relative monotonic context if needed.

Do not put metric-comparison semantics into this trait.

The workload outcome may contain bounded diagnostic metadata and artifact references/bytes suitable for later staging, but M002 must not invent normalized throughput/latency metrics.

## 8. Fake workload implementation

Provide a deterministic fake workload for qualification.

It must support at least:

- immediate success;
- bounded delayed success;
- deterministic failure on a selected invocation;
- pending/blocking behavior to test timeout;
- cancellation observation;
- invocation recording;
- drain success;
- drain failure/timeout simulation if needed.

The fake may live under a test-support module or dev/test surface if a production public fake would pollute the API.

Tests must never depend on real network timing.

## 9. Reset hook contract

Do not silently interpret `ResetPolicy::Service` as “restart a process” in M002.

Introduce a narrow explicit reset hook/registry analogous in spirit to the readiness-probe registry.

Required mapping:

- `ResetPolicy::None` — no reset hook required;
- `ResetPolicy::Service { service }` — requires a registered reset hook for that service/reset target;
- `ResetPolicy::Reference { reference }` — requires a registered reset hook for that reference.

Missing required reset capability must fail during run preflight before startup.

The reset hook must be asynchronous, timeout-bounded, and redaction-safe.

A deterministic fake reset hook is sufficient for M002.

Future Eggstack/service integrations may register a real restart/reinitialize hook. M002 must not invent protocol-specific reset behavior.

## 10. Phase state machine

Add a typed phase vocabulary and phase event stream.

At minimum represent:

- startup/readiness;
- warmup invocation;
- measured trial;
- reset;
- cooldown;
- drain;
- teardown;
- finalization.

Because M001's `startup()` currently owns both spawn and readiness, M002 may represent that as one outer `StartupReadiness` phase while retaining the existing per-service `Spawned`/`Ready` lifecycle events.

Do not rewrite M001 solely to manufacture a separate readiness method.

Each phase event should contain:

- stable sequence;
- phase kind;
- warmup/trial ordinal or TrialId when relevant;
- start offset from one run monotonic origin;
- elapsed duration when complete;
- terminal phase outcome;
- redaction-safe failure category when relevant.

Do not serialize wall-clock timestamps as measurement truth.

## 11. Canonical M002 execution order

The normal sequence is:

1. orchestrator preflight;
2. create run monotonic origin;
3. start/readiness through `LocalSession`;
4. execute warmup invocation 1..N;
5. execute measured trial 1;
6. if another measured trial remains:
   - execute configured reset hook, if any;
   - execute configured cooldown, if any;
7. execute next measured trial;
8. repeat step 6/7 until measured count is reached;
9. drain workload;
10. tear down services;
11. stage remaining lifecycle/phase evidence;
12. finalize bundle with `ExecutionStatus::Completed`, no comparison verdict.

Important semantics:

- warmups do not trigger reset/cooldown by themselves;
- warmup state intentionally carries into measured trial 1;
- reset happens before cooldown so cooldown can act as post-reset stabilization time;
- no reset/cooldown runs after the final measured trial;
- evidence serialization/hashing is outside measured elapsed time.

If implementation evidence demonstrates that reset-before-cooldown creates a concrete contradiction with an existing invariant, stop and update this plan rather than silently reversing order.

## 12. Timeout contract

M002 must use `TrialPolicy.timeouts` rather than allow indefinitely blocking workload/reset phases.

Define and document a small stable set of M002 timeout keys.

Preferred initial keys:

- `measurement` — required upper bound for each measured workload invocation;
- `warmup` — optional override for warmups; falls back to `measurement`;
- `reset` — required when reset policy is not `None`;
- `drain` — required cleanup bound for workload drain.

Unknown timeout keys MUST NOT be silently ignored by M002. Either reject them in M002 preflight or explicitly preserve a documented namespace for later milestones.

`cooldown_ms` itself bounds cooldown and needs no separate timeout.

The workload's own `duration_ms` is workload semantics, not a substitute for the runner's safety timeout. The safety timeout may be larger than the intended workload duration.

Do not add an undocumented hardcoded hour/minute timeout to make tests pass.

## 13. Measurement clock and interval

Use `std::time::Instant` or Tokio's monotonic time only internally.

For a measured trial:

~~~text
pre-trial checks
  OUTSIDE

measurement_start = monotonic_now()
await workload.execute(measured trial)
measurement_end = monotonic_now()

serialize/stage result
  OUTSIDE
~~~

Persist:

- start offset from run origin;
- elapsed nanoseconds or another explicit integer duration unit;
- terminal trial-execution status.

Do not include:

- reset;
- cooldown;
- evidence writes;
- bundle hashing;
- log staging;
- service startup/readiness;
- service teardown;
- runner report rendering.

If the workload executor itself writes raw output while it is running, that is workload-driver behavior and should be documented by that driver later. M002's own artifact serialization must not be inside the timing window.

## 14. Versioned measured-trial result contract

Because `TrialDescriptor.result` already requires a `TrialResult` artifact, add a small runtime-independent versioned trial-execution result DTO to `eggbench-core`.

It must record only execution facts needed before Measurement/Comparison exists.

A representative v1 shape:

~~~text
schema_version
trial_id
measurement_start_offset_ns
measurement_elapsed_ns
terminal_status
optional redaction-safe failure category
optional workload diagnostic artifact references
~~~

Terminal status should distinguish at least:

- completed;
- failed;
- timed_out;
- cancelled.

Do not include a comparison verdict.

Do not add normalized throughput/latency/error-rate fields merely to avoid a later schema revision. Measurement/Comparison owns metric semantics and may extend or version the trial result later.

The trial-result schema version must be explicit.

## 15. Warmup evidence

Warmups must be structurally impossible to confuse with measured trials.

Requirements:

- warmups do not receive a measured `TrialId`;
- warmup records do not appear in `BundleManifest.trials`;
- if retained, warmup records live under a separate path such as `warmups/NNN/result.json`;
- retain them using a distinct artifact role/label, not `ArtifactRole::TrialResult`;
- warmup timing is diagnostic only;
- later comparison code must not need a “filter out warmup trial IDs” convention.

## 16. Per-trial evidence layout

Each measured trial must have a deterministic directory and mandatory result artifact.

Representative layout:

~~~text
trials/
  001/
    result.json
    artifacts/
  002/
    result.json
    artifacts/
warmups/
  001/
    result.json
runner-phases.json
~~~

Exact zero padding may differ but must be deterministic.

For every measured trial that begins:

- allocate one stable positive `TrialId`;
- create a result record even if the workload subsequently fails, times out, or is cancelled, when evidence staging remains possible;
- add one `TrialDescriptor` referencing its result artifact;
- preserve earlier completed trial descriptors after later failure.

Do not create a descriptor for a measured trial that was never entered.

## 17. Trial IDs and ordering

Assign measured `TrialId` values deterministically from execution order, starting at 1 unless current core constraints justify another scheme.

Warmup ordinal is a separate namespace.

Manifest trial ordering must reflect execution order and must not depend on directory lexicographic sorting.

Do not use request IDs, process IDs, or vector indices as implicit durable identity.

## 18. Cancellation semantics

Cancellation must be tested from every M002 phase.

### Before startup

Reuse M001 behavior: no process starts and no trial is fabricated.

### Startup/readiness

Reuse M001 cancellation and mandatory cleanup.

### Warmup

Stop the active workload invocation promptly, mark run `Cancelled`, retain warmup/phase evidence if possible, then perform drain and teardown.

### Measured trial

Stop the active workload invocation promptly. Record a cancelled measured-trial result if the trial had entered its measurement window. Retain earlier trials. Mark run `Cancelled`.

### Reset

Stop/reset hook promptly if it is cancellable, mark run `Cancelled`, do not begin the next trial, then drain/teardown.

### Cooldown

Interrupt the cooldown sleep promptly, mark run `Cancelled`, then drain/teardown.

### Drain

Once cleanup/drain starts, user cancellation does not skip required cleanup. Drain runs to its independent bound. The run remains `Cancelled`.

### Teardown

Cancellation never aborts teardown. Existing service cleanup remains mandatory and bounded.

This distinction must be explicit in code/tests: cancellation stops experimental work, not safety cleanup.

## 19. Failure semantics

### Preflight failure

No services start. Do not fabricate trial evidence.

If a bundle has already been staged sufficiently to finalize an invalid/failed preflight record, it may be finalized truthfully; otherwise return the typed preflight error. Do not create synthetic successful evidence.

### Startup/readiness failure

Preserve M001 initiating error plus cleanup failures. No measured trial is fabricated.

### Warmup failure or timeout

Mark run `ExecutionStatus::Failed`. No later warmup/trial executes. Drain/teardown still run.

### Measured workload failure or timeout

Record the entered trial's failed/timed-out result when possible. Stop later trials. Mark run `Failed`. Drain/teardown still run.

### Reset failure or timeout

Stop before the next measured trial. Mark run `Failed`. Retain completed earlier trials. Drain/teardown still run.

### Drain failure or timeout

Preserve the prior primary run outcome. If the experiment otherwise completed, drain failure makes execution `Failed`. Teardown still runs.

### Teardown failure

Preserve any existing workload/reset failure as primary and attach cleanup details. If experimental phases otherwise completed, teardown failure makes execution `Failed`.

No M002 failure produces `ComparisonVerdict::Fail`.

## 20. Run outcome versus API failure

Avoid an API where an ordinary measured workload regression/failure causes evidence to disappear inside `Result::Err`.

Introduce or evolve a high-level outcome that can preserve:

- execution status;
- completed/entered trial records;
- primary redaction-safe failure;
- drain failure;
- cleanup failures;
- phase timeline;
- finalized bundle reference when finalization succeeds.

Operational experiment failure is a run outcome.

Use a Rust error only for failures that prevent the runner from truthfully completing its API contract, such as unrecoverable evidence-staging/finalization failure or impossible internal state.

Exact types may differ, but callers must be able to receive a valid failed/cancelled bundle without scraping an error string.

## 21. Evidence finalization

M002 should add one high-level path that can produce a finalized synthetic run bundle.

The orchestrator may accept a caller-created `BundleWriter` already containing:

- source plan;
- resolved plan;
- synthetic/test environment fingerprint.

M002 must not invent production environment fingerprinting; M003 owns that.

On finalization:

- use the actual `ExecutionStatus`;
- comparison verdict is always `None`;
- include measured TrialDescriptors entered during execution;
- include runner phase evidence;
- include existing lifecycle logs/metadata;
- include driver inventory already available from ResolvedPlan where appropriate;
- verify the completed bundle in end-to-end tests.

If evidence finalization itself fails, do not claim a valid completed bundle. Preserve the operational run failure context where possible.

## 22. Workload drain

The workload executor must expose a bounded drain/cleanup step even though the fake implementation is simple.

Drain occurs:

- after all measured trials complete;
- after workload failure;
- after cancellation;
- after reset failure if workload state may still exist.

Drain does not mean service teardown. It is workload-adapter cleanup such as stopping generator tasks or collecting raw output.

User cancellation does not skip drain.

A drain timeout/failure is recorded distinctly from service teardown failure.

## 23. Phase preflight

Before service startup, M002 must validate all orchestration requirements that can be known:

- required timeout keys exist;
- unknown timeout keys are rejected according to the M002 namespace policy;
- measured trial count is valid;
- required reset hook exists;
- workload executor is present;
- workload executor claims it can accept the resolved workload shape if a runner-facing capability check is introduced;
- evidence output bounds are sufficient for required mandatory runner result artifacts where this can be computed conservatively.

Do not start services and only then discover a missing reset hook or missing measurement timeout.

## 24. Deterministic seed handling

If `ResolvedPlan.seed` exists, derive deterministic per-invocation seeds without depending on process hash randomization.

Warmup and measured-trial seed namespaces must not collide.

A simple documented deterministic derivation is sufficient.

If no seed exists, the fake workload must still remain deterministic unless the test explicitly supplies randomness.

Do not add a general RNG framework to core.

## 25. Phase/evidence bounds

Runner phase and trial metadata are bounded artifacts.

Do not accumulate unbounded phase history or arbitrary workload metadata in memory.

Use plan artifact bounds when staging.

Set a conservative hard cap on phase events derived from:

- warmup count;
- measured trial count;
- between-trial reset/cooldown phases;
- fixed startup/drain/teardown phases.

Reject absurd execution counts before allocating proportional structures where core validation does not already provide a sufficient bound.

## 26. Interaction with Measurement/Comparison

M002 creates execution evidence, not performance interpretation.

It must make the next subsystem possible by providing:

- stable measured TrialIds;
- measured elapsed windows;
- clear completed/failed/cancelled/timed-out trial state;
- strict warmup separation;
- deterministic execution order;
- per-trial artifact locations.

It must not:

- calculate p50/p99;
- decide higher/lower-is-better;
- compare baseline/candidate;
- bootstrap trial observations;
- emit pass/fail/inconclusive comparison verdicts.

Closing M002 supplies the local trial evidence prerequisite for Measurement/Comparison M001.

## 27. Interaction with External Oracles

The workload interface created here should be sufficient for a later external command adapter, but M002 must not implement binary discovery or parse tool output.

Do not tailor the interface specifically to oha.

External Oracles M001 will own:

- executable discovery;
- exact version probes;
- argv translation;
- raw output capture;
- external parser errors.

M002 owns only run-phase orchestration.

## 28. Cleanup of post-corrective bookkeeping

Include two small predecessor cleanup items because they directly affect runner truthfulness and are not independent milestones:

1. update the stale `crates/eggbench-runner/tests/lifecycle.rs` module comment that still refers to “inconclusive zero-trial evidence”; it must describe completed execution with no comparison;
2. fix or remove the duplicate lifecycle test assumption that every Unix target is `Supported`. The canonical platform test must continue to expect:
   - Linux/macOS: `Supported`;
   - other Unix: `Unqualified`;
   - non-Unix/Windows: `Unsupported`.

Do not otherwise reopen the closed platform corrective.

## 29. Expected production changes

Likely changes include:

- a new orchestration/phase module in `eggbench-runner`;
- a runner-facing workload executor trait;
- reset-hook registry;
- fake workload/reset test support;
- phase event/outcome types;
- a versioned trial-execution result DTO in `eggbench-core`;
- evidence staging helpers for warmups/trials/phases;
- a high-level run coordinator API;
- extensions to `RunnerError` or a separate orchestration error/outcome type;
- tests and documentation.

Do not create `eggbench-drivers` merely for a fake. That crate should appear when the first real adapter milestone requires it.

## 30. Schema and compatibility effects

Expected durable schema addition:

- trial-execution result schema v1.

Existing schemas:

- ExperimentPlan v1 remains unchanged unless a concrete implementation blocker proves otherwise;
- ResolvedPlan v1 remains unchanged;
- evidence manifest v2 remains unchanged unless adding the trial-result schema identifier to the manifest is demonstrably necessary.

Prefer recording the trial-result schema inside each result artifact rather than changing the manifest solely to duplicate that version.

Any required existing-schema change must be called out in closure and must not silently weaken manifest-v1/v2 compatibility.

## 31. Focused test matrix

At minimum:

### Happy path

- zero warmups, one measured trial;
- multiple warmups + multiple measured trials;
- exact measured TrialId order;
- warmups absent from manifest.trials;
- reset then cooldown between trials;
- no reset/cooldown after final trial;
- completed execution has no comparison verdict;
- finalized bundle verifies.

### Measurement boundary

- fake workload sleeps for known bounded interval;
- measured elapsed excludes an intentionally slow reset;
- measured elapsed excludes an intentionally slow cooldown;
- measured elapsed excludes slow evidence serialization/staging;
- startup/readiness delay does not enter trial elapsed.

Do not assert overly tight wall-clock values on hosted CI. Use wide bounds or a controllable test clock abstraction where justified.

### Failure

- warmup failure;
- warmup timeout;
- measured trial failure;
- measured trial timeout;
- reset failure;
- reset timeout;
- drain failure;
- drain timeout;
- teardown failure after successful trials;
- workload failure plus teardown failure preserves workload failure as primary;
- missing reset hook fails before spawn;
- missing required timeout fails before spawn;
- unknown timeout key fails before spawn.

### Cancellation

- before startup;
- during startup/readiness;
- during warmup;
- during measured workload;
- during reset;
- during cooldown;
- cancellation remains set during drain but drain still runs;
- cancellation remains set during teardown but teardown still runs.

### Evidence

- failed entered trial still has a TrialResult when staging is possible;
- cancelled entered trial still has a TrialResult;
- unentered future trial has no descriptor;
- earlier completed trials survive later failure;
- warmup artifacts use non-TrialResult role;
- phase artifact is bounded/versioned;
- no comparison verdict appears;
- bundle digest verification passes.

### Regression

- all M001 lifecycle tests remain green;
- cwd symlink confinement remains green;
- hermetic environment tests remain green;
- Linux/macOS/Windows platform behavior remains green;
- corrected platform truthfulness test covers other Unix as unqualified.

## 32. Cross-platform qualification

M002's orchestration logic should be portable even though managed Windows process execution is still unsupported.

CI expectations:

### Linux

Run full M002 orchestration integration tests.

### macOS

Run full M002 orchestration integration tests, including real managed-process lifecycle underneath the fake workload.

### Windows

Compile/check the orchestration code and run core/runner tests that do not require managed execution. A managed M002 run must still fail preflight through the existing platform capability boundary, not through compile errors.

Do not add Windows process emulation to make an M002 test green.

## 33. Documentation

Update at least:

- `docs/local-runner-lifecycle.md` or add `docs/trial-orchestration.md`;
- `architecture/` runner documentation if a new orchestration ownership page is clearer;
- README status summary;
- subsystem roadmap milestone status after closure;
- registry.

Documentation must include:

- exact phase order;
- exact measured interval;
- reset-before-cooldown rule;
- cancellation versus mandatory cleanup behavior;
- timeout key namespace;
- warmup versus measured-trial evidence distinction;
- workload/reset extension seams;
- explicit statement that M002 does not calculate performance metrics or verdicts.

## 34. Static and dependency guards

After implementation:

- `eggbench-core` must still have no Tokio/process/network dependency;
- `eggbench-runner` may own Tokio orchestration but no concrete HTTP/load-generator dependency;
- no Eggstack sibling crate should enter the dependency tree in M002;
- no external benchmark binary should become required;
- no shell invocation should appear.

Use `cargo tree` plus focused source/dependency inspection in closure evidence.

## 35. Broad verification

Required before closure:

    cargo fmt --all -- --check
    cargo check --workspace --all-targets --locked
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    cargo test --workspace --all-features --locked
    cargo +1.89.0 check --workspace --all-targets --locked
    cargo tree --locked
    git diff --check

Hosted qualification must remain green on:

- Linux stable;
- Linux MSRV;
- macOS stable;
- Windows stable supported subset.

Record the workflow run ID in closure.

## 36. Acceptance criteria

M002 closes only when:

1. a synthetic experiment can execute startup/readiness, warmups, N measured trials, reset/cooldown, drain, teardown, and evidence finalization;
2. the measured interval excludes all runner-owned setup/reset/cooldown/evidence/cleanup work;
3. measured TrialIds and result artifacts are deterministic and versioned;
4. warmups cannot be mistaken for measured trials;
5. cancellation is tested in every phase and cannot skip drain/teardown;
6. failure stops later trials while retaining earlier/entered evidence;
7. reset capability is explicit and preflighted;
8. workload execution is behind a narrow reusable seam exercised by a deterministic fake;
9. no real transport/load-generator/statistics dependency is introduced;
10. completed/failed/cancelled runs finalize with truthful `ExecutionStatus` and no comparison verdict;
11. the finalized synthetic `.eggb` verifies;
12. Linux/macOS hosted orchestration tests and Windows supported-subset checks are green;
13. the two minor post-corrective bookkeeping/test truthfulness items are corrected;
14. no unresolved correctness/security/lifecycle blocker remains for M003 or Measurement/Comparison M001.

Closing M002:

- makes Local Runner M003 dependency-ready for planning;
- supplies local trial evidence needed to make Measurement/Comparison M001 dependency-ready for planning;
- does not itself unblock Eggstack integrations until the measurement contract is sufficiently stable.

## 37. Stop conditions

Stop for planning review rather than improvising if:

- the workload seam needs protocol-specific methods to support the fake;
- reset semantics require direct protocol knowledge in the runner;
- a correct measurement window requires per-request instrumentation in M002;
- trial evidence cannot be represented without prematurely defining metric statistics;
- manifest v2 must change materially;
- M002 would require a concrete Eggstack or external load-generator dependency;
- cancellation-safe workload cleanup cannot be represented separately from service teardown;
- a platform regression invalidates the closed M001/corrective guarantees.

## 38. Closure evidence required

The closure record must include:

- implementation commit(s);
- requirement-to-evidence matrix;
- phase-order diagram from the landed API;
- workload/reset interface definitions;
- trial-result schema version and representative JSON;
- warmup-versus-trial artifact tree;
- measurement-boundary test evidence;
- failure/cancellation matrix;
- evidence bundle verification result;
- lifecycle regression results;
- dependency tree proving no concrete network/load-generator dependency;
- MSRV result;
- hosted Linux/macOS/Windows workflow run ID;
- documentation paths;
- known limitations;
- unresolved findings classified by severity;
- disposition: closed, conditionally closed, corrective required, or blocked.
