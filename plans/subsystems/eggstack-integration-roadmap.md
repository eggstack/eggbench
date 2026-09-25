# Eggstack Integration Roadmap

Status: active

Long-term references:

- plans/000-long-term-specification.md — Eggstack ownership and reuse
- plans/002-long-term-roadmap.md — Phase 5

Related ADRs:

- plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md
- plans/adrs/ADR-0004-eggstack-composition-and-independent-oracles.md

## 1. Purpose and ownership boundary

This subsystem integrates Eggbench with existing Eggstack capabilities while preserving sibling ownership.

Eggbench owns adapter configuration, lifecycle participation, normalization, and provenance only.

## 2. Planning-time sibling state — 2026-09-22

- Eggfetch exposes published eggfetch-core 0.2.0, HTTP/1.1 and HTTP/2, experimental HTTP/3, streaming, TLS, pooling, metrics, and a dedicated benchmark crate.
- Eggress 1.0.8 contains narrow relay, outbound, metrics, protocol, and testkit crates.
- EggServe exposes published 0.2-series Rust serving crates and controlled server/runtime behavior; H2/H3 remain opt-in or experimental.
- Eggchaos 0.1.0 is pre-release and owns deterministic bounded user-space byte-stream faults plus an Eggfetch adapter.
- EggReplay 0.1.0 is pre-release and owns semantic HTTP fixture/replay behavior; current v0.1 transport is intentionally H1-focused.
- Eggprobe 0.1.0 is pre-release, JSON-first, with direct DNS/TCP/TLS probes, Eggfetch HTTP, and Eggress routing.
- Gregg exposes a versioned HTTP status API including CPU, memory, disk I/O, network, and optional frequency telemetry.
- Eggsec owns scope-enforced security and load-testing semantics and structured results; its production load-test route uses Eggfetch.

Versions are planning evidence, not eternal pins. Every implementation plan must re-audit the sibling public surface before adoption.

## 2A. M002 handoff re-audit — 2026-09-24

The M002 implementation handoff re-audited the sibling public surfaces after C002 closure:

- Eggress 1.0.10 is published on crates.io with Rust 1.89. The required listener-free TCP route seam remains available through eggress-outbound: OutboundConnector, typed detailed connection errors, OutboundInfo, and native chain execution with no listener.
- M002 targets eggress-outbound directly rather than eggress-embed and pins exact 1.0.10 crates; it does not depend on mutable main solely for unreleased internals.
- Eggchaos v0.1.0 is now a published qualified release; eggchaos-core is the M002 production seam. Its BidirectionalChaosStream composes over an existing AsyncRead + AsyncWrite stream and owns deterministic directional byte-stream faults.
- eggchaos-eggfetch is not the M002 composition seam because its ChaosDialer establishes a direct TCP connection itself. Eggbench needs Eggress route establishment first, then eggchaos-core wrapping of the returned logical stream.
- Eggchaos has newer datagram work on main, but M002 remains explicitly stream-only. UDP/datagram impairment is not pulled into this milestone.

## 2B. M003 handoff re-audit — 2026-09-25

M003 re-audited EggReplay and Eggprobe after M002 closure.

### EggReplay

- Current default branch is workspace 0.1.0 / Rust 1.89 and continues active development beyond the originally qualified v0.1 line.
- Current planning HEAD observed: `29ce133f9845aef2efb5080988796b7ae1dbc7f8`.
- The JSON CLI is the stable seam selected for Eggbench: `validate` and `replay --output json`, envelope schema 1, RegressionReport schema 2, and documented stable exit classes.
- Long-running `serve` is not selected because the bound address is currently exposed as human stderr while running and the JSON result arrives only on shutdown.
- Eggbench therefore uses EggReplay as an external semantic workload and takes no EggReplay Rust production dependency in M003a.

### Eggprobe

- The qualified release of record is `v0.1.1` at `53ea53d`, Rust 1.89, with machine plan/report schema 0.3.
- Current main also reports package version 0.1.1 but has moved to unreleased schema 0.4/native work; SemVer alone is therefore not an adequate compatibility check.
- M003b consumes the external JSON CLI, generates schema-0.3 plans, and performs an explicit schema handshake before managed startup.
- Initial diagnostic families are DNS/TCP/TLS/HTTP only. Schema-0.4 native families, Eggprobe compare statistics, and routed diagnostic translation are outside M003.

