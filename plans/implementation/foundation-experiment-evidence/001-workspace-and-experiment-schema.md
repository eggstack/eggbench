# Foundation Experiment and Evidence M001 — Workspace and Typed Experiment Schema

Status: ready for handoff

Repository baseline: 032eb324d299f865c989b59226e70eff28d77d09

Source roadmap:

- plans/subsystems/foundation-experiment-evidence-roadmap.md — M001

Controlling ADRs:

- plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md
- plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md
- plans/adrs/ADR-0003-trial-level-comparison-and-regression-verdicts.md

Primary class: infrastructure.

## 1. Objective

Create the initial Rust workspace and dependency-light eggbench-core crate with a versioned, validated experiment-plan model.

This milestone must establish the semantic foundation without performing network I/O, spawning processes, implementing statistics, or committing to concrete driver libraries.

## 2. Current implementation evidence

At the repository baseline:

- the repository contains planning documents only;
- there is no Cargo workspace;
- there are no Rust sources;
- no compatibility burden exists beyond the accepted planning contracts;
- the initial implementation can therefore choose clean typed representations without migration shims.

## 3. Required invariants

M001 must preserve:

1. eggbench-core has no Tokio dependency;
2. eggbench-core has no Clap dependency;
3. eggbench-core has no Hyper/Eggfetch/Eggress/EggServe or other concrete network dependency;
4. every serialized top-level contract carries an explicit schema version;
5. invalid experiment structure is rejected before execution exists;
6. durations, rates, bytes, percentages, and metric direction are not represented as ambiguous untyped strings internally;
7. service dependency cycles are rejected;
8. every referenced service/target name resolves or validation fails;
9. secret values are not required in a serializable plan snapshot;
10. no individual request/sample model is introduced as the default statistical unit.

## 4. Explicit non-goals

Do not:

- spawn a process;
- open sockets;
- implement an HTTP workload;
- add a database;
- add a benchmark runner;
- implement bootstrap statistics;
- add Eggstack dependencies;
- implement remote execution;
- add a dashboard;
- add netem;
- add a compatibility shim for nonexistent legacy data.

## 5. Workspace foundation

Create a root Cargo workspace using resolver 3.

Target:

- Rust MSRV 1.89.0;
- edition 2024 unless repository/tooling evidence shows a concrete reason to stay on 2021;
- unsafe_code = forbid at workspace level;
- missing_docs = warn for public library surface;
- pedantic Clippy with narrowly documented allowances rather than broad suppression.

Create:

- crates/eggbench-core

Do not create the other canonical crates merely as empty placeholders unless compilation or documentation proves a real need. They will be introduced by their owning milestones.

Initial dependencies should remain small. Reasonable candidates are:

- serde;
- serde_json;
- toml;
- thiserror;
- uuid or another lightweight ID implementation only if it materially improves the model.

Avoid chrono if std::time plus serialized explicit duration forms are sufficient for M001. Wall-clock timestamp ownership belongs to evidence/runner work.

## 6. Schema/version foundation

Define explicit version identifiers rather than bare magic integers spread through the code.

At minimum:

- ExperimentPlan schema version;
- normalized core schema namespace/version helper;
- forward-compatible unknown-field policy decision documented in code/docs.

Do not claim compatibility semantics that tests do not enforce.

## 7. Typed identifiers

Introduce opaque validated identifiers for at least:

- ExperimentId or ExperimentName where identity/name separation is justified;
- ServiceId/ServiceName;
- WorkloadId/WorkloadName where needed;
- MetricName;
- DriverName or requested driver kind;
- Artifact-independent subject label.

IDs used for lookup should reject empty names and pathological lengths.

Do not expose OS PIDs as durable identifiers.

## 8. ExperimentPlan model

Implement a top-level ExperimentPlan capable of representing the following without execution:

- schema/version;
- experiment metadata;
- subject;
- topology;
- services;
- workload;
- trial policy;
- warmup;
- cooldown/reset intent;
- telemetry requests;
- comparison/gate requests;
- environment/testbed policy;
- artifact/log bounds;
- deterministic seed.

The first schema should remain deliberately bounded. Do not turn the plan into a generic DAG workflow language.

## 9. Subject model

Represent enough subject identity to distinguish:

- managed command/process subject;
- externally managed target;
- opaque user label;
- optional source revision/digest hints that can later be resolved into evidence.

Do not require every subject to be a process.

## 10. Topology/service model

A service plan must support:

- stable name;
- service kind as a declarative request;
- dependency names;
- configuration payload appropriate to the core schema;
- managed versus external lifecycle intent;
- readiness request;
- shutdown request;
- working-directory intent;
- bounded log policy.

Do not place a closure, trait object, Tokio type, Child handle, SocketAddr-only restriction, or driver implementation object into serialized plan types.

Validation must:

- reject duplicate service names;
- reject unknown dependencies;
- reject self-dependency;
- reject cycles;
- reject invalid empty argv for command-like service requests if command service is represented in M001;
- reject contradictory managed/external lifecycle fields.

