# Foundation Post-Closure Corrective C001 — Execution Status and Comparison Verdict Separation

Status: closed

Closure record: `plans/closure/foundation-experiment-evidence-post-closure-corrective/001-status.md`

Repository baseline: `319b5816604e39578af4e20f5880945123b042e1`

Source corrective:

- `plans/subsystems/foundation-experiment-evidence-post-closure-corrective-addendum.md`

Predecessor evidence:

- `plans/closure/foundation-experiment-evidence/003-status.md`
- `plans/closure/local-runner-lifecycle/001-status.md`

Controlling ADRs:

- `plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md`
- `plans/adrs/ADR-0003-trial-level-comparison-and-regression-verdicts.md`

Primary class: invariant/schema corrective.

## 1. Objective

Remove the semantic overload in `RunStatus` before real trial and comparison data is added.

New evidence must represent execution/lifecycle outcome separately from comparison verdict. Existing manifest-v1 bundles must remain explicitly readable without being silently reinterpreted as stronger evidence.

## 2. Corrective trigger

The current bundle contract has:

~~~text
RunStatus =
  Succeeded
  Failed
  Cancelled
  Invalid
  Inconclusive
~~~

and `BundleManifest.status: RunStatus`.

This conflates execution state with comparison result. The concrete symptom is Local Runner M001: a successful lifecycle-only run with zero measured trials is stored as `Inconclusive` solely to avoid calling it a performance success.

The canonical terminology does not support that conflation.

## 3. Required invariants

1. Execution status and comparison verdict are independent types.
2. Successful lifecycle completion does not imply comparison pass.
3. Absence of comparison is represented as absence, not fabricated inconclusive.
4. Operational failure is not comparison failure.
5. Existing manifest-v1 evidence remains readable.
6. New writes use the corrected schema only.
7. Completed bundles remain immutable.
8. No statistical algorithm is added.
9. Existing path confinement, digest verification, staging, and finalization behavior do not regress.
10. `eggbench-core` remains runtime/network independent.

## 4. Expected schema changes

Introduce an execution-status type with semantics equivalent to:

~~~text
completed
failed
cancelled
invalid
~~~

Introduce a comparison-verdict type with semantics equivalent to:

~~~text
pass
fail
inconclusive
invalid
~~~

The exact type names may differ, but public names must be explicit and unambiguous.

For the new manifest schema, replace the overloaded status representation with:

- required execution status;
- optional comparison verdict or a future-proof typed comparison-summary slot;
- existing optional comparison artifact reference.

If comparison verdict is stored directly in the manifest now, validation must require coherent relationship with the comparison artifact. If the implementation prefers verdict to live only in a typed `comparison.json`, the manifest still needs a non-ambiguous way to represent “comparison not performed” and must not reuse execution status for it.

Document the chosen relationship.

## 5. Manifest versioning

Bump the evidence manifest schema for new writes.

Preferred implementation:

- retain a small private/public legacy DTO for manifest v1;
- detect schema version before deserializing into the current manifest;
- verify v1 artifacts using the same safe path and digest machinery;
- expose legacy status explicitly to callers or normalize only with a visible legacy marker;
- never mutate/rewrite v1 on read;
- emit only the corrected current schema from `BundleWriter`.

Do not weaken the current additive-field policy accidentally.

## 6. Legacy mapping constraints

Legacy mapping is necessarily lossy for at least some v1 values.

Examples:

- v1 `Succeeded` can safely mean execution completed, but says nothing about comparison;
- v1 `Inconclusive` might represent a comparison verdict or the Local Runner M001 “no comparison performed” workaround;
- v1 `Failed` might be interpreted as execution failure, but future comparison failure must not reuse it.

Therefore do not map v1 `Inconclusive` directly to current comparison `Inconclusive` without legacy provenance that states the ambiguity.

A dedicated `LegacyManifestV1`/legacy status accessor is preferable to inventing precision.

