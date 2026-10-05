---
name: eggbench-contract-versioning
description: |
  Change a serialized contract in eggbench — a plan, resolved plan, bundle
  manifest, trial result, comparison receipt, environment fingerprint, or
  telemetry/qualification schema — without breaking evidence already on disk.
  Use when a task adds or renames a plan field, changes what an existing field
  means, introduces a new schema version, or touches a version constant.
  Triggers on "bump the schema version", "add a field to the plan", "schema v11",
  "v4+ rejects", "deny_unknown_fields", "readable versions", "backward
  compatible", "what breaks for existing bundles". Do not use for changing
  driver behavior that adds no serialized field, for pure documentation audits
  (use `eggbench-docs-truth`), or for non-serialized internals such as process
  lifecycle or metrics arithmetic.
---

# Eggbench contract versioning

## Inputs to collect

- The contract being changed, and whether the change alters *interpretation* of
  an existing field or only adds a new one. The two take different paths.
- Whether any `.eggb` bundle, comparison receipt, or baseline alias already
  exists that must stay readable. Immutable evidence on disk is the reason this
  discipline exists, so "nobody has run it yet" needs confirming, not assuming.
- The plan schema version the change targets, read from the code, not from a
  document.

## When a bump is required

The governing rule, stated in `architecture/overview.md`: a version bump is
required when a field changes interpretation. Additive top-level fields stay
backward compatible; nested and variant fields stay strict. In practice:

- Adding a new optional top-level field to a manifest-like structure, keeping
  its meaning, does not require a bump and should not get one — a gratuitous
  bump rejects evidence that was valid.
- Adding a field whose absence would change how a reader interprets the rest of
  the document, repurposing a field, or changing a variant's contents does
  require a bump plus a readable-range extension.
- Plan schemas are the exception that proves the rule: `deny_unknown_fields`
  means each plan version is a closed set, so a new field that only some
  versions understand is expressed as a per-field version gate rather than
  silently accepted.

## Procedure: adding a plan schema version

1. Add the constant next to its siblings in `crates/eggbench-core/src/lib.rs`.
   The existing chain is `EXPERIMENT_PLAN_SCHEMA_VERSION` (the current version,
   unsuffixed) followed by `_2` through `_10`; follow that shape so the current
   version is greppable without a number.

2. Extend the accepted-version chain in the plan validator. The check is an
   explicit `&&` chain of inequality comparisons, not a range, because a range
   would silently accept a version that was never defined.

3. Add the version to each per-field gate that should now accept the new field.
   Gates are also explicit inequality chains. Getting this wrong produces the
   worst failure mode in this system: the field parses, then a later rule
   rejects the combination, so the error message points at the wrong cause.

4. Decide the cross-version rejection rules explicitly, in both directions. A
   field belonging to v7+ must be rejected by v6, and the pre-existing fields
   of v6 must be rejected by v7 if the design intends v7 to be a replacement
   rather than a superset. `docs/experiment-plan.md` records the per-version
   rules and is the contract other agents read.

5. Add tests in all three directions: a v(N+1) plan validates, a v(N) plan with
   the new field fails to parse, and a v(N+1) plan carrying a field owned by an
   older version fails validation. A test that only exercises the happy path
   leaves the gate unverified — the repository's test suite is predominantly
   positive-path, which is exactly how the earlier schema-versus-prose drift
   went unnoticed.

## Procedure: changing a reader-owned contract

`ResolvedPlan`, the bundle manifest, and the comparison receipt each keep a
legacy constant per readable version and a conversion into the current view:

- Resolved plan: `RESOLVED_PLAN_SCHEMA_VERSION` is current, `_5` through `_1`
  are the readable set, and validation accepts current plus each of them.
- Bundle manifest: `EVIDENCE_MANIFEST_SCHEMA_VERSION` is current;
  `LegacyManifestV1` plus `into_current_view` is the read path for v1.
- Comparison receipt: `COMPARISON_RECEIPT_SCHEMA_VERSION` is current, `_3`
  through `_1` are readable.

Adding a version means extending the constant set, extending the accept branch,
and giving the new legacy shape a conversion that fills defaults explicitly
rather than deriving them. Then add a test that reads a fixture of the older
version and asserts the converted view. Note that neither manifest struct
declares `deny_unknown_fields`, so top-level manifest tolerance is
deliberately blanket; a nested or variant field added under that tolerance is
still strict, and that asymmetry is a live open decision rather than a settled
rule.

## Finish the change in the same pass

The largest single class of defect found in this repository's deep-dive audit
was a version bump that was verified against the code while the document stating
the version was left alone — `ResolvedPlan` v3 to v6, `TrialExecutionResult` v1
to v2, `ComparisonReceipt` v2 to v4. The code was right and the prose was
wrong, repeatedly. Treat the documentation edit as part of the change:

- `docs/experiment-plan.md` for plan versions and per-version rules.
- `docs/evidence-bundle.md` and `docs/comparison.md` for their receipts.
- `architecture/core-plan-schema.md`, `architecture/core-evidence-bundle.md`,
  `architecture/core-comparison.md` for the deep dives and their `file.rs:LINE`
  citations.
- `AGENTS.md` if the rule you implement is one a future change would trip on.

Then run `python3 scripts/verify-architecture-docs.py` to confirm the citations
still resolve, and the gate skill for the build and test matrix.

## Failure handling

- If the change alters bytes that land inside a bundle, say so explicitly. The
  manifest is manifest-last and digests are recorded, so a new provenance field
  changes bundle digests and is a comparability decision, not a silent
  improvement. A precedent: deriving a provenance label that had been
  hand-written is safe, adding a previously unrecorded crate is not.
- If a version constant has no reader, a bump is incomplete — an unreadable
  previous version is evidence loss, not backward compatibility.
- If a test for the rejection direction is hard to write, that difficulty is
  the finding. Do not drop the direction and ship the permissive direction.

## Examples

Input: "Add a `retention_hint` string to the telemetry config in the plan."
Output: decide bump vs additive from the rule above, add the gate, add both
rejection tests, then update `docs/experiment-plan.md` and
`architecture/core-plan-schema.md` in the same commit.

## Windows (win32) platform notes

The gates are `cargo` and `python` invocations and are platform-independent.
The one behaviour that differs is that a managed `run` cannot execute on
Windows, so a contract change that needs live evidence to be proven must be
qualified on Linux; the Windows lane still catches compile, lint, and
`eggstack_path` test breakage.
