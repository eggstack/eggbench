# SynVoid qualification guide (M002, synthetic routine scope)

This guide covers the reproducible SynVoid security + performance
qualification suite. Routine scope runs against the synthetic stand-in;
live scope replaces it with the real SynVoid minimal binary plus the
SynVoid-owned exported assets. Live scope is a named condition in the
M002 closure until the upstream asset contract closes.

## Upstream asset contract

SynVoid owns the qualification export: live-proxy-compatible WAF fixture
subset, Detect/Pass-to-observable mapping, minimal loopback config
materialization, and provenance. Expected manifest fields and the six
harness verification checks are frozen in
`qualification/synvoid/v1/upstream-manifest.md`. Eggbench never
translates SynVoid's internal Detect/Pass semantics itself; mismatches
fail closed (`Invalid`).

Upstream plan (open):
`dbowm91/synvoid:plans/eggbench_security_qualification_asset_contract.md`

## Supported Linux/minimal build

```sh
cargo build --locked --release --no-default-features
synvoid --foreground --config-path <materialized-config>
```

Loopback-only data plane, no public bind, no admin/mesh/DNS requirement.
Normal Eggbench cancellation/drain/teardown owns the process tree.
Reference audit: SynVoid `7f1b79452a683e758e0b4ea1e70f6c0f2463f0d1`,
package `1.1.0`.

## Baseline materialization

From `qualification/synvoid/v1` (see `baselines/README.md`):

1. Materialize upstream qualification assets (live) or use the synthetic
   workspace (routine).
2. Build the minimal binary (live) or rely on the stand-in (routine).
3. Execute each baseline-required scenario into `baselines/`; record
   manifest digests.
4. Run the candidate profile against those immutable bundles; no
   automatic baseline discovery exists.

## Candidate qualification

```sh
cd qualification/synvoid/v1
eggbench qualify run perf.profile.json --output /tmp/synvoid-perf-candidate
eggbench qualify inspect /tmp/synvoid-perf-candidate/qualification-receipt.json --json
```

External-oracle scenarios run outside qualify (deviation D4):

```sh
eggbench run scenarios/oracle-oha-c8.json /tmp/oha-candidate.eggb --workload-driver oha --json
eggbench compare baselines/oracle-oha-c8.eggb /tmp/oha-candidate.eggb --json
```

## Correctness vs performance verdicts

Correctness (`eggbench.security-correctness.v2`, fixed corpus) and
performance (Measurement M003 statistical-relative gates) are
independent families with conservative combined precedence
(Invalid > Fail > Inconclusive > Pass). Faster execution never overrides
a security-correctness failure. Statistical uncertainty yields
Inconclusive, never Pass.

Frozen v1 guardrails: throughput regression 15%, p95 latency regression
20%, error_rate absolute 0 (controlled local fixture).

## External-oracle role

`oha`/`h2load` provide independence of load-generation/parser
implementation for the same benign GET class. Each driver is qualified
against its own explicit baseline bundle; native and oracle results are
never numerically compared to each other.

## Gregg host telemetry meaning

Gregg observations (`host_cpu_percent`, `host_memory_used_bytes`,
`host_memory_percent`, optional network rates) are host/testbed metrics,
not SynVoid-process metrics; evidence must say so. The checked-in
profiles declare no collector (deviation D5); enable it only where a
qualified daemon is provisioned, with gates added solely after
repeatability is demonstrated.

## Deferred shapes (M003 candidates)

Mixed malicious/benign traffic under load, request-body attack
performance campaigns, explicit connection-churn/keepalive controls,
SynVoid Prometheus/event-loop/queue ingestion, challenge/stall/tarpit
semantics, HTTP/2/TLS variants, and network-path/fault injection.

## Cleanup and evidence locations

`qualify run` refuses existing output dirs, stages atomically, and every
scenario tears down its managed services (grace 3s, descendant cleanup
owned by the runner). Evidence: suite dir (`expansion.json`,
`scenarios/*.eggb`, `scenarios/*.comparison.json`,
`qualification-receipt.json`); live harness keeps bounded provenance
summaries for 30 days as CI artifacts.
