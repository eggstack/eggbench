# Eggbench

Eggbench is a local-first experiment and evidence system for repeatable software performance and qualification work. It validates versioned experiment plans, resolves them against typed driver capabilities, runs managed workloads through explicit trial lifecycles, and finalizes immutable `.eggb` evidence bundles. The runner owns startup, readiness, warmups, measured trials, drain, bounded logs, and teardown. Measurement normalization and offline comparison are separate capabilities.

The local CLI surface is:

```text
eggbench validate <plan>             parse and validate a plan
eggbench doctor   <plan>             validate, resolve, and preflight without starting work
eggbench run      <plan> <bundle>    execute and finalize a bundle
eggbench inspect  <bundle>           verify and summarize a finalized bundle
```

Production resolution uses the catalog owned by `eggbench-drivers`. The external-process oracles (`oha`, `h2load`, and `iperf3`) are always catalogued. The `eggstack-http` feature adds the EggServe controlled origin and the native Eggfetch HTTP workload. The opt-in `eggstack-path` feature adds the listener-free Eggress route and deterministic Eggchaos stream-fault path:

```text
Eggfetch -> Eggress route -> Eggchaos accepted byte stream -> EggServe origin
```

`eggstack-path` implies `eggstack-http`; default and `eggstack-http`-only builds do not link Eggress or Eggchaos. `gregg` remains an orthogonal telemetry feature. Route and fault selection belongs in the plan; there is no route or fault CLI flag. Trial-synchronized Gregg telemetry is available behind its own `gregg` feature; independent external traffic generators remain separate drivers.

Network-path runs are schema v3, use route-first/fault-second semantics, reject credential-bearing routes, and never fall back from a requested proxy route to a direct connection. One Eggfetch client is retained for the whole workload executor/run, so warmups and measured trials can reuse pooled physical connections. **Eggchaos faults are user-space accepted byte-stream impairments, never packet/datagram loss.**

Complete examples are [`examples/eggstack-path.json`](examples/eggstack-path.json) and the intentionally rejected [`examples/eggstack-path-paired-unsupported.json`](examples/eggstack-path-paired-unsupported.json).

Measurement normalizes each measured trial into `trials/NNN/metrics.json`, with one observed, missing, or invalid record per requested metric. Comparison is offline and deterministic:

```text
eggbench compare <baseline.eggb> <candidate.eggb>
eggbench compare --alias <baseline.eggbaseline.json> <candidate.eggb>
eggbench compare --absolute-only <candidate.eggb>
eggbench compare --paired <bundle.eggb>
```

Path-free unpaired comparison uses `eggbench.trial-bootstrap.v1`; comparisons involving a network path use `eggbench.trial-bootstrap-network-path.v1`; paired comparison uses `eggbench.trial-bootstrap-paired.v1`. Network-path configuration is comparison-critical, while ephemeral runtime facts are not. See [comparison](docs/comparison.md) for the identity and policy rules.

- [Experiment plan schema](docs/experiment-plan.md)
- [Core architecture](architecture/core.md)
- [Driver resolution](docs/driver-capabilities.md)
- [Evidence bundles](docs/evidence-bundle.md)
- [Local runner lifecycle](docs/local-runner-lifecycle.md)
- [Trial orchestration](docs/trial-orchestration.md)
- [Environment fingerprint](docs/environment-fingerprint.md)
- [CLI reference](docs/cli.md)
- [Metrics and trial normalization](docs/metrics.md)
- [Comparison policy](docs/comparison.md)
- [Baselines](docs/baselines.md)
- [Eggstack HTTP and network path](docs/eggstack-http.md)
- [Gregg host telemetry](docs/gregg-telemetry.md)
- [External measurement oracles](docs/external-oracles.md)
- [Active implementation plans](plans/registry.md)
