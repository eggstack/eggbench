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
| Foundation experiment/evidence post-closure corrective | active | plans/subsystems/foundation-experiment-evidence-post-closure-corrective-addendum.md | C001 ready | none |
| Local runner/lifecycle post-closure corrective | active | plans/subsystems/local-runner-lifecycle-post-closure-corrective-addendum.md | C001 blocked | Foundation corrective C001 |
| Local runner/lifecycle | active | plans/subsystems/local-runner-lifecycle-roadmap.md | M001 closed; M002 blocked; M003 blocked | M002 waits for both post-closure correctives and then needs a fresh implementation plan |
| Measurement/comparison | proposed | plans/subsystems/measurement-comparison-roadmap.md | M001 blocked | corrected foundation status/verdict contract + local trial evidence |
| Eggstack integrations | proposed | plans/subsystems/eggstack-integration-roadmap.md | M001 blocked | Foundation + local runner + measurement contracts |
| External measurement oracles | proposed | plans/subsystems/external-oracles-roadmap.md | M001 blocked | Driver contracts + local runner |
| Security qualification | proposed | plans/subsystems/security-qualification-roadmap.md | M001 blocked | Measurement + integration layers |
| Distributed execution | deferred | plans/subsystems/distributed-execution-roadmap.md | entry gate not met | Local lifecycle/evidence stable + concrete remote provider; evaluate Eggwork first |

## Historical subsystem closures

| Subsystem | Status | Roadmap | Closure evidence |
|---|---|---|---|
| Foundation experiment/evidence | closed | plans/subsystems/foundation-experiment-evidence-roadmap.md | plans/closure/foundation-experiment-evidence/001-status.md; 002-status.md; 003-status.md |
| Local runner/lifecycle M001 | closed historical predecessor | plans/subsystems/local-runner-lifecycle-roadmap.md | plans/closure/local-runner-lifecycle/001-status.md; post-closure corrective records the SHA erratum and additional findings |

Historical closure records remain evidence of what was accepted at the time. Corrective work does not silently rewrite them.

## Dependency-ready implementation plans

| Subsystem | Milestone | Status | Implementation plan | Handoff note |
|---|---|---|---|---|
| Foundation experiment/evidence post-closure corrective | C001 Execution status and comparison verdict separation | ready | plans/implementation/foundation-experiment-evidence-post-closure-corrective/001-execution-status-and-comparison-verdict-separation.md | Must land before further runner trial evidence or comparison work |

## Prewritten blocked implementation plans

| Subsystem | Milestone | Status | Implementation plan | Blocker |
|---|---|---|---|---|
| Local runner/lifecycle post-closure corrective | C001 Filesystem/environment/platform qualification | blocked | plans/implementation/local-runner-lifecycle-post-closure-corrective/001-filesystem-environment-and-platform-qualification.md | Foundation corrective C001 |
| Local runner/lifecycle | M002 Trial phase orchestration + fake workload | blocked | not yet written | Foundation corrective C001 + Local Runner corrective C001; then write a fresh plan against corrected repository |

## Current execution order and dependency gates

### Gate A — Historical foundation

Foundation M001-M003 are closed historical predecessor work. They established the typed plan, driver resolution, and immutable evidence bundle.

A post-closure audit found one schema-semantic issue: manifest v1 overloads execution outcome and comparison verdict in one `RunStatus` type.

### Gate B — Foundation status/verdict corrective

Run Foundation post-closure corrective C001 first.

It must separate execution status from comparison verdict, emit a corrected manifest schema for new writes, retain explicit manifest-v1 read compatibility, and migrate Local Runner M001's zero-trial evidence from fabricated `inconclusive` to “execution completed, comparison not performed.”

Do not begin Local Runner M002 or Measurement/Comparison implementation until this closes.

### Gate C — Local Runner M001 corrective

After Foundation C001 closes, run Local Runner post-closure corrective C001.

It must:

- reject filesystem-resolved cwd escapes including symlinks;
- make the current `env_clear()` behavior an explicit hermetic execution contract;
- eliminate implicit ambient-PATH executable resolution;
- scope Unix-only process dependencies correctly;
- add Linux/macOS/Windows CI with truthful platform support;
- record an erratum for the invalid full SHA in the historical M001 closure record.

The predecessor closure incorrectly lists `9387a45e1bbf9c1f9a55fb8ad875b07f6f880d21`. The actual M001 implementation commit is `9387a459103078c1ccdbca7d4db41ae0f6cefc11`. The corrective must preserve the old record and add explicit errata.

### Gate D — Local Runner M002

Only after both corrective C001 milestones close should a fresh Local Runner M002 implementation plan be written.

M002 remains warmup -> measured trial -> cooldown/reset phase orchestration with a fake workload. It must consume the corrected evidence status model and corrected process/session boundary.

### Gate E — Comparison

Metric normalization and comparison follow actual trial evidence.

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

The only dependency-ready implementation plan is:

`plans/implementation/foundation-experiment-evidence-post-closure-corrective/001-execution-status-and-comparison-verdict-separation.md`

After it closes, the next handoff is:

`plans/implementation/local-runner-lifecycle-post-closure-corrective/001-filesystem-environment-and-platform-qualification.md`

Do not write or execute Local Runner M002 until both correctives close.
