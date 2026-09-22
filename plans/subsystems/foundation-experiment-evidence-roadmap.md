# Foundation Experiment and Evidence Roadmap

Status: active

Repository audit baseline: planning-only repository after canonical documents and ADRs.

Long-term references:

- plans/000-long-term-specification.md — execution model, experiment plan, evidence, crate architecture
- plans/003-planning-process.md

Related ADRs:

- plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md
- plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md

## 1. Purpose and ownership boundary

This subsystem owns Eggbench's dependency-light domain model: workspace foundation, schema versions, typed experiment plan, validation, resolved-plan representation, driver capability vocabulary, evidence manifest, artifact identities, and portable bundle reading/writing primitives.

It does not own process execution, Tokio orchestration, concrete Eggstack integrations, statistical policy implementation beyond core comparison DTOs, or CLI presentation.

## 2. Work classification

### Invariants

- eggbench-core has no Tokio, Clap, process, or concrete network dependency.
- Every durable serialized contract has an explicit schema version.
- Invalid plans fail before side effects.
- Secret values are not required inside serializable resolved plans.
- Driver capability mismatch is explicit.
- Completed bundle manifests are immutable evidence.
- Bundle paths are confined and traversal-safe.

### Capabilities

- validate an experiment plan;
- inspect a bundle;
- serialize/deserialize stable plan/evidence contracts.

### Infrastructure

- Rust workspace;
- core IDs and units;
- plan schema;
- validation;
- driver descriptors/capabilities;
- evidence manifest;
- artifact hashing/finalization helpers.

### Polish

- JSON Schema generation;
- diagnostic path formatting;
- human-readable validation errors.

## 3. Non-goals

No network I/O, process spawning, load generation, statistics engine, database, dashboard, remote execution, or direct Eggstack runtime dependency in core.

## 4. Current state

The repository began empty on 2026-09-22. Canonical planning and ADRs define the target architecture, but no Rust workspace or runtime code exists.

This is a true greenfield foundation; there is no legacy API or storage schema to preserve.

## 5. Target architecture

~~~text
eggbench-core
  ids/
  schema/
  plan/
  validation/
  resolved/
  metrics/
  comparison-dto/
  evidence/
  driver-contracts/
  errors/
~~~

The actual module names may differ, but dependency direction must remain inward and runtime-free.

## 6. Dependency graph

~~~text
M001 Workspace + experiment schema
      |
      v
M002 Driver capability + resolved-plan contract
      |
      v
M003 Evidence bundle + artifact finalization
~~~

M001 is ready.

M002 is hard-blocked on M001.

M003 is hard-blocked on M001 and interface-blocked on M002 because bundle manifests must identify resolved driver choices.

## 7. Milestones

### M001 — Workspace and typed experiment schema

Class: infrastructure

Objective: create the Rust workspace and dependency-light core types/validation required to represent an experiment without executing it.

Deliverables:

- root workspace targeting Rust 1.89+;
- eggbench-core;
- SchemaVersion, typed IDs, units/durations/rates;
- ExperimentPlan;
- subject/topology/service/workload/telemetry/gate/environment models;
- strict validation;
- deterministic JSON/TOML round trips;
- fixtures for valid/invalid plans;
- secret-reference/redacted-value boundary;
- core error taxonomy.

Exit: representative plans validate/round-trip; invalid/cyclic/ambiguous plans fail deterministically; core remains runtime/network-free.

### M002 — Driver capability and resolved-plan contract

Class: infrastructure

Objective: define how symbolic plan requests resolve to concrete available driver capabilities before execution.

Dependencies: M001 closed.

Deliverables:

- stable driver category vocabulary;
- driver identity/version/capability descriptors;
- capability requirements in plan/resolved plan;
- explicit unsupported errors;
- resolved executable/reference forms;
- redaction-safe resolved plan;
- fake registry/resolver for tests.

Exit: fake drivers prove successful resolution and fail-closed unsupported behavior with no I/O.

### M003 — Immutable evidence bundle contract

Class: infrastructure/capability foundation

Objective: create the first portable .eggb directory format and atomic finalization/inspection primitives.

Dependencies: M001 closed; M002 interface stable.

Deliverables:

- manifest schema;
- artifact metadata + SHA-256;
- staging/finalization;
- safe relative-path handling;
- plan/resolved-plan/environment/trial artifact references;
- incomplete/corrupt bundle diagnostics;
- read-only inspector library API;
- size/count bounds;
- redaction test fixtures.

Exit: a synthetic run can finalize, reopen, verify, inspect, detect corruption, and remain readable without a database.

## 8. Cross-cutting requirements

Compatibility: schema version changes are explicit. Historical fixtures become compatibility tests.

Security: no secrets in snapshots by default. Reject bundle traversal and unsafe artifact paths.

Performance: core operations are not hot-path request processing; prioritize determinism and bounded memory over micro-optimization.

Documentation: add architecture docs alongside implementation once code exists. Canonical plan docs remain architecture authority until then.

## 9. Verification strategy

At minimum:

- cargo fmt --all -- --check
- cargo check --workspace --all-targets --locked
- cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
- cargo test --workspace --all-features --locked
- cargo +1.89.0 check --workspace --all-targets --locked
- serialization fixture tests
- property coverage for path/plan invariants where useful

## 10. Risks and decision points

- Avoid overdesigning topology as a general workflow language.
- Avoid making trait object serialization part of the schema.
- Keep human-friendly TOML separate from normalized resolved JSON semantics.
- Do not create a database merely to generate IDs or aliases.
- If schemars materially complicates MSRV/dependencies, JSON Schema generation may follow after M001 rather than block it.

## 11. Completion definition

This roadmap closes when the repository has a stable typed experiment contract, explicit driver capability resolution, and immutable self-describing evidence bundles usable by later runner/statistics work.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | closed | plans/implementation/foundation-experiment-evidence/001-workspace-and-experiment-schema.md | plans/closure/foundation-experiment-evidence/001-status.md | none |
| M002 | ready | plans/implementation/foundation-experiment-evidence/002-driver-capability-and-resolved-plan-contract.md | none | none |
| M003 | blocked | plans/implementation/foundation-experiment-evidence/003-immutable-evidence-bundle.md | none | M001 + M002 interface |
