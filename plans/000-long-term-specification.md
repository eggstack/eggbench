# Eggbench Long-Term Architecture and Product Specification

Status: canonical long-term implementation directive

Companion documents:

- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

This document defines the intended end state for Eggbench. It establishes product scope, experiment semantics, architectural ownership, evidence requirements, security properties, interoperability boundaries, and acceptance criteria. The roadmap decomposes this specification into dependency-ordered work. The terminology document is normative when implementation or documentation uses overlapping terms such as experiment, run, trial, workload, observation, metric, baseline, candidate, testbed, or verdict.

The keywords MUST, MUST NOT, REQUIRED, SHOULD, SHOULD NOT, and MAY are normative.

## 1. Product definition

Eggbench is a Rust-native network and security performance regression laboratory.

Its purpose is not merely to generate load. Eggbench defines, executes, records, and compares reproducible experiments across networked systems while preserving enough provenance to decide whether an observed change is attributable to the subject under test rather than to the harness, host, workload, topology, or environment.

The primary operating model is:

```text
experiment specification
        |
        v
validation + resolution
        |
        v
testbed/environment preflight
        |
        v
topology startup + readiness
        |
        v
warmup
        |
        v
repeated measured trials
        |
        v
cooldown/reset/teardown
        |
        v
normalized observations
        |
        v
baseline/candidate comparison
        |
        v
versioned evidence bundle + verdict
```

Eggbench MUST remain useful as a local single-machine tool. Distributed execution MAY be added later through an explicit remote-execution boundary; Eggbench itself MUST NOT become a general remote scheduler.

## 2. Primary goals

Eggbench MUST provide:

1. A versioned, machine-readable experiment contract independent of the CLI.
2. Deterministic local experiment orchestration with explicit startup, readiness, warmup, measurement, cooldown, reset, and teardown phases.
3. Repeated-trial measurements with latency distributions, throughput, error rates, resource observations, and target-specific metrics.
4. Baseline/candidate comparison that distinguishes practical regression thresholds from sampling uncertainty.
5. Testbed and environment provenance sufficient to detect invalid or weak comparisons.
6. Immutable, portable evidence bundles containing resolved inputs, raw measurements, normalized summaries, logs, hashes, versions, and comparison results.
7. First-class reuse of Eggstack components where they already own transport, proxying, faults, diagnostics, replay, telemetry, or security workload semantics.
8. External measurement-driver support so Eggbench can use independent implementations when benchmarking Eggstack components.
9. Security-performance experiments in which correctness/security assertions are independent gates from performance metrics.
10. JSON-first automation with a thin CLI and no requirement for a server, database, dashboard, Python runtime, container runtime, or cloud service.
11. Cross-platform operation on Linux, macOS, and Windows where the selected drivers support it, with ARM64/SBC qualification as a first-class target.
12. A future distributed-execution seam that can delegate process execution and artifact transfer to Eggwork or another explicit execution provider without moving experiment semantics out of Eggbench.

## 3. Non-goals

Eggbench is not initially:

- a replacement for Criterion;
- a general-purpose HTTP client;
- a new HTTP/TCP/UDP load-generator protocol stack;
- a proxy implementation;
- a chaos/fault engine;
- a packet simulator;
- a security scanner or exploit framework;
- a packet-capture suite;
- a CI service;
- a time-series database;
- a dashboard product;
- a distributed scheduler;
- a remote shell;
- a Kubernetes or cloud provisioning system;
- a source-of-truth performance leaderboard across unlike machines.

Eggbench MAY drive or ingest those systems through adapters. It MUST NOT absorb their complete scope.

## 4. Architectural principles

### 4.1 Experiment semantics are the product

Eggbench's core value is the reproducible experiment and evidence contract. Protocol implementations, traffic generators, fault injectors, and telemetry systems are replaceable drivers.

### 4.2 Typed core, thin presentation

The canonical experiment plan, resolved plan, observations, comparison model, verdicts, and bundle manifest MUST live in a library crate that does not depend on CLI parsing or process presentation.

