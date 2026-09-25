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

Status: active handoff sequence. M003a is closed; M003b is closed; M003 is closed/hosted-qualified.

Implementation plans:

- `plans/implementation/eggstack-integration/003a-eggreplay-semantic-replay-workload.md` — **closed**. External EggReplay semantic workload using the JSON CLI, immutable fixture identity, and semantic finding evidence. Implementation `adc3c15`; closure `plans/closure/eggstack-integration/003a-status.md`.
- `plans/implementation/eggstack-integration/003b-eggprobe-pre-post-diagnostics-and-m003-closure.md` — **closed**. Generic diagnostic lifecycle seam, Eggprobe schema-0.3 external adapter, and combined M003 closure qualification. Implementation `7712995`; closure `plans/closure/eggstack-integration/003b-status.md`; hosted qualification run `36140143375` (Linux stable, Linux MSRV, macOS stable, Windows stable).

M003a planning commit: `5e67fd0`. M003b planning commit: `46e7aa0`.

M003 deliberately does not import EggReplay/Eggprobe Rust networking engines. M003a uses EggReplay's machine CLI as a workload; M003b uses Eggprobe's qualified machine CLI as pre/post diagnostic evidence outside measured intervals.

### M004 — Eggsec workload/correctness adapter

Status: unblocked for planning on M003 closure. Measurement/comparison prerequisites are already closed/qualified; M003a/M003b are closed and the combined integration layer is hosted-qualified by run `36140143375`. M004 may now be re-audited and planned; no M004 implementation plan exists yet.
