# Eggbench

Eggbench is a local-first experiment and evidence system for repeatable software performance and qualification work. The foundation validates typed experiment plans, resolves them against declared driver capabilities, and defines immutable evidence bundles. The local runner owns managed process startup, readiness, warmups, repeated measured trials, explicit reset/cooldown, workload drain, bounded logs, and teardown. M002 records execution evidence only; measurement normalization and comparison remain separate capabilities.

The M003 local-runner slice completes the first end-to-end CLI:

```text
eggbench validate <plan>      # parse + semantic plan validation
eggbench doctor   <plan>      # validate + driver/capability/environment preflight
eggbench run      <plan> <bundle>  # full lifecycle, finalized .eggb bundle
eggbench inspect  <bundle>    # open, verify, and summarize a finalized bundle
```

Production `eggbench run` is substrate-only until an External Oracles / Eggstack driver lands: the production catalog owned by `eggbench-drivers` is empty, so `run` fails before managed startup with a stable capability category. A deterministic fake workload remains injectable in tests and qualification harnesses only. Production traffic generators (for example `oha`, `h2load`, `Eggfetch`) belong to External Oracles / Eggstack Integrations milestones. The shared external-command substrate (trusted resolution, bounded argv execution, versioned parsers) is documented in [external drivers](docs/external-drivers.md).

Measurement M001 normalizes every measured trial into `trials/NNN/metrics.json`: one `observed`/`missing`/`invalid` record per requested metric, with explicit units, direction, aggregation, and provenance. Trial — not request — is the comparison unit. No baseline comparison or verdict is implemented yet.

- [Experiment plan schema](docs/experiment-plan.md)
- [Core architecture](architecture/core.md)
- [Driver resolution](docs/driver-capabilities.md)
- [Evidence bundles](docs/evidence-bundle.md)
- [Local runner lifecycle](docs/local-runner-lifecycle.md)
- [Trial orchestration](docs/trial-orchestration.md)
- [Environment fingerprint](docs/environment-fingerprint.md)
- [CLI reference](docs/cli.md)
- [Metrics and trial normalization](docs/metrics.md)
- [Active implementation plans](plans/registry.md)

