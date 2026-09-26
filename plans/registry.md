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
| post-M003 live external-tool qualification corrective | closed | plans/subsystems/post-m003-live-tool-qualification-corrective-addendum.md | C001 stopped with evidence; C002 closed | Live-binary interoperability qualified by C002 (implementation 98f16e6; hosted live-tool run 36177440371; four-lane run 36177440220); historical M003 closure preserved; M004 implementation unblocked |
| Local runner/lifecycle | closed | plans/subsystems/local-runner-lifecycle-roadmap.md | M001/M002/M003 closed; C001 closed | none |
| Measurement/comparison | closed | plans/subsystems/measurement-comparison-roadmap.md | M001 qualified; M002 hosted-qualified; M003 closed/hosted-qualified | none; qualified by C002 run 36029547565 |
| Eggstack integrations | closed | plans/subsystems/eggstack-integration-roadmap.md | M001-M004 closed/qualified (M004a `273e5b1`, M004b `b2de53e`); live Eggsec/combined qualification green | M004 substrate complete; Security Qualification M001 owns broader profiles |
| External measurement oracles | active | plans/subsystems/external-oracles-roadmap.md | M001/M002 hosted-qualified; C002 closed (lint/qualification corrective); M003 future | none blocking; M003 netem remains the later milestone |
| Security qualification | active | plans/subsystems/security-qualification-roadmap.md | M001a closed; M001b ready; M001c authored and dependency-blocked | M001c requires M001b closure |
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

| Subsystem | Milestone | Status | Plan | Dependencies |
|---|---|---|---|---|
| Security qualification | M001b Fixed-corpus HTTP correctness family | ready | plans/implementation/security-qualification/001b-fixed-corpus-http-correctness-family.md | M001a closure: plans/closure/security-qualification/001a-status.md |

M001a closed at `plans/closure/security-qualification/001a-status.md`; M001b is the next dependency-ready plan. M001c remains hard-blocked on M001b closure.

Historical post-M003 live-tool C001 stopped with evidence and successor C002 is closed at `plans/closure/post-m003-live-tool-qualification-corrective/002-status.md`. No corrective handoff remains open.

## Authored but dependency-blocked implementation plans

| Subsystem | Milestone | Status | Plan | Blocker |
|---|---|---|---|---|
| Security qualification | M001c Qualification suite execution/receipt + M001 closure | blocked | plans/implementation/security-qualification/001c-qualification-suite-execution-and-m001-closure.md | hard dependency: M001b closure |
| Security qualification | M001c Qualification suite execution/receipt + M001 closure | blocked | plans/implementation/security-qualification/001c-qualification-suite-execution-and-m001-closure.md | hard dependency: M001b closure |

External Oracles M003 netem remains separate.

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

External Oracles M001/M002 are hosted-qualified. Eggstack M001-M003 are closed and hosted-qualified, including additive real-binary EggReplay/Eggprobe interoperability through post-M003 live-tool C002 (implementation `98f16e6`; hosted runs `36177440371` and `36177440220`).

Eggstack M004 closed as two ordered stages:

1. **M004a Eggsec strict WAF correctness adapter** — closed (implementation `273e5b1`; closure `plans/closure/eggstack-integration/004a-status.md`).
2. **M004b security correctness gate family and M004 closure** — closed (implementation `b2de53e`; umbrella closure `plans/closure/eggstack-integration/004b-status.md`).

M004a consumes Eggsec through an external strict-scope CLI boundary, initially only the bounded `waf --json` bypass semantics on local/private targets. It introduces a distinct correctness execution/evidence category and never turns Eggsec security results into performance metrics.

M004b consumes M004a's sanitized evidence through a dedicated correctness gate family. ComparisonReceipt v3 records a performance-only verdict, a typed correctness section, and a conservative combined verdict. Performance cannot override failed correctness. Security Qualification M001 now owns the broader reusable profiles/corpora follow-up and is unblocked for its own implementation planning.