JSON and other serialized forms are projections of typed structures, not strings scraped from CLI output.

### 4.3 Explicit ownership

Eggbench MUST own experiment orchestration, measurement normalization, comparison semantics, provenance, and evidence.

It MUST delegate protocol behavior to the component that already owns that behavior whenever a stable seam exists.

### 4.4 Measure the subject, not setup by accident

Unless an experiment explicitly measures startup, fixture construction, runtime creation, process spawn, readiness, or teardown, those phases MUST remain outside the timed measurement interval.

The resolved plan and result MUST identify which phases are included in every reported metric.

### 4.5 Trials are the default experimental unit

Individual requests within one trial are correlated observations and MUST NOT automatically be treated as independent statistical samples for version-to-version inference.

Repeated trials, not request count, are the default comparison unit.

Raw request latency distributions MAY be retained for descriptive tail analysis.

### 4.6 Practical significance and uncertainty are separate

A regression policy MUST define an operationally meaningful threshold independently from the uncertainty estimate.

A small statistically detectable effect MAY remain operationally acceptable. A large point estimate with insufficient evidence MUST be representable as inconclusive rather than forced into pass or fail.

### 4.7 Testbed identity is first-class

Host architecture, operating system, kernel where available, CPU, memory, toolchain, build profile, feature set, driver versions, subject revision, binary hashes, topology, and run configuration MUST be represented in evidence.

Numeric regression verdicts SHOULD require a compatible testbed policy. Cross-testbed comparisons MAY be produced descriptively but MUST NOT silently masquerade as same-testbed regression evidence.

### 4.8 Evidence is append-only

A completed run's evidence bundle is immutable. Human-friendly baseline aliases MAY move, but they MUST point to immutable bundle identities or digests.

### 4.9 Independent oracles are valuable

When Eggbench benchmarks a component built on Eggfetch/Hyper/Tokio, an external driver such as oha, h2load, or iperf3 MAY provide a useful independent measurement path.

Eggbench MUST NOT require every workload to be generated by Eggstack itself.

### 4.10 Local-first, bounded, and inspectable

Local execution is the initial supported mode. Processes, logs, telemetry, trial counts, output sizes, histories, and artifacts MUST have explicit bounds.

Remote execution is a later adapter boundary, not hidden SSH logic inside the local runner.

## 5. Canonical execution model

A resolved experiment executes through these phases:

```text
Parse
  -> Validate
  -> Resolve drivers/paths/versions
  -> Preflight environment
  -> Prepare evidence staging
  -> Start services in dependency order
  -> Readiness checks
  -> Warmup
  -> Trial loop
       -> optional reset
       -> optional cooldown
       -> synchronized telemetry start
       -> workload measurement
       -> synchronized telemetry stop
       -> trial finalization
  -> Subject/topology drain
  -> Teardown in reverse dependency order
  -> Evidence finalization
  -> Optional comparison
```

Every transition MUST be observable as structured state.

Cancellation or failure MUST converge through bounded cleanup. Teardown errors MUST be recorded and MUST NOT overwrite the original failure cause.

An experiment that fails before measurement MUST produce an invalid/error result rather than a fabricated performance verdict.

## 6. Canonical experiment plan

The experiment plan MUST be schema-versioned and contain, at minimum:

- experiment identity and human-readable name;
- subject description;
- topology/services;
- workload;
- trial count;
- warmup policy;
- measurement duration or completion rule;
- cooldown/reset policy;
- telemetry sources;
- comparison/gate policy;
- environment/testbed policy;
- artifact/log bounds;
- deterministic seed where randomness exists.

The plan MAY refer to environment variables or secret references, but serialized evidence MUST redact secret values.

Before execution, Eggbench MUST create a resolved plan containing concrete executable paths, driver versions, feature/capability selections, expanded non-secret defaults, and stable identities. The original and resolved forms MUST both remain inspectable.

## 7. Topology and service model

A topology contains named services with explicit dependencies.

Initial service kinds SHOULD include:

- command/process;
- EggServe fixture;
- Eggchaos fault endpoint;
- externally managed service;
- later driver-specific services where justified.

Each managed service MUST define:

- executable/driver identity;
- argv or typed configuration;
- explicit working directory;
- environment policy;
- dependency names;
- readiness policy;
- shutdown policy;
- log policy;
- artifact policy.

The service graph MUST be acyclic.

Shell interpolation MUST NOT be the canonical process contract. Command execution SHOULD use argv arrays so quoting does not become experiment semantics.

On Unix, process-group ownership SHOULD permit descendant cleanup. On Windows, Job Object or equivalent bounded ownership SHOULD be used when practical.

## 8. Workload model

A workload is an experiment input that produces one or more measurement streams.

Eggbench MUST support multiple workload-driver implementations behind a common contract. Initial classes are:

- native Eggfetch HTTP workloads;
- external command-backed generators;
- EggReplay-driven semantic HTTP workloads;
- Eggsec-owned security workload profiles;
- raw throughput tools such as iperf3.

Workloads MUST report their offered-load model.

At minimum Eggbench MUST distinguish:

- closed-loop concurrency;
- open-loop/fixed arrival rate;
- finite request count;
- time-bounded execution.

When a workload claims corrected latency under open-loop load, the correction method and driver must be recorded.

## 9. Metrics and observations

A metric definition MUST include:

- stable name;
- unit;
- directionality: higher-is-better, lower-is-better, target-range, or informational;
- aggregation semantics;
- source;
- whether it is eligible for gating.

Core metrics SHOULD include when available:

- throughput/rate;
- latency minimum/mean and p50/p90/p95/p99/p99.9;
- error and timeout rates;
- status/error-category distribution;
- bytes sent/received;
- process or host CPU;
- RSS/high-water memory;
- network throughput;
- disk I/O;
- connection counts;
- event-loop or target-specific counters.

Raw histogram or sample artifacts SHOULD be retained when a driver exposes them. Normalized summaries MUST never imply precision the source did not provide.

## 10. Statistical comparison

The default version-comparison unit is one completed measured trial.

For paired same-testbed experiments, Eggbench SHOULD support paired candidate/baseline comparison and deterministic interleaving to reduce temporal drift.

Relative comparisons SHOULD use ratios, preferably transformed into log-ratio space for symmetric treatment of percentage change.

The first stable comparison implementation SHOULD use transparent resampling over trial-level observations rather than assuming per-request independence.

Every gated metric MUST define a practical regression threshold.

A relative regression gate SHOULD resolve to one of:

- pass: evidence is consistent with remaining within the allowed regression budget;
- fail: evidence supports a regression beyond the configured practical threshold;
- inconclusive: available trials do not distinguish pass from fail at the configured uncertainty level;
- invalid: measurement or comparability requirements were violated.

Exact statistical methods, confidence levels, minimum trial counts, and small-sample rules are controlled by an ADR and versioned comparison-policy identifier.

## 11. Baselines

A baseline is a reference to immutable prior evidence, not merely a floating numeric threshold.

Eggbench MUST support:

- explicit bundle baseline;
- explicit absolute budget with no prior bundle;
- human-managed alias to an immutable bundle;
- descriptive comparison without a gating baseline.

A baseline selection MUST be recorded in the candidate result.

Automatic historical baseline selection MAY be added later but MUST be deterministic and inspectable.

## 12. Evidence bundle

The initial canonical bundle is a directory with a versioned manifest.

A bundle MUST be self-describing enough to inspect without a database.

A representative layout is:

```text
<run>.eggb/
  manifest.json
  plan.toml
  resolved-plan.json
  environment.json
  topology.json
  subject.json
  trials/
    001/
      result.json
      telemetry.ndjson
      stdout.log
      stderr.log
      artifacts/
  comparison.json
  report.json
```

Not every file is mandatory for every driver, but the manifest MUST state which artifacts exist.

The manifest MUST include cryptographic digests for retained artifacts and schema/version identifiers for typed files.