The milestone is decomposed because replay is a workload contract while diagnostics are one-shot lifecycle evidence with different timing/failure semantics.

## 2C. Post-M003 live-tool qualification corrective — 2026-09-25

Historical M003 remains closed and hosted-qualified for Eggbench-owned contracts, but both M003 closure records explicitly lacked real external binary execution.

The additive corrective is:

- `plans/subsystems/post-m003-live-tool-qualification-corrective-addendum.md`
- `plans/implementation/post-m003-live-tool-qualification-corrective/001-eggreplay-eggprobe-live-contract-qualification.md` — STOPPED with evidence
- `plans/implementation/post-m003-live-tool-qualification-corrective/002-eggprobe-adapter-contract-correction.md` — CLOSED (`plans/closure/post-m003-live-tool-qualification-corrective/002-status.md`)

Qualification inputs are pinned:

- EggReplay exact source revision `d39f4b794620a2d0647688a914e0e7a6be42e184`, which retains envelope schema 1 / session schema 2 / RegressionReport schema 2 after the EggServe 0.3 runtime adoption;
- Eggprobe immutable `v0.1.1` at `53ea53d`, machine schema 0.3;
- current Eggprobe `0ce9597a...` only as a negative control proving real schema-0.4 rejection despite the same package-version line.

The corrective is evidence-first and expects no production Rust change. It adds real binary interoperability qualification and must preserve the historical M003 closure records unchanged. (C001 held to evidence-only; C002 required the narrow adapter correction plus the two recorded live-proven deviations D1/D2.)

C001 outcome (2026-09-25): STOPPED with evidence
(`plans/closure/post-m003-live-tool-qualification-corrective/001-status.md`).
Live execution proved the EggReplay side matches the real binary exactly but
the M003b adapter's plan/report dialect is rejected by real `eggprobe v0.1.1`.

C002 outcome (2026-09-25): CLOSED
(`plans/closure/post-m003-live-tool-qualification-corrective/002-status.md`).
The adapter now speaks the real schema-0.3 contract, the deferred live
qualification is green 20/20 locally plus hosted (live-tool run
`36177440371`, four-lane run `36177440220` on implementation `98f16e6`),
with recorded deviations limited to controlled-origin `Date` suppression
and compare-reader/writer alignment (neither changes metric, lifecycle, or
verdict semantics).

M004 implementation is unblocked subject to its own implementation plan.

## 2D. M004 Eggsec handoff re-audit — 2026-09-25

M004 re-audited Eggsec after M003 live-tool qualification closed.

Audited Eggsec default-branch HEAD:

`0509ac668adfd78e9899cd3428a807d0b3c9f27b`

Relevant findings:

- Eggsec remains workspace version 0.1.0 / Rust 1.89 and continues active development; no immutable Git release/tag was selected by this audit for the M004 machine contract.
- The broad Eggsec application crate is not an acceptable Eggbench production dependency.
- `eggsec-report-model` is a narrow data-contract crate, but the selected WAF CLI does not emit that model; importing it would not remove the external machine-contract problem.
- Generic `eggsec scan --json` emits `PipelineReport`, whose stage success primarily means execution success and whose WAF stage does not preserve a WAF regression result in the pipeline report. It is therefore not the initial correctness authority.
- `eggsec ci` is a passive gate over pre-existing findings and does not execute the assessment.
- Eggsec's internal `WafRegressionReport` is useful architecture evidence but is not the selected stable CLI seam.
- The narrow usable seam is strict-scope `eggsec waf --json`: its `ScanResults` carries explicit per-finding `bypass_successful` semantics owned by Eggsec.
- Eggsec provides no-network `preflight ... --profile guarded --json`, global `--scope`, and `--strict-scope`; these are selected to fail closed before traffic.
- Initial M004 deliberately excludes public targets, arbitrary scan profiles, stress/flood, raw packets, NSE, db-pentest, web-proxy, C2/post-exploitation, remote/cluster, credentials, custom routes, and manual enforcement overrides.

M004 is split because security execution/evidence and comparison-verdict composition are separate contracts:

1. `plans/implementation/eggstack-integration/004a-eggsec-strict-waf-correctness-adapter.md` — **ready**.
2. `plans/implementation/eggstack-integration/004b-security-correctness-gate-and-m004-closure.md` — **authored, blocked on M004a closure**.

M004a adds a distinct correctness execution category and sanitized Eggsec evidence. M004b adds ComparisonReceipt v3 correctness-gate plumbing and conservative combined verdict precedence. Security results are never converted into performance metrics.

## 3. Invariants

- No copied sibling protocol implementation.
- Integration seam, version, and capability set are recorded.
- Process/JSON adapter is preferred over depending on an unsupported sibling-internal crate.
- Requested routed behavior never silently falls back to direct.
- Credentials are redacted.
- Eggchaos stream faults are never relabeled packet faults.
- Sibling-specific failures retain enough source provenance to diagnose.

## 4. Integration policy

Preference order:

1. published narrow Rust crate with a documented reusable boundary;
2. stable local HTTP/control API with machine-readable schema;
3. stable CLI with machine-readable output;
4. exact git revision only when a pre-release capability has no stable seam and the plan includes an explicit removal gate.

The adapter should not force the full sibling application's dependency closure into a minimal Eggbench build when a process boundary is sufficient.

## 5. Dependency graph

~~~text
Runner + evidence + measurement contracts
       |
       +--> M001 EggServe + Eggfetch + Gregg
       |
       +--> M002 Eggress + Eggchaos
       |
       +--> M003 EggReplay + Eggprobe
       |
       --> M004 Eggsec security adapter
~~~

## 6. Milestones

### M001 — Controlled origin, native HTTP workload, host telemetry

Integrate EggServe, Eggfetch, and Gregg.

Prove one loopback experiment with a controlled origin, native HTTP workload, host telemetry, raw/normalized artifacts, and no duplicate HTTP implementation.

Gregg remains optional; lack of Gregg must not prevent a smaller local environment fingerprint.

### M002 — Route and stream-fault topology

Integrate listener-free Eggress seams where publicly available and Eggchaos fault orchestration. Record route/fault provenance and deterministic seeds.

No loopback proxy should be introduced solely to bridge two embeddable libraries when a stable in-process seam exists.

### M003 — Replay and diagnostics

Integrate EggReplay fixtures/workloads and Eggprobe preflight/postflight diagnostics through stable machine interfaces.

Probe latency is diagnostic unless an experiment explicitly declares it as a workload metric.

### M004 — Eggsec workload/correctness adapter

Consume explicit Eggsec profiles/results without importing scanner semantics into Eggbench core. Security correctness becomes a separate gate source.

## 7. Verification strategy

Every adapter needs:

- exact version/revision capture;
- capability detection;
- unsupported-option negative tests;
- local deterministic fixtures;
- raw evidence retention;
- normalized metric fixtures;
- credential redaction where relevant;
- cancellation/cleanup tests;
- dependency graph audit for feature isolation.

## 8. Risks and decision points

- Eggchaos v0.1.0 is published but may evolve rapidly. EggReplay remains an actively evolving 0.1 workspace, so M003a isolates through its machine CLI. Eggprobe has a qualified v0.1.1/schema-0.3 release while main has unreleased schema 0.4 work, so M003b pins the machine schema contract rather than trusting SemVer alone.
- Eggsec is broad; importing it as a library may be unjustifiably heavy.
- Eggress exposes many crates; only the smallest necessary seam should be used.
- Eggfetch native workloads are not independent oracles when Eggfetch itself is the subject.

## 9. Completion definition

The roadmap closes when Eggbench can construct useful network/security experiments mostly from Eggstack components while retaining the option to use independent external drivers.

## 10. Milestone status

### M001 — Controlled origin, native HTTP workload, host telemetry

Status: M001 hosted-qualified by C002 (run `36029547565`); M002 is implemented and closed under `plans/closure/eggstack-integration/002-status.md`; hosted qualification run `36085136434` (M001a historical closure `plans/closure/eggstack-integration/001a-status.md`, commit `8426e08`; M001b historical closure `plans/closure/eggstack-integration/001b-status.md`, commit `a0ff206`). Corrective qualification evidence: `plans/closure/post-m003-hosted-qualification-corrective/002-status.md`.

