# Eggbench Planning and Agent-Handoff Process

Status: normative planning governance

This document defines how Eggbench's long-term architecture is translated into actionable work without allowing short-lived implementation details to destabilize the canonical specification, terminology, or roadmap.

The keywords MUST, MUST NOT, REQUIRED, SHOULD, SHOULD NOT, and MAY are normative.

## 1. Purpose

Eggbench uses two planning horizons.

1. **Long-term planning** defines product identity, experiment semantics, architectural ownership, invariants, non-goals, dependency ordering, and end-state acceptance.
2. **Interim planning** defines bounded implementation work against a concrete repository baseline and is intended for handoff to coding agents.

Interim work MAY discover evidence that warrants a long-term revision, but MUST NOT silently rewrite the intended architecture to match the easiest implementation.

## 2. Document classes

### 2.1 Canonical long-term documents

Canonical long-term documents are:

- `plans/000-long-term-specification.md`;
- `plans/001-terminology-and-domain-model.md`;
- `plans/002-long-term-roadmap.md`;
- this planning-governance document.

These documents SHOULD remain stable during normal feature implementation.

They MAY be amended only when product direction changes intentionally, a contradiction/material omission is found, an accepted ADR changes the end state, or the maintainer explicitly requests a long-term revision.

### 2.2 Architecture decision records

ADRs capture durable decisions that affect several milestones, subsystems, schemas, or public contracts.

An ADR MUST state:

- context and forces;
- considered alternatives;
- selected decision;
- consequences/tradeoffs;
- affected specification sections and subsystem roadmaps;
- compatibility/migration implications;
- security/reliability implications;
- status.

Accepted ADR history MUST remain visible. A later decision supersedes rather than rewrites prior history.

### 2.3 Subsystem roadmaps

A subsystem roadmap translates the canonical specification into one coherent workstream.

It MUST define:

- purpose and ownership boundary;
- long-term references;
- related ADRs;
- invariants;
- capabilities/infrastructure/polish classification;
- non-goals;
- current-state summary;
- target architecture;
- dependency graph;
- ordered milestones;
- cross-cutting security, portability, compatibility, observability, and evidence concerns;
- risks/decision points;
- milestone status.

A subsystem roadmap SHOULD avoid exact line numbers and brittle file-by-file instructions.

### 2.4 Milestone implementation plans

A milestone implementation plan is the primary coding-agent handoff artifact.

It MUST be independently executable, bounded, and tied to a repository baseline.

It MUST include:

- source roadmap/milestone;
- relevant ADRs and long-term requirements;
- objective;
- current implementation evidence;
- invariants;
- explicit non-goals;
- expected production changes;
- schema/storage/protocol/compatibility effects;
- ordered work packages;
- failure/cancellation/restart semantics where relevant;
- focused and broad verification;
- static guards/documentation updates where justified;
- acceptance and stop conditions;
- closure evidence required.

Material deviation from the plan MUST be recorded rather than hidden.

### 2.5 Closure records

A closure record determines whether implementation is actually complete.

It MUST include:

- implementation commits or pull requests;
- requirement-to-evidence matrix;
- tests/guards run and outcomes;
- schema/migration/compatibility evidence;
- security and lifecycle evidence where applicable;
- documentation/operational evidence;
- known limitations;
- unresolved findings classified by severity;
- disposition: closed, conditionally closed, corrective pass required, or blocked.

Compilation alone is never sufficient closure evidence.

### 2.6 Archive records

Completed, superseded, or abandoned interim planning MAY move under `plans/archive/` when it is no longer useful in the active registry.

Canonical documents and accepted ADRs are not archived merely because initial implementation completed.

## 3. Work classification

Every planned item has one primary class.

### Invariant

A property that must survive releases and implementation strategies.

Examples:

- setup does not enter a steady-state measurement accidentally;
- trial is the default comparison unit;
- completed evidence is immutable;
- unsupported driver semantics do not silently downgrade;
- security correctness gates remain independent from performance.

Invariant work normally deserves property tests, static guards, or schema-level evidence.

### Capability

User/developer/operator visible behavior.

Examples:

- running a plan;
- comparing two bundles;
- using an oha workload;
- running a SynVoid WAF qualification profile.

Capability closure requires end-to-end evidence.

### Infrastructure

Internal machinery consumed by capabilities.

Examples:

- evidence manifest;
- process lifecycle runner;
- driver registry;
- bootstrap comparison library.

Infrastructure MUST NOT be presented as a completed user capability until a real consumer exists.

### Polish

Ergonomics, documentation, performance tuning, diagnostics, cleanup, or maintainability work that does not establish the principal capability boundary.