Audited Eggsec source baseline for M004 planning: `0509ac668adfd78e9899cd3428a807d0b3c9f27b` (workspace 0.1.0, Rust 1.89; evolving source tree, no immutable release selected for this machine contract).

External Oracles M003 netem remains a separate later system-level impairment boundary.

### Gate G — Security profiles

Security qualification retains separate correctness and performance gate families. Faster execution never overrides a security-correctness failure.

Measurement prerequisites are already closed. Eggstack M004a/M004b delivered the generic execution/evidence and combined-verdict substrate (both closed). Security Qualification M001 is now fully planned as M001a -> M001b -> M001c. M001a is closed; M001b is ready; M001c is hard-blocked on M001b closure. M001 owns reusable named profiles, corpus/config digests, expected-outcome matrices, fixed-corpus observable correctness, and suite-level qualification receipts above the initial WAF-bypass family.

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

M002 handoff re-audit on 2026-09-24:

- Eggress 1.0.10 is published on crates.io with Rust 1.89 and exposes the audited eggress-outbound listener-free TCP routing, typed detailed failures, and OutboundInfo; the implementation pins 1.0.10 exactly.
- Eggchaos v0.1.0 is published and qualified; eggchaos-core exposes the protocol-neutral deterministic stream fault engine and BidirectionalChaosStream used by the M002 design.
- eggchaos-eggfetch is deliberately not selected because its dialer owns a direct TCP connection; M002 composes Eggress route first and eggchaos-core second.
- Eggchaos datagram work on main remains outside M002.

M003 handoff re-audit on 2026-09-25:

- EggReplay current planning HEAD observed: `29ce133f`; workspace 0.1.0 / Rust 1.89. Its JSON CLI exposes envelope schema 1 and RegressionReport schema 2. M003a uses `validate` + `replay --output json`; it does not parse long-running `serve` readiness stderr or import EggReplay Rust crates.
- Eggprobe qualified release of record: `v0.1.1` at `53ea53d`, machine schema 0.3. Current main has unreleased schema 0.4 work while retaining package version 0.1.1, so M003b performs an explicit machine-schema handshake and consumes DNS/TCP/TLS/HTTP only.
- Both integrations use existing trusted external-process machinery and preserve sibling ownership. No EggReplay/Eggprobe production Rust dependency is planned.

M004 Eggsec handoff re-audit on 2026-09-25:

- Eggsec audited HEAD `0509ac668adfd78e9899cd3428a807d0b3c9f27b`, workspace 0.1.0 / Rust 1.89; no immutable release was selected for the M004 machine contract.
- The broad Eggsec Rust crate and generic `scan --json` pipeline are not selected. `eggsec-report-model` is narrow but is not the WAF CLI output contract.
- Initial production seam is external `eggsec waf --json` with generated exact local/private scope, global `--strict-scope`, and guarded no-network preflight. Manual override flags, stress/flood, raw packets, NSE, remote/C2, db/web-proxy, credentials, and public targets are outside M004.
- M004a consumes only Eggsec's explicit `bypass_successful` boolean plus structural report consistency; it does not interpret payload text or severity.
- M004b keeps security correctness out of TrialMetrics/bootstrap and introduces an independent correctness receipt section with conservative combined verdict precedence.
- Security Qualification M001 owns later reusable profile/corpus semantics rather than duplicating the M004 structural substrate.

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

Security Qualification M001b is dependency-ready:

`plans/implementation/security-qualification/001b-fixed-corpus-http-correctness-family.md`

Ordered sequence:

1. M001a — profile/corpus/config identity, generalized bounded content-tree hashing, deterministic expansion, and generic static HTTP bindings;
2. M001b — fixed-corpus HTTP correctness through Eggfetch, correctness policy v2, and ComparisonReceipt v4;
3. M001c — bounded serial suite execution, immutable qualification receipt, and M001 closure.

M001b must not begin before M001a closes. M001c must not begin before M001b closes.

No Eggsec or SynVoid upstream blocker is currently identified. External Oracles M003 netem remains separate.
