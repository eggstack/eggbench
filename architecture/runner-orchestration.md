# Runner orchestration: the phase coordinator

## Purpose

`crates/eggbench-runner/src/orchestration.rs` is the only place in the workspace that decides
**what runs, in what order, and what counts as measured**. It sits above `LocalSession`, which
owns processes and readiness, and below the driver adapters, which own protocol parsing. It does
not spawn anything itself, does not parse workload output, and does not interpret a metric — it
sequences invocations, bounds them, and stages their evidence.

Three decisions define the module. **One measured invocation per measured interval** — the timed
region brackets exactly one `WorkloadExecutor::execute` call and nothing the runner owns; the
timer is closed by an explicit adapter signal, post-processing gets a separate budget, and every
byte of runner-owned serialization happens in the caller after the interval returns
(`crates/eggbench-runner/src/orchestration.rs:1860`). **One cleanup boundary** — every terminal
exit after startup falls through a single straight-line tail that drains the workload, tears
down services, and stages evidence before any disposition is returned; no `?` exists in that
region, so staging failures are stored rather than propagated
(`crates/eggbench-runner/src/orchestration.rs:1445`). **Evidence is a side effect of the
schedule, not a phase of it** — the run's own phase timeline is serialized from a state that is
already terminal (`crates/eggbench-runner/src/orchestration.rs:1715`).

`execute_run` (`crates/eggbench-runner/src/orchestration.rs:749`) is a convenience wrapper
supplying empty diagnostic and correctness registries — a plan requesting either fails closed at
preflight. All real work happens in `execute_run_with_diagnostics`
(`crates/eggbench-runner/src/orchestration.rs:789`), which carries the comment "Keep the
canonical phase transition order auditable in one place".

## Source layout

| Region | Lines | Role |
|---|---|---|
| Adapter seams (`InvocationKind` … `ResetRegistry`) | 48–531 | Types drivers implement, plus telemetry window helpers |
| `PhaseKind` / `PhaseOutcome` / `PhaseEvent` / `RunOutcome` | 533–621 | Phase vocabulary and the recorded timeline |
| `OrchestrationError` | 623–669 | Three-variant failure surface |
| `RunState` | 671–727 | Mutable run state that survives the cleanup tail |
| `execute_run` → finalization | 729–1759 | The coordinator |
| Preflight helpers | 1761–2111 | Corpus inputs, `PhaseConfig`, telemetry/diagnostic/correctness preflight |
| ↳ `execute_invocation` | 1804–1884 | One bounded invocation and the measurement boundary |
| Diagnostic / correctness phase bodies | 2113–2551 | One-shot pre/post slots, security checks |
| Evidence staging helpers | 2553–3022 | Sanitized records, indexes, byte bounds |
| Run-evidence validation and capacity preflight | 3024–3251 | `RunEvidenceContract` enforcement, bundle budget reservation |
| Phase bookkeeping and seed/schedule derivation | 3253–3380 | `begin_phase`/`finish_phase`, `paired_assignment`, seeds |
| `stage_trial` / `stage_warmup` / artifact staging | 3382–3594 | Per-trial and per-warmup evidence |
| `FakeWorkload` | 3614–3735 | Deterministic qualification adapter |
| `paired_assignment_tests` | 3737–3794 | In-module unit tests |

Constants that bound the run: `MAX_PHASE_EVENTS = 10_000` (line 41),
`MAX_WORKLOAD_ARTIFACTS_PER_INVOCATION = 256` (line 42), `MAX_TELEMETRY_ARTIFACTS_PER_TRIAL = 8`
(line 393), `TRIAL_RESULT_SCHEMA_VERSION = 2` (line 44), `WARMUP_RECORD_SCHEMA_VERSION = 1`
(line 46).

## Key types

### Phase model

| Type | Line | Role |
|---|---|---|
| `PhaseKind` | 536 | Eleven phase identities, `snake_case` serialized |
| `PhaseOutcome` | 570 | `Completed` / `Failed` / `TimedOut` / `Cancelled` |
| `PhaseEvent` | 584 | One `deny_unknown_fields` record per phase |
| `RunOutcome` | 605 | Finalized bundle + status + primary failure + phases |
| `RunState` | 678 | The mutable state that lets cleanup survive a staging error |

