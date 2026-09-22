# ADR-0001: Typed Core and Driver Boundaries

Status: accepted

Date: 2026-09-22

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#41-experiment-semantics-are-the-product`
- `plans/000-long-term-specification.md#42-typed-core-thin-presentation`
- `plans/000-long-term-specification.md#6-canonical-experiment-plan`
- `plans/000-long-term-specification.md#18-crate-architecture`

Affected subsystem roadmaps:

- `plans/subsystems/foundation-experiment-evidence-roadmap.md`
- `plans/subsystems/local-runner-lifecycle-roadmap.md`
- `plans/subsystems/eggstack-integration-roadmap.md`
- `plans/subsystems/external-oracles-roadmap.md`

## Context

Eggbench needs to orchestrate many different benchmark subjects and generators without becoming another protocol stack. A CLI-first design would make command formatting the de facto architecture, while one giant runner crate would couple experiment semantics to Tokio, processes, Eggstack libraries, and optional external tools.

The core product is the reproducible experiment/evidence contract.

## Decision drivers

- stable machine-readable plans/results;
- low coupling between experiment semantics and concrete drivers;
- ability to use native Eggstack libraries and external executables;
- minimal default dependency graph;
- testability without network I/O;
- future embedding by CI, agents, or other Rust applications;
- no duplicate transport/security implementations.

## Considered options

### Option A — CLI-first application

Fast to prototype but creates unstable schemas, stringly integration, and poor embedding.

Rejected.

### Option B — One monolithic async crate

Simplifies initial wiring but couples schemas to runtime/process/network choices and makes lightweight downstream use difficult.

Rejected.

### Option C — Typed core with narrow driver categories

Keep domain types/validation/comparison/evidence contracts in a dependency-light core; runner owns lifecycle; adapters own concrete integrations; CLI is presentation.

Selected.

## Decision

The initial workspace SHALL contain:

- `eggbench-core`;
- `eggbench-runner`;
- `eggbench-drivers`;
- `eggbench-cli`.

`eggbench-core` SHALL NOT depend on Tokio, Clap, Hyper, Eggfetch, Eggress, subprocess APIs, or platform-specific runtime code.

The core SHALL own:

- IDs;
- schema versions;
- experiment and resolved-plan types;
- validation;
- topology/workload/telemetry/gate descriptions;
- metric definitions;
- comparison inputs/outputs;
- evidence manifest types;
- stable error taxonomy for domain/compatibility errors.

Runtime integration SHALL use distinct interfaces rather than one all-purpose driver:

- service;
- workload;
- telemetry;
- fault;
- diagnostic;
- execution provider.

Drivers SHALL advertise capabilities. Unsupported requested behavior SHALL fail explicitly before measurement rather than silently fall back.

The CLI SHALL compile arguments/files into core types, invoke library APIs, and render results. It SHALL NOT contain an independent execution path.

## Consequences

### Positive

- experiment semantics can be tested without I/O;
- external and Eggstack integrations remain replaceable;
- minimal builds stay small;
- future bindings can reuse the same contract;
- process/CLI quirks do not become schema semantics.

### Negative

- more up-front type design;
- adapter glue is explicit;
- capability negotiation must be maintained.

## Compatibility and migration

The repository is greenfield. Public schemas MUST have explicit version fields before first release.

Driver trait/API stability is internal during early 0.x unless explicitly promoted.

## Security and reliability

Plan validation must reject ambiguous/unsafe representations before I/O where possible. Secret values must not be copied into serializable resolved/evidence forms.

Driver panics or process failures must be contained by runner error handling and produce truthful run status.

## Verification

Conformance requires:

- `eggbench-core` builds with no Tokio/Clap/network dependencies;
- representative plan/result JSON round trips;
- fake drivers exercising runner contracts;
- CLI tests proving one library execution path;
- unsupported-capability tests proving no semantic fallback.

## Supersession

None.
