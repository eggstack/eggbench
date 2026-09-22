# Foundation Experiment and Evidence M002 — Driver Capability and Resolved-Plan Contract

Status: closed

Repository planning baseline: 032eb324d299f865c989b59226e70eff28d77d09

Source roadmap:

- plans/subsystems/foundation-experiment-evidence-roadmap.md — M002

Hard dependency:

- Foundation M001 must be closed.

Controlling ADRs:

- plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md
- plans/adrs/ADR-0004-eggstack-composition-and-independent-oracles.md

Primary class: infrastructure.

## 1. Objective

Define how a validated symbolic ExperimentPlan resolves against available local driver implementations into a concrete, redaction-safe ResolvedPlan before execution begins.

This milestone creates capability negotiation and fake driver-resolution machinery only. It does not execute workloads or processes.

## 2. Entry review

Before implementation, inspect the M001 closure record and current core types.

If M001 changed schema names or deliberately deferred fields, update this plan narrowly while preserving the ADR invariants.

Re-audit no runtime/network dependency has entered eggbench-core.

## 3. Required invariants

- resolution occurs before measurement side effects;
- unsupported requested semantics fail explicitly;
- no driver may silently downgrade protocol, load model, route, fault, TLS, or lifecycle behavior;
- resolved forms are serializable and redaction-safe;
- concrete runtime handles do not enter the resolved schema;
- driver identity/version/capability data is evidence;
- multiple driver categories remain distinct.

## 4. Driver categories

Define stable category semantics for:

- service;
- workload;
- telemetry;
- fault;
- diagnostic;
- execution provider.

Do not force every category into one giant trait.

If a category is not yet used by a consumer, its core representation may be an enum/descriptor rather than a public trait. Avoid speculative abstraction.

## 5. Driver identity and capability model

Represent at least:

- canonical driver name;
- adapter version;
- upstream tool/library name;
- upstream version/revision when known;
- driver category;
- capability set;
- platform constraints;
- optional machine-output schema version;
- whether the driver is in-process or external-process backed.

Capabilities must be structured enough to validate requested semantics.

Examples include:

- HTTP versions;
- open-loop support;
- corrected latency support;
- proxy/routing support;
- fault families;
- telemetry fields;
- external binary availability.

Do not create a universal string bag if a small typed vocabulary can represent the first milestones.

## 6. Resolution model

Create a resolver that consumes:

- validated ExperimentPlan;
- available driver descriptors;
- explicit user driver selections/default policy;
- non-secret environment/path inputs.

It produces:

- ResolvedPlan;
- exact driver selections;
- normalized defaults;
- concrete executable paths for external drivers where applicable later;
- declared capabilities;
- warnings;
- redaction-safe references.

M002 tests use fake descriptors; no external executable discovery is required yet.

## 7. Fail-closed behavior

Resolution must fail on:

- missing required driver;
- category mismatch;
- requested capability absent;
- ambiguous driver selection;
- unsupported platform when known;
- incompatible service/workload relationship;
- unsupported open/closed-loop mode;
- unsupported requested telemetry when marked required.

Optional telemetry may be explicitly omitted with a structured warning only if the plan marks it optional.

## 8. Default-selection policy

Defaults must be deterministic and inspectable.

Do not select a driver based on arbitrary hash-map iteration or whichever binary appears first in PATH.

The first release may require explicit driver names for ambiguous categories rather than inventing complex preference scoring.

## 9. Resolved plan

ResolvedPlan must retain enough information to reproduce what Eggbench intended to execute:

- source plan schema/version;
- resolution schema/version;
- selected drivers and capabilities;
- normalized topology;
- normalized workload mode;
- resolved non-secret defaults;
- environment-policy identifier;
- comparison-policy request;
- deterministic seed;
- warnings.

It must not contain:

- open file/socket handles;
- Tokio tasks;
- process IDs;
- plaintext credentials;
- callable closures;
- implementation-specific trait objects.

## 10. Fake registry/test driver

Create a small in-memory registry sufficient to prove:

- exact driver selection;
- deterministic defaults;
- capability success;
- capability denial;
- optional telemetry warning;
- ambiguous selection failure;
- version/provenance retention.

Do not add a plugin system.

## 11. Compatibility

Assign an explicit ResolvedPlan schema version.

A driver upgrade may change runtime behavior without changing plan schema; therefore resolved evidence must include driver/upstream versions.

Unknown future driver capabilities should be preserved/ignored only according to an explicit additive compatibility rule.

## 12. Documentation

Add:

- architecture/drivers.md;
- docs/driver-capabilities.md;
- update architecture/core.md with plan -> resolve boundary.

Document the preference order from ADR-0004 for future Eggstack/external integrations.

## 13. Verification

Required:

    cargo fmt --all -- --check
    cargo check --workspace --all-targets --locked
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    cargo test --workspace --all-features --locked
    cargo +1.89.0 check --workspace --all-targets --locked
    cargo tree --locked
    git diff --check

Focused tests must prove every fail-closed case above.

## 14. Acceptance

M002 closes when fake drivers can resolve representative M001 plans into deterministic serializable ResolvedPlans, unsupported semantics fail before I/O, and the core still owns no concrete networking/runtime implementation.

Closing M002 satisfies the interface dependency for M003.

## 15. Stop conditions

Stop if the design requires runtime trait objects or external executable probing inside eggbench-core, or if one adapter-specific feature would force a generic capability language more complex than the current use cases justify.

## 16. Closure evidence required

Record:

- implementation commit;
- capability matrix fixture;
- failure-case matrix;
- sample resolved plan;
- dependency tree;
- documentation;
- unresolved issues;
- disposition.
