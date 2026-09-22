# Local Runner M002 — Post-Closure Evidence-Safety Corrective Addendum

Status: active

Repository audit baseline: `02212d5d9f78816eec5ef1b89d45e99c0dbe02d5`

Predecessor work:

- `plans/subsystems/local-runner-lifecycle-roadmap.md` — M001 and M002 historically closed.
- `plans/implementation/local-runner-lifecycle/002-warmup-trial-cooldown-reset-state-machine.md`
- `plans/closure/local-runner-lifecycle/002-status.md`
- `plans/closure/local-runner-lifecycle-post-closure-corrective/001-status.md`

Long-term references:

- `plans/000-long-term-specification.md#5-canonical-execution-model`
- `plans/000-long-term-specification.md#12-evidence-bundle`
- `plans/003-planning-process.md#8-corrective-passes`

Related ADRs:

- `plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md`
- `plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md`
- `plans/adrs/ADR-0005-local-first-execution-and-remote-provider-boundary.md`

## 1. Purpose and corrective trigger

M002 correctly established warmup/trial/reset/cooldown/drain/teardown orchestration and closed with broad cross-platform evidence.

A post-closure audit found one cleanup-safety defect and one evidence-truthfulness defect at the boundary between orchestration and bundle staging:

1. **Post-start evidence-staging errors can bypass drain and teardown.**
   Calls such as `stage_warmup(...)?` and `stage_trial(...)?` may return `OrchestrationError::Evidence` after managed services have started and after workload execution has occurred. Because the error propagates immediately from the middle of `execute_run()`, the function can return before the workload drain and `LocalSession::shutdown()` paths execute.

   Dynamic workload artifacts can trigger this even though M002 preflights mandatory runner evidence capacity. Examples include unsafe artifact names, too many workload artifacts, per-artifact overflow, or total-byte exhaustion caused by adapter-supplied output.

2. **Persisted finalization-phase evidence is not the same phase state returned to the caller.**
   The current orchestrator marks `PhaseKind::Finalization` complete, stages `runner-phases.json`, then publishes the bundle with `writer.finalize()`, and finally calls `finish_phase()` again in memory. Therefore the persisted finalization event cannot include publication itself and may differ from `RunOutcome.phases`.

These findings are narrow and do not require redesigning M002, the workload/reset seams, trial-result schema, manifest v2, or the process lifecycle.

Historical M002 closure remains immutable evidence of what was accepted at the time. This corrective records the newly discovered defects rather than rewriting that closure.

## 2. Why previous verification missed the defects

M002 tests exercised:

- workload failure;
- timeout;
- cancellation;
- reset failure;
- drain failure;
- teardown failure;
- mandatory evidence-capacity preflight;
- successful bundle finalization.

They did **not** exercise a bundle-registration/staging failure that occurs after startup and after a workload invocation but before the common drain/teardown tail.

The finalization tests verified that the published bundle was readable and that a finalization event existed, but did not compare the persisted phase event to the returned `RunOutcome.phases` record or require the event definition to exclude/include publication consistently.

The corrective must add regressions for both missed classes.

## 3. Work classification

Primary class: invariant/correctness corrective.

### Invariants

- Every post-start terminal path attempts workload drain when applicable and managed service teardown.
- Evidence errors must not strand runner-owned processes or workload-owned resources.
- The initiating error remains primary when cleanup also fails.
- No finalized bundle claims stronger evidence than was actually persisted.
- A phase event has one defined semantic interval; persisted and returned representations must agree.
- Evidence failure must not fabricate a successful execution outcome.
- Measurement intervals remain unchanged and exclude all evidence work.
- Existing manifest v2, trial-result v1, workload/reset seams, and process ownership remain intact.

### Capability

- Return a redaction-safe evidence/finalization failure while still proving cleanup was attempted.

### Infrastructure

- one cleanup guard/common tail for post-start orchestration exits;
- explicit phase-artifact/finalization semantics;
- adversarial workload-artifact test support.

## 4. Non-goals

Do not:

- implement M003;
- implement Measurement/Comparison;
- change trial timing;
- change reset-before-cooldown ordering;
- add new workload protocols;
- add environment collection or CLI;
- change manifest v2 unless a concrete implementation blocker requires an ADR/planning review;
- add transaction rollback for already staged temporary bundle files beyond existing BundleWriter semantics;
- turn workload artifact validation into a separate driver subsystem.

