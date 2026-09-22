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