## 7. BundleWriter behavior

Update finalization API so call sites provide execution status separately from any comparison verdict/summary.

Reject contradictory combinations, including any the selected representation makes impossible by construction.

Examples to enforce:

- completed + no comparison: valid;
- completed + comparison pass/fail/inconclusive: valid when comparison evidence exists according to schema;
- failed + comparison pass: invalid;
- cancelled + comparison verdict: normally invalid;
- comparison artifact with no required verdict/typed summary: invalid once that relationship is part of the current schema.

Keep failed/preflight bundles with zero trials representable.

## 8. Local Runner call-site migration

Update the Local Runner M001 zero-trial evidence test and bundle staging call sites.

The resulting assertion must be semantically equivalent to:

~~~text
execution_status == completed
comparison == none
trials == []
~~~

The test must also assert that no API or serialization path renders that as a performance pass.

Do not introduce a runner-local comparison concept.

## 9. Fixtures

Preserve the existing checked-in v1 `.eggb` fixture as legacy compatibility evidence.

Add a new current-version fixture, preferably including:

- completed execution;
- at least one trial if useful;
- no comparison in one fixture;
- comparison-bearing current fixture if the comparison DTO is already sufficiently typed.

Do not alter the old fixture in place and call it v2.

## 10. API compatibility

The crate is pre-release, so source compatibility is secondary to semantic correctness.

However:

- deprecated aliases are acceptable only if they cannot perpetuate ambiguous writes;
- do not retain a public `RunStatus` alias whose variants continue to mix the two domains;
- callers must migrate deliberately.

If a temporary compatibility accessor is added, mark it legacy/read-only and document removal criteria.

## 11. Documentation

Update:

- `architecture/evidence.md`;
- `docs/evidence-bundle.md`;
- `docs/local-runner-lifecycle.md`;
- `README.md` if it refers to run status/verdict semantics.

Add a short compatibility table explaining manifest v1 versus current manifest.

## 12. Tests

At minimum add/adjust tests for:

- current completed/no-comparison bundle;
- current failed/no-comparison bundle;
- current cancelled/no-comparison bundle;
- current invalid execution bundle;
- comparison pass/fail/inconclusive representation where supported;
- contradictory status/verdict combinations;
- legacy v1 fixture open and full digest verification;
- v1 `Inconclusive` retained as explicitly ambiguous legacy state;
- current manifest unsupported future version;
- existing missing/wrong-size/wrong-digest/extra-file/path/symlink tests;
- Local Runner lifecycle-only evidence uses no comparison verdict.

## 13. Verification

Required before closure:

    cargo fmt --all -- --check
    cargo check --workspace --all-targets --locked
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    cargo test --workspace --all-features --locked
    cargo +1.89.0 check --workspace --all-targets --locked
    cargo tree --locked
    git diff --check

Record exact test counts in closure.

## 14. Acceptance

C001 closes when:

- no current write API uses the overloaded `RunStatus`;
- new manifests separate execution status from comparison semantics;
- zero-trial lifecycle completion is represented without a fabricated verdict;
- old v1 evidence remains explicitly readable and verifiable;
- Local Runner compiles/tests against the corrected contract;
- all M003 evidence-safety guarantees remain green.

Closing C001 unblocks the Local Runner post-closure corrective.

## 15. Stop conditions

Stop and update the corrective/ADR rather than improvising if:

- preserving v1 reading would require mutating completed bundles;
- comparison semantics must be implemented to separate the types;
- a proposed compatibility alias allows new ambiguous writes;
- the change would require moving evidence/runtime ownership out of `eggbench-core`.

## 16. Closure evidence required

Record:

- implementation commit;
- current manifest schema version;
- legacy/current fixture paths;
- status/verdict type definitions;
- legacy mapping/ambiguity policy;
- Local Runner migration evidence;
- corruption/path-safety regression matrix;
- MSRV and dependency-tree results;
- known limitations;
- disposition.
