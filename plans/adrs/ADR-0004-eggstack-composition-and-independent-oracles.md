# ADR-0004: Eggstack Composition and Independent Oracles

Status: accepted

Date: 2026-09-22

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#43-explicit-ownership`
- `plans/000-long-term-specification.md#49-independent-oracles-are-valuable`
- `plans/000-long-term-specification.md#14-eggstack-ownership-and-reuse`
- `plans/000-long-term-specification.md#15-external-driver-policy`

Affected subsystem roadmaps:

- `plans/subsystems/eggstack-integration-roadmap.md`
- `plans/subsystems/external-oracles-roadmap.md`
- `plans/subsystems/security-qualification-roadmap.md`

## Context

Eggstack already contains mature or emerging components for HTTP, proxying, serving, chaos, replay, diagnostics, host telemetry, and security assessment. Reimplementing those inside Eggbench would enlarge its attack surface and undermine the ecosystem.

At the same time, benchmarking an Eggstack component exclusively through the same underlying implementation can create circular evidence. Independent generators are methodologically useful.

## Decision drivers

- maximize reuse;
- minimize duplicated protocol code;
- keep Eggbench lightweight;
- preserve independent benchmark paths;
- avoid binding Eggbench to unstable sibling internals;
- retain exact driver provenance.

## Decision

Eggbench SHALL prefer the narrowest stable integration seam.

Preference order:

1. published narrow Rust crate with an appropriate ownership contract;
2. stable machine-readable local HTTP/control API;
3. stable CLI with machine-readable output;
4. exact git revision only when required for a pre-release integration and accompanied by an explicit removal gate.

Eggbench SHALL NOT copy sibling implementation code to avoid an integration seam.

Initial ownership map:

- Eggfetch: HTTP client/TLS/pooling/streaming;
- Eggress: routing/proxy/relay;
- EggServe: inbound controlled origin;
- Eggchaos: deterministic user-space byte-stream faults;
- EggReplay: semantic HTTP fixtures/replay;
- Eggprobe: DNS/TCP/TLS/HTTP diagnostics;
- Gregg: host telemetry;
- Eggsec: security workload/scope/finding semantics.

Eggbench owns none of those protocol semantics.

External drivers such as oha, h2load, and iperf3 are first-class optional oracles. Their raw machine output SHALL be preserved alongside normalized Eggbench metrics.

No driver may silently fall back from a requested protocol, route, impairment type, TLS policy, load model, or target mode.

If a sibling public seam is insufficient, the roadmap SHALL record the missing capability and either use a process adapter or wait; Eggbench MUST NOT create a private fork of the sibling protocol implementation.

## Stream faults versus packet faults

Eggchaos operates on user-space byte streams and MUST be described as such.

Linux netem, when later supported, operates at a link/packet scheduling layer and MUST be reported separately.

A result cannot relabel stream slicing or dropped stream chunks as packet loss.

## Independent-oracle rule

When the subject under test and the proposed driver share substantial implementation machinery that could mask or create the effect under study, qualification SHOULD include an independent driver where feasible.

Examples:

- Eggfetch performance: use oha/h2load for end-to-end server tests in addition to Eggfetch's internal Criterion benches;
- SynVoid/Hyper path: use oha/h2load as external load source;
- raw relay capacity: use iperf3 where suitable.

Independent drivers supplement rather than replace component-native microbenchmarks.

## Consequences

### Positive

- low duplication;
- clear project ownership;
- smaller default dependency graph;
- stronger independent evidence;
- sibling improvements flow through adapters.

### Negative

- several adapters and output parsers;
- pre-release sibling APIs may temporarily require process integration;
- normalized metrics cannot erase all driver differences.

## Compatibility and migration

Each driver stores its implementation identifier, version/revision, capability set, and normalized-result schema version in evidence.

Driver upgrades that change semantics require requalification and may require a new adapter schema version.

## Security and reliability

Credentials from Eggfetch/Eggress/Eggsec routes must be redacted before evidence.

External executable resolution must be explicit and recorded; Eggbench must not search arbitrary working-directory binaries ahead of operator-selected paths without a documented policy.

## Verification

Each driver integration requires:

- capability tests;
- unsupported-option negative tests;
- local deterministic fixture;
- version discovery;
- raw-output retention;
- normalized-metric parity fixture;
- secret redaction tests where relevant.

## Supersession

None.
