# Baselines

A baseline is an explicit immutable bundle reference, never a mutable
database or an automatic "latest successful run".

## Bundle identity

A bundle's stable identity is its manifest schema version, run id, and the
SHA-256 of the exact finalized `manifest.json` bytes (plus subject
revision/digest for presentation only). Two paths with the same run id but
different manifest digests are different evidence and are never treated as
aliases. Filesystem paths are not identity. Identity is derived only after
normal bundle verification.

## Explicit bundle reference

`eggbench compare <baseline.eggb> <candidate.eggb>` records the resolved
immutable identity of both bundles in the receipt, not just the paths.

## Human-managed alias file

`*.eggbaseline.json`:

```json
{
  "schema_version": 1,
  "alias": "good-baseline",
  "bundle_path": "baseline.eggb",
  "manifest_sha256": "<hex of manifest.json>",
  "note": "optional human note"
}
```

Resolution rules:

1. alias files are read explicitly by path; no global registry exists;
2. relative bundle paths resolve relative to the alias file;
3. the referenced bundle is opened and verified;
4. the computed manifest digest must equal the alias digest — mismatch fails
   closed with code 2 and touches nothing;
5. the receipt records what immutable identity the alias resolved to.

`eggbench compare --alias <baseline.eggbaseline.json> <candidate.eggb>`.

## Candidate-only comparison

`eggbench compare --absolute-only <candidate.eggb>` evaluates
candidate-only absolute gates with no baseline. Relative/statistical gates
resolve to invalid (`baseline_required`) rather than guessing.

## Environment policy

- `strict_same_testbed`: any comparison-critical testbed/workload/driver/
  topology mismatch makes baseline-dependent primary gates `Invalid`
  (descriptive estimates retained with mismatch provenance).
- `warn_on_mismatch`: mismatch suppresses gate verdicts to `descriptive`
  with explicit warnings; absolute candidate-only gates still evaluate.
- `cross_testbed_descriptive`: baseline effects are always `descriptive`,
  even when fingerprints match; no relative/statistical verdict is emitted.

Absolute candidate-only gates evaluate under every policy.

## Receipts

Comparison emits a standalone versioned JSON receipt (stdout in machine
mode, or `--output <comparison.json>`), never a mutation of either bundle.
The existing manifest `comparison_verdict` field stays reserved for future
run-time comparison before finalization.