## 5. Current implementation evidence

At the audit baseline:

- `execute_run()` owns all phases in one auditable function.
- warmup success/failure calls `stage_warmup(...)?` inside the experimental control flow;
- measured success/failure calls `stage_trial(...)?` inside the experimental control flow;
- `stage_workload_artifacts()` can fail for:
  - more than 256 artifacts per invocation;
  - unsafe artifact names;
  - `ArtifactPath` validation;
  - BundleWriter artifact-count/per-artifact/total-byte bounds;
- those failures propagate through `?` before the unconditional drain/teardown section;
- mandatory evidence-capacity preflight intentionally reserves only predictable runner evidence and cannot know arbitrary adapter output;
- `PhaseKind::Finalization` is documented as evidence serialization and bundle publication;
- current code finishes the finalization event before staging the phase artifact and before `writer.finalize()`, then mutates the in-memory event again after publication;
- M002 closure contains no adversarial dynamic workload-artifact test.

## 6. Target control-flow property

After any managed process successfully starts, all exits from orchestration must conceptually pass through one cleanup boundary:

~~~text
experimental work
   |
   | success / workload failure / reset failure / cancellation
   | evidence staging failure
   v
workload drain attempt
   v
service teardown attempt
   v
evidence/finalization disposition
   v
return outcome or orchestration error
~~~

The implementation may use a scoped state object, explicit common tail, helper function, or another Rust-safe pattern.

Do not use panic-catching as normal control flow.

## 7. Evidence-error cleanup semantics

When a staging/registration error occurs after startup:

1. preserve the original `BundleError` as the primary orchestration error;
2. stop further warmups/trials/reset/cooldown work;
3. attempt workload drain if the workload may have been entered;
4. always attempt `LocalSession::shutdown()` if managed startup began;
5. collect drain/teardown failures as secondary cleanup diagnostics without replacing the primary evidence error;
6. do not publish a finalized bundle unless the bundle can be completed truthfully;
7. leave any incomplete staging directory under the existing BundleWriter incomplete/staging semantics;
8. do not emit `ExecutionStatus::Completed`.

The exact public error shape may evolve. It must be possible to inspect:

- the primary evidence error;
- whether drain was attempted and whether it failed;
- managed cleanup failures.

A narrow `OrchestrationError::Evidence { source, cleanup... }` or separate cleanup context is acceptable.

## 8. Cleanup applicability

The corrective should track enough orchestration state to avoid misleading cleanup calls.

- If failure occurs before startup and before workload entry, no drain/teardown is required beyond existing preflight semantics.
- If services started but no workload invocation occurred, teardown is required; drain MAY be skipped if the executor was never entered and the workload contract explicitly permits that.
- If any workload invocation began, drain must be attempted even if staging its output failed.
- Teardown must be attempted whenever managed startup created owned processes.

Prefer simple conservative cleanup over fragile micro-optimization.

## 9. Dynamic workload-artifact regressions

Add a test workload capable of returning controlled artifacts.

At minimum qualify these post-start errors:

### Unsafe artifact name

Return a successful measured workload output containing an artifact name such as `../escape` or a separator-containing name.

Expected:

- workload invocation occurred;
- staging fails;
- drain is entered;
- service teardown completes;
- no managed descendant remains;
- error retains the evidence/path-safety cause.

### Artifact-count overflow

Return more than `MAX_WORKLOAD_ARTIFACTS_PER_INVOCATION`.

Expected cleanup behavior is identical.

### Bundle-bound overflow

Use a writer whose mandatory evidence preflight passes, then return a dynamic workload artifact large enough to exceed remaining per-artifact or total-byte capacity.

This test is essential because it proves that preflight cannot and need not predict arbitrary adapter output in order to preserve lifecycle safety.

If practical, cover both warmup and measured-trial staging paths. At minimum one must be a measured entered trial and one should cover warmup to guard both call sites.

## 10. Finalization-phase semantics

Choose one truthful definition and document it.

Preferred approach:

- redefine `PhaseKind::Finalization` as **runner evidence staging before immutable bundle publication**;
- finish the finalization event once;
- stage `runner-phases.json` after that event is terminal;
- call `writer.finalize()` after the phase artifact is fixed;
- do not mutate `RunOutcome.phases` after its persisted representation is staged;
- document that immutable publication itself is not representable inside the bundle it publishes.

