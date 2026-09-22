# Eggbench

Eggbench is a local-first experiment and evidence system for repeatable software performance and qualification work. The foundation validates typed experiment plans, resolves them against declared driver capabilities, and defines immutable evidence bundles. The local runner owns managed process startup, readiness, warmups, repeated measured trials, explicit reset/cooldown, workload drain, bounded logs, and teardown. M002 records execution evidence only; measurement normalization and comparison remain separate capabilities.

- [Experiment plan schema](docs/experiment-plan.md)
- [Core architecture](architecture/core.md)
- [Driver resolution](docs/driver-capabilities.md)
- [Evidence bundles](docs/evidence-bundle.md)
- [Local runner lifecycle](docs/local-runner-lifecycle.md)
- [Trial orchestration](docs/trial-orchestration.md)
- [Active implementation plans](plans/registry.md)
