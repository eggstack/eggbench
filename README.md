# Eggbench

Eggbench is a local-first experiment and evidence system for repeatable software performance and qualification work. The foundation validates typed experiment plans, resolves them against declared driver capabilities, and defines immutable evidence bundles. The local runner owns managed process startup, readiness, bounded logs, and teardown. Trial scheduling and comparison capabilities are planned separately.

- [Experiment plan schema](docs/experiment-plan.md)
- [Core architecture](architecture/core.md)
- [Driver resolution](docs/driver-capabilities.md)
- [Evidence bundles](docs/evidence-bundle.md)
- [Local runner lifecycle](docs/local-runner-lifecycle.md)
- [Active implementation plans](plans/registry.md)
