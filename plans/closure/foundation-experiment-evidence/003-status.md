# Foundation Experiment and Evidence M003 Closure

Disposition: **closed**

Implementation commit: `6a803128f1e715b26e7408d3295e6cc4a09d9839` (`feat(evidence): add immutable eggeb bundles`).

## Requirement-to-evidence matrix

| Requirement | Evidence | Outcome |
|---|---|---|
| Versioned, authoritative manifest | `BundleManifest` v1; `manifest.json` is emitted only during finalization; checked-in synthetic manifest fixture | Pass |
| Subject, plan, resolved-plan, environment, trial, driver, comparison/report provenance | Typed manifest fields and role-checked artifact references | Pass |
| Versioned environment placeholder and testbed policy classes | `EnvironmentFingerprint` v1 with comparison-critical, warning-only, and informational field classes | Pass |
| Bounded streaming artifacts and hashes | `BundleWriter::add_artifact`, 64 KiB stream buffer, persisted limits, SHA-256 | Pass |
| Safe staging and immutable finalization | Sibling staging, staged-byte re-verification, manifest-last write, same-directory no-replace publication on Linux; no copy fallback | Pass |
| Confined artifact paths and symlink handling | Portable path validation; Unix directory-handle no-follow opens; traversal and symlink escape tests | Pass |
| Read-only inspection and corruption reporting | `BundleReader` opens metadata and verifies exact files, sizes, and hashes without repair | Pass |
| Interruption, zero-trial, and multi-trial behavior | Dropped staging rejection; failed zero-trial and multiple-trial bundle tests | Pass |
| Secret redaction boundary | Bearer token, proxy password, cookie, and environment secret values represented only by references; plan/resolved/manifest snapshot assertions | Pass |
| No database requirement and documentation | Synthetic checked-in `.eggb`, `architecture/evidence.md`, `docs/evidence-bundle.md` | Pass |

## Corruption and path-safety matrix

| Condition | Evidence | Outcome |
|---|---|---|
| Existing destination | Destination collision test | Rejected |
| Interrupted staging | Staging directory passed to inspector | Rejected as incomplete |
| Missing artifact | Deleted required file | Detected |
| Wrong size | Modified artifact length | Detected |
| Wrong digest | Same-size content replacement | Detected |
| Extra file | Unmanifested file added | Detected |
| Staging modified after registration | Finalize-time verification | Publication refused |
| Absolute/parent/reserved/duplicate path | ArtifactPath and duplicate registration tests | Rejected |
| Symlink to outside bundle | Unix symlink test | Rejected without following |
| Unsupported manifest version | Schema version mutation fixture test | Rejected |
| Additive top-level field | Manifest compatibility test | Ignored under v1 additive-field rule |
| Oversized stream request | 2 MiB generated reader | Maximum reader request remained 64 KiB |

## Verification

- `cargo fmt --all -- --check` — pass
- `cargo check --workspace --all-targets --locked` — pass
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass
- `cargo test --workspace --all-features --locked` — pass (27 tests)
- `cargo +1.89.0 check --workspace --all-targets --locked` — pass
- `cargo tree --locked` — pass; core has filesystem/hash support but no runtime, process, or concrete network dependency
- `git diff --check` — pass

## Bundle fixture and platform note

Checked-in bundle tree: `crates/eggbench-core/tests/fixtures/example.eggb/` containing `manifest.json`, `plan.json`, `resolved-plan.json`, `environment.json`, and `trials/001/result.json`. The fixture is opened and fully verified in tests.

On Linux, final publication uses a no-replace directory rename and Unix artifact opens walk directory handles with no-follow flags. Other platforms use a create-new finalization lock and same-directory rename. Windows flushes artifact and manifest files; portable directory-entry syncing is not available through `std`, so entry durability follows OS/filesystem behavior. Cross-platform runtime qualification beyond the Linux implementation host remains for platform CI.

## Findings and disposition

- M003 does not implement automatic host fingerprint collection, a database, CLI, comparison engine, or runtime retention/garbage collection.
- No unresolved correctness findings.
- No material deviation from the plan.

M003 is closed. All Foundation M001-M003 dependencies are satisfied. Local Runner M001 was reviewed and can be unblocked; its ready handoff is recorded at `plans/implementation/local-runner-lifecycle/001-managed-process-and-readiness-lifecycle.md`.
