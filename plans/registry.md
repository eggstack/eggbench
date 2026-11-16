# Planning Registry

This page is a current-state index. Long-term plans and accepted ADRs define direction;
subsystem roadmaps define milestones; implementation plans and closure records preserve
handoff and acceptance evidence.

Completed plans and closure records remain for traceability. This index omits duplicated
work logs and dated sibling-repository snapshots; re-audit external dependencies at handoff.

## Current subsystem status

| Subsystem | Status | Current direction |
|---|---|---|
| [Foundation experiment and evidence](subsystems/foundation-experiment-evidence-roadmap.md) | Closed | M001–M003 and the post-closure corrective are complete. |
| [Local runner and lifecycle](subsystems/local-runner-lifecycle-roadmap.md) | Closed | M001–M003 and recorded correctives are complete. |
| [Measurement and comparison](subsystems/measurement-comparison-roadmap.md) | Closed | M001–M003 are qualified. |
| [Eggstack integrations](subsystems/eggstack-integration-roadmap.md) | Closed | M001–M004 are delivered and qualified. |
| [External measurement oracles](subsystems/external-oracles-roadmap.md) | Active | M001/M002 are complete; Linux netem M003 remains future work. |
| [Security qualification](subsystems/security-qualification-roadmap.md) | M001–M003 closed | The roadmap records qualification evidence and any later follow-up. |
| [Architecture deep-dive corrective](subsystems/architecture-deep-dive-corrective-addendum.md) | C001 closed; C002 hosted checks passed; C003 decision-gated | C002 hosted run `43510476976` passed; its closure record predates that run. |
| [Distributed execution](subsystems/distributed-execution-roadmap.md) | Deferred | Follow ADR-0005; do not add remote execution machinery to Eggbench. |

## Canonical direction and governance

- [Long-term specification](000-long-term-specification.md)
- [Terminology and domain model](001-terminology-and-domain-model.md)
- [Long-term roadmap](002-long-term-roadmap.md)
- [Planning and agent-handoff process](003-planning-process.md)
- [Accepted ADRs](adrs/)

## Planning rules

- Read the relevant subsystem roadmap before proposing work.
- Re-audit sibling repositories and external tool contracts at handoff; dated planning observations are not version pins.
- Use one bounded implementation plan per milestone and record material deviations.
- Close work with evidence in `plans/closure/`. Corrective work adds a new record; it does not rewrite historical closure evidence.
- Archive completed interim plans only when they no longer serve as handoff evidence. Preserve canonical plans, accepted ADRs, and closure records.

Current status belongs in the subsystem roadmaps and closure records. Update this index when a subsystem's status or next dependency changes.
