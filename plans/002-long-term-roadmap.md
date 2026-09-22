# Eggbench Long-Term Implementation Roadmap

Status: execution roadmap for `plans/000-long-term-specification.md`

Terminology: `plans/001-terminology-and-domain-model.md`

This roadmap orders the work needed to reach the long-term Eggbench architecture. It is dependency-ordered, not calendar-ordered. Every phase must leave the repository in a coherent state and must preserve the experiment/evidence contracts established by earlier phases.

## Cross-phase execution rules

Every phase MUST:

1. preserve typed-core ownership over CLI behavior;
2. keep setup/readiness/teardown outside measurement unless explicitly measured;
3. preserve trial-level statistical semantics;
4. version machine-readable contracts before exposing them as durable interfaces;
5. fail explicitly rather than silently degrade driver semantics;
6. keep logs/artifacts/processes bounded;
7. record driver, subject, toolchain, and environment provenance;
8. keep security correctness independent from performance gates;
9. update architecture/planning documentation and closure evidence;
10. avoid duplicating transport/fault/diagnostic behavior already owned by Eggstack components.

## Phase 0 — Planning and architectural contracts

### Objective

Freeze the initial product boundary, terminology, comparison philosophy, Eggstack ownership map, and planning process before implementation.

### Deliverables

- canonical specification;
- terminology/domain model;
- long-term roadmap;
- planning governance;
- active registry;
- ADRs for core ownership, evidence/provenance, statistical comparison, Eggstack/external-driver boundaries, and local/remote execution;
- subsystem roadmaps;
- first bounded implementation plans.

### Dependencies

None.

### Exit criteria

- no unresolved architectural decision blocks the foundation milestone;
- first implementation plan is dependency-ready;
- future work has explicit owners and blockers.

## Phase 1 — Foundation workspace and typed experiment contract

### Objective

Create the minimum Rust workspace and versioned domain model without performing network I/O.

### Deliverables

- Rust 1.89+ workspace;
- `eggbench-core`;
- schema/version identifiers;
- typed IDs;
- ExperimentPlan and validation;
- ResolvedPlan;
- topology/service/workload/telemetry/gate models;
- units and metric descriptors;
- serialization fixtures;
- redaction-safe configuration representations;
- initial compatibility/error model.

### Dependencies

Phase 0.

### Exit criteria

- representative plan fixtures round-trip deterministically;
- invalid plans fail before I/O;
- core has no Tokio, process, or concrete network dependency;
- schema version is explicit.

## Phase 2 — Driver contracts and immutable evidence

### Objective

Establish narrow integration interfaces and the durable evidence format before adding a complex runner.

### Deliverables

- driver capability model;
- service/workload/telemetry/fault/diagnostic driver interfaces;
- capability negotiation and unsupported errors;
- evidence manifest;
- atomic staging/finalization;
- artifact digesting;
- bounded log/artifact metadata;
- interrupted-bundle semantics;
- `validate` and `inspect` library foundations.

### Dependencies

Phase 1.

### Exit criteria

- fake drivers prove orchestration-facing contracts without network I/O;
- completed bundles are immutable/self-describing;
- corrupt/incomplete bundle detection is deterministic.

## Phase 3 — Local runner and process lifecycle

### Objective

Execute deterministic local process/service topologies safely and reproducibly.

### Deliverables

- `eggbench-runner`;
- process ownership;
- dependency-ordered startup;
- readiness;
- warmup;
- trial loop;
- cooldown/reset;
- drain;
- reverse-order teardown;
- cancellation;
- bounded stdout/stderr;
- failure provenance;
- local environment fingerprint;
- CLI `validate`, `doctor`, `run`, `inspect`.

### Dependencies

Phases 1–2.

### Exit criteria

- success, failure, cancellation, readiness timeout, and teardown-error paths produce truthful evidence;
- no managed child process remains after bounded cleanup in qualification tests;
- measurement windows exclude setup by default;
- Linux/macOS/Windows process semantics have platform-specific tests or explicit capability limits.

## Phase 4 — Measurement normalization and baseline comparison

### Objective

Turn repeated trials into transparent same-testbed regression evidence.

### Deliverables

- trial result normalization;
- metric registry/vocabulary;
- latency histogram references;
- baseline references;
- environment comparability policy;
- absolute and relative gates;
- trial-level bootstrap comparison;
- practical threshold handling;
- pass/fail/inconclusive/invalid semantics;
- deterministic report generation;
- `eggbench compare`.

### Dependencies

Phases 1–3.

### Exit criteria

- statistical unit tests cover improvement, regression, noisy overlap, insufficient trials, zeros, missing data, and directionality;
- request count cannot accidentally become comparison sample count;
- environment mismatch behavior is explicit;
- comparison policy has a version identifier.

## Phase 5 — First-party Eggstack integration

### Objective

Reuse the existing Eggstack network and telemetry components without absorbing their implementation scope.

