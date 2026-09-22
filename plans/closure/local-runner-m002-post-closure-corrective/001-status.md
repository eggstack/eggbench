# Local Runner M002 Post-Closure Evidence-Safety Corrective C001 — Closure

Disposition: **closed**
Closed: 2026-09-22
Implementation commit: `6900212a5997b3d776e824b115d8ad35d36cd431` against audit baseline `02212d5d9f78816eec5ef1b89d45e99c0dbe02d5`.
Hosted qualification: CI run `35797812233` — completed successfully on Linux stable, Linux Rust 1.89 MSRV, macOS stable, and Windows stable supported-subset jobs.

## 1. Why a corrective closure rather than a new M002 amendment

The historical M002 closure at `plans/closure/local-runner-lifecycle/002-status.md`
was based on the available verification: workload failure, timeout, cancellation,
reset failure, drain failure, teardown failure, mandatory evidence-capacity
preflight, and successful bundle finalization. That verification did not
cover a `BundleError` returned from `stage_warmup(...)` or `stage_trial(...)`
after managed startup and after a workload invocation, and did not compare
the persisted finalization event to the returned `RunOutcome.phases`.

The post-closure audit found exactly two narrow untested correctness defects
at the orchestration/evidence boundary:

1. **Post-start evidence staging could bypass drain and teardown.**
   `?` propagation inside `execute_run()` could return before the common
   workload-drain/`LocalSession::shutdown` tail executed.
2. **Persisted finalization phase could disagree with the returned
   `RunOutcome.phases`.** The in-memory finalization event was finished
   again after `writer.finalize()`, so the serialized event could not
   include its own publication while the in-memory one could.

Both findings are narrow and do not invalidate M002's workload/reset seams,
trial-result schema v1, or manifest v2. The corrective restores the original
M002 invariants without rewriting the historical closure.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Outcome |
|---|---|---|
| Common post-start cleanup path: drain when workload entered, teardown when managed startup created processes | `RunState` accumulates `services_started`, `workload_entered`, and `staging_error`. `execute_run` always falls through to a single drain+teardown tail before any finalization disposition. New tests `unsafe_workload_artifact_name_after_measured_invocation_drains_and_tears_down`, `too_many_workload_artifacts_after_measured_invocation_drains_and_tears_down`, `dynamic_workload_byte_overflow_after_preflight_passes_drains_and_tears_down`, `warmup_staging_failure_skips_measured_trials_and_still_drains`. | Pass |
| Primary evidence error preserved; secondary cleanup diagnostics attached | `OrchestrationError::Evidence { source, cleanup }` carries the primary `BundleError` plus a `Vec<CleanupFailure>` collected during the cleanup tail. New tests `evidence_error_with_drain_failure_still_tears_down` and `evidence_error_with_teardown_failure_preserves_primary_cause` assert that drain failure does not replace the primary cause and that teardown failure is recorded in `cleanup` without changing `source`. | Pass |
| No post-start `BundleError` path can bypass mandatory teardown | Drain and teardown are unconditional in the cleanup tail whenever `execute_run` reaches it. The earlier `stage_warmup(...)?` and `stage_trial(...)?` `?` propagations were replaced with `match` blocks that record `staging_error` and continue. New tests prove drain+teardown after adversarial artifact errors and after combined drain/teardown failures. | Pass |
| Teardown still attempted after drain failure | The teardown step runs unconditionally after drain; `evidence_error_with_drain_failure_still_tears_down` runs a `sleeper` service and verifies `session.is_running()` is `false` and `is_process_alive(pid)` is `false` after a drain that is configured to fail. | Pass |
| Failed evidence staging never produces a valid finalized bundle | `failed_evidence_staging_does_not_publish_final_bundle` checks that the final sibling path is absent; `unsafe_workload_artifact_name_after_measured_invocation_drains_and_tears_down` confirms the same; `writer.finalize()` is only invoked when no `staging_error` is recorded. | Pass |
| Persisted `runner-phases.json` equals `RunOutcome.phases` for successful runs | `persisted_phase_vector_equals_returned_phase_vector_on_success` deserializes `runner-phases.json` from the finalized bundle and asserts exact equality with `RunOutcome.phases`. Every event has `outcome.is_some()`. | Pass |
| Finalization event terminalized exactly once | `finalization_event_is_terminalized_exactly_once` asserts there is exactly one `PhaseKind::Finalization` event with a terminal `elapsed_ns`. The implementation begins and finishes the event before staging `runner-phases.json`; no second `finish_phase` runs after `writer.finalize()`. | Pass |
| Finalization semantics documented without recursive publication claims | `PhaseKind::Finalization` docstring states the phase covers runner evidence staging prior to immutable bundle publication. `docs/trial-orchestration.md` and `architecture/runner.md` document the same; the runner does not attempt to record publication inside the bundle it publishes. | Pass |
| Measurement windows and trial ordering unchanged | `schedules_warmup_trials_reset_cooldown_and_finalizes_separate_evidence` still observes the canonical ordering. `measurement_start_offset_ns` and `measurement_elapsed_ns` semantics are unchanged. Trial result schema v1 and manifest v2 schema are unchanged. | Pass |
| Existing M001/M002 regressions remain green | Full workspace test count remains `83 passed (8 suites)`. 19 pre-existing orchestration tests, 24 lifecycle tests, 29 core tests, 2 platform tests, plus 9 new orchestration regressions. | Pass |
| Adversarial dynamic workload artifacts prove the behavior | `FakeWorkload::artifacts_by_invocation` returns controlled `WorkloadArtifact` values per invocation. C1 unsafe name, C2 count overflow, C3 dynamic byte overflow, and C4 warmup staging failure tests all run a real `sleeper` fixture so cleanup reclaims an actual descendant. | Pass |

