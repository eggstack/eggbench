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
| Local runner M002 post-closure corrective | closed | plans/subsystems/local-runner-m002-post-closure-corrective-addendum.md | C001 closed | Hosted CI run 35797812233 passed all required jobs |
| post-M003/M001 qualification corrective | closed | plans/subsystems/post-m003-m001-qualification-corrective-addendum.md | C001 closed | Hosted CI run 35808371805 passed all required jobs |
| post-M003 combined hosted qualification corrective | closed | plans/subsystems/post-m003-hosted-qualification-corrective-addendum.md | C001 stopped (historical); C002 closed | Combined qualification closed by hosted run 36029547565 (four lanes green); closure: plans/closure/post-m003-hosted-qualification-corrective/002-status.md |
| Local runner/lifecycle | closed | plans/subsystems/local-runner-lifecycle-roadmap.md | M001/M002/M003 closed; C001 closed | none |
| Measurement/comparison | closed | plans/subsystems/measurement-comparison-roadmap.md | M001 qualified; M002 hosted-qualified; M003 closed/hosted-qualified | none; qualified by C002 run 36029547565 |
| Eggstack integrations | active | plans/subsystems/eggstack-integration-roadmap.md | M001 hosted-qualified; M002 plan-authorable | M002 implementation unblocked subject to its own authored implementation plan |
| External measurement oracles | active | plans/subsystems/external-oracles-roadmap.md | M001/M002 hosted-qualified; C002 closed (lint/qualification corrective); M003 future | none blocking; M003 netem remains the later milestone |
| Security qualification | proposed | plans/subsystems/security-qualification-roadmap.md | M001 blocked | Measurement + integration layers |
| Distributed execution | deferred | plans/subsystems/distributed-execution-roadmap.md | entry gate not met | Local lifecycle/evidence stable + concrete remote provider; evaluate Eggwork first |

## Historical subsystem closures

| Subsystem | Status | Roadmap | Closure evidence |
|---|---|---|---|
| Foundation experiment/evidence | closed | plans/subsystems/foundation-experiment-evidence-roadmap.md | plans/closure/foundation-experiment-evidence/001-status.md; 002-status.md; 003-status.md |
| Foundation status/verdict corrective C001 | closed | plans/subsystems/foundation-experiment-evidence-post-closure-corrective-addendum.md | plans/closure/foundation-experiment-evidence-post-closure-corrective/001-status.md |
| Local runner/lifecycle M001 | closed historical predecessor | plans/subsystems/local-runner-lifecycle-roadmap.md | plans/closure/local-runner-lifecycle/001-status.md; SHA erratum: plans/closure/local-runner-lifecycle/001-errata.md; corrective closure: plans/closure/local-runner-lifecycle-post-closure-corrective/001-status.md |
| Local runner/lifecycle M002 | closed historical predecessor | plans/subsystems/local-runner-lifecycle-roadmap.md | plans/closure/local-runner-lifecycle/002-status.md; C001 evidence-safety corrective: plans/closure/local-runner-m002-post-closure-corrective/001-status.md |
| Local runner/lifecycle post-closure corrective C001 | closed | plans/subsystems/local-runner-lifecycle-post-closure-corrective-addendum.md | plans/closure/local-runner-lifecycle-post-closure-corrective/001-status.md |
| Local runner/lifecycle M003 | closed | plans/subsystems/local-runner-lifecycle-roadmap.md | plans/closure/local-runner-lifecycle/003-status.md; corrective: plans/closure/post-m003-m001-qualification-corrective/001-status.md |
| Measurement/comparison M001 | closed (qualified) | plans/subsystems/measurement-comparison-roadmap.md | plans/closure/measurement-comparison/001-status.md; qualification corrective: plans/closure/post-m003-m001-qualification-corrective/001-status.md |
| Measurement/comparison M002 | closed | plans/subsystems/measurement-comparison-roadmap.md | plans/closure/measurement-comparison/002-status.md |

Historical closure records remain evidence of what was accepted at the time. Corrective work does not silently rewrite them.

## Dependency-ready implementation plans

| Subsystem | Milestone | Status | Implementation plan | Handoff note |
|---|---|---|---|---|
| post-M003 combined hosted qualification corrective | C002 Driver stable-Clippy debt and final hosted qualification | closed | plans/implementation/post-m003-hosted-qualification-corrective/002-drivers-stable-clippy-and-final-hosted-qualification.md | Closed by implementation a8fbcea + 0e32ff0 and four-lane green hosted run 36029547565; closure: plans/closure/post-m003-hosted-qualification-corrective/002-status.md |

C001 remains stopped historical work at plans/implementation/post-m003-hosted-qualification-corrective/001-current-tip-ci-and-closure-reconciliation.md and must not be re-executed.

## Authored but dependency-blocked implementation plans

No new capability implementation plan is dependency-ready.

Eggstack M002 route/stream-fault topology may be authored now that C002 closed (implementation unblocked subject to its own plan). External Oracles M003 netem remains the later milestone; security qualification still waits on its measurement + integration layer dependencies.

## Current execution order and dependency gates

### Gate A — Historical foundation

Foundation M001-M003 are closed historical predecessor work. They established the typed plan, driver resolution, and immutable evidence bundle.

A post-closure audit found one schema-semantic issue: manifest v1 overloads execution outcome and comparison verdict in one `RunStatus` type.

### Gate B — Foundation status/verdict corrective

Foundation post-closure corrective C001 is closed as recorded at `plans/closure/foundation-experiment-evidence-post-closure-corrective/001-status.md`. It separates execution status from comparison verdict, emits manifest v2 for new writes, retains an explicit manifest-v1 reader, and records Local Runner M001 lifecycle-only evidence as “execution completed, comparison not performed.”

