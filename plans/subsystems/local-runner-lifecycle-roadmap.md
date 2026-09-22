# Local Runner and Lifecycle Roadmap

Status: active

Long-term references:

- plans/000-long-term-specification.md — canonical execution model, topology, environment, CLI surface
- plans/002-long-term-roadmap.md — Phases 3 and 8

Related ADRs:

- plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md
- plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md
- plans/adrs/ADR-0005-local-first-execution-and-remote-provider-boundary.md

## 1. Purpose and ownership boundary

This subsystem owns deterministic local lifecycle execution: managed processes/services, dependency ordering, readiness, warmup, repeated trial scheduling, cooldown/reset, cancellation, drain, teardown, bounded logs, and local environment capture.

It does not own workload protocol implementations, statistics, remote execution, or security semantics.

## 2. Invariants

- Managed descendants are not intentionally orphaned.
- Teardown is attempted after every post-start terminal path.
- The original failure cause is preserved when cleanup also fails.
- Readiness and warmup are outside measurement by default.
- Phase transitions are structured evidence.
- Logs are bounded/spooled rather than unbounded RAM buffers.
- OS process IDs are diagnostics, not durable service identity.
- One host monotonic clock owns local phase durations.
- Externally managed services are never falsely reported as runner-owned.

## 3. Non-goals

No remote SSH, mandatory containers, general service supervision, daemon mode, distributed scheduler, or benchmark statistics.

## 4. Target architecture

~~~text
ResolvedPlan
    |
LocalExecutionProvider
    |
Topology supervisor
    |
phase state machine
    |
trial coordinator
    |
Evidence staging
~~~

Platform-specific process ownership remains behind a narrow abstraction.

## 5. Dependency graph

~~~text
Foundation M001-M003
       |
       v
M001 Managed process/service lifecycle
       |
       v
M002 Trial phase orchestration + fake workload
       |
       v
M003 Environment fingerprint + CLI lifecycle
~~~

## 6. Milestones

### M001 — Managed process and readiness lifecycle

Implement argv-based spawn, explicit cwd/env, process-group or Job Object ownership, stdout/stderr spooling, readiness probes, startup timeout, graceful then forced shutdown, reverse dependency teardown, and deterministic fixtures.

Acceptance includes no managed descendant remaining after qualified cleanup tests.

### M002 — Warmup, trial, cooldown, and reset state machine

Implement structured phases, cancellation, repeat count, per-trial artifact directories, monotonic timing, fake workload driver, reset hooks, cooldown, drain, and truthful terminal status.

Cancellation must be exercised from every lifecycle phase.

### M003 — Local environment fingerprint and CLI integration

Capture OS, architecture, CPU, memory, toolchain, subject metadata, and relevant driver/executable versions. Add validate, doctor, run, and inspect commands with JSON stdout discipline and human logs/progress on stderr.

## 7. Cross-cutting platform requirements

Linux: use process groups/session ownership or an equivalent bounded descendant strategy.

macOS: preserve the same semantic contract without Linux-only procfs assumptions.

Windows: use Job Objects or another documented ownership mechanism where practical; unsupported descendant-cleanup semantics must be surfaced as capability limits rather than implied.

ARM64/SBC: avoid high fixed-memory buffers and platform assumptions that make Raspberry Pi-class execution second-class.

## 8. Verification strategy

Must include:

- successful service graph;
- cyclic dependency rejection;
- child failure before readiness;
- readiness timeout;
- cancellation during startup, readiness, warmup, measurement, cooldown, drain, and teardown;
- graceful shutdown followed by forced cleanup;
- teardown error plus original workload failure;
- descendant process cleanup;
- bounded stdout/stderr truncation/spooling;
- externally managed service behavior;
- platform capability reporting.

## 9. Risks and decision points

- Process-tree cleanup differs by platform; do not fake equivalence.
- Readiness probes must not accidentally become benchmark traffic.
- Shell execution is intentionally not the canonical process path.
- A general workflow engine is out of scope; service dependencies should remain simple.

## 10. Completion definition

The roadmap closes when one synthetic local experiment executes repeatably and produces truthful finalized evidence on supported local platforms with bounded cleanup.

## 11. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | closed | plans/implementation/local-runner-lifecycle/001-managed-process-and-readiness-lifecycle.md | plans/closure/local-runner-lifecycle/001-status.md | none |
| M002 | ready | not yet written | none | Both post-closure correctives are closed; write and review a fresh implementation plan before execution |
| M003 | blocked | not yet written | none | M002 |
