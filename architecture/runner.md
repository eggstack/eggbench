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

## M003 pre-start preparation

`LocalEnvironmentCollector::collect()` produces a versioned
[`EnvironmentFingerprint`](../docs/environment-fingerprint.md) before
managed startup. Collection failure cannot leave managed processes running
because startup has not begun.

`SubjectSnapshot::build()` records the resolved executable path, its
SHA-256 digest, the declared revision/digest hints, and whether the
declared digest matches the observed digest for managed subjects. External
and label subjects keep declared identity only.

`prepare_bundle(...)` stages the four primary evidence artifacts
(`plan.json`, `resolved-plan.json`, `environment.json`, `subject.json`) into
a still-unpublished `BundleWriter` before the first measured workload
invocation. The helper centralizes CLI boilerplate and keeps CLI input
parsing out of the runner's execution engine.

The CLI in [`eggbench-cli`](../docs/cli.md) consumes these helpers and
delegates measurement work to `execute_run`; it does not duplicate
orchestration.

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
