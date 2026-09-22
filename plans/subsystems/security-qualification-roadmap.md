# Security Performance Qualification Roadmap

Status: proposed

Long-term references:

- plans/000-long-term-specification.md — security-performance experiments
- plans/002-long-term-roadmap.md — Phase 7

Related ADR:

- plans/adrs/ADR-0004-eggstack-composition-and-independent-oracles.md

## 1. Purpose and ownership boundary

This subsystem defines how Eggbench combines security correctness evidence with performance and resource evidence.

Eggbench does not decide whether a payload is malicious or whether a WAF/scanner finding is semantically correct. Those meanings remain with Eggsec, SynVoid, or another explicit security workload owner.

## 2. Invariants

- Correctness and performance are independent gate families.
- Performance cannot override failed correctness.
- Corpus, profile, and target configuration digests are evidence.
- Expected security outcomes are fixed before candidate interpretation.
- Local/private lab targets are the safe default.
- No unbounded stress or flood profile is introduced as ordinary CI.
- Security payload/log redaction requirements are explicit.

## 3. Initial target: SynVoid

SynVoid is an especially useful first subject because it exposes a reverse proxy/WAF data plane, modern HTTP transport, rate limiting and bot controls, CPU/offload-sensitive detection paths, event-loop/resource telemetry, and prior performance campaign methodology.

Initial qualification profiles should cover:

- benign small requests;
- suspicious/malicious requests with expected dispositions;
- mixed benign/malicious traffic;
- representative small and larger body paths;
- connection churn;
- bounded concurrency ladder such as 1/8/32/128 where appropriate;
- routed subject versus direct-origin control;
- RSS, CPU, event-loop, queue, and target-specific telemetry where exposed.

## 4. Dependency graph

~~~text
Measurement gates + Eggstack/external drivers
       |
M001 Qualification-profile and correctness-gate contract
       |
M002 SynVoid reproducible profile suite
       |
M003 Reusable Eggstack security qualification patterns
~~~

## 5. Milestones

### M001 — Profile and correctness contract

Define reusable profile expansion, corpus/config digesting, expected-outcome assertions, security-result adapter semantics, and combined verdict rules.

A correctness failure must remain distinguishable from a benchmark invalidity or performance regression.

### M002 — SynVoid suite

Build local controlled-origin WAF/proxy profiles and compare native and external workload drivers. Preserve target configuration and security result evidence.

Start with bounded local profiles rather than internet-facing or defense-lab flood modes.

### M003 — Reusable security performance patterns

Extend to Eggsec defense-validation/load profiles and other authorized local security services without turning Eggbench into a scanner.

## 6. Verification strategy

Use fixed local corpora with known expected dispositions, intentional false-positive/false-negative fixture failures, target configuration hash changes, performance-only regressions, correctness-only regressions, and combined failures.

## 7. Completion definition

The roadmap closes when a security optimization cannot be reported as successful solely because it is faster while required security behavior regressed.

## 8. Milestone status

Blocked on measurement and integration layers.