Finalization SHOULD be atomic: build under a staging directory, flush required files, write the manifest last, and rename into final position. Interrupted staging data MUST be distinguishable from a valid completed bundle.

## 13. Environment and testbed model

An environment fingerprint SHOULD record:

- OS and version;
- kernel version where applicable;
- architecture and target triple;
- CPU model and logical/physical core information where available;
- current CPU frequency or frequency evidence where available;
- total memory;
- Rust toolchain;
- build profile;
- relevant feature flags;
- subject commit/revision and dirty-state marker;
- subject binary digest;
- workload-driver binary digest/version;
- Eggbench version;
- selected network interface facts;
- driver-specific environment fields.

Gregg SHOULD be used for live host telemetry when available. Eggbench MUST still function without Gregg using a smaller local fingerprint.

Environment policy MAY classify drift such as thermal throttling, major background CPU load, memory pressure, or frequency instability as invalidating or warning evidence.

## 14. Eggstack ownership and reuse

Eggbench SHOULD compose the Eggstack ecosystem as follows:

### Eggfetch

Eggfetch owns HTTP client semantics, TLS, pooling, streaming, and supported proxy-aware request behavior. Eggbench MAY use `eggfetch-core` for native workloads and MUST NOT build a competing HTTP stack.

### Eggress

Eggress owns proxy routing, protocol negotiation, relay behavior, and related transport metrics. Eggbench MAY use narrow public Eggress crates or listener-free connector seams. It MUST NOT duplicate proxy protocol implementations.

### EggServe

EggServe owns deterministic inbound HTTP server/runtime behavior. Eggbench MAY use it to provide local origin fixtures and controlled server-side workloads.

### Eggchaos

Eggchaos owns deterministic user-space byte-stream faults. Eggbench MAY orchestrate fault plans and record seeds/configuration. It MUST NOT describe stream slicing/drop semantics as IP packet loss.

### EggReplay

EggReplay owns semantic HTTP recording, fixture storage, and deterministic replay. Eggbench MAY use recorded fixtures as workloads or controlled origins without absorbing the fixture format.

### Eggprobe

Eggprobe owns structured DNS/TCP/TLS/HTTP diagnostics. Eggbench MAY run preflight/postflight probes and retain reports as diagnostic evidence. Probe timings are not automatically benchmark timings.

### Gregg

Gregg owns host status/telemetry collection. Eggbench SHOULD consume the v2 status API when configured rather than recreating a remote host-monitor daemon.

### Eggsec

Eggsec owns security assessment/load profiles, scope/authorization semantics, payload corpora, and security-result meaning. Eggbench MAY drive an explicit Eggsec profile and combine its correctness results with performance evidence. Eggbench MUST NOT become a second scanner.

### SynVoid and I2PR

SynVoid, I2PR, and other systems are benchmark subjects, not Eggbench implementation dependencies unless a narrow public instrumentation interface is explicitly promoted.

## 15. External driver policy

External tools are optional adapters, not mandatory runtime dependencies.

Initial useful adapters include:

- oha for HTTP load generation and machine-readable latency/throughput evidence;
- h2load for an independent HTTP/1.1, HTTP/2, and where supported HTTP/3 path;
- iperf3 for TCP/UDP capacity experiments;
- Linux `tc netem` for explicitly privileged packet/link impairment experiments.

Every external driver MUST:

- discover and record its exact version;
- validate required capabilities before starting measurement;
- parse machine-readable output where available;
- preserve raw output as evidence;
- fail explicitly when unsupported rather than silently changing semantics.

## 16. Security-performance experiments

Security regressions MUST have separate correctness and performance gates.

A faster run is not a successful security optimization if required detections, denials, or protocol invariants regress.

A security qualification profile SHOULD bind:

- workload/corpus identity and digest;
- expected security outcomes;
- target configuration digest;
- correctness assertions;
- performance metrics;
- resource metrics.

Eggbench records the combined result but MUST NOT reinterpret Eggsec/SynVoid-specific security semantics.

