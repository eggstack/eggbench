# Local Runner M002 Closure — Warmup, Trials, Cooldown, Reset

Disposition: **closing; timeout regression qualification pending**
Implemented through commit `87e89691568d7fb0b018667d8174bb1dfe9e6f0c`, hosted and qualified by [CI run 35789353964](https://github.com/eggstack/eggbench/actions/runs/35789353964). Three timeout regression tests are being added and require a final hosted run.

## Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Execute startup/readiness, warmups, measured repetitions, inter-trial reset/cooldown, drain, teardown, and finalize one bundle | `crates/eggbench-runner/src/orchestration.rs`; `schedules_warmup_trials_reset_cooldown_and_finalizes_separate_evidence` |
| Keep setup, reset/cooldown, serialization, drain, teardown, and finalization outside measured execution | `execute_invocation` samples one monotonic start immediately before invoking the adapter; `measurement_elapsed_ns` stops when that invocation returns. The end-to-end timing assertion checks a delayed fake invocation and the persisted run-relative offset. |
| Deterministic positive IDs and versioned trial results | `TrialId` allocation and `TrialExecutionResult` schema version 1 in `crates/eggbench-core/src/evidence.rs`; end-to-end result decoding asserts schema/status. |
| Structurally separate warmups | Warmups use ordinal-only records under `warmups/NNN/`, with an `Other { label: warmup }` role. The success and warmup-failure tests assert that these do not enter `manifest.trials`. |
| Cancellation and cleanup | `tests/orchestration.rs` covers cancellation before startup, warmup, measured work, reset, cooldown, drain, and teardown. `tests/lifecycle.rs` covers cancellation during readiness and before spawn; the process cleanup suite verifies mandatory teardown. |
| Stop after failure and retain entered/completed evidence | `later_trial_failure_preserves_earlier_trial_and_still_drains`, `reset_failure_keeps_completed_trial_and_stops_before_next_trial`, and `warmup_failure_stops_experiment_without_fabricating_a_trial`. |
| Explicit reset capability and preflight | `missing_reset_hook_fails_preflight_before_start_or_drain`; `ResetRegistry` maps service/reference names to object-safe async `ResetHook`s. |
| Bounded workload/reset/drain and known timeout namespace | `preflight` requires `measurement` and `drain`, supports `warmup` fallback, requires `reset` when configured, rejects unknown and zero-valued timeout keys; invocation, reset, and drain calls use Tokio bounds. |
| Reject insufficient mandatory evidence bounds before startup | `insufficient_evidence_bounds_fail_preflight_before_startup`; `preflight_evidence_capacity` checks mandatory artifact count, phase/lifecycle metadata size, declared service log caps, and total bytes. |
| Bound warmup, reset, and drain timeouts | New regression cases verify each timeout preserves truthful status/evidence and continues mandatory cleanup. |
| No concrete transport, load generator, shell, or comparison semantics | `cargo tree --locked` and source/dependency inspection: `eggbench-core` has no Tokio/process/network dependency; `eggbench-runner` adds no concrete network/load-generator dependency. M002 always finalizes with `comparison_verdict: None`. |
| Truthful immutable evidence | End-to-end test opens and verifies the finalized `.eggb`, checks trial order, result artifacts, warmup role/path, phase timeline, and no comparison verdict. Lifecycle logs/metadata and resolved driver inventory are staged before publication. |
| Correct the two predecessor bookkeeping items | `tests/lifecycle.rs` now says completed lifecycle evidence has no comparison verdict and asserts Linux/macOS supported, other Unix unqualified, non-Unix unsupported. |

## Landed execution contract

```text
preflight → startup/readiness → warmups → measured trial 1
                                              ↓
                     next trial ← reset → cooldown
                          ↓
               workload drain → teardown → finalization
```

Reset precedes cooldown and both occur only between measured trials. The
measurement interval is exactly one `WorkloadExecutor::execute` call for a
measured invocation. The separate workload drain is bounded and still runs
after experimental failure/cancellation; `LocalSession::shutdown` remains
mandatory cleanup.

The public runner seams are `WorkloadExecutor`, `DrainContext`, `ResetHook`,
and `ResetRegistry` in `crates/eggbench-runner/src/orchestration.rs`. The
deterministic fake is namespaced under `eggbench_runner::test_support`.

Trial execution schema v1 example:

```json
{
  "schema_version": 1,
  "trial_id": 1,
  "measurement_start_offset_ns": 12500000,
  "measurement_elapsed_ns": 10040000,
  "terminal_status": "completed",
  "failure_category": null
}
```

Representative artifact layout:

```text
plan.json
resolved-plan.json
environment.json
runner-phases.json
lifecycle/lifecycle.json
warmups/001/result.json
trials/001/result.json
trials/002/result.json
```

## Verification

- `cargo fmt --all -- --check` — passed.
- `cargo check --workspace --all-targets --locked` — passed.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — passed.
- `EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-features --locked` — passed: 29 core tests, 24 lifecycle tests, 19 orchestration tests, and 2 platform tests. The sentinel is required by an existing hermetic-environment assertion and is set by CI.
- `cargo +1.89.0 check --workspace --all-targets --locked` — passed.
- `cargo tree --locked` and focused core source/dependency inspection — passed; runtime/process ownership remains in runner and no concrete transport or benchmark dependency was added.
- `git diff --check` — passed.
- Hosted CI run `35789353964` — passed: Linux stable, Linux 1.89 MSRV, macOS stable including process-group and filesystem-confinement qualification, and Windows supported subset.

## Documentation and known limits

Updated `docs/local-runner-lifecycle.md`, added `docs/trial-orchestration.md`
and `architecture/runner.md`, updated `architecture/drivers.md` and README,
and recorded the roadmap/registry transition.

M002 supplies only orchestration infrastructure and a deterministic fake.
Callers still supply the workload implementation and pre-stage source plan,
resolved plan, and environment evidence. Production workload protocols,
environment fingerprinting, CLI behavior, metric normalization, and
comparison remain later work. No unresolved correctness, security, lifecycle,
or portability finding remains within M002 scope.

## Dependency disposition

- Local Runner M003 (environment fingerprint and CLI lifecycle) is now ready
  for implementation-plan authoring.
- Measurement/Comparison M001 (metric normalization and trial evidence) is
  now ready for implementation-plan authoring against real local trial
  evidence.
- Eggstack integrations and security qualification remain blocked on their
  measurement/integration contracts. External Oracles remains independently
  ready for plan authoring; actual performance qualification still waits for
  the measurement contract.
