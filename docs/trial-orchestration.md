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
