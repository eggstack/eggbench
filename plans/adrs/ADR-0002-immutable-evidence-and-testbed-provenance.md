# ADR-0002: Immutable Evidence and Testbed Provenance

Status: accepted

Date: 2026-09-22

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#47-testbed-identity-is-first-class`
- `plans/000-long-term-specification.md#48-evidence-is-append-only`
- `plans/000-long-term-specification.md#12-evidence-bundle`
- `plans/000-long-term-specification.md#13-environment-and-testbed-model`

Affected subsystem roadmaps:

- `plans/subsystems/foundation-experiment-evidence-roadmap.md`
- `plans/subsystems/local-runner-lifecycle-roadmap.md`
- `plans/subsystems/measurement-comparison-roadmap.md`

## Context

Performance numbers without environment and input provenance are easy to misuse. Existing Eggstack performance campaigns already treat host/toolchain/revision differences as material, and SynVoid's recent work demonstrated how benchmark methodology can accidentally measure runtime setup instead of the intended hot path.

Eggbench needs durable evidence that can be audited later without requiring a central database.

## Decision drivers

- portable evidence;
- reproducibility;
- baseline integrity;
- no mandatory database/service;
- detection of unlike-testbed comparisons;
- interruption/corruption handling;
- bounded artifacts.

## Considered options

### Option A — Store only summarized JSON

Small, but discards raw histograms, logs, driver output, and provenance needed to investigate regressions.

Rejected.

### Option B — SQLite as canonical storage

Useful for indexing, but turns a mutable database into evidence authority and complicates portability/recovery.

Rejected as canonical format; MAY be added later as an index.

### Option C — Immutable directory bundle with manifest

Selected.

## Decision

Every finalized run SHALL produce one immutable evidence bundle.

The canonical initial form is a directory conventionally ending in `.eggb`.

The bundle SHALL include a versioned `manifest.json` containing:

- run ID;
- run status;
- schema versions;
- creation/finalization timestamps;
- subject identity;
- environment/testbed fingerprint identity;
- original/resolved plan artifact references;
- trial artifact references;
- comparison/report references where present;
- per-artifact size and SHA-256 digest;
- driver/version inventory;
- completion/finalization marker.

Evidence generation SHALL use a staging directory. A run is not finalized until required artifacts are flushed, digests are computed, the manifest is written last, and the staging directory is atomically renamed where the platform supports it.

Interrupted or partial staging directories SHALL be distinguishable from completed bundles.

Completed bundles SHALL NOT be mutated in place by ordinary Eggbench operations.

Named baselines or aliases MAY be mutable pointers, but comparisons SHALL record the immutable bundle identity/digest actually used.

The environment fingerprint SHALL distinguish:

- comparison-critical fields;
- warning-only fields;
- informational fields.

Same-testbed gating SHALL use a versioned comparability policy rather than requiring a byte-identical environment JSON document.

## Consequences

### Positive

- evidence survives without a service;
- bundles can be archived, copied, hashed, diffed, and inspected;
- a later index/dashboard does not become authority;
- interrupted writes are detectable;
- baseline aliases cannot rewrite historical comparison inputs.

### Negative

- bundles may consume more disk than summary-only results;
- large raw artifacts require retention bounds;
- schema readers must handle historical versions.

## Compatibility and migration

Additive fields should remain readable where safe. Incompatible manifest semantics require a major manifest schema version.

A future packaged single-file archive must be a transport encoding of the same logical bundle, not a new evidence model.

## Security and reliability

Secret-bearing environment variables, authorization headers, proxy credentials, tokens, and raw security payloads MUST be redacted or explicitly excluded according to driver policy.

Artifact paths must be relative, normalized, and confined to the bundle root.

Bundle readers must reject path traversal and digest mismatches.

## Verification

Conformance requires tests for:

- successful atomic finalization;
- interrupted staging;
- digest mismatch;
- manifest missing/duplicate artifacts;
- path traversal/symlink escape during inspection;
- secret redaction fixtures;
- immutable baseline reference recording.

## Supersession

None.
