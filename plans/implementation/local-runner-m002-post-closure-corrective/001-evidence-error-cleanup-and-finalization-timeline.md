# Local Runner M002 Post-Closure Corrective C001 — Evidence-Error Cleanup and Finalization Timeline

Status: ready for handoff

Repository baseline: `02212d5d9f78816eec5ef1b89d45e99c0dbe02d5`

Source corrective:

- `plans/subsystems/local-runner-m002-post-closure-corrective-addendum.md`

Predecessor evidence:

- `plans/implementation/local-runner-lifecycle/002-warmup-trial-cooldown-reset-state-machine.md`
- `plans/closure/local-runner-lifecycle/002-status.md`

Controlling ADRs:

- `plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md`
- `plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md`
- `plans/adrs/ADR-0005-local-first-execution-and-remote-provider-boundary.md`

Primary class: invariant/correctness corrective.

## 1. Objective

Repair two narrow M002 defects without expanding runner scope:

1. ensure every evidence-staging error after managed startup still executes workload drain and managed teardown;
2. make persisted and returned finalization-phase evidence identical and semantically truthful.

No new workload protocol, metric, comparison, environment, CLI, or manifest semantics are introduced.

## 2. Defect reproduction

Current `execute_run()` contains post-start fallible staging calls such as:

~~~text
stage_warmup(...)?;
stage_trial(...)?;
~~~

These occur before the common drain/teardown tail.

A workload can therefore execute successfully and then return output that causes staging to fail. Examples:

- unsafe artifact name;
- too many workload artifacts;
- per-artifact bound exceeded;
- total bundle bound exceeded.

The propagated `?` returns `OrchestrationError::Evidence` immediately and can skip workload drain and `LocalSession::shutdown()`.

Current finalization flow also does:

~~~text
begin finalization
finish finalization
stage runner-phases.json
writer.finalize(...)
finish same in-memory finalization event again
~~~

The published phase file and returned phase vector can therefore disagree.

## 3. Required invariants

- After the first managed process starts, all terminal paths attempt required cleanup.
- Evidence staging failure cannot orphan runner-owned descendants.
- If any workload invocation began, workload drain is attempted before return.
- Managed teardown is attempted whenever startup created owned processes.
- Primary evidence failure is preserved when drain/teardown also fail.
- No failed evidence publication is represented as a valid finalized bundle.
- Persisted phase evidence equals returned phase evidence for successful finalization.
- A phase event is completed exactly once.
- Finalization terminology does not claim a bundle contains evidence of its own immutable publication.
- Measured timing semantics remain unchanged.
- Trial result schema v1 and manifest v2 remain unchanged unless a stop condition is hit.

## 4. Non-goals

Do not:

- redesign `execute_run` into a general workflow engine;
- add M003;
- add metric normalization/comparison;
- change workload/reset public semantics beyond what cleanup propagation requires;
- add a concrete network driver;
- change reset/cooldown ordering;
- add automatic artifact truncation;
- silently discard invalid workload artifacts and continue;
- catch panics as routine error handling.

## 5. Work package A — Common post-start cleanup path

Refactor orchestration so fallible evidence staging cannot return past cleanup.

Acceptable shapes include:

- accumulate a primary orchestration failure and break to one cleanup tail;
- a scoped execution state object whose final method performs drain/teardown;
- an internal helper wrapping the experimental phase body and then always executing cleanup.

Requirements:

1. determine whether startup created owned processes;
2. determine whether the workload executor was entered;
3. on evidence staging failure, stop further experimental phases;
4. if workload was entered, run bounded drain;
5. if managed processes started, run `session.shutdown()`;
6. preserve evidence error as primary;
7. collect drain/teardown diagnostics as secondary;
8. only then return `OrchestrationError`.

Do not duplicate drain/teardown code across every staging call.

## 6. Work package B — Error shape

Evolve `OrchestrationError` if needed so a post-start evidence error carries cleanup context.

A representative shape:

