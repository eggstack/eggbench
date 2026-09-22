# ADR-0005: Local-First Execution and Remote Provider Boundary

Status: accepted

Date: 2026-09-22

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#410-local-first-bounded-and-inspectable`
- `plans/000-long-term-specification.md#22-distributed-execution`

Affected subsystem roadmaps:

- `plans/subsystems/local-runner-lifecycle-roadmap.md`
- `plans/subsystems/distributed-execution-roadmap.md`

## Context

Network laboratories eventually need separate generator, subject, and origin machines. Implementing SSH, credentials, remote process trees, node leases, artifact transfer, reconnection, and scheduling inside Eggbench would create a second remote-execution product.

Eggwork is being designed separately as a generic remote execution fabric. Eggbench should be able to consume such a fabric when its contract is stable.

## Decision drivers

- keep the first release small;
- perfect lifecycle/evidence semantics locally first;
- avoid duplicated SSH/scheduler/security machinery;
- keep experiment semantics independent from placement;
- permit multi-node experiments later.

## Considered options

### Option A — Build SSH execution into Eggbench immediately

Rejected. It mixes benchmark semantics with remote execution and credentials before local lifecycle is mature.

### Option B — Require containers/Kubernetes

Rejected. It narrows deployment unnecessarily and would not serve SBC/local-lab targets well.

### Option C — Local provider first, explicit future ExecutionProvider interface

Selected.

## Decision

The only required execution provider for the initial product is the local runner.

Eggbench SHALL model execution placement through an `ExecutionProvider` boundary once remote work becomes dependency-ready.

The provider owns:

- process start/stop;
- working directory;
- environment injection;
- stdout/stderr transport;
- capability inventory;
- cancellation;
- descendant cleanup status;
- artifact upload/download;
- host identity.

Eggbench continues to own:

- experiment plan;
- topology dependency ordering;
- phase timing;
- trial semantics;
- workload intent;
- telemetry alignment;
- evidence manifest;
- comparison;
- verdict.

A future Eggwork adapter is the preferred remote implementation if Eggwork exposes the necessary stable contract.

Eggbench SHALL NOT add hidden SSH fallback when a remote provider is unavailable.

## Time semantics

Cross-node wall clocks cannot be assumed perfectly synchronized.

Future distributed execution must define:

- per-node monotonic timestamps;
- coordinator/run event ordering;
- clock offset/uncertainty evidence when wall-clock alignment is required;
- whether a metric needs only local duration versus cross-node timestamp correlation.

This decision is deferred to the distributed roadmap; local experiments use one host monotonic clock for runner phase timing.

## Consequences

### Positive

- simpler and safer initial runner;
- remote credentials remain outside Eggbench;
- local and remote experiments can share one domain model;
- Eggwork can evolve independently.

### Negative

- first release cannot orchestrate true multi-host topologies;
- some network-capacity experiments remain manual/external until the provider exists.

## Compatibility

No remote wire protocol is defined by this ADR.

The local runner must avoid assumptions that would make remote placement impossible, such as exposing OS process IDs as durable service identity.

## Security and reliability

Remote provider identity, authentication, authorization, artifact integrity, and lease semantics belong to the provider. Eggbench must record provider identity and cleanup outcome but must not duplicate its authority model.

## Verification

Before distributed execution is enabled:

- the same experiment plan must resolve with local and fake-remote providers;
- provider failure/cancellation must preserve run truthfulness;
- node-specific artifacts must retain provenance;
- no SSH credential handling may appear in Eggbench core/runner unless a later ADR explicitly changes this boundary.

## Supersession

None.