Implementation plans:

- `plans/implementation/eggstack-integration/001a-eggserve-controlled-origin-and-eggfetch-http.md`
- `plans/implementation/eggstack-integration/001b-gregg-host-telemetry.md`

Qualification note:

- M001a and M001b are landed and hosted-qualified by C002 run `36029547565`. Their original closure records explicitly left four-lane hosted qualification outstanding; C002 supplied the final current-tip qualification after C001 stopped on driver Clippy debt; neither corrective reopened Eggstack semantics.

### M002 — Route and stream-fault topology

Status: closed. Implementation plan: `plans/implementation/eggstack-integration/002-egress-route-and-eggchaos-stream-fault-topology.md`. Planning commit: `8828cdb`. Implementation commit: `f816a65`; closure: `plans/closure/eggstack-integration/002-status.md`; hosted qualification: run `36085136434` (Linux stable, Linux MSRV, macOS stable, Windows stable). C002 is closed and no dependency gate remains. The plan uses a first-class schema-v3 network-path contract, published listener-free `egress-outbound`, and published `eggchaos-core`; it explicitly rejects paired/network-path and external-oracle/network-path combinations in M002 rather than weakening existing connection/trial semantics.

### M003 — Replay and diagnostics

Status: M003a/M003b are closed and M003 is hosted-qualified for Eggbench-owned contracts. Additive real-binary interoperability qualification is closed under the post-M003 live-tool corrective (C001 stopped with evidence, C002 closed); historical closure remains unchanged.

Implementation plans:

- `plans/implementation/eggstack-integration/003a-eggreplay-semantic-replay-workload.md` — **closed**. External EggReplay semantic workload using the JSON CLI, immutable fixture identity, and semantic finding evidence. Implementation `adc3c15`; closure `plans/closure/eggstack-integration/003a-status.md`.
- `plans/implementation/eggstack-integration/003b-eggprobe-pre-post-diagnostics-and-m003-closure.md` — **closed**. Generic diagnostic lifecycle seam, Eggprobe schema-0.3 external adapter, and combined M003 closure qualification. Implementation `7712995`; closure `plans/closure/eggstack-integration/003b-status.md`; hosted qualification run `36140143375` (Linux stable, Linux MSRV, macOS stable, Windows stable).

M003a planning commit: `5e67fd0`. M003b planning commit: `46e7aa0`.

M003 deliberately does not import EggReplay/Eggprobe Rust networking engines. M003a uses EggReplay's machine CLI as a workload; M003b uses Eggprobe's qualified machine CLI as pre/post diagnostic evidence outside measured intervals.

Live-tool corrective addendum (2026-09-25): C001 proved live that the M003b adapter's plan/report dialect does not match real `eggprobe v0.1.1` (closure `plans/closure/post-m003-live-tool-qualification-corrective/001-status.md`); C002 corrected the adapter and completed the deferred live qualification (closure `plans/closure/post-m003-live-tool-qualification-corrective/002-status.md`; implementation `98f16e6`; hosted live-tool run `36177440371`; four-lane run `36177440220`). Recorded deviations: controlled-origin `Date` suppression (testbed determinism, no semantic change) and compare-reader/writer alignment (no verdict semantic change).

### M004 — Eggsec workload/correctness adapter

Status: planned/active handoff sequence. M004a is ready; M004b is authored and blocked on M004a closure.

Implementation plans:

- `plans/implementation/eggstack-integration/004a-eggsec-strict-waf-correctness-adapter.md` — **ready**. Adds schema-v6 security-check intent, a distinct correctness executor/category, strict local/private Eggsec WAF execution, sanitized evidence, and real-tool qualification.
- `plans/implementation/eggstack-integration/004b-security-correctness-gate-and-m004-closure.md` — **blocked on M004a closure**. Adds the independent security-correctness gate family, ComparisonReceipt v3, conservative combined verdict precedence, and umbrella M004 closure.

Planning commits:

- M004a: `aeed8f7`
- M004b: `5b5ef97`

M004 intentionally starts with the narrow Eggsec WAF bypass semantic seam. It does not make Eggbench a generic scanner and does not encode security correctness as a performance metric.