~~~text
Evidence {
  source: BundleError,
  drain_failure: Option<FailureCategory>,
  cleanup: Vec<CleanupFailure>
}
~~~

Exact fields may differ.

Requirements:

- `Display` remains redaction-safe;
- source `BundleError` remains inspectable;
- cleanup failure does not replace source;
- callers can distinguish preflight failure from evidence/finalization failure;
- do not store secret-bearing workload output in the error.

If a finalization error occurs after successful drain/teardown, its cleanup context may be empty.

## 7. Work package C — Adversarial workload artifact support

Extend test support with a workload that can return controlled `WorkloadArtifact` values, or extend `FakeWorkload` minimally.

Qualification cases:

### C1 — Unsafe name after measured invocation

Return:

~~~text
name = "../escape"
bytes = small
~~~

Assert:

- measured invocation was entered;
- evidence error is returned;
- drain was called;
- teardown ran;
- no managed process remains;
- error source is path/manifest safety related.

### C2 — Artifact-count overflow

Return more than `MAX_WORKLOAD_ARTIFACTS_PER_INVOCATION`.

Assert the same cleanup guarantees.

### C3 — Runtime byte-bound overflow

Construct a BundleWriter whose mandatory preflight capacity succeeds. Return a dynamic workload artifact large enough to exceed the writer's remaining capacity.

This must demonstrate:

- preflight succeeds;
- services start;
- workload executes;
- staging fails;
- cleanup still occurs.

### C4 — Warmup staging failure

Cause a warmup output staging failure and assert no measured trial begins, drain runs, and teardown completes.

## 8. Work package D — Cleanup-failure composition

Add focused injected failures:

- evidence staging failure + drain failure;
- evidence staging failure + teardown failure;
- evidence staging failure + both cleanup failures if practical.

Acceptance:

- evidence error remains primary;
- secondary cleanup failures remain observable;
- teardown is still attempted even if drain fails;
- no later experimental phase runs.

Reuse existing cleanup-failure test adapters where possible.

## 9. Work package E — Finalization phase semantics

Adopt the corrective addendum's preferred semantics unless implementation evidence forces review:

`PhaseKind::Finalization` means **runner evidence staging prior to immutable bundle publication**.

Implementation order:

1. begin finalization event;
2. complete all runner-owned evidence preparation needed before publication;
3. finish finalization event exactly once;
4. serialize/stage `runner-phases.json`;
5. do not mutate `phases` afterward;
6. call `writer.finalize()`;
7. return the same phase vector that was serialized.

Update docs/comments so publication itself is not claimed to be measurable from inside the published bundle.

Do not add a second persisted publication event that would create the same recursion problem.

## 10. Work package F — Phase equality regression

On successful run:

- read `runner-phases.json`;
- deserialize to `Vec<PhaseEvent>`;
- assert exact equality with `RunOutcome.phases`;
- assert every event has `outcome.is_some()`;
- assert exactly one `PhaseKind::Finalization`;
- assert finalization has one terminal duration/outcome;
- assert no mutation happens after phase serialization.

If `PhaseEvent` lacks traits needed for direct equality/deserialize tests, add only the minimal derives.

## 11. Work package G — Documentation and historical disposition

Update:

- `docs/trial-orchestration.md`;
- `architecture/runner.md` if finalization semantics are described there;
- runner API docs around `execute_run`;
- registry/roadmap after closure.

Do not edit `plans/closure/local-runner-lifecycle/002-status.md`.

The corrective closure must reference the historical M002 closure and explain the newly discovered missed verification cases.

## 12. Failure semantics after corrective

### Evidence staging failure before workload entry

If no workload was entered, drain may be skipped. If managed processes started, teardown is mandatory.

### Evidence staging failure after workload entry

Drain mandatory, teardown mandatory.

### Drain failure after evidence failure

Evidence error remains primary; drain failure is secondary; teardown still mandatory.

### Teardown failure after evidence failure

