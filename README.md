# Eggbench

A local-first experiment and evidence system. You describe an experiment as a
versioned plan; Eggbench resolves it against typed driver capabilities, runs
managed workloads through explicit trial lifecycles, and finalizes an
**immutable** `.eggb` bundle. Comparison is a separate, offline, deterministic
step over two finished bundles — the runner never decides whether your change
was an improvement.

## Quickstart

Needs Rust 1.89+ (MSRV). The `eggstack-path` feature makes this example
self-contained: origin, route, faults, and workload all run in-process, so no
external benchmark tools are required.

```bash
cargo build --release -p eggbench-cli --features eggstack-path --locked
EGGBENCH=./target/release/eggbench
mkdir -p out
```

Parse and check the plan, then resolve drivers and preflight — both start
nothing:

```bash
$EGGBENCH validate examples/eggstack-path.json   # eggbench: validate ok
$EGGBENCH doctor   examples/eggstack-path.json   # eggbench: doctor ok
```

Execute and finalize a bundle. The parent directory must already exist and the
bundle name must be new; either violation fails with exit `5` before any
measurement starts.

```bash
$EGGBENCH run examples/eggstack-path.json out/quickstart.eggb   # eggbench: run ok
$EGGBENCH inspect out/quickstart.eggb                           # eggbench: inspect ok
```

`inspect` reports only pass/fail in text mode; add `--json` for the run id,
execution status, resolved drivers, and observed metrics. A bundle is
self-describing and read-only after publication:

```text
out/quickstart.eggb/
├── manifest.json        written last; the publication record
├── plan.json            the plan as submitted
├── resolved-plan.json   capabilities and driver provenance chosen for it
├── environment.json     host fingerprint used for comparability
├── subject.json         the thing under test
├── network-path.json    route and stream-fault evidence
├── runner-phases.json   the phase schedule that actually ran
├── lifecycle/           process and runtime-topology records
└── trials/001/          metrics.json, result.json, and raw artifacts/
```

Compare offline. `--absolute-only` needs one bundle; two bundles give a
baseline-versus-candidate verdict:

```bash
$EGGBENCH compare --absolute-only out/quickstart.eggb            # eggbench: compare ok
$EGGBENCH run examples/eggstack-path.json out/second.eggb
$EGGBENCH compare out/quickstart.eggb out/second.eggb             # eggbench: compare ok
```

## Commands

```text
eggbench validate <plan>                  parse and semantically validate a plan
eggbench doctor   <plan>                  resolve drivers and preflight, starting nothing
eggbench run      <plan> <bundle>         execute and finalize a new .eggb bundle
eggbench inspect  <bundle>                verify and summarize a finalized bundle
eggbench compare  <baseline> <candidate>  compare two finalized bundles offline
```

Comparison also takes `--alias <baseline.eggbaseline.json>`, `--absolute-only`,
and `--paired`. `--json` and `--quiet` are global. Route, fault, network-path,
and seed selection live in the plan, not in flags.

A separate family works on a security-qualification profile and publishes a
qualification receipt rather than a bundle:
`qualify validate|expand <profile>`, `qualify run <profile> --output <dir>`,
and `qualify inspect <receipt.json>`. It bypasses the JSON envelope, and its
exit code is the aggregate verdict, so a correctly-detected failure exits `6`.

| Exit code | Meaning |
|---|---|
| `0` | Success |
| `1` | Internal/unclassified failure |
| `2` | Parse, schema, or plan-validation error |
| `3` | Capability, preflight, or unsupported-path error |
| `4` | Run finalized with a non-success execution status |
| `5` | Bundle I/O or verification failure |
| `6` / `7` / `8` | Comparison verdict `Fail` / `Inconclusive` / `Invalid` |

## Features

Additive; `default = []`.

| Feature | Adds |
|---|---|
| `eggstack-http` | EggServe controlled origin, the native Eggfetch HTTP workload, and the fixed HTTP corpus correctness driver |
| `eggstack-path` | Listener-free Eggress route and Eggchaos stream faults; implies `eggstack-http` |
| `gregg` | Trial-synchronized Gregg host telemetry |
| `prometheus-http` | Prometheus-scrape telemetry adapter |

`validate` works on any build, but `doctor` and `run` need the feature matching
your plan — a network-path plan on a default build fails with
`unsupported_network_path` and exit `3`. Default and `eggstack-http`-only
builds never link Eggress or Eggchaos. The seven external-process drivers
(`oha`, `h2load`, `iperf3`, `eggreplay-semantic`, `eggprobe`, `eggsec-waf`,
`eggsec-load`) are always catalogued and resolve their binary at preflight, so
a missing tool is a preflight failure rather than a link error.

## Examples

- [`examples/eggstack-path.json`](examples/eggstack-path.json) — the quickstart above
- [`examples/eggstack-path-paired-unsupported.json`](examples/eggstack-path-paired-unsupported.json) — intentionally rejected: paired experiments with a network path
- [`examples/eggstack-replay.json`](examples/eggstack-replay.json) — schema v4 semantic replay
- [`examples/eggstack-diagnostics.json`](examples/eggstack-diagnostics.json) — schema v5 replay plus pre/post diagnostics
- [`examples/security-profile.json`](examples/security-profile.json) — a `qualify` profile

## Documentation

**Contracts** — [plan schema](docs/experiment-plan.md) ·
[CLI reference](docs/cli.md) · [driver capabilities](docs/driver-capabilities.md) ·
[external driver execution](docs/external-drivers.md) ·
[evidence bundles](docs/evidence-bundle.md) · [metrics](docs/metrics.md) ·
[comparison policy](docs/comparison.md) · [baselines](docs/baselines.md) ·
[paired experiments](docs/paired-experiments.md) ·
[Eggstack HTTP and network path](docs/eggstack-http.md) ·
[Gregg telemetry](docs/gregg-telemetry.md) ·
[security qualification](docs/security-qualification.md) ·
[SynVoid profile](docs/synvoid-qualification.md) ·
[runner lifecycle](docs/local-runner-lifecycle.md) ·
[trial orchestration](docs/trial-orchestration.md) ·
[environment fingerprint](docs/environment-fingerprint.md) ·
[measurement oracles](docs/external-oracles.md)

**Architecture** — [overview and deep-dive index](architecture/overview.md) ·
[core](architecture/core.md) · [runner](architecture/runner.md) ·
[drivers](architecture/drivers.md) · [evidence](architecture/evidence.md)

**Project** — [active implementation plans](plans/registry.md)