This gate closed before Local Runner M002 began; the corrected status/verdict contract remains controlling for subsequent work.

### Gate C — Local Runner M001 corrective

Local Runner post-closure corrective C001 is closed at `plans/closure/local-runner-lifecycle-post-closure-corrective/001-status.md`. Its implementation added filesystem-resolved cwd confinement, a hermetic child environment contract, explicit executable resolution, truthful platform support, and cross-platform qualification. The historical M001 SHA erratum is recorded at `plans/closure/local-runner-lifecycle/001-errata.md`.

The predecessor closure incorrectly listed `9387a45e1bbf9c1f9a55fb8ad875b07f6f880d21`; the actual M001 implementation commit is `9387a459103078c1ccdbca7d4db41ae0f6cefc11`. This gate is historical and has no remaining action item.

### Gate D — Local Runner M002 and post-closure evidence-safety corrective

Local Runner M002 has a historical closure at
`plans/closure/local-runner-lifecycle/002-status.md`.

The post-closure audit found two untested correctness defects in the
orchestration/evidence boundary:

- dynamic workload-artifact staging errors could return from `execute_run`
  after managed startup but before the common workload-drain/service-teardown
  tail;
- the persisted finalization phase could differ from the returned
  `RunOutcome.phases` because the phase was serialized before bundle
  publication and then mutated again in memory.

Corrective C001 closed at
`plans/closure/local-runner-m002-post-closure-corrective/001-status.md` and is fully qualified by hosted CI run `35797812233`, which passed Linux stable, Linux Rust 1.89 MSRV, macOS stable, and Windows stable supported-subset jobs.
Historical M002 closure remains preserved; the corrective records the new defects and the evidence that proves cleanup is now mandatory.

### Gate E — Comparison and current-tip hosted qualification

The earlier post-M003/M001 corrective is closed and qualified by hosted run 35808371805.

Measurement M002 (80ff6d1) and Measurement M003 (49a4105) subsequently landed with local closure evidence. C001 fixed the first hosted defects at implementation 808c35f, including M003 ceiling-division lint, Windows ResolvedPlan v2 fixture completion, and feature-dependent CLI unused-mut cleanup.

Fresh hosted run 36017662684 then passed Linux Rust 1.89 and passed cargo check on Linux/macOS/Windows stable, but all three stable jobs stopped at all-feature Clippy because stable 1.98 exposed pre-existing eggbench-drivers lint debt.

C001 correctly stopped under its frozen-contract rule. Its successor C002 (implementation a8fbcea + 0e32ff0) cleared the driver debt plus narrowly masked follow-on findings and closed with four-lane green hosted run 36029547565. Measurement M003 is therefore closed/hosted-qualified and the accumulated post-M001 feature set carries current-tip hosted qualification.

### Gate F — Drivers and integrations

External Oracles M001 (7afa054) and M002 (3384a89) are implementation-closed. Eggstack M001 is implementation-closed through M001a (8426e08) and M001b (a0ff206).

C002 cleared the frozen-code driver lint class and produced the final four-lane green qualification (run 36029547565), closing the combined gate while preserving:

- oha/h2load/iperf3 argv semantics;
- parser acceptance and metric values;
- driver capabilities/version floors;
- Debug redaction;
- schemas/dependencies/MSRV.

C002 closure records:

- External Oracles M001/M002 hosted-qualified;
- Eggstack M001a/M001b current integrated cross-platform qualification;
- Eggstack M002 implementation unblocked subject to its own authored implementation plan;
- External Oracles M003 remains the later netem milestone.

### Gate G — Security profiles

Security qualification retains separate correctness and performance gates. Faster execution never overrides a security-correctness failure.

### Gate H — Distributed execution

Distributed execution remains deferred. Do not add SSH/scheduler/credential machinery to Eggbench. Evaluate Eggwork against ADR-0005 once local lifecycle/evidence semantics are stable.

## Planning-time sibling integration facts to re-audit before implementation

As of 2026-09-23 planning:

- Eggfetch core is currently 0.2.0, Rust 1.89, with a lean `standard-http1` embedding profile; its Criterion/resource benchmark crate remains unpublished and Eggfetch-owned.
- Eggress 1.0.8 exposes narrow reusable crates including relay/outbound/metrics/testkit surfaces.
- EggServe workspace is currently 0.2.1; `eggserve-server` + `eggserve-primitives` are the documented direct generic H1 embedding seam.
- Eggchaos, EggReplay, and Eggprobe are pre-release 0.1 projects with useful but evolving integration seams.
- Gregg is currently 1.0.14; `gregg-protocol` exposes the versioned v2 wire contract and `/v2/status` is the universal status endpoint.
- Eggsec owns structured scoped security/load-testing semantics.
- SynVoid is an initial high-value benchmark subject, not an Eggbench dependency.

These are planning observations, not permanent version pins. Every integration plan must inspect current sibling state at handoff.

Observed 2026-09-24 during External Oracles M002 grounding (local host):
Eggress workspace is 1.0.10 (many narrow crates incl. relay/outbound/
metrics/routing/embed); EggServe workspace is 0.2.2; Eggsec is 0.1.0;
Eggchaos/EggReplay/Eggprobe have no locally available seam; `oha 1.16.0`
(built from crates.io), `h2load nghttp2/1.59.0`, and `iperf 3.16` all
verified live against loopback.

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

No corrective handoff remains dependency-ready: C002 is closed (four-lane green run 36029547565) and C001 is stopped historical work that must not be re-executed.

The next capability milestone is Eggstack M002 route/stream-fault topology, which may proceed to implementation once its own implementation plan is authored (no such plan exists yet; do not invent one here). External Oracles M003 netem remains the later milestone.