This avoids recursive/self-referential evidence.

Alternative: split publication into a non-persisted caller-side operation with an explicitly different name. Do not claim the persisted phase includes its own publication.

The core invariant is:

~~~text
decode(runner-phases.json) == RunOutcome.phases
~~~

for every successfully finalized run, modulo no hidden mutation after serialization.

## 11. Finalization failure semantics

A failure in `writer.finalize()` occurs after drain/teardown in the current normal order, so no additional process cleanup should be needed.

However:

- return the finalization/evidence error truthfully;
- do not return a `RunOutcome` with a finalized bundle path/manifest;
- do not mutate the already-staged phase timeline after serialization;
- retain existing incomplete/staging-directory semantics;
- if a cleanup failure already occurred earlier, preserve it as context without replacing the finalization error if finalization prevents completion of the API contract.

Add a focused fault test where finalization fails if BundleWriter offers a deterministic way to induce it without platform-fragile filesystem races. If no stable injection seam exists, document this as reviewed behavior rather than adding a large test-only abstraction.

## 12. Public error/outcome contract

The current distinction remains sound:

- operational experiment failures with truthful finalized evidence return `RunOutcome`;
- failures that prevent truthful evidence completion return `OrchestrationError`.

The corrective must strengthen the second case so an `OrchestrationError` after startup still carries cleanup evidence.

Do not convert a bundle-staging failure into an ordinary failed run merely to avoid an error return; if the evidence contract cannot be finalized, the API must say so.

## 13. Phase timeline consistency regression

For a successful run:

1. deserialize `runner-phases.json`;
2. compare it exactly with `RunOutcome.phases`;
3. assert every persisted event is terminal;
4. assert there is exactly one finalization-phase event;
5. assert the event is finished exactly once.

If serialization-friendly type equality is awkward, add the necessary derives or compare normalized DTOs. Do not use string formatting as the canonical equality check.

## 14. M002 closure disposition

Do not rewrite `plans/closure/local-runner-lifecycle/002-status.md`.

The corrective closure record must explicitly state:

- M002's original closure was based on the available verification;
- the post-closure audit found an untested post-start evidence-error path and finalization-event mismatch;
- this corrective restores the original M002 invariants.

## 15. Dependency graph

~~~text
M002 historical closure
        |
        v
M002 post-closure corrective C001
        |
        +--> M003 plan authoring/implementation
        |
        +--> Measurement/Comparison M001 plan authoring/implementation
~~~

External Oracles M001 may remain independently plan-authorable, but should not consume the corrected orchestration path in implementation until this corrective closes.

## 16. Milestone

### C001 — Evidence-error cleanup and finalization timeline truthfulness

Class: invariant/correctness corrective.

Implementation plan:

- `plans/implementation/local-runner-m002-post-closure-corrective/001-evidence-error-cleanup-and-finalization-timeline.md`

Status: ready.

Exit conditions:

- no post-start evidence staging error can bypass drain/teardown;
- adversarial workload artifact tests prove cleanup;
- original evidence error remains primary with cleanup diagnostics attached;
- persisted `runner-phases.json` exactly matches returned phase state;
- finalization-phase semantics are documented without recursive publication claims;
- existing M001/M002 tests and hosted qualification remain green.

## 17. Verification strategy

Focused:

- unsafe dynamic workload artifact after measured invocation;
- too many dynamic workload artifacts;
- dynamic bundle byte-bound overflow after preflight passes;
- warmup staging failure;
- evidence failure + drain failure;
- evidence failure + teardown failure;
- primary evidence error preserved;
- no descendant process left alive;
- phase timeline persisted/returned equality;
- one terminal finalization event;
- successful bundle verification unchanged.

Broad:

- format;
- check;
- Clippy all targets/features;
- full workspace tests;
- Rust 1.89 check;
- cargo tree;
- diff check;
- hosted Linux/macOS/Windows supported-subset CI.

## 18. Completion definition

This corrective closes when evidence production failures are lifecycle-safe and phase evidence is self-consistent, restoring confidence that every M002 post-start terminal path obeys mandatory cleanup before M003 and Measurement/Comparison build on the runner.
