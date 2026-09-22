# Foundation Experiment and Evidence — Post-Closure Status Semantics Corrective Addendum

Status: active

Repository audit baseline: `319b5816604e39578af4e20f5880945123b042e1`

Predecessor work:

- `plans/subsystems/foundation-experiment-evidence-roadmap.md` — M001-M003 closed.
- `plans/closure/foundation-experiment-evidence/001-status.md`
- `plans/closure/foundation-experiment-evidence/002-status.md`
- `plans/closure/foundation-experiment-evidence/003-status.md`
- `plans/subsystems/local-runner-lifecycle-roadmap.md` — M001 closed against the foundation bundle contract.

Long-term references:

- `plans/000-long-term-specification.md#5-canonical-execution-model`
- `plans/000-long-term-specification.md#10-statistical-comparison`
- `plans/000-long-term-specification.md#12-evidence-bundle`
- `plans/001-terminology-and-domain-model.md#4-run`
- `plans/001-terminology-and-domain-model.md#34-verdict`
- `plans/001-terminology-and-domain-model.md#35-invalid-run`
- `plans/003-planning-process.md`

Related ADRs:

- `plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md`
- `plans/adrs/ADR-0003-trial-level-comparison-and-regression-verdicts.md`

## 1. Purpose and corrective trigger

Foundation M003 correctly established immutable evidence bundles, but the first manifest contract conflates two independent concepts in one `RunStatus` enum:

- execution/lifecycle state: whether the run completed, failed operationally, was cancelled, or is invalid as measurement evidence;
- comparison verdict: whether completed evidence passes, fails, is inconclusive, or is invalid under a comparison/gate policy.

The ambiguity is now observable in production code. `RunStatus` contains `Succeeded`, `Failed`, `Cancelled`, `Invalid`, and `Inconclusive`. Local Runner M001 therefore finalizes a successful zero-trial lifecycle-only bundle as `Inconclusive` so it cannot be mistaken for a performance pass.

That representation conflicts with the canonical terminology: a run has an execution outcome, while a comparison has a verdict. A lifecycle-only run can complete successfully without having any comparison verdict at all.

This should be corrected before Local Runner M002 starts creating real trial evidence and before the measurement/comparison workstream makes the ambiguity part of additional schemas.

The predecessor M001-M003 closures remain historical evidence. This addendum does not rewrite them.

## 2. Work classification

### Invariants

- Execution/lifecycle outcome and comparison verdict are separate typed concepts.
- A completed run does not imply a performance pass.
- Absence of a comparison is representable without fabricating `inconclusive`.
- Comparison `fail` is not confused with process/runtime failure.
- Invalid measurement/comparability evidence remains distinguishable from operational failure.
- Completed evidence remains immutable.
- Historical manifest-v1 evidence remains readable or receives an explicit bounded legacy reader; it is never silently reinterpreted as stronger evidence.
- New bundle writes use one unambiguous schema contract.
- Trial remains the default statistical comparison unit.
- No comparison statistics are implemented by this corrective.

### Capability

- Inspect a bundle and determine independently:
  - whether execution completed;
  - whether a comparison exists;
  - if it exists, the comparison verdict.

### Infrastructure

- separated status/verdict enums;
- evidence manifest schema revision;
- bounded manifest-v1 compatibility;
- runner call-site migration;
- checked-in legacy/current fixtures.

### Polish

- documentation and diagnostics use “execution status” and “comparison verdict” consistently.

## 3. Non-goals

Do not:

- implement bootstrap comparison;
- define final metric aggregation rules beyond the existing ADR;
- add baseline selection;
- create trial scheduling;
- change testbed comparability policy;
- add a database/index;
- rewrite predecessor closure records to hide the v1 design;
- infer a comparison verdict from execution status.

## 4. Current implementation evidence

At the audit baseline:

- `crates/eggbench-core/src/evidence.rs` defines `RunStatus::{Succeeded, Failed, Cancelled, Invalid, Inconclusive}`.
- `BundleManifest` has one `status: RunStatus` field and no independent comparison verdict.
- `BundleManifest` already has an optional `comparison` artifact reference, demonstrating that comparison is structurally optional.
- Foundation M003 tests can finalize `RunStatus::Succeeded` bundles with zero trials.
- Local Runner M001 deliberately finalizes a zero-trial lifecycle-only bundle as `RunStatus::Inconclusive`.
- `docs/local-runner-lifecycle.md` says that lifecycle-only evidence is “inconclusive, never a performance pass.”
- The canonical terminology separately defines Run and Verdict.
- No released Eggbench compatibility contract exists yet, but the repository already contains checked-in manifest-v1 evidence and the long-term architecture requires historical completed evidence to remain readable.

## 5. Corrective target model

The target conceptual model is:

~~~text
Run
  execution_status:
    completed
    failed
    cancelled
    invalid

  comparison:
    none
    or {
      policy_id
      verdict:
        pass
        fail
        inconclusive
        invalid
      artifact/reference...
    }
~~~

The exact Rust names may be `ExecutionStatus`, `ComparisonVerdict`, and `Option<ComparisonVerdict>` or a typed comparison summary. The implementation MUST preserve the semantic separation even if names differ.

A zero-trial Local Runner M001 lifecycle bundle should become:

~~~text
execution_status = completed
comparison_verdict = none
trials = []
~~~

It must not claim `pass`, `fail`, or `inconclusive` merely because no comparison was performed.

## 6. Manifest compatibility policy

The corrective SHOULD create a new evidence-manifest schema version for new writes because the meaning of the existing `status` field changes materially.

Preferred compatibility shape:

1. New writers emit manifest v2 only.
2. `BundleReader` can still identify and parse manifest v1.
3. The v1 representation remains explicit as legacy evidence; do not erase the original `RunStatus`.
4. If an internal normalized view is provided, legacy mapping must retain an explicit “legacy/ambiguous” marker where information cannot be recovered exactly.
5. The checked-in v1 synthetic bundle remains a compatibility fixture.
6. Add a checked-in v2 synthetic bundle exercising the separated fields.
7. Verification of digests/path safety remains identical across versions.

Do not silently rewrite a v1 bundle on open.

If implementation can preserve v1 reading only by maintaining a small dedicated legacy DTO rather than complicating the current manifest type, prefer the dedicated legacy DTO.

## 7. Execution-status semantics

Define execution status narrowly:

- **completed** — the planned execution lifecycle completed sufficiently to finalize evidence; this does not imply a comparison result;
- **failed** — an operational/runtime phase failed;
- **cancelled** — the run was cancelled;
- **invalid** — execution produced evidence that cannot be treated as a valid run according to a required measurement/preflight contract.

The implementation plan may refine whether certain preflight failures are “failed” versus “invalid,” but the distinction must be documented and tested.

Do not add `inconclusive` to execution status.

## 8. Comparison-verdict semantics

Define comparison verdict independently:

- **pass**;
- **fail**;
- **inconclusive**;
- **invalid**.

This corrective creates the type/schema slot only. ADR-0003 continues to control how a later comparison algorithm chooses the value.

A manifest with no comparison artifact normally has no comparison verdict.

A manifest with a comparison artifact must have a verdict once the comparison schema is implemented, unless a clearly versioned future partial-comparison state is added.

## 9. Aggregate/report implications

Reports and CLI presentation must eventually display these independently, for example:

~~~text
execution: completed
comparison: not performed
~~~

or:

~~~text
execution: completed
comparison: fail
~~~

The corrective need not implement the final CLI, but DTO/Debug/JSON terminology must not continue to call execution completion a performance success.

## 10. Local Runner migration

Update Local Runner M001 evidence tests and helper APIs to use the new execution-status contract.

Specifically:

- lifecycle-only finalized evidence records execution completed;
- it records no comparison verdict;
- tests explicitly prove “completed without comparison” is not equivalent to performance pass;
- runner errors/cancellation map only to execution status, not comparison verdict;
- no runner-local duplicate verdict enum is introduced.

## 11. Documentation and historical evidence

Update:

- `architecture/evidence.md`;
- `docs/evidence-bundle.md`;
- `docs/local-runner-lifecycle.md`;
- relevant README wording if needed.

Do not rewrite predecessor closure records to make the old contract appear not to have existed.

Instead, the corrective closure record must state that manifest v1/RunStatus is historical prerelease evidence superseded for new writes by the corrected contract.

## 12. Dependency graph

~~~text
C001 status/verdict separation
   |
   +--> Local Runner post-closure corrective
   |
   +--> Local Runner M002 plan/implementation
   |
   --> Measurement/Comparison M001+
~~~

Local Runner M002 must remain blocked until C001 closes.

The Local Runner M001 filesystem/platform corrective may be prewritten now but should not be implemented concurrently if it edits the same lifecycle evidence tests.

## 13. Milestone

### C001 — Execution status and comparison verdict separation

Class: invariant/schema corrective.

Implementation plan:

- `plans/implementation/foundation-experiment-evidence-post-closure-corrective/001-execution-status-and-comparison-verdict-separation.md`

Status: ready.

Exit conditions:

- new evidence writes use separated execution/comparison semantics;
- zero-trial lifecycle evidence is “completed, no comparison”;
- manifest-v1 compatibility is retained explicitly;
- current and legacy fixture tests pass;
- no comparison algorithm has been smuggled into core;
- Local Runner compiles/tests against the corrected contract.

## 14. Verification strategy

At minimum:

- current manifest v2 round trip;
- legacy manifest v1 open/verify;
- completed/no-comparison bundle;
- completed/pass comparison-shaped fixture if comparison DTO exists;
- completed/fail comparison-shaped fixture;
- failed execution/no-comparison;
- cancelled execution/no-comparison;
- invalid execution semantics;
- impossible/contradictory field combinations rejected;
- existing corruption/path/digest tests remain green;
- runner zero-trial lifecycle test updated.

Broad verification remains the repository standard: format, check, Clippy, all-features tests, Rust 1.89 check, cargo tree, and diff check.

## 15. Completion definition

This corrective closes when Eggbench evidence can answer “did execution complete?” independently from “did the candidate pass comparison?” and when all new bundle writes use that distinction without breaking explicit read access to historical v1 evidence.

## 16. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| C001 | ready | `plans/implementation/foundation-experiment-evidence-post-closure-corrective/001-execution-status-and-comparison-verdict-separation.md` | none | none |
