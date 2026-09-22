# Foundation Post-Closure Corrective C001 — Closure

Disposition: **closed**

Implementation commit: `f10e03d224a3ca62cec04cd124154ace7685398f` (`feat(evidence): separate execution status and verdict`).

## Requirement-to-evidence matrix

| Requirement | Evidence | Outcome |
|---|---|---|
| Execution and comparison are separate typed domains | `ExecutionStatus` and `ComparisonVerdict`; current `BundleManifest` v2 has required execution status and optional comparison verdict | Pass |
| Lifecycle completion does not imply a performance pass | Local Runner zero-trial test checks completed execution, no verdict, and no trials | Pass |
| Comparison absence remains absence | v2 fixture and zero-trial lifecycle test record `comparison_verdict: null`/`None` | Pass |
| Contradictory comparison metadata is rejected | Validation rejects verdict without artifact, artifact without verdict, and failed/cancelled execution with verdict | Pass |
| Existing manifest-v1 evidence remains readable and verifiable | Original `example.eggb` v1 fixture is unchanged; full digest verification passes | Pass |
| v1 ambiguity is retained explicitly | `BundleReader::legacy_status()` returns `LegacyRunStatus::Inconclusive`; no comparison verdict is inferred | Pass |
| New writes use corrected schema only | `BundleWriter::finalize` requires `ExecutionStatus` and emits manifest v2 | Pass |
| No comparison algorithm was introduced | Only the typed verdict contract and coherence validation were added | Pass |
| Evidence-safety guarantees remain intact | Existing missing/wrong-size/wrong-digest/extra-file/path/symlink tests remain green | Pass |
| Core remains runtime/network independent | Dependency tree retains no Tokio, process, or concrete network dependency in `eggbench-core` | Pass |

## Schema and fixture evidence

- Current manifest schema: v2.
- Legacy v1 fixture: `crates/eggbench-core/tests/fixtures/example.eggb/` (preserved unchanged).
- Current v2 fixture: `crates/eggbench-core/tests/fixtures/current-v2.eggb/`.
- Current API types: `ExecutionStatus` and `ComparisonVerdict`.
- The v1 `inconclusive` value is preserved as a legacy status because it cannot distinguish “comparison not performed” from an inconclusive comparison. `BundleReader::legacy_status()` makes this provenance visible; no v2 comparison verdict is inferred.
- The Local Runner lifecycle-only evidence now records `ExecutionStatus::Completed`, no comparison verdict, and zero trials.

## Verification

- `cargo fmt --all -- --check` — pass
- `cargo check --workspace --all-targets --locked` — pass
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass
- `cargo test --workspace --all-features --locked` — pass (49 tests across 6 suites)
- `cargo +1.89.0 check --workspace --all-targets --locked` — pass
- `cargo tree --locked` — pass; core remains free of runtime/process/network dependencies
- `git diff --check` — pass

## Known limitations and unresolved findings

- No comparison algorithm or statistical policy is implemented; ADR-0003 remains controlling.
- Manifest-v1 `inconclusive` evidence remains inherently ambiguous. It is readable and verifiable but cannot be upgraded to a precise comparison result.
- No unresolved correctness or security findings remain for this corrective.

## Dependency disposition

Local Runner post-closure corrective C001 is unblocked and marked ready in the registry. Local Runner M002 remains blocked until that corrective closes and a fresh M002 implementation plan is written. Measurement/Comparison remains blocked on local trial evidence; this schema correction alone does not unblock it.
