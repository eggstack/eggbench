# Foundation Experiment and Evidence M003 — Immutable Evidence Bundle

Status: blocked

Repository planning baseline: 032eb324d299f865c989b59226e70eff28d77d09

Source roadmap:

- plans/subsystems/foundation-experiment-evidence-roadmap.md — M003

Dependencies:

- Foundation M001 closed;
- Foundation M002 resolved-plan interface stable or closed.

Controlling ADR:

- plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md

Primary class: infrastructure/capability foundation.

## 1. Objective

Implement the first portable, immutable .eggb directory-bundle contract, including manifest schema, artifact digests, safe staging/finalization, verification, and read-only inspection.

Use synthetic trial artifacts only. Do not add the full local runner in this milestone.

## 2. Required invariants

- a completed bundle has one authoritative finalized manifest;
- manifest is written after required artifacts;
- finalized bundles are not mutated by ordinary APIs;
- every retained artifact has bounded metadata and SHA-256 digest;
- all artifact paths are relative and bundle-confined;
- interrupted staging is not mistaken for completed evidence;
- bundle verification detects missing, extra-required, size, and digest failures;
- secret fixtures remain redacted;
- a database is not required.

## 3. Initial bundle shape

Support a logical structure equivalent to:

~~~text
<run>.eggb/
  manifest.json
  plan.toml
  resolved-plan.json
  environment.json
  topology.json
  subject.json
  trials/
    001/
      result.json
      telemetry.ndjson
      stdout.log
      stderr.log
      artifacts/
  comparison.json
  report.json
~~~

Synthetic M003 bundles need not populate every optional artifact.

The manifest, not directory naming convention, determines what is authoritative.

## 4. Manifest contract

At minimum record:

- manifest schema version;
- run ID;
- run terminal status;
- created/finalized times or placeholders appropriate to synthetic tests;
- ExperimentPlan artifact reference;
- ResolvedPlan artifact reference;
- environment artifact reference;
- zero or more trial descriptors;
- optional comparison/report descriptors;
- driver inventory;
- artifact table with relative path, logical role, media/type hint, byte size, SHA-256;
- finalization marker.

Avoid embedding large artifact contents in the manifest.

## 5. Run/trial identifiers

Introduce RunId and TrialId if M001 did not.

Trial identity must be stable within the bundle and independent of directory sort accidents.

Do not use OS process IDs.

## 6. Staging/finalization

Implement an API resembling:

- create staging bundle;
- write/register bounded artifacts;
- finalize required metadata;
- compute/verify hashes;
- flush;
- write manifest last;
- atomically rename staging to final path when supported.

If the destination already exists, fail rather than overwrite immutable evidence unless an explicit test-only or future maintenance API is used.

Cross-filesystem rename behavior must be explicit; do not silently copy a half-finalized bundle and call it atomic.

## 7. Bounds

Define conservative initial defaults for:

- artifact count;
- individual artifact size metadata;
- manifest size;
- path length/depth;
- log/telemetry registration counts.

The bundle API need not enforce all future runtime byte limits yet, but it must not accept unbounded in-memory blobs by design.

Prefer streaming file hashing/copying helpers.

## 8. Safe path handling

Reject:

- absolute artifact paths;
- parent traversal;
- normalized escape;
- empty path;
- manifest path collision;
- duplicate logical path;
- unsafe symlink escape when opening/verifying artifacts.

Inspection must not follow a symlink outside the bundle root.

## 9. Verification and inspection

Provide a read-only inspector that can:

- open manifest;
- validate schema;
- enumerate artifacts;
- verify hashes/sizes;
- report missing/corrupt artifacts;
- expose plan/resolved-plan/environment/trial metadata;
- distinguish incomplete staging from finalized evidence.

Inspection must not rewrite or repair the bundle automatically.

## 10. Environment placeholder

M003 does not implement full host fingerprinting, but the bundle schema must reserve a versioned EnvironmentFingerprint artifact contract usable by the local runner later.

Use synthetic fixture values in tests.

## 11. Comparison placeholder

Comparison is optional in M003.

The manifest may reference a future comparison.json, but do not implement statistical comparison or fabricate verdicts.

## 12. Redaction and sensitive artifacts

Add fixtures proving representative bearer tokens, proxy credentials, cookies, and secret environment values do not enter normal plan/resolved/manifest snapshots.

Raw security payload artifact policy is deferred to security integrations; M003 establishes that artifacts can carry sensitivity classification/role metadata without exposing secrets in manifest labels.

## 13. CLI boundary

A minimal library inspector is required.

If M003 adds a tiny inspect binary for proving usability, it must remain a presentation adapter and must not force the full future eggbench-cli structure prematurely. It is acceptable to defer CLI until local-runner M003.

## 14. Documentation

Add:

- architecture/evidence.md;
- docs/evidence-bundle.md;
- schema/example bundle tree;
- corruption/incomplete behavior;
- immutability statement;
- retention/indexing non-goals.

## 15. Tests

At minimum:

- finalize/reopen/verify synthetic bundle;
- manifest written last semantic fixture;
- destination already exists;
- interrupted staging;
- missing artifact;
- wrong size;
- wrong digest;
- duplicate path;
- absolute path;
- parent traversal;
- symlink escape on supported platforms;
- unknown schema version;
- additive unknown field behavior according to policy;
- zero-trial failed/preflight-style synthetic bundle if schema permits;
- multiple trials;
- secret redaction;
- large file hashing without full-buffer requirement.

## 16. Verification

Required:

    cargo fmt --all -- --check
    cargo check --workspace --all-targets --locked
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    cargo test --workspace --all-features --locked
    cargo +1.89.0 check --workspace --all-targets --locked
    cargo tree --locked
    git diff --check

## 17. Acceptance

M003 closes when Eggbench can create a synthetic staging bundle, finalize it immutably, reopen it without a database, verify every artifact, detect corruption/traversal/incomplete state, and expose enough typed metadata for the local runner to adopt without changing the evidence model.

Closing M003 unblocks Local Runner M001.

## 18. Stop conditions

Stop if bundle finalization requires a database or server, if safe artifact confinement cannot be implemented without changing the format, or if the current plan/resolved schemas are too unstable to identify required evidence. Resolve the upstream contract instead of inventing bundle-local duplicates.

## 19. Closure evidence required

Record:

- implementation commit;
- synthetic bundle tree;
- manifest fixture;
- corruption/path-safety test matrix;
- large-file hashing behavior;
- MSRV result;
- docs;
- unresolved findings;
- disposition.
