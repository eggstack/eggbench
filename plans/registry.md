# Eggbench Active Planning Registry

This file is the compact control surface for active planning. Detailed requirements remain in canonical specifications, ADRs, subsystem roadmaps, implementation plans, future closure records, and Git history.

Canonical direction remains in:

- plans/000-long-term-specification.md
- plans/001-terminology-and-domain-model.md
- plans/002-long-term-roadmap.md
- plans/003-planning-process.md

## Status vocabulary

- **proposed** — roadmap or plan exists but is not approved/dependency-ready for execution.
- **ready** — dependencies and interfaces are satisfied; plan may be handed off.
- **active** — implementation or closure work is in progress.
- **blocked** — a named dependency or evidence requirement prevents progress.
- **closing** — implementation landed and closure evidence is being gathered.
- **closed** — closure record accepted.
- **conditionally closed** — substantial work landed but a named evidence condition remains.
- **superseded** — replaced by another document.
- **archived** — retained for traceability but no longer active.
- **deferred** — intentionally outside the current implementation horizon.

## Accepted architectural decisions

| ADR | Status | Decision |
|---|---|---|
| plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md | accepted | Typed dependency-light core; separate runner/drivers/CLI; explicit capability failures |
| plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md | accepted | Immutable .eggb bundles, manifest-last finalization, first-class testbed provenance |
| plans/adrs/ADR-0003-trial-level-comparison-and-regression-verdicts.md | accepted | Trial-level inference, practical thresholds, deterministic bootstrap policy, pass/fail/inconclusive/invalid |
| plans/adrs/ADR-0004-eggstack-composition-and-independent-oracles.md | accepted | Reuse Eggstack ownership; prefer stable seams; retain independent external benchmark drivers |
| plans/adrs/ADR-0005-local-first-execution-and-remote-provider-boundary.md | accepted | Local runner first; future remote execution delegated through ExecutionProvider/Eggwork boundary |

## Active subsystem roadmaps

| Subsystem | Status | Roadmap | Current milestone | Dependencies or blockers |
|---|---|---|---|---|
| Local runner/lifecycle | active | plans/subsystems/local-runner-lifecycle-roadmap.md | M001 ready; M002-M003 blocked | Foundation M001-M003 closed; later milestones depend on predecessor closure. |
| Measurement/comparison | proposed | plans/subsystems/measurement-comparison-roadmap.md | M001 blocked | Foundation schemas + local trial evidence. |
| Eggstack integrations | proposed | plans/subsystems/eggstack-integration-roadmap.md | M001 blocked | Foundation + local runner + measurement contracts. |
| External measurement oracles | proposed | plans/subsystems/external-oracles-roadmap.md | M001 blocked | Driver contracts + local runner. |
| Security qualification | proposed | plans/subsystems/security-qualification-roadmap.md | M001 blocked | Measurement + integration layers. |
| Distributed execution | deferred | plans/subsystems/distributed-execution-roadmap.md | entry gate not met | Local lifecycle/evidence stable + concrete remote provider; evaluate Eggwork first. |

## Latest subsystem closure

| Subsystem | Status | Roadmap | Closure evidence |
|---|---|---|---|
| Foundation experiment/evidence | closed | plans/subsystems/foundation-experiment-evidence-roadmap.md | plans/closure/foundation-experiment-evidence/001-status.md; 002-status.md; 003-status.md |

## Dependency-ready implementation plans

| Subsystem | Milestone | Status | Implementation plan | Dependencies / handoff note |
|---|---|---|---|---|
| Local runner/lifecycle | M001 Managed process and readiness lifecycle | ready | plans/implementation/local-runner-lifecycle/001-managed-process-and-readiness-lifecycle.md | Foundation M001-M003 closed; resolved/evidence contracts are available. |

## Prewritten implementation plans

| Subsystem | Milestone | Status | Implementation plan | Blocker |
|---|---|---|---|---|
| Local runner/lifecycle | M001 Managed process and readiness lifecycle | ready | plans/implementation/local-runner-lifecycle/001-managed-process-and-readiness-lifecycle.md | None; predecessors closed. |