A `PhaseEvent` records **when a phase started relative to the run's single monotonic origin, how
long it took, how it terminated, and why** — never a raw clock value, never a message string.
`sequence` is the vector index (`crates/eggbench-runner/src/orchestration.rs:3262`),
`start_offset_ns` is `origin.elapsed()` at `begin_phase`, `elapsed_ns` is the saturating
difference computed at `finish_phase` (3281), and `outcome` / `failure_category` are set
together at that same point. `warmup_ordinal` and `trial_id` are `Option`, so phase identity is
carried in the record rather than inferred from position. Events are opened and closed by two
functions only (`crates/eggbench-runner/src/orchestration.rs:3253`, 3273); an event with
`outcome: None` exists only in memory and never reaches `runner-phases.json`.

`FailureCategory` (line 267) is the redaction-safe failure vocabulary. It is also the input to
three small translators, which is where most of the failure semantics actually live:

| Function | Line | Mapping |
|---|---|---|
| `status_for` | 3288 | `Cancelled` → `Cancelled`, everything else → `Failed` |
| `outcome_for` | 3302 | `Cancelled` → `Cancelled`, `TimedOut` → `TimedOut`, else `Failed` |
| `workload_failure` | 3296 | Adapter categories collapse to `WorkloadFailed` except `Cancelled`/`TimedOut` |

`ExecutionStatus::Invalid` is **not** reachable through these translators. It is set explicitly
in three places — a required pre-workload diagnostic that returned a negative or failed
disposition (`crates/eggbench-runner/src/orchestration.rs:2213`), an operational correctness
failure (`crates/eggbench-runner/src/orchestration.rs:2435`), and an HTTP-corpus result with
invalid cases (`crates/eggbench-runner/src/orchestration.rs:2509`).

### Adapter seams

| Type | Line | Contract |
|---|---|---|
| `WorkloadExecutor` | 296 | `execute` / `drain` / `run_evidence`, object-safe, `&mut self` |
| `InvocationContext` | 107 | Per-invocation frozen input |
| `InvocationKind` | 51 | `Warmup { ordinal }` or `Measured { trial_id, arm }` |
| `MeasurementSignal` | 68 | `watch` channel the adapter closes to stop the timer |
| `WorkloadOutput` | 250 | Raw observations; never normalized evidence |
| `WorkloadArtifact` | 144 | One bounded driver file |
| `DrainContext` | 133 | Independent cleanup bound, cancellation is informational |
| `RunEvidenceContract` | 165 | Typed self-validating payload + exact schema version |
| `RunEvidenceArtifact` | 155 | Validated, ≤128 KiB run-level artifact |
| `ResetContext` | 320 | Named target + independent bound |
| `ResetHook` / `ResetRegistry` | 332 / 518 | Explicit capability registry; a missing hook is preflight failure |
| `ResetBinding` / `PhaseConfig` | 340 / 346 | Resolved timeouts and the selected hook |

Two constraints on `InvocationContext` matter to drivers. `bindings` is the startup-established
`RuntimeBindings` snapshot, cloned per invocation from `session.runtime_bindings()` and
documented read-only — workload code must not mutate topology through it. `seed` and
`schedule_seed` are separate on purpose (see the paired schedule below).

## How it works

### Preflight, before any side effect

Four preflight groups run before managed startup, and all of them fail with
`OrchestrationError::Preflight` — a static `&'static str` verdict (628), deliberately carrying
no runtime detail.

1. `preflight(resolved, resets)` (800 → 1886). Re-checks that a paired plan has an even measured
   count ≥ 2 (1892) because hand-built resolved plans bypass plan validation, and rejects any
   timeout key outside `{measurement, warmup, reset, drain, telemetry}` (1900) — an unknown name
   fails rather than being ignored. `measurement` and `drain` are required; `warmup` and
   `telemetry` default to the measurement bound; `reset` is required exactly when `ResetPolicy`
   is not `None` (1936). Zero-valued timeouts are rejected. The reset **target is resolved to a
   registered hook here** (1944), before startup — `ResetPolicy::Service` names a capability,
   not a process restart.
2. `preflight_http_corpus_inputs` (801 → 1761). Every corpus ref and digest is loaded and
   validated, and every scheduled case id must exist in the corpus.
