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

## M003: subject telemetry and load profiles

`qualification/synvoid/v2` is the M003 workspace. Routine scope runs against
the synthetic stand-in; live scope materializes the owner contracts from the
pinned SynVoid tree with
`cargo xtask eggbench-qualification export` and drives the real minimal
binary. Both scopes run the same stages in
`scripts/qualification/synvoid-m003/run-live-qualification.sh`.

Subject telemetry is the owner's: `telemetry/telemetry-mapping.json` is the
owner mapping byte-for-byte (raw SHA-256
`622f6a13c4353cc7465cce39a57ed86fa0db2fe4124258e6f06226c1748d2d99`;
contract `synvoid.eggbench-telemetry.v2`), and plans pin its Eggbench
content identity. The owner declares 12 samples, 10 required and 2 optional;
the optional pair stays absent in the minimal runtime and is recorded as
missing with a warning, never as zero. Ten required fields that drift, reset,
or disappear fail the run closed.

The live harness starts one owner instance for the whole qualification and
declares the subject and its controlled origin as external services in the
plans. A per-scenario managed subject would contend for the owner metrics
port, and a child that loses that race publishes no series, which would make
the telemetry evidence describe whichever process still held the port.

Load shapes are `body-pooled`, `body-fresh`, `mixed-80-20-pooled`,
`mixed-80-20-fresh`, an origin-only control, and the telemetry-pressure and
optional-absence trials. Only `body-gated-c8` carries performance policy
gates (statistical-relative throughput and p95 latency against an explicit
baseline bundle); every other performance observation is diagnostic.
`eggsec-load` executes one reviewed case per trial as the security-owner
path, and `oha` supplies transport corroboration where the request shape is
supported. Both are optional external oracles and neither substitutes for the
native driver.

## Still deferred

Explicit connection-churn/keepalive controls, challenge/stall/tarpit
semantics, HTTP/2/TLS variants, and network-path/fault injection.

## Cleanup and evidence locations

`qualify run` refuses existing output dirs, stages atomically, and every
scenario tears down its managed services (grace 3s, descendant cleanup
owned by the runner). Evidence: suite dir (`expansion.json`,
`scenarios/*.eggb`, `scenarios/*.comparison.json`,
`qualification-receipt.json`); live harness keeps bounded provenance
summaries for 30 days as CI artifacts.
