# Distributed Execution Provider Roadmap

Status: deferred

Long-term references:

- plans/000-long-term-specification.md — distributed execution
- plans/002-long-term-roadmap.md — Phase 10

Related ADR:

- plans/adrs/ADR-0005-local-first-execution-and-remote-provider-boundary.md

## 1. Purpose and ownership boundary

This subsystem will allow one Eggbench experiment to place generator, subject, origin, telemetry, or fault services on different machines through an external execution provider.

It does not own remote authentication, node enrollment, scheduler policy, SSH, or artifact transport internals.

## 2. Entry gate

Do not begin implementation until:

- local runner lifecycle is closed;
- evidence and comparison schemas are stable enough to carry node provenance;
- at least one concrete remote execution provider exposes a stable contract;
- Eggwork has been evaluated as the preferred provider.

## 3. Invariants

- Local and remote plans share experiment semantics.
- Provider owns remote process authority.
- Eggbench records provider and node identities plus cleanup outcomes.
- Node clocks are not assumed synchronized.
- Cross-node telemetry alignment declares uncertainty.
- No hidden SSH fallback exists.

## 4. Proposed milestones

### M001 — ExecutionProvider qualification contract

Use local and fake-remote implementations to freeze placement, capability, artifact, cancellation, cleanup, and monotonic-time contracts.

### M002 — Eggwork adapter

If Eggwork satisfies M001, implement remote process, artifact, and capability mapping with no duplicate credential store.

### M003 — Multi-node experiment qualification

Prove generator -> subject -> origin topologies, node-local telemetry, failure/cancellation, artifact return, and cross-node timing semantics.

## 5. Time and ordering requirements

Node-local durations use node monotonic clocks.

Cross-node event ordering may use coordinator receipt order plus optional wall-clock offset evidence. Do not claim sub-millisecond cross-node alignment unless the provider/testbed has measured synchronization quality.

## 6. Completion definition

The roadmap closes when distributed placement changes where work executes without changing what an Eggbench experiment means.

## 7. Milestone status

Deferred. No implementation plan should be handed off yet.
