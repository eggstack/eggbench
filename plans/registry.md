# Eggbench Active Planning Registry

This file is the compact control surface for active planning. Detailed requirements remain in canonical specifications, ADRs, subsystem roadmaps, implementation plans, closure records, and Git history.

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

## Active subsystem roadmaps and correctives

| Subsystem | Status | Roadmap / corrective | Current milestone | Dependencies or blockers |
|---|---|---|---|---|
| Foundation experiment/evidence post-closure corrective | closed | plans/subsystems/foundation-experiment-evidence-post-closure-corrective-addendum.md | C001 closed | none |
| Local runner/lifecycle post-closure corrective | closed | plans/subsystems/local-runner-lifecycle-post-closure-corrective-addendum.md | C001 closed | none |
| Local runner M002 post-closure corrective | active | plans/subsystems/local-runner-m002-post-closure-corrective-addendum.md | C001 ready | none |
| Local runner/lifecycle | active | plans/subsystems/local-runner-lifecycle-roadmap.md | M001 closed; M002 historical closure + C001 corrective ready; M003 blocked | M002 evidence-safety corrective C001 must close before M003 |
| Measurement/comparison | blocked | plans/subsystems/measurement-comparison-roadmap.md | M001 blocked | Local Runner M002 evidence-safety corrective C001 must close before consuming trial orchestration |
| Eggstack integrations | proposed | plans/subsystems/eggstack-integration-roadmap.md | M001 blocked | Foundation + local runner + measurement contracts |
| External measurement oracles | ready | plans/subsystems/external-oracles-roadmap.md | M001 ready for planning | Driver boundary + qualified local runner command substrate; implementation plan not yet written |
| Security qualification | proposed | plans/subsystems/security-qualification-roadmap.md | M001 blocked | Measurement + integration layers |
| Distributed execution | deferred | plans/subsystems/distributed-execution-roadmap.md | entry gate not met | Local lifecycle/evidence stable + concrete remote provider; evaluate Eggwork first |

## Historical subsystem closures

| Subsystem | Status | Roadmap | Closure evidence |
|---|---|---|---|
| Foundation experiment/evidence | closed | plans/subsystems/foundation-experiment-evidence-roadmap.md | plans/closure/foundation-experiment-evidence/001-status.md; 002-status.md; 003-status.md |
| Foundation status/verdict corrective C001 | closed | plans/subsystems/foundation-experiment-evidence-post-closure-corrective-addendum.md | plans/closure/foundation-experiment-evidence-post-closure-corrective/001-status.md |
| Local runner/lifecycle M001 | closed historical predecessor | plans/subsystems/local-runner-lifecycle-roadmap.md | plans/closure/local-runner-lifecycle/001-status.md; SHA erratum: plans/closure/local-runner-lifecycle/001-errata.md; corrective closure: plans/closure/local-runner-lifecycle-post-closure-corrective/001-status.md |
| Local runner/lifecycle M002 | closed | plans/subsystems/local-runner-lifecycle-roadmap.md | plans/closure/local-runner-lifecycle/002-status.md |
| Local runner/lifecycle post-closure corrective C001 | closed | plans/subsystems/local-runner-lifecycle-post-closure-corrective-addendum.md | plans/closure/local-runner-lifecycle-post-closure-corrective/001-status.md |

Historical closure records remain evidence of what was accepted at the time. Corrective work does not silently rewrite them.

## Dependency-ready implementation plans

| Subsystem | Milestone | Status | Implementation plan | Handoff note |
|---|---|---|---|---|
| Local runner M002 post-closure corrective | C001 Evidence-error cleanup and finalization timeline | ready | plans/implementation/local-runner-m002-post-closure-corrective/001-evidence-error-cleanup-and-finalization-timeline.md | Must close before M003 or Measurement/Comparison implementation |

## Blocked next milestones

| Subsystem | Milestone | Status | Implementation plan | Blocker |
|---|---|---|---|---|
| Local runner/lifecycle | M003 Environment fingerprint + CLI lifecycle | blocked | not yet written | M002 corrective C001 |
| Measurement/comparison | M001 Metric normalization and trial evidence | blocked | not yet written | M002 corrective C001 |

## Current execution order and dependency gates

### Gate A — Historical foundation

Foundation M001-M003 are closed historical predecessor work. They established the typed plan, driver resolution, and immutable evidence bundle.

A post-closure audit found one schema-semantic issue: manifest v1 overloads execution outcome and comparison verdict in one `RunStatus` type.

### Gate B — Foundation status/verdict corrective