### Deliverables

Initial integration order:

1. EggServe controlled-origin driver;
2. Eggfetch native HTTP workload driver;
3. Gregg telemetry source;
4. Eggress route/relay integration;
5. Eggchaos stream-fault integration;
6. EggReplay semantic replay integration;
7. Eggprobe pre/post diagnostic integration;
8. Eggsec security-workload adapter.

Each integration MUST use a stable public library seam when it is narrow and mature; otherwise use an explicit process/JSON adapter until a public seam exists.

### Dependencies

Phases 2–4. Individual integrations may proceed independently when their required sibling API is stable.

### Exit criteria

- Eggbench owns no duplicate HTTP/proxy/fault/probe/security implementation;
- every adapter records sibling version/revision/capabilities;
- unsupported combinations fail explicitly;
- integration tests use local fixtures.

## Phase 6 — Independent external measurement oracles

### Objective

Provide independent load/capacity paths and avoid circular benchmarking.

### Deliverables

- external command-driver substrate;
- exact executable/version capture;
- raw output retention;
- parsers for oha, h2load, and iperf3;
- generator-saturation detection where exposed;
- Linux-only optional netem adapter with privilege/capability checks;
- parity fixtures for normalized metrics.

### Dependencies

Phases 2–4.

### Exit criteria

- Eggfetch/Hyper-based subjects can be driven by a non-Eggfetch HTTP generator;
- raw external output is retained beside normalized metrics;
- unavailable binaries are capability errors, not fallback to a different workload;
- netem results identify packet/link impairment rather than stream faults.

## Phase 7 — Security-performance qualification profiles

### Objective

Make security performance experiments resistant to the common failure mode of trading correctness for speed.

### Deliverables

- reusable qualification-profile schema;
- correctness gate adapter;
- corpus/configuration digests;
- SynVoid initial profiles;
- Eggsec-driven local security workloads;
- benign, malicious, and mixed-traffic profiles;
- concurrency and large-body variants;
- correctness/performance combined reporting.

### Dependencies

Phases 4–6 plus stable security-driver semantics.

### Exit criteria

- a faster candidate fails if required security outcomes regress;
- security corpus/configuration identity is immutable in evidence;
- performance reports do not reinterpret security findings.

## Phase 8 — Cross-platform and release qualification

### Objective

Turn the local laboratory into a dependable distributable tool.

### Deliverables

- Linux x86_64 and aarch64 qualification;
- macOS Apple Silicon qualification;
- Windows x86_64/arm64 where practical;
- ARM64 SBC qualification;
- release profile/installer policy;
- shell completions;
- JSON schema/export documentation;
- resource-overhead benchmarks for Eggbench itself;
- stable v0.x compatibility statement;
- one-command local verification.

### Dependencies

Phases 3–7.

### Exit criteria

- local runner lifecycle is truthful on each supported platform;
- minimal feature build avoids optional external/Eggstack integrations;
- release artifacts identify supported driver matrix.

## Phase 9 — Historical indexing and CI ergonomics

### Objective

Add convenience around immutable bundles without changing their authority.

### Deliverables

- local bundle index/cache;
- named baseline aliases;
- retention policy;
- JUnit/Markdown/HTML export;
- optional Bencher-compatible export;
- bounded trend summaries;
- CI examples.

### Dependencies

Stable bundle/comparison schemas.

### Exit criteria

- deleting the index does not destroy evidence;
- bundles remain independently inspectable;
- aliases resolve to immutable identities.

## Phase 10 — Distributed execution through an explicit provider

### Objective

Run one experiment across several machines without turning Eggbench into a scheduler.

### Deliverables

- ExecutionProvider contract hardening;
- remote capability inventory;
- clock/timestamp strategy for cross-node telemetry;
- artifact return contract;
- Eggwork adapter if Eggwork exposes the required stable API;
- generator/subject/origin multi-node topology support;
- remote cancellation and cleanup evidence.

### Dependencies

Stable local runner/evidence model and a qualified remote execution provider.

### Exit criteria

- experiment semantics are unchanged between local and remote providers;
- Eggbench contains no independent SSH scheduler/credential subsystem;
- remote failures preserve node-specific provenance and cleanup status.

## Phase 11 — Advanced experiment design

### Objective

Add higher-order experiment ergonomics only after core correctness is mature.

### Possible deliverables

- bounded parameter matrices;
- deterministic randomized/interleaved A/B schedules;
- noise calibration runs;
- capacity-search policies;
- adaptive trial extension when a result is inconclusive;
- profile libraries for Eggstack projects.

These capabilities require separate subsystem planning before implementation.

## Completion definition

The roadmap is substantially complete when Eggbench can reproducibly run and compare realistic network/security experiments, retain auditable immutable evidence, reuse Eggstack correctly, use independent external oracles, protect correctness in security benchmarks, distribute across supported platforms, and later delegate multi-node execution without changing its experiment model.