Local/private test targets SHOULD be the safe default. Nonlocal targets require explicit configuration and remain the operator's responsibility to authorize.

## 17. Library and CLI surface

The CLI is a presentation adapter over library contracts.

The initial command family SHOULD converge on:

```text
eggbench validate <plan>
eggbench doctor <plan>
eggbench run <plan>
eggbench compare <baseline.eggb> <candidate.eggb>
eggbench inspect <bundle.eggb>
```

A later `matrix` command MAY expand a bounded parameter matrix.

Machine output MUST be available without ANSI/progress noise on stdout. Logs/progress SHOULD use stderr or structured sinks.

## 18. Crate architecture

The initial workspace SHOULD remain small:

```text
crates/
  eggbench-core
  eggbench-runner
  eggbench-drivers
  eggbench-cli
```

`eggbench-core` owns typed schemas, validation, metric/comparison contracts, identifiers, and evidence manifests. It SHOULD avoid Tokio and concrete network stacks.

`eggbench-runner` owns asynchronous lifecycle execution, process ownership, trial orchestration, cancellation, and evidence staging/finalization.

`eggbench-drivers` owns optional Eggstack and external adapters. Feature gates SHOULD prevent heavyweight or platform-specific integrations from entering the minimal build.

`eggbench-cli` owns argument parsing and human/machine presentation only.

A new crate requires a demonstrated ownership boundary, not merely a file-count preference.

## 19. Portability and toolchain

The project SHOULD target Rust 1.89 or newer to align with current Eggstack distribution policy.

Default code SHOULD forbid unsafe Rust unless a later accepted ADR creates a narrow reviewed exception.

Linux, macOS, and Windows are first-class local-runner platforms. Linux-only functionality such as netem MUST be feature/capability gated and clearly reported.

ARM64 Linux, including Raspberry Pi-class systems, is a qualification target. Source-build-only architectures MAY be supported where prebuilt distribution is not yet available.

## 20. Resource and harness overhead

Eggbench MUST measure and bound its own interference where practical.

The runner SHOULD avoid:

- unbounded in-memory logs;
- a shared async mutex on per-request hot paths;
- parsing human output when machine output exists;
- high-frequency polling when push/streaming telemetry exists;
- running expensive result rendering during the measured interval.

When Eggbench itself is the bottleneck, the run MUST be able to report that the generator/collector saturated or that the requested offered load was not achieved.

Control-path experiments SHOULD be supported so users can estimate harness/origin ceilings separately from subject overhead.

## 21. Compatibility and versioning

The following require explicit version identifiers:

- experiment plan schema;
- resolved-plan schema;
- evidence manifest;
- trial result;
- metric vocabulary;
- comparison policy;
- driver result formats where normalized.

Readers SHOULD ignore unknown additive fields where safe. Incompatible semantic changes require a major schema-version change or accepted migration ADR.

Completed evidence MUST remain readable or fail with an actionable compatibility error.

## 22. Distributed execution

Distributed execution is deferred until the local experiment/evidence model is stable.

The preferred future relationship is:

```text
Eggbench
  owns experiment semantics, topology intent, timing, evidence, comparison
        |
        v
Eggwork or ExecutionProvider
  owns remote process execution, node capability, cancellation,
  artifact transfer, authentication, and leases
```

Eggbench MUST NOT grow a second distributed scheduler or hidden SSH orchestration path merely to execute remote benchmarks.

## 23. Long-term acceptance

The long-term product is coherent when:

- a user can define one versioned experiment and reproduce it locally;
- lifecycle phases and measurement windows are explicit;
- repeated trials generate immutable evidence bundles;
- same-testbed candidate/baseline comparisons produce transparent pass/fail/inconclusive/invalid results;
- Eggstack transports/faults/replay/diagnostics/telemetry/security semantics are reused rather than reimplemented;
- independent external oracles can be used where methodologically useful;
- security correctness and performance are gated separately;
- supported platforms clean up processes deterministically;
- machine-readable output is stable enough for CI and agent consumption;
- remote execution can be added through a provider boundary without changing experiment semantics.