Polish normally follows correctness/capability closure.

## 4. Dependency model

Milestone dependencies are:

- **hard** — implementation cannot correctly begin before the dependency closes;
- **interface** — work may proceed against a written contract/test double;
- **soft** — parallel implementation is possible but integration waits;
- **operational** — implementation may land but qualification/release waits on external evidence.

A milestone is ready only when all hard dependencies are closed and interface dependencies have stable contracts.

The registry MUST state blockers explicitly.

## 5. Milestone sizing

A milestone SHOULD be small enough for one implementation agent to inspect the repository, implement production changes, add focused tests, run verification, update documentation, and report residual risks in one coherent pass.

A milestone is too large when it combines several independent capability boundaries, unrelated migrations, or unresolved architecture decisions.

A milestone is too small when it creates no meaningful closure boundary unless it is a corrective action.

Prefer a vertical contract with a consumer over a broad refactor with no usable path.

## 6. Agent handoff authority

The default authority order is:

1. canonical long-term specification and terminology;
2. accepted ADRs;
3. subsystem roadmap;
4. milestone implementation plan;
5. current repository evidence.

An implementation agent MUST inspect current code before editing and MUST preserve unrelated changes.

When repository reality conflicts with an implementation plan, the agent SHOULD preserve long-term invariants, record the discrepancy, and make the smallest coherent adjustment. It MUST NOT invent new architecture merely to finish the checklist.

## 7. Eggbench-specific handoff requirements

Plans touching measurement MUST identify:

- what is inside/outside the timed interval;
- the experimental unit;
- primary vs diagnostic metrics;
- units and directionality;
- required trial count or minimum evidence;
- environment comparability rules;
- driver saturation/failure behavior;
- whether a result can fail, pass, become inconclusive, or become invalid.

Plans touching process lifecycle MUST identify startup, readiness, cancellation, drain, teardown, descendant cleanup, and log bounds.

Plans touching Eggstack integrations MUST identify the sibling project's canonical ownership and the exact public/process seam being consumed.

Plans touching security workloads MUST identify correctness authority and must not use faster runtime as a substitute for correctness.

## 8. Corrective passes

A corrective pass is a new implementation plan.

It MUST:

- reference the original milestone and closure evidence;
- enumerate unclosed requirements/defects;
- explain why previous verification missed them;
- add regression evidence preventing recurrence;
- avoid reopening unrelated closed scope.

Repeated corrective passes are evidence that milestone sizing or subsystem contracts require revision.

## 9. Updating subsystem roadmaps

Subsystem roadmaps MAY evolve as implementation reveals new dependencies or better decomposition.

Updates MUST preserve completed milestone history, dependency rationale, and explicit status of deferred/superseded work.

A roadmap MUST NOT mark a capability complete merely because its infrastructure exists.

## 10. Registry requirements

`plans/registry.md` is the compact active planning control surface.

It SHOULD contain only:

- active subsystem roadmaps;
- dependency-ready implementation plans;
- blocked milestones and blockers;
- active/recent corrective plans;
- latest closure status.

It MUST link to source documents rather than duplicate them.

## 11. Required planning review

Before handoff, verify:

1. long-term references are correct;
2. durable architecture decisions are resolved or isolated behind an ADR;
3. dependencies are ready;
4. scope/non-goals are bounded;
5. ownership and invariants are explicit;
6. schema/compatibility effects are explicit;
7. lifecycle/failure/cancellation semantics are explicit;
8. security/authorization effects are explicit;
9. measurement and statistical semantics are unambiguous;
10. verification and closure criteria are sufficient.

If those cannot be answered, the plan is not ready.

## 12. Planning anti-patterns

Avoid:

- adding transient TODO lists to canonical specs;
- putting all future work into one implementation plan;
- treating request count as statistical sample count without explicit justification;
- timing setup unintentionally;
- accepting cross-host numeric deltas as regression evidence without a comparability policy;
- parsing human CLI output when machine output exists;
- allowing a driver to silently drop an unsupported option;
- reimplementing Eggstack protocol behavior for convenience;
- changing security semantics to win a benchmark;
- equating compilation with closure;
- hiding failed/unrun verification;
- adding dashboards/databases/remote scheduling before local evidence semantics stabilize.

## 13. Initial subsystem decomposition

Initial roadmaps are organized around:

- foundation experiment/evidence contract;
- local runner and lifecycle;
- measurement/comparison;
- Eggstack integrations;
- external measurement oracles;
- security qualification;
- distributed execution provider boundary.

The decomposition may change when ownership evidence warrants it, but changes must be explicit.