3. `preflight_diagnostics` (857 → 2073) and `preflight_correctness` (867 → 2092). Every
   requested source must have a registered executor; no diagnostic or correctness check ever
   runs unregistered.
4. `preflight_evidence_capacity` (823 → 3106). Reserves the bundle budget the run *must* be able
   to hold: artifact count for warmup records, per-trial `result.json` + `metrics.json`,
   telemetry slots, security and corpus records, per-service log caps, and a per-artifact floor
   for `runner-phases.json`, `lifecycle.json`, and `runtime-topology.json`. A plan that cannot
   fit its own mandatory evidence is rejected before any process starts. A phase-count bound is
   computed alongside (806) and checked against `MAX_PHASE_EVENTS`; it is an admission check and
   a capacity hint (`Vec::with_capacity` at 826), not a runtime assert.

### Phase order

`origin` is captured once (825) and every offset in every artifact is relative to it.

| # | Phase | Line | Runs when |
|---|---|---|---|
| 1 | Telemetry preflight, external endpoints | 838 | before startup |
| 2 | `StartupReadiness` | 889 | always; cancelled before start is a `Cancelled` event |
| 3 | Telemetry preflight, managed endpoints | 951 | startup completed |
| 4 | `DiagnosticsPre` × plan order | 975 | startup completed |
| 5 | `CorrectnessChecks` × plan order | 995 | after pre-diagnostics |
| 6 | `Warmup` × `1..=warmup` | 1010 | status still `Completed` |
| 7 | `MeasuredTrial` × `1..=measured` | 1096 | status still `Completed` |
| 7a | `Reset` (between trials only) | 1383 | `number < trial_count` and a hook is bound |
| 7b | `Cooldown` (after reset) | 1428 | `cooldown_ms` is set |
| 8 | `Drain` (workload) | 1456 | unconditional |
| 9 | `Drain` (telemetry) | 1518 | any active collector |
| 10 | `DiagnosticsPost` × plan order | 1576 | services still alive |
| 11 | `Teardown` | 1590 | always |
| 12 | `Finalization` | 1700 | always |
| — | `writer.finalize(...)` | 1728 | after the timeline is serialized |

Reset and cooldown are guarded by `number < trial_count`, so nothing follows the final trial,
and reset is ordered before cooldown — the cooldown is post-reset stabilization, not pre-trial
settling. Every optional phase is additionally gated on `state.status ==
ExecutionStatus::Completed && state.staging_error.is_none()`, which is what makes a staging
failure stop later experimental work without skipping cleanup.

### Trial schedule and preflight of reset capability

Warmups run first, in their own ordinal namespace. Each stages
`warmups/{ordinal:03}/result.json` with a `WarmupRecord` carrying schema version, ordinal,
elapsed, status, and failure category — and deliberately **no** arm or pair fields
(`crates/eggbench-runner/src/orchestration.rs:3515`). Warmups never receive `TrialMetrics`:
`stage_warmup` calls `stage_workload_artifacts` with `measured = false` (3546), which uses a
distinct artifact role. Measured trials run `1..=resolved.trials.measured.get()`; each gets a
fresh `TrialId`, a telemetry window, one `execute_invocation`, and a `stage_trial` that
normalizes. After a trial completes and only when more trials remain, the bound `ResetHook` is
invoked (1383) with its own `ResetContext` and its own bound, under a `tokio::select!` racing
cancellation against the reset timeout (1398). A reset failure cancels the child token,
collapses adapter categories to `ResetFailed` (1411), sets the primary failure, and breaks the
trial loop.

### The measurement interval

`execute_invocation` (1826) is the whole boundary. It builds the context, records `start =
Instant::now()` and `start_offset_ns` **before** the adapter is ever polled (1855), then runs a
four-arm `select!` (1860):

| Arm | Result |
|---|---|
| `cancel.cancelled()` | `Cancelled`, no elapsed reported |
| deadline at `limit` | `TimedOut`, no elapsed reported |
| `executor.execute` completes | uses wall time from `start` |
| `measurement_receiver.changed()` | uses the adapter-reported elapsed |

