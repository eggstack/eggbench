# Runner ownership and execution phases

`eggbench-runner` owns local side effects; `eggbench-core` owns portable,
runtime-independent plans and evidence contracts. `LocalSession` owns managed
process groups, readiness, bounded logs, and mandatory reverse teardown.
`execute_run` owns experiment phase order and delegates process work to the
session rather than duplicating supervision.

The phase coordinator preflights timeout names and explicit reset
capabilities before startup. It executes warmups and measured invocations
through `WorkloadExecutor`, invokes a registered `ResetHook` between measured
trials, and always separates workload drain from service teardown. The
measurement interval contains only one measured workload invocation. All
runner-owned serialization and evidence finalization happen after the timer
stops.

The coordinator records typed run-relative `PhaseEvent`s, versioned trial
execution facts, separate warmup artifacts, lifecycle logs, and the resolved
driver inventory. Bundle finalization records execution status and leaves the
comparison verdict absent. This is execution infrastructure, not a
performance-comparison capability.

## Post-start cleanup contract

Every terminal exit from `execute_run` that follows a successful managed
startup routes through one cleanup boundary: workload drain is attempted if
the executor was reached, and `LocalSession::shutdown` is attempted whenever
managed startup created owned processes. Any error encountered during
evidence staging accumulates as the primary cause and is preserved across
the cleanup tail. Drain or teardown failures observed during that tail are
attached as secondary cleanup diagnostics on the returned
`OrchestrationError::Evidence` and never replace the primary cause. No
failed evidence publication is represented as a valid finalized bundle.

## Finalization phase

`PhaseKind::Finalization` records runner evidence staging prior to immutable
bundle publication. The event is finished exactly once, `runner-phases.json`
is staged from that terminal state, and no further mutation occurs before
`writer.finalize()` is called. The bundle it publishes therefore cannot
contain evidence of its own publication; the runner documents that fact
rather than attempting recursive evidence.