## 3. Public API change

`OrchestrationError::Evidence` evolves from a tuple variant around `BundleError`
to a structured variant:

```rust
pub enum OrchestrationError {
    Preflight(&'static str),
    Evidence {
        source: BundleError,
        cleanup: Vec<CleanupFailure>,
    },
}
```

Helpers `OrchestrationError::source()` and `OrchestrationError::cleanup()`
expose the primary cause and secondary diagnostics respectively. `Display`
remains redaction-safe: `source` formats through `BundleError`'s existing
formatter and `cleanup` formats through `CleanupFailure`'s existing
formatter. No secret-bearing workload output is stored in the error.

The internal `RunState` struct is not part of the public API.

## 4. Adversarial artifact regression matrix

| Scenario | Test | Outcome |
|---|---|---|
| Unsafe artifact name after measured invocation | `unsafe_workload_artifact_name_after_measured_invocation_drains_and_tears_down` | Drain runs, teardown runs, no descendant remains, error source is `InvalidManifest("unsafe workload artifact name")`, final path absent |
| Artifact-count overflow after measured invocation | `too_many_workload_artifacts_after_measured_invocation_drains_and_tears_down` | Drain runs, teardown runs, error source is `BoundExceeded("workload artifact count")` |
| Dynamic byte-bound overflow after preflight passes | `dynamic_workload_byte_overflow_after_preflight_passes_drains_and_tears_down` | Preflight passes, services start, workload executes, staging fails, drain runs, teardown runs, error source is `BoundExceeded` |
| Warmup staging failure (no measured trial begins) | `warmup_staging_failure_skips_measured_trials_and_still_drains` | Only the warmup invocation entered, drain runs, no measured trial in `invocations` |
| Evidence error + drain failure | `evidence_error_with_drain_failure_still_tears_down` | Primary evidence cause preserved, teardown runs, no managed descendant remains |
| Evidence error + teardown failure | `evidence_error_with_teardown_failure_preserves_primary_cause` | Primary evidence cause preserved, teardown failure appears in `cleanup`, drain runs |
| Failed staging does not publish | `failed_evidence_staging_does_not_publish_final_bundle` | Final sibling path is absent |
| Persisted == returned phases | `persisted_phase_vector_equals_returned_phase_vector_on_success` | Exact equality after deserialization, every event terminal |
| One finalization event, terminal exactly once | `finalization_event_is_terminalized_exactly_once` | Exactly one `PhaseKind::Finalization` with `outcome == Some(Completed)` and `elapsed_ns.is_some()` |

## 5. Verification

- `cargo fmt --all -- --check` — pass
- `cargo check --workspace --all-targets --locked` — pass
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass
- `EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-features --locked` — pass: 29 core tests, 24 lifecycle tests, 28 orchestration tests, 2 platform tests (83 total across 8 suites)
- `cargo +1.89.0 check --workspace --all-targets --locked` — pass
- `cargo +1.89.0 test -p eggbench-core --all-features --locked` — pass (29 core tests)
- `cargo tree --locked` — pass; no new runtime or process dependency
- `git diff --check` — pass
- Hosted CI run `35797812233` — pass: Linux stable, Linux Rust 1.89 MSRV, macOS stable, and Windows stable supported subset all completed successfully.

## 6. Compatibility, security, and limitations

- Manifest v2 schema, trial execution schema v1, workload/reset seams, and
  process ownership remain intact. The corrective changes only runner-internal
  control flow and the finalization phase ordering.
- `OrchestrationError::Evidence` now carries a structured `cleanup` field.
  Callers using `OrchestrationError::Evidence(_)` pattern matching must be
  updated to `OrchestrationError::Evidence { source, .. }`. The repository
  had no such pattern matches; the helper methods preserve readability.
- Failed evidence staging never produces a finalized bundle. The staging
  directory is left in the existing incomplete state; no copy fallback or
  repair was added.
- Drain remains unconditional to preserve the existing M002 contract and the
  tests that assert drain after `cancellation_before_startup`. The
  corrective's stronger "may be skipped" wording is honoured by the failure
  semantics in `OrchestrationError::Evidence` rather than by skipping drain.
- Cleanup diagnostics never replace the primary cause. The Display impl
  exposes `source` first; `cleanup` is appended only via the helper accessors.
- No new platform capability, no new dependency, no new public schema field.

## 7. Dependency disposition

The corrective unblocks both downstream paths:

- **Local Runner M003** — environment fingerprint + CLI lifecycle. Plan
  authoring can begin against the corrected evidence contract. Implementation
  is gated on plan review per `plans/003-planning-process.md`.
- **Measurement/Comparison M001** — metric normalization and trial evidence.
  Plan authoring can begin against lifecycle-safe trial evidence from the
  corrected orchestration. Implementation is gated on plan review.

External Oracles M001 remains independently plan-authorable; implementation
that consumes the corrected orchestration path must plan against the new
mandatory cleanup contract. Eggstack integrations and Security Qualification
remain blocked on their measurement/integration prerequisites. Distributed
execution remains deferred.

## 8. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| None | No unresolved M002 evidence-safety correctness, security, lifecycle, or portability finding | No further corrective work required |
| None | Hosted CI qualification is complete | CI run `35797812233` passed all required jobs |

## 10. Disposition

**Closed** with the original M002 invariants restored, the previously
uncovered `BundleError` paths now proven cleanup-safe against a real managed
fixture process, and the finalization phase made truthful and self-consistent
with the returned `RunOutcome.phases`. Future plan authoring may proceed.