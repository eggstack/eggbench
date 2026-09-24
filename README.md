# Eggbench

Eggbench is a local-first experiment and evidence system for repeatable software performance and qualification work. The foundation validates typed experiment plans, resolves them against declared driver capabilities, and defines immutable evidence bundles. The local runner owns managed process startup, readiness, warmups, repeated measured trials, explicit reset/cooldown, workload drain, bounded logs, and teardown. M002 records execution evidence only; measurement normalization and comparison remain separate capabilities.

The M003 local-runner slice completes the first end-to-end CLI:

```text
eggbench validate <plan>      # parse + semantic plan validation
eggbench doctor   <plan>      # validate + driver/capability/environment preflight
eggbench run      <plan> <bundle>  # full lifecycle, finalized .eggb bundle
eggbench inspect  <bundle>    # open, verify, and summarize a finalized bundle
```

Production `eggbench run` resolves against the production catalog owned by `eggbench-drivers`. The catalog always carries the external-process oracles (`oha`, `h2load`, `iperf3`, selectable with `--workload-driver`); with the `eggstack-http` feature it additionally executes the first Eggstack-native path — an EggServe loopback controlled origin driven by a native Eggfetch workload (the unique workload default):

```sh
cargo build -p eggbench-cli --features eggstack-http
./target/debug/eggbench run crates/eggbench-core/tests/fixtures/eggstack-loopback.json loopback.eggb
```

See [Eggstack HTTP](docs/eggstack-http.md). Trial-synchronized host
telemetry from a loopback Gregg daemon is available behind the `gregg`
cargo feature (`eggbench-cli/gregg`); see
[Gregg telemetry](docs/gregg-telemetry.md). A deterministic fake workload remains injectable in tests and qualification harnesses only. Independent external traffic generators (for example `oha`, `h2load`) belong to External Oracles milestones. The shared external-command substrate (trusted resolution, bounded argv execution, versioned parsers) is documented in [external drivers](docs/external-drivers.md).

Measurement M001 normalizes every measured trial into `trials/NNN/metrics.json`: one `observed`/`missing`/`invalid` record per requested metric, with explicit units, direction, aggregation, and provenance. Trial — not request — is the comparison unit. No baseline comparison or verdict is implemented yet.

Measurement M002 compares two immutable bundles under policy `eggbench.trial-bootstrap.v1` without modifying them:

```text
eggbench compare <baseline.eggb> <candidate.eggb>
eggbench compare --alias <baseline.eggbaseline.json> <candidate.eggb>
eggbench compare --absolute-only <candidate.eggb>
```

Deterministic trial-level bootstrap (10,000 resamples, 95% interval), practical thresholds, digest-pinned baseline aliases, and pass/fail/inconclusive/invalid aggregation with additive exit codes 6/7/8. See [comparison](docs/comparison.md) and [baselines](docs/baselines.md).

Measurement M003 runs drift-controlled paired experiments in one bundle and compares arms under policy `eggbench.trial-bootstrap-paired.v1`:

```text
eggbench compare --paired <bundle.eggb>
```

Predeclared alternating baseline/candidate schedule, per-trial arm and pair identities, pair-resampling bootstrap, descriptive drift diagnostics, and fail-closed guards against post-hoc pairing. See [paired experiments](docs/paired-experiments.md).

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
- [Eggstack HTTP native path](docs/eggstack-http.md)
- [Gregg host telemetry](docs/gregg-telemetry.md)
- [External measurement oracles](docs/external-oracles.md)
- [Active implementation plans](plans/registry.md)

