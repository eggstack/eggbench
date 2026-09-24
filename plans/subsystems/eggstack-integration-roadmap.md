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

- Eggress default branch is workspace 1.0.10 / Rust 1.89; the latest published GitHub release inspected is v1.0.9. The required listener-free TCP route seam already exists in published v1.0.9 through eggress-outbound: OutboundConnector, typed detailed connection errors, OutboundInfo, and native chain execution with no listener.
- M002 therefore targets eggress-outbound directly rather than eggress-embed. Implementation must pin the newest published compatible 1.0.x after re-audit and must not depend on mutable main solely for unreleased internals.
- Eggchaos v0.1.0 is now a published qualified release; eggchaos-core is the M002 production seam. Its BidirectionalChaosStream composes over an existing AsyncRead + AsyncWrite stream and owns deterministic directional byte-stream faults.
- eggchaos-eggfetch is not the M002 composition seam because its ChaosDialer establishes a direct TCP connection itself. Eggbench needs Eggress route establishment first, then eggchaos-core wrapping of the returned logical stream.
- Eggchaos has newer datagram work on main, but M002 remains explicitly stream-only. UDP/datagram impairment is not pulled into this milestone.

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

- Eggchaos v0.1.0 is published, but its API may still evolve rapidly; EggReplay/Eggprobe remain evolving integration seams. Pin published versions and preserve narrow ownership boundaries.
- Eggsec is broad; importing it as a library may be unjustifiably heavy.
- Eggress exposes many crates; only the smallest necessary seam should be used.
- Eggfetch native workloads are not independent oracles when Eggfetch itself is the subject.

## 9. Completion definition

The roadmap closes when Eggbench can construct useful network/security experiments mostly from Eggstack components while retaining the option to use independent external drivers.

## 10. Milestone status

### M001 — Controlled origin, native HTTP workload, host telemetry

Status: M001 hosted-qualified by C002 (run `36029547565`); M002 is now ready for handoff under its authored implementation plan (M001a historical closure `plans/closure/eggstack-integration/001a-status.md`, commit `8426e08`; M001b historical closure `plans/closure/eggstack-integration/001b-status.md`, commit `a0ff206`). Corrective qualification evidence: `plans/closure/post-m003-hosted-qualification-corrective/002-status.md`.

Implementation plans:

- `plans/implementation/eggstack-integration/001a-eggserve-controlled-origin-and-eggfetch-http.md`
- `plans/implementation/eggstack-integration/001b-gregg-host-telemetry.md`

Qualification note:

- M001a and M001b are landed and hosted-qualified by C002 run `36029547565`. Their original closure records explicitly left four-lane hosted qualification outstanding; C002 supplied the final current-tip qualification after C001 stopped on driver Clippy debt; neither corrective reopened Eggstack semantics.

### M002 — Route and stream-fault topology

Status: ready for handoff. Implementation plan: `plans/implementation/eggstack-integration/002-egress-route-and-eggchaos-stream-fault-topology.md`. Planning commit: `8828cdb`. C002 is closed and no dependency gate remains. The plan uses a first-class schema-v3 network-path contract, published listener-free `egress-outbound`, and published `eggchaos-core`; it explicitly rejects paired/network-path and external-oracle/network-path combinations in M002 rather than weakening existing connection/trial semantics.

### M003 — Replay and diagnostics

Status: blocked on M001/M002 integration seams.

### M004 — Eggsec workload/correctness adapter

Status: blocked on measurement comparison plus preceding integration seams.