Evidence error remains primary; teardown failure is secondary.

### Bundle finalization failure

At this point experimental work, drain, and teardown should already be complete. Return finalization/evidence error with no fabricated `RunOutcome`.

## 13. Evidence staging directory semantics

Do not attempt to turn a failed staging tree into a valid bundle.

Preserve existing BundleWriter semantics:

- unpublished staging directory remains incomplete;
- BundleReader does not treat it as completed evidence;
- no copy fallback;
- no repair/mutation in this corrective.

Tests may inspect that the final path was not published.

## 14. Focused regression tests

Required:

- measured unsafe artifact name -> drain + teardown;
- measured artifact-count overflow -> drain + teardown;
- measured dynamic byte overflow after preflight -> drain + teardown;
- warmup staging error -> no measured trial + drain + teardown;
- evidence error + drain failure -> teardown still runs;
- evidence error + teardown failure -> primary evidence cause retained;
- no descendant remains after evidence error;
- final output path absent for non-finalized bundle;
- persisted phase vector equals returned phase vector;
- one finalization event, terminal exactly once;
- successful M002 bundle still verifies.

## 15. Existing regressions that must remain green

- M001 dependency ordering/readiness;
- cwd symlink confinement;
- hermetic environment;
- explicit executable paths;
- Linux/macOS process-group cleanup;
- cancellation in all M002 phases;
- workload/reset/drain timeout cases;
- evidence capacity preflight;
- trial ordering/warmup separation;
- manifest v2 status/verdict semantics;
- Windows supported subset.

## 16. Cross-platform verification

Linux and macOS must run the adversarial evidence-error tests with a real managed fixture process so cleanup is demonstrated against actual process ownership.

Windows does not support managed execution; it must compile/check the changed orchestration/error code and retain explicit unsupported behavior.

No new platform capability is introduced.

## 17. Broad verification

Required before closure:

    cargo fmt --all -- --check
    cargo check --workspace --all-targets --locked
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    cargo test --workspace --all-features --locked
    cargo +1.89.0 check --workspace --all-targets --locked
    cargo tree --locked
    git diff --check

Hosted CI:

- Linux stable;
- Linux Rust 1.89;
- macOS stable;
- Windows supported subset.

Record workflow run ID in closure.

## 18. Acceptance criteria

C001 closes only when:

1. no post-start `BundleError` path can bypass mandatory managed teardown;
2. any post-workload evidence error also attempts workload drain;
3. drain failure cannot prevent service teardown;
4. primary evidence error is preserved with secondary cleanup diagnostics;
5. adversarial dynamic workload artifacts prove the behavior;
6. failed evidence staging does not publish a valid final bundle;
7. `runner-phases.json` exactly equals returned `RunOutcome.phases` on successful runs;
8. finalization phase is terminalized once and its documented meaning is truthful;
9. measurement windows and all existing M002 behavior remain unchanged;
10. hosted qualification is green with no unresolved lifecycle/security finding.

Closing this corrective re-unblocks:

- Local Runner M003 plan authoring/implementation;
- Measurement/Comparison M001 plan authoring/implementation.

## 19. Stop conditions

Stop for planning review if:

- fixing cleanup requires changing the `WorkloadExecutor` ownership model materially;
- BundleWriter cannot expose necessary failure context without a manifest/schema change;
- a truthful phase timeline requires manifest self-reference or recursive publication;
- M002 timing boundaries must change;
- the fix introduces a new runtime/network dependency.

## 20. Closure evidence required

Record:

- implementation commit(s);
- exact control-flow change used to guarantee cleanup;
- public error shape;
- adversarial artifact regression matrix;
- proof of drain/teardown attempts;
- descendant cleanup evidence;
- persisted-vs-returned phase equality evidence;
- successful bundle verification;
- full test counts;
- dependency tree;
- MSRV result;
- hosted CI run ID;
- documentation updates;
- known limitations;
- unresolved findings by severity;
- disposition.