Foundation post-closure corrective C001 is closed as recorded at `plans/closure/foundation-experiment-evidence-post-closure-corrective/001-status.md`. It separates execution status from comparison verdict, emits manifest v2 for new writes, retains an explicit manifest-v1 reader, and records Local Runner M001 lifecycle-only evidence as “execution completed, comparison not performed.”

This gate closed before Local Runner M002 began; the corrected status/verdict contract remains controlling for subsequent work.

### Gate C — Local Runner M001 corrective

Local Runner post-closure corrective C001 is closed at `plans/closure/local-runner-lifecycle-post-closure-corrective/001-status.md`. Its implementation added filesystem-resolved cwd confinement, a hermetic child environment contract, explicit executable resolution, truthful platform support, and cross-platform qualification. The historical M001 SHA erratum is recorded at `plans/closure/local-runner-lifecycle/001-errata.md`.

It must:

- reject filesystem-resolved cwd escapes including symlinks;
- make the current `env_clear()` behavior an explicit hermetic execution contract;
- eliminate implicit ambient-PATH executable resolution;
- scope Unix-only process dependencies correctly;
- add Linux/macOS/Windows CI with truthful platform support;
- record an erratum for the invalid full SHA in the historical M001 closure record.

The predecessor closure incorrectly lists `9387a45e1bbf9c1f9a55fb8ad875b07f6f880d21`. The actual M001 implementation commit is `9387a459103078c1ccdbca7d4db41ae0f6cefc11`. The corrective must preserve the old record and add explicit errata.

### Gate D — Local Runner M002 and post-closure evidence-safety corrective

Local Runner M002 has a historical closure at
`plans/closure/local-runner-lifecycle/002-status.md`.

A post-closure audit found two untested correctness defects in the orchestration/evidence boundary:

- dynamic workload-artifact staging errors can occur after managed startup and return before workload drain/service teardown;
- the persisted finalization phase can differ from the returned `RunOutcome.phases` because the phase is serialized before bundle publication and then mutated again in memory.

Corrective C001 is registered at:

`plans/implementation/local-runner-m002-post-closure-corrective/001-evidence-error-cleanup-and-finalization-timeline.md`

It is the only dependency-ready runner handoff. Historical M002 closure remains preserved.

### Gate E — Comparison

Measurement/Comparison M001 remains blocked until M002 corrective C001 closes.

ADR-0003 remains controlling: trial is the statistical unit; practical threshold and uncertainty are separate; pass/fail/inconclusive/invalid are comparison verdicts, not process lifecycle states.

### Gate F — Integrations

EggServe/Eggfetch/Gregg remain the first Eggstack integration slice after runner/comparison foundations.

Eggress/Eggchaos, EggReplay/Eggprobe, and Eggsec follow as separate milestones.

External oha/h2load/iperf3 drivers proceed after the external command substrate and measurement model are stable.

### Gate G — Security profiles

Security qualification retains separate correctness and performance gates. Faster execution never overrides a security-correctness failure.

### Gate H — Distributed execution

Distributed execution remains deferred. Do not add SSH/scheduler/credential machinery to Eggbench. Evaluate Eggwork against ADR-0005 once local lifecycle/evidence semantics are stable.

## Planning-time sibling integration facts to re-audit before implementation

As of 2026-09-22 planning:

- Eggfetch core is published in the 0.2 series and already carries its own Criterion/resource benchmark machinery.
- Eggress 1.0.8 exposes narrow reusable crates including relay/outbound/metrics/testkit surfaces.
- EggServe exposes 0.2-series reusable serving crates.
- Eggchaos, EggReplay, and Eggprobe are pre-release 0.1 projects with useful but evolving integration seams.
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
6. execution status and comparison verdict are not conflated;
7. lifecycle/cancellation/cleanup semantics are explicit;
8. filesystem/environment/platform guarantees are truthful;
9. security/correctness effects are explicit;
10. machine-readable schema/version effects are explicit;
11. closure evidence is sufficient to prove more than compilation.

## Next handoff

The next dependency-ready implementation plan is:

`plans/implementation/local-runner-m002-post-closure-corrective/001-evidence-error-cleanup-and-finalization-timeline.md`

After corrective C001 closes:

- Local Runner M003 becomes ready for plan authoring/implementation;
- Measurement/Comparison M001 becomes ready for plan authoring/implementation against lifecycle-safe trial evidence.

External Oracles M001 remains independently plan-authorable, but implementation that consumes the M002 orchestration path should wait for this corrective to close.
