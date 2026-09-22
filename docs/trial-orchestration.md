# Local trial orchestration (M002)

`eggbench_runner::execute_run` coordinates one resolved local experiment above
`LocalSession`. The session remains the sole owner of managed service
processes. Workload adapters and reset hooks are explicit asynchronous seams;
M002 includes a deterministic `FakeWorkload` for qualification and defines no
protocol-specific workload or reset behavior.

## Phase order

```text
preflight → startup/readiness → warmup 1..N → measured trial 1
                                              ↓
                  next measured trial ← reset → cooldown
                         ↓
               workload drain → service teardown → evidence finalization
```

Reset and cooldown occur only between measured trials. Reset runs first, so
cooldown is post-reset stabilization time. Warmup state carries into measured
trial 1. No reset or cooldown follows the final trial.

Every code path that reaches the post-experimental tail — workload/reset
failure, cancellation, evidence-staging failure — runs the same drain and
teardown sequence before any evidence/finalization disposition is returned.
Workload drain is attempted whenever the executor was reached, and managed
`LocalSession::shutdown` is attempted whenever startup created owned processes.
A drain or teardown failure is reported as secondary cleanup diagnostics; it
never replaces the primary failure cause.

## Measurement boundary

Each measured interval begins immediately before `WorkloadExecutor::execute`
is invoked and ends when that invocation returns, errors, times out, or is
cancelled. Startup/readiness, warmups, reset, cooldown, workload drain,
service teardown, runner-owned artifact serialization, hashing, and bundle
publication are outside that interval. Timing uses one host monotonic origin;
evidence stores run-relative nanosecond offsets and durations, never a raw
clock value.

One completed repetition receives one positive `TrialId`. Its
`TrialExecutionResult` schema v1 stores the ID, measurement offset and
duration, terminal execution status, and an optional typed redaction-safe
failure category. It carries no normalized metrics or comparison verdict.
Warmups have their own ordinal namespace and `warmups/NNN/result.json`
artifacts with a distinct role; they never appear in `manifest.trials`.

## Finalization phase semantics

`PhaseKind::Finalization` records **runner evidence staging prior to
immutable bundle publication**. It covers staging of `runner-phases.json` and
any other runner-owned artifacts required before publication. The event is
finished exactly once; the serialized phase event is identical to the in-memory
event returned to the caller.

Immutably publishing the bundle with `writer.finalize()` is the step that
follows the finalization event. That publication itself is not representable
inside the bundle it produces; the runner therefore does not claim that the
finalization phase interval covers its own publication.

A failure during staging — including workload artifact staging that fails
after a measured invocation, lifecycle log/metadata staging, or phase
artifact staging — is reported as `OrchestrationError::Evidence { source,
cleanup }`. The primary cause is the source bundle error; any drain or
teardown failure observed while attempting mandatory cleanup is attached as
secondary cleanup diagnostics and never replaces the primary cause. Failed
evidence publication never produces a finalized bundle; the staging directory
remains incomplete and the final sibling path is not created.

## Timeout keys

M002 recognizes exactly these `TrialPolicy.timeouts` names:

| Key | Meaning |
|---|---|
| `measurement` | Required safety bound for each measured invocation |
| `warmup` | Optional warmup bound; defaults to `measurement` |
| `reset` | Required when reset policy is not `none` |
| `drain` | Required independent workload cleanup bound |

Unknown keys fail preflight before startup. `cooldown_ms` bounds cooldown
directly. Workload `duration_ms` remains workload intent and does not replace
the runner safety timeout.

## Cancellation and failure

Cancellation stops active warmup, measured invocation, reset, or cooldown and
prevents later experimental work. An entered measured invocation receives a
cancelled result. The coordinator then runs workload drain to its independent
bound and always attempts `LocalSession::shutdown`; cancellation cannot skip
safety cleanup. Workload/reset failures stop later trials while preserving
entered trial evidence. Drain and teardown errors are reported separately
without replacing an earlier primary failure. Completed, failed, and cancelled
experiments finalize with truthful `ExecutionStatus` and no comparison
verdict.

## Extension seams and scope

`WorkloadExecutor` is mutable and object-safe: it accepts an
`InvocationContext`, then receives an independent `DrainContext`. It can
return bounded diagnostic files which the runner stages after measurement.
`ResetRegistry` maps the explicitly named service or reference to a
`ResetHook`; a missing hook is a preflight error. `ResetPolicy::Service` does
not mean process restart.

M002 produces execution evidence only. It does not calculate throughput,
latency distributions, error rates, baselines, or pass/fail/inconclusive
performance verdicts. Those semantics belong to Measurement/Comparison.