## Current execution order and dependency gates

### Gate A — Foundation schema

Foundation M001 is closed.

M001 must create a Rust 1.89+ workspace and runtime-free eggbench-core with versioned ExperimentPlan semantics, validation, topology/load/gate/environment models, fixtures, and documented dependency boundaries.

Do not begin process execution, statistics, Eggstack integration, or external drivers inside M001.

### Gate B — Driver resolution

After M001 closure, M002 became ready and is now closed.

M002 freezes driver identity/capability and ResolvedPlan semantics using fake drivers only. Unsupported behavior must fail before I/O.

### Gate C — Evidence

With M001 and M002 closed, M003 implemented immutable evidence bundles and inspection; the evidence interface is closed.

Foundation M003 closure supplied evidence staging/finalization. Local Runner M001's dependency is satisfied.

### Gate D — Local execution

After Foundation M003 closed, Local Runner M001 was planned against the resulting repository baseline and is ready.

The runner work must prove startup/readiness/cancellation/teardown and descendant cleanup before real load generators are integrated.

### Gate E — Comparison

Metric normalization and comparison follow actual trial evidence. Do not implement statistical gates against synthetic request-level samples before the local trial model exists.

ADR-0003 remains controlling.

### Gate F — Integrations

EggServe/Eggfetch/Gregg are the first Eggstack integration slice after the runner/comparison foundations because together they provide a controlled origin, a native HTTP workload, and host telemetry.

Eggress/Eggchaos, EggReplay/Eggprobe, and Eggsec follow as separate milestones.

External oha/h2load/iperf3 drivers should proceed once the external command substrate and measurement model are stable. They are required before claiming broad independent end-to-end qualification of Eggfetch/Hyper-based subjects.

### Gate G — Security profiles

Security qualification is not merely load testing.

Correctness/corpus/configuration evidence must be fixed independently from performance. A candidate that is faster while violating required security outcomes fails the qualification.

### Gate H — Distributed execution

Distributed execution remains deferred.

Do not add SSH/scheduler/credential machinery to Eggbench. Once local evidence/lifecycle are stable, evaluate Eggwork against the ExecutionProvider requirements in ADR-0005 and the distributed roadmap.

## Planning-time sibling integration facts to re-audit before implementation

As of 2026-09-22 planning:

- Eggfetch core is published in the 0.2 series and already carries its own Criterion/resource benchmark machinery.
- Eggress 1.0.8 exposes narrow reusable crates including relay/outbound/metrics/testkit surfaces.
- EggServe exposes 0.2-series reusable serving crates.
- Eggchaos, EggReplay, and Eggprobe are pre-release 0.1 projects with useful but still evolving integration seams.
- Gregg exposes v2 host telemetry over HTTP.
- Eggsec owns structured scoped security/load-testing semantics.
- SynVoid is an initial high-value benchmark subject, not an Eggbench dependency.

These are planning observations, not permanent version pins. Every integration plan must inspect current sibling state at handoff.

## Planning review checklist for new implementation plans

Before marking a plan ready, verify:

1. canonical specification/terminology references are correct;
2. all hard dependencies are closed;
3. driver/protocol ownership is not duplicated;
4. timed versus untimed phases are explicit;
5. experimental unit and primary metrics are explicit;
6. pass/fail/inconclusive/invalid semantics are explicit where applicable;
7. lifecycle/cancellation/cleanup semantics are explicit;
8. security/correctness effects are explicit;
9. machine-readable schema/version effects are explicit;
10. closure evidence is sufficient to prove more than compilation.

## Next handoff

The next implementation plan currently ready is:

plans/implementation/local-runner-lifecycle/001-managed-process-and-readiness-lifecycle.md

Foundation M001-M003 closure evidence is recorded under plans/closure/foundation-experiment-evidence/. Foundation M003 closed and unblocked Local Runner M001 planning; implementation remains outside the current handoff sequence.