The fourth arm is the measured case. It snapshots the elapsed the adapter published via
`MeasurementSignal::finish` (`crates/eggbench-runner/src/orchestration.rs:1865`) and *then*
grants the adapter a separate `postprocess_limit` to finish its work (1869). So the recorded
duration is the adapter's own measurement of the timed workload portion, not the runner's wall
clock; post-processing cannot inflate the recorded number but can delay the next phase, bounded
separately (`drain_timeout` for both warmups and measured invocations, 1034 / 1194); and if the
adapter never signals, the fallback at 1879 is wall time from `start` to select resolution — an
upper bound that includes post-processing, never an underestimate.

`MeasurementSignal` is a `tokio::sync::watch` sender (line 68): closing it is idempotent and the
last value wins, so a driver that signals twice cannot rewrite the interval. The child
cancellation token is cancelled after the select (1878) so a lingering post-processing poll is
released even on success. Because the outer `select!` is already resolved when the fourth arm
runs, the `limit` deadline is no longer polled, so an invocation's total bound is `limit +
postprocess_limit`, not `limit`.

Everything the runner owns is outside this region. `stage_trial` is called by the coordinator
*after* `execute_invocation` returns (1228, 1287, 1353); artifact staging, provenance
resolution, `normalize_trial_metrics` (`crates/eggbench-runner/src/orchestration.rs:3466`),
`validate()`, and serialization are all inside that later call. Telemetry brackets the interval
without entering it: the window opens at 1135, before `execute_invocation`, and closes at 1204,
after it returns. One adapter-supplied value can still cross the boundary: a completed measured
invocation prefers `output.measurement_elapsed` over the recorded elapsed
(`crates/eggbench-runner/src/orchestration.rs:1200`) — a self-reported measurement, the same
category of claim as the signal.

### M003 paired schedule

A paired resolved plan never restarts services. Both arm services start once and stay up for the
whole run; per-trial arm switching is a **workload-target override**, so executors need no
per-arm code.

| Decision | Line | Implementation |
|---|---|---|
| Alternating, baseline first | 3347 | odd trial → `Baseline`, even → `Candidate` |
| Pair identity `(n+1)/2` | 3353 | `number.div_ceil(2)`, non-wrapping by construction |
| Warmup arms | 3358 | same alternation, no pair identity |
| Arm switching | 3369 | `resolved.workload.with_target(arm_service)` |
| Arm on the invocation | 1111, 1191 | `InvocationKind::Measured { trial_id, arm }` |
| Arm-namespaced seed | 3309 | `0x5452_4941…  \| arm_bit \| trial_id`, candidate sets one bit |
| Shared method schedule | 3323 | pair ordinal via `div_ceil(2)`, identical across both arms of a pair |
| Staged facts | 44, 1218 | schema v2 `TrialExecutionResult` records `arm` and `pair_id` |

`derive_seed` and `derive_schedule_seed` are deliberately different derivations. `seed` varies
per arm so the two variants never share workload input within a pair; `schedule_seed` collapses
the trial id to the pair ordinal when an arm is present, so the *method* — request mix,
ordering, pacing — is identical for both arms of a pair and varies only between pairs. Warmups
use the ordinal namespace and no arm bit, matching the arm-less warmup record. Arm and pair are
written to the staged trial result on **every** terminal path, including the telemetry-start
failure at 1161 and the telemetry-stop failure at 1284, so a partial run's evidence is still
pairable.

### The post-start cleanup contract

The structure is what enforces the contract, not a comment.

1. **State, not control flow.** `RunState` (678) carries three flags that survive the tail:
   `services_started` (698), `workload_entered` (700), and `staging_error` (703).
   `staging_error` is `Option<BundleError>` precisely so an error can be *recorded*
   mid-experiment without unwinding.
2. **No `?` in the tail.** From the tail comment at 1445 to the first `return` at 1721, every
   fallible staging call is written as `if let Err(error) = … { state.staging_error =
   Some(error) }` (1639, 1642, 1650, 1658, 1666, 1674, 1678, 1714). There is no early exit and
   no propagating `?`, so no staging failure can skip drain, teardown, or the later staging
   steps.
3. **One drain, one teardown.** `executor.drain` is called unconditionally under its own
   `drain_timeout` (1462) — the comment at 1452 records that the adapter is the canonical owner
   of its cleanup hook and is expected to be idempotent when no invocation was entered.
   `LocalSession::shutdown` is called iff `state.services_started` (1591), which is set only on
   the successful startup path (899). On a failed startup the flag stays false because
   `LocalSession::startup` already tore down what it had started itself and returns its cleanup
   failures to the caller, which folds them into `state.cleanup_failures` (925). Post-workload
   diagnostics are gated on the same flag (1575) so they run while services can still answer.
4. **Precedence, not replacement.** A drain failure becomes primary only when nothing earlier
   stands (`state.status == ExecutionStatus::Completed`, 1471 / 1493). A teardown failure
   becomes primary only when `cleanup_failures` is still empty (1598) and status is `Completed`
   (1607). Telemetry stop and drain failures are *always* appended to `state.cleanup_failures`
   (1267, 1323, 1535) and never rewrite the primary. Cancellation re-checked after each tail
   step converts a `Completed` run to `Cancelled` without erasing an earlier failure (1506,
   1562, 1594).
5. **One conversion point.** `staging_error.take()` at 1720 is the only place a run becomes
   `OrchestrationError::Evidence { source, cleanup }`. A `writer.finalize` failure is the second
   and last (1742). Both return before a `RunOutcome` exists, so a failed publication cannot be
   represented as a finalized bundle — the caller receives an error, and `bundle_path` /
   `manifest` are only ever constructed from a successful `finalize` (1750–1758).

### Finalization

`PhaseKind::Finalization` is opened at 1700 and finished exactly once at 1707, unconditionally
and as `Completed`, even when a staging error is already pending. `runner-phases.json` is then
serialized from `&state.phases` (`crates/eggbench-runner/src/orchestration.rs:1715`) — a state
in which the finalization event already has its terminal offset, duration, and outcome, and
which is never mutated again. `writer.finalize()` runs afterwards (1728).

That ordering is the whole point: the timeline is a snapshot of a terminated state machine, so
the bundle cannot contain evidence of its own publication, and the runner does not attempt to
record a step that is not representable inside the artifact it produces. The
`PhaseKind::Finalization` doc comment (558–564) says so explicitly. Note the scope precisely:
`Finalization` covers the serialization of `runner-phases.json` only. Lifecycle logs and
metadata, runtime topology, the diagnostic index, the security index, and adapter run evidence
are all staged *before* the phase opens (1637–1690), so a failure in any of them still leaves a
truthful terminal timeline.

## Invariants

| # | Invariant | Enforced at |
|---|---|---|
| 1 | One measured workload invocation per measured interval | 1860–1877 |
| 2 | All runner-owned serialization and normalization happen after the timer stops | 1228, 1287, 1353, 3466 |
| 3 | Warmups never receive `TrialMetrics` or arm/pair tags | 3508–3548 |
| 4 | Paired runs start baseline and alternate; pair is `div_ceil(2)` | 3347–3354 |
| 5 | Per-trial seeds are arm-namespaced; the method schedule seed is not | 3309, 3323 |
| 6 | Reset and cooldown only between measured trials, reset first | 1379–1439 |
| 7 | Every terminal exit after startup passes the single cleanup tail | 1445–1725 |
| 8 | Staging errors never replace the primary cause and never skip cleanup | 703, 1720 |
| 9 | A failed publication never yields a finalized bundle | 1720, 1742 |
| 10 | The phase timeline is serialized from a terminal state | 1700–1715 |
| 11 | Driver artifact names are single safe path components; telemetry names never shadow workload provenance | 3441, 3485, 3560 |
| 12 | Only `core::metrics` writes `TrialMetrics` | 3466 |
| 13 | Bundle finalization always carries `None::<ComparisonVerdict>` | 1730 |
| 14 | Every phase offset is relative to one monotonic origin; no raw clock value is stored | 825, 3285 |

## Failure model

`OrchestrationError` has three variants and no more (625):

| Variant | Line | Meaning | Cleanup attached? |
|---|---|---|---|
| `Preflight(&'static str)` | 628 | Configuration cannot be executed safely | no — nothing started |
| `TelemetryPreflight(String)` | 635 | A required source failed its probe; carries per-source detail | no — before any workload |
| `Evidence { source, cleanup }` | 643 | Evidence could not be truthfully finalized | yes — secondary `CleanupFailure`s |

The split between the first two is deliberate: a static configuration verdict is a `&'static
str`, while a telemetry failure is per-source runtime detail that must stay reportable. Both are
pre-workload.

Experimental failures are **not** errors. A workload timeout, cancellation, or adapter failure
is a truthful `ExecutionStatus` plus a staged failed trial, and the run still finalizes a bundle
— that is what makes a failure analyzable. `OrchestrationError` is reserved for "the evidence
cannot be trusted or published".

| Failure origin | Primary | `ExecutionStatus` | Phase event |
|---|---|---|---|
| Startup | `StartupFailed` / `Cancelled` | `Failed` / `Cancelled` | 908–937 |
| Required managed telemetry preflight | `TelemetryFailed` + reason | `Failed` | none; recorded on `RunOutcome` (966) |
| Per-trial telemetry start | `TelemetryFailed` | `Failed` | 1147 |
| Per-trial telemetry stop | `TelemetryFailed`, stops attached as cleanup | `Failed` | 1270 |
| Warmup / measured workload | `WorkloadFailed` / `TimedOut` / `Cancelled` | via `status_for` | 1070, 1327 |
| Reset | `ResetFailed` / `TimedOut` / `Cancelled` | via `status_for` | 1417 |
| Required pre-diagnostic negative | `DiagnosticFailed` | `Invalid` | 2215 |
| Correctness operational failure | `CorrectnessFailed` | `Invalid` | 2441 |
| Any staging failure | `source` bundle error | none — no bundle | none; held in `staging_error` |
| `finalize` failure | `source` bundle error | none — no bundle | none; held in `staging_error` |

Two distinctions are load-bearing. An optional diagnostic negative or operational failure
records evidence and warning and lets the run continue (2244–2261) — only a *required* negative
changes the status. And a security check returning a valid `Fail` observation is evidence, not
an execution failure: it finishes the phase as `Completed` and the run proceeds into warmups and
trials (2421–2429) so comparison can report security and performance independently. Only an
operational correctness failure — spawn, timeout, preflight denial, invalid tool output — sets
`Invalid`.

## Boundaries

**Owned here:** phase order and gating, per-invocation timeouts and cancellation racing, the
measurement boundary, the paired schedule, seed derivation, per-trial and per-warmup evidence
staging, normalization invocation, run-level indexes, the cleanup boundary, and the finalization
order.

**Owned elsewhere.** `LocalSession` owns process groups, readiness, bounded log capture, runtime
bindings, and reverse teardown; orchestration calls exactly five of its methods — `startup`
(896), `events` (910), `runtime_bindings` (980), `shutdown` (1592), `spawn_order` (3112) — and
interprets none of their internals. Drivers own protocol parsing: a driver returns
`Vec<RawMetricObservation>` and the runner never inspects a driver field beyond artifact names,
media types, and byte counts. `eggbench_core::metrics` owns every semantic decision about a
metric; orchestration calls `normalize_trial_metrics` (3466), `trial_metrics_path` (3472), and
the result's own `validate()` / `to_json_bytes()` chain, appending telemetry warnings afterwards
(3469) before re-validating (3470). Comparison is out of scope by construction: `finalize` is
always called with `None::<ComparisonVerdict>` (1730).

**Extension points this module defines:** `WorkloadExecutor`, `WorkloadArtifact`,
`WorkloadOutput`, `InvocationContext`, `InvocationKind`, `MeasurementSignal`, `DrainContext`,
`RunEvidenceContract`, `RunEvidenceArtifact`, `ResetContext`, `ResetHook`, `ResetRegistry`. Each
is object-safe and independently implementable; the runner never downcasts an adapter.
`RunEvidenceContract` is the only one implementable outside this crate without also implementing
a workload, being a single `Serialize` bound plus one validator.

## Tests and qualification

The module's own `#[cfg(test)]` block covers only the pair schedule (3737–3794): the
alternating/ceiling table for trials 1–6, a `u32::MAX` non-wrapping regression on `div_ceil`,
and the property that both arms of one pair share a `schedule_seed` while the next pair does
not.

Everything else is qualified through `FakeWorkload` (3614), a deterministic `WorkloadExecutor`
whose fields steer the state machine and the evidence paths: `fail_on` and `pending_on` select
the failing or cancellation-bound invocation, `drain_delay` / `drain_fails` drive the drain
branch, and the four `*_by_invocation` vectors inject adversarial artifact names, counts, and
bytes per invocation. It records `invocations`, `workload_targets`, `invocation_seeds`, and
`drained` so a test can assert the schedule, the per-invocation arm target, and the seed
derivation directly. Integration coverage lives in
`crates/eggbench-runner/tests/orchestration.rs` (the primary qualification target), with
`tests/telemetry.rs` and `tests/metrics.rs` covering the telemetry window and the normalization
path.

## Review focus

1. **`execute_invocation`'s four-arm select** (1860–1877). Arm order, what `reported_elapsed` is
   set from, and which budget bounds post-processing determine what falls inside the measured
   interval. `staging_error` must never appear in this function.
2. **The absence of `?` in the tail** (1445–1725). A new staging step added with `?` or an early
   `return` breaks the cleanup contract silently. New staging must use `if let Err(error) = … {
   state.staging_error = … }`.
3. **Precedence guards.** Every `state.status == ExecutionStatus::Completed` check before
   assigning a primary failure is a deliberate "do not mask an earlier cause" rule.
4. **Telemetry window symmetry.** `start_trial_telemetry` (399) already best-effort-stops
   windows it opened when a later start fails, which is why the call site needs no cleanup at
   1144. If that helper changes, the call site leaks polling tasks.
5. **Finalization ordering** (1700–1721). The event must be finished exactly once *before*
   `stage_phase_artifacts`, and `state.phases` must not be touched after. A new
   post-finalization event would put an unterminated event into `runner-phases.json`.
6. **The phase-count bound** (806–822) is `warmups + 3·trials + 2 + diagnostics + correctness`.
   In the maximal configuration (reset *and* cooldown between every pair, plus an active
   telemetry collector) the real count is `warmups + 3·trials + 3`, because telemetry adds a
   second `Drain` event at 1518. `Vec::with_capacity` absorbs the shortfall and
   `MAX_PHASE_EVENTS` has headroom, but the per-artifact reservation at 3163 derives from the
   same bound, so the `runner-phases.json` floor is one event short in that configuration.
7. **`services_started` is set only on the successful startup path** (899). That is correct
   today because `LocalSession::startup` self-cleans on every failure path, but it couples the
   teardown gate to that internal guarantee: if startup ever returned with processes still
   running, both `LocalSession::shutdown` (1591) and post-workload diagnostics (1575) would be
   skipped.
8. **`workload_entered` is write-only.** Assigned at 1040, 1066, 1201, and 1310, never read;
   drain is unconditional with the rationale at 1452. The in-tree docs assert the opposite
   gating — see the divergence note below.
9. **Adversarial driver input.** Artifact names (3441, 3485, 3560), counts (3422, 3555), and
   byte sizes (2652, 2749, 2857) are re-checked at the runner boundary rather than trusted. New
   artifact kinds need the same treatment.
10. **Network-path evidence is fail-closed** (1682): a plan with `network_path` returning no
   evidence is a staging error, and `network-path.json` is rejected outright for a path-free
   plan (3062).

## Related

- [overview.md](overview.md) — system shape, end-to-end flow, bundle layout
- [runner.md](runner.md) — crate-level ownership, paired schedule, cleanup and
finalization contracts
- [runner-process-lifecycle.md](runner-process-lifecycle.md) — `LocalSession`
startup, readiness, and reverse teardown
- [runner-adapter-seams.md](runner-adapter-seams.md) — service, telemetry,
correctness, and diagnostic registries
- [core-metrics.md](core-metrics.md) — `normalize_trial_metrics` and the
`TrialMetrics` contract this module invokes
- [core-evidence-bundle.md](core-evidence-bundle.md) — `BundleWriter`,
manifest-last publication, atomic finalize
- [../docs/trial-orchestration.md](../docs/trial-orchestration.md) — the M002
contract doc for phase order, the measurement boundary, and timeout keys
- [../docs/paired-experiments.md](../docs/paired-experiments.md) — the
alternating-baseline-first schedule and the paired comparison policy
- [../docs/metrics.md](../docs/metrics.md) — raw observations versus normalized
metrics
