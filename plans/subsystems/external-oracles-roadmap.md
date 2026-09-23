# External Measurement Oracles Roadmap

Status: active

Long-term references:

- plans/000-long-term-specification.md — independent oracles and external driver policy
- plans/002-long-term-roadmap.md — Phase 6

Related ADR:

- plans/adrs/ADR-0004-eggstack-composition-and-independent-oracles.md

## 1. Purpose and ownership boundary

This subsystem owns adapters for established external benchmark tools that provide independent load, protocol, or capacity evidence.

It owns executable discovery, version capture, invocation translation, machine-output parsing, raw-output preservation, normalized metric mapping, and capability errors.

It does not vendor or reimplement those tools.

## 2. Initial tools

### oha

Primary HTTP load adapter. Useful for JSON output, rate and concurrency controls, and corrected-latency modes.

### h2load

Independent HTTP protocol/load path useful for HTTP/1.1, HTTP/2, and supported HTTP/3 qualification.

### iperf3

Raw TCP/UDP throughput and capacity evidence.

### Linux tc/netem

Optional privileged packet/link impairment driver. It is not a portable default and is not equivalent to Eggchaos stream faults.

## 3. Invariants

- Exact executable path and version are recorded.
- Machine-readable output is used where available.
- Raw output is retained.
- No protocol/load-model semantic fallback is allowed.
- Parsers are versioned and fixture-tested.
- Invocation uses argv rather than an implicit shell string.
- Failure to achieve offered load is visible where source evidence permits.
- External tool absence does not break minimal Eggbench installation.

## 4. Dependency graph

~~~text
Driver contracts + local runner
       |
M001 External command adapter substrate
       |
       +--> M002 oha/h2load/iperf3
       |
       --> M003 Linux netem
~~~

## 5. Milestones

### M001 — External command-driver substrate

Create explicit binary resolution and version probes, argv execution, bounded capture, raw artifact retention, parser/error contract, and fixture-based tests.

Binary resolution policy must avoid accidentally choosing an unrelated executable from an untrusted working directory.

### M002 — HTTP and capacity oracles

Implement oha, h2load, and iperf3 adapters with capability matrices and normalized metric mapping.

Do not force all tools into one common denominator; driver-specific evidence may remain as raw/typed extension fields.

### M003 — Optional Linux netem

Add explicit interface/qdisc ownership, privilege preflight, deterministic cleanup/restore, and link-layer fault provenance.

Never modify a pre-existing qdisc without an ownership and restoration contract.

## 6. Verification strategy

Fixture parsers for supported tool versions, malformed/truncated output, nonzero exit, missing binary, unsupported option, version mismatch, cancellation, log bounds, and raw-output digesting.

## 7. Completion definition

The roadmap closes when Eggbench can qualify network subjects with independent generators without making those executables mandatory dependencies.

## 8. Milestone status

M001 is ready for implementation at `plans/implementation/external-oracles/001-external-command-driver-substrate.md`. It establishes the shared `eggbench-drivers` crate/catalog and external-command substrate. M002 remains blocked on M001 closure.
