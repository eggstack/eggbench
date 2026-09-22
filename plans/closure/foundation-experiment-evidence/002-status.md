# Foundation Experiment and Evidence M002 Closure

Disposition: **closed**

Implementation commit: `770074f3e8b420907c1d0246d3e61e8a00f66b14` (`feat(foundation): resolve plans against driver capabilities`).

## Requirement-to-evidence matrix

| Requirement | Evidence | Outcome |
|---|---|---|
| Distinct stable driver categories and typed identity/capability records | `crates/eggbench-core/src/resolved.rs`; six `DriverCategory` variants and structured `Capability` vocabulary | Pass |
| Deterministic selection and exact explicit selection | In-memory registry resolver; tests for unique default, explicit selection, ambiguity, category mismatch | Pass |
| Fail closed before side effects | Resolver takes only plan/descriptors/options; tests cover missing driver, capability, platform, service compatibility, required telemetry | Pass |
| Optional telemetry is explicit and inspectable | Structured warning for omitted optional source/fields; tests cover missing optional and missing required driver/capability | Pass |
| External binary paths are explicit inputs | External-process descriptor requires `ExternalBinary` and a supplied non-empty path; no PATH probing | Pass |
| ResolvedPlan captures reproducibility data | ResolvedPlan v1 includes source schema version, all source intent needed later, selected descriptor/capabilities/versions, paths, defaults, seed and warnings | Pass |
| Schema fixture and documentation | `driver-capabilities.json`, `sample-resolved-plan.json`, `architecture/drivers.md`, `docs/driver-capabilities.md` | Pass |
| Core remains free of runtime/network implementation | Locked dependency tree contains no Tokio, Clap, process, or concrete network dependency | Pass |

## Failure-case matrix

| Failure | Result |
|---|---|
| Required driver missing | `MissingDriver` |
| Explicit driver category mismatch | `CategoryMismatch` |
| Multiple defaults/candidates | `AmbiguousSelection` |
| Capability absent | `UnsupportedCapability` |
| Known platform unsupported | `UnsupportedPlatform` |
| Workload incompatible with target service type | `IncompatibleService` |
| Required telemetry unavailable | `MissingDriver` or `UnsupportedCapability` |
| External executable path absent | `MissingExecutablePath` |

## Verification

- `cargo fmt --all -- --check` — pass
- `cargo check --workspace --all-targets --locked` — pass
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass
- `cargo test --workspace --all-features --locked` — pass (15 tests)
- `cargo +1.89.0 check --workspace --all-targets --locked` — pass
- `cargo tree --locked` — pass; no runtime, process, or concrete network implementation dependency
- `git diff --check` — pass

## Limitations and findings

- M002 uses fake in-memory descriptors and caller-supplied paths. It does not discover binaries or instantiate drivers.
- Capability vocabulary and unknown-field policy are version 1 contracts; new serialized capability variants require an explicit compatibility decision.
- No unresolved findings.

M002 is closed. M003 can consume the stable `ExperimentPlan` v1 and `ResolvedPlan` v1 contracts, including driver provenance, without defining bundle-local copies. M003 is ready. Local Runner M001 remains blocked until M003 provides usable evidence staging/finalization.