## 11. Trial and timing policy

Represent:

- measured trial count;
- warmup policy;
- duration-based or finite-completion trial intent;
- cooldown duration;
- reset mode/reference;
- per-phase timeout requests where known.

Reject:

- zero measured trials;
- zero/negative semantic durations where invalid;
- contradictory duration/count combinations;
- unbounded values beyond declared plan limits.

Do not implement timing.

## 12. Workload/load-model contract

The plan must distinguish at least:

- closed-loop concurrency;
- open-loop offered rate;
- finite-count workload;
- time-bounded workload.

Do not require all drivers to support all modes. M001 describes requested semantics; M002 resolves capabilities.

Workload targets must reference a named service or explicit external target in an unambiguous way.

## 13. Metric and gate request model

Define metric descriptors sufficient for later comparison:

- stable metric name;
- unit;
- direction: higher-is-better, lower-is-better, target-range, informational;
- primary/gating versus diagnostic intent.

Gate requests must represent:

- absolute budget;
- relative regression budget;
- later statistical relative gate request.

Do not implement statistical calculations.

Validation must reject a gate on an informational-only metric unless the schema explicitly promotes it.

## 14. Environment policy model

Represent comparison/testbed intent without implementing host fingerprinting.

At minimum:

- strict same-testbed required;
- warn/descriptive mismatch mode;
- explicitly cross-testbed descriptive mode.

Do not define a raw environment JSON hash as the full comparability policy.

## 15. Secret/redaction boundary

Do not serialize literal secret values as required fields in the canonical plan.

If environment injection or credentials must be represented, use references such as environment-variable name, secret reference identifier, or explicitly redacted placeholder.

Add Debug/Display tests proving redacted forms do not reveal a representative secret.

## 16. Serialization contract

Support:

- TOML plan input/output where structurally reasonable;
- JSON canonical machine representation;
- deterministic round-trip fixture behavior.

Commit representative fixtures under a clear test/fixtures path:

- minimal valid experiment;
- multi-service valid topology;
- open-loop workload;
- absolute + relative gate example;
- invalid cycle;
- invalid unknown service;
- invalid zero trials;
- invalid contradictory workload.

M001 does not need to preserve whitespace/comment round trips.

## 17. Error model

Define typed validation errors with stable categories and human-readable context.

Do not expose internal serde error strings as the only public diagnostic.

At minimum distinguish:

- parse/schema error;
- unsupported schema version;
- validation error;
- duplicate identity;
- missing reference;
- cycle;
- invalid bound;
- contradictory configuration.

## 18. Documentation

Add at least:

- architecture/core.md — ownership and dependency rules;
- docs/experiment-plan.md — initial schema semantics and examples;
- README.md — concise project definition/status with pointers to plans/docs.

Do not copy the full long-term specification into README.

## 19. Tests

At minimum:

- minimal valid round trip;
- JSON and TOML parse parity;
- schema-version rejection;
- duplicate service;
- unknown dependency;
- cycle detection including multi-node cycle;
- zero trials;
- invalid duration/rate;
- closed/open-loop distinction survives round trip;
- gate direction/unit survives round trip;
- secret reference Debug/Display redaction;
- reasonable size/count bounds;
- property test or equivalent generated DAG validation if lightweight.

## 20. Static/dependency guards

Add a test or lightweight script only if it is simpler than inspection to enforce that eggbench-core does not gain forbidden runtime/network dependencies.

Preferred first approach: document and verify with cargo tree in closure evidence rather than creating an elaborate custom guard prematurely.

## 21. Verification

Required before closure:

    cargo fmt --all -- --check
    cargo check --workspace --all-targets --locked
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    cargo test --workspace --all-features --locked
    cargo +1.89.0 check --workspace --all-targets --locked
    cargo tree --locked
    git diff --check

If cargo +1.89.0 is unavailable on the implementation host, closure must record that as unrun rather than silently substituting another toolchain.

## 22. Acceptance

M001 closes when:

- workspace and eggbench-core exist;
- representative plans round-trip;
- invalid plans fail deterministically;
- core remains free of runtime/network ownership;
- schema versioning is explicit;
- validation semantics are documented;
- no M002 driver-resolution behavior has leaked into implementation;
- required verification is green or accurately recorded with a named operational blocker.

Closing M001 makes Foundation M002 ready.

## 23. Stop conditions

Stop and request a planning/ADR update rather than improvising if implementation discovers that:

- one requested plan field necessarily requires a concrete network/process type in core;
- the topology needs a general workflow engine to represent the stated first-release cases;
- the secret-reference boundary cannot represent a required integration without storing the secret;
- the chosen serialized schema creates an incompatible semantic ambiguity that cannot be fixed additively before first release.

## 24. Closure evidence required

The closure record must include:

- implementation commit;
- workspace/crate tree;
- dependency tree showing no forbidden core dependencies;
- fixture/test matrix;
- MSRV result;
- schema/version identifiers;
- documentation paths;
- unresolved findings;
- recommendation closed, conditionally closed, corrective required, or blocked.
