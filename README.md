# Eggbench

Eggbench runs local experiments from versioned plans and saves the results in
immutable `.eggb` bundles. You can validate a plan, check that its drivers are
available, run it, inspect the evidence, and compare completed runs offline.
Running an experiment and deciding whether it improved are separate steps.

## Quickstart

Install Rust 1.89 or newer. This example enables `eggstack-path`, which includes
the origin, route, stream faults, and workload needed by the example. It does
not require external benchmark tools.

```bash
cargo build --release -p eggbench-cli --features eggstack-path --locked
export EGGBENCH=./target/release/eggbench
mkdir -p out
```

Validate the plan, then check driver capabilities and the local environment.
These commands do not start managed services. If the plan requests diagnostics,
`doctor` also performs the `eggprobe` schema handshake.

```bash
$EGGBENCH validate examples/eggstack-path.json
$EGGBENCH doctor examples/eggstack-path.json
```

Run the plan and inspect the finalized bundle. The output directory must
already exist, and the bundle path must be new. Inspection verifies the bundle
and is read-only. Use `--json` to see details such as the run id, execution
status, resolved drivers, and metrics.

```bash
$EGGBENCH run examples/eggstack-path.json out/quickstart.eggb
$EGGBENCH inspect out/quickstart.eggb --json
```

Compare two completed bundles without rerunning either experiment. A comparison
verdict can be Pass, Fail, Inconclusive, or Invalid. The command's exit code
reports that verdict.

```bash
$EGGBENCH run examples/eggstack-path.json out/candidate.eggb
$EGGBENCH compare out/quickstart.eggb out/candidate.eggb --json
```

For an absolute-threshold check from one bundle, use
`eggbench compare --absolute-only <bundle.eggb>`.

## Commands

```text
eggbench validate <plan>
eggbench doctor <plan>
eggbench run <plan> <new-bundle.eggb>
eggbench inspect <bundle.eggb>
eggbench compare <baseline.eggb> <candidate.eggb>
eggbench qualify validate|expand <profile>
eggbench qualify run <profile> --output <new-directory>
eggbench qualify inspect <receipt.json>
```

`--json` and `--quiet` are global options. Plan commands accept JSON or TOML;
use `-` with `--input-format json|toml` to read a plan from standard input.
Network route, fault, and seed settings belong in the plan. `--workload-driver`
on `doctor` or `run` selects a registered workload driver explicitly.

`qualify` runs a security qualification profile and writes a receipt rather
than an `.eggb` bundle. Its exit code is the aggregate verdict: `0` for Pass,
`6` for Fail, `7` for Inconclusive, and `8` for Invalid. The other commands use
these exit codes:

| Code | Meaning |
|---|---|
| `0` | Success |
| `1` | Internal or unclassified failure |
| `2` | Parse, schema, or plan-validation error |
| `3` | Capability, preflight, or unsupported-path error |
| `4` | Run finalized with a non-success execution status |
| `5` | Bundle I/O or verification failure |
| `6` / `7` / `8` | Comparison verdict: Fail / Inconclusive / Invalid |

## Features and drivers

Features are opt-in; the default build enables none.

| Feature | Adds |
|---|---|
| `eggstack-http` | EggServe controlled origin, Eggfetch HTTP workload, and HTTP corpus correctness checks |
| `eggstack-path` | Eggress routing and Eggchaos stream faults; also enables `eggstack-http` |
| `gregg` | Trial-synchronized Gregg host telemetry |
| `prometheus-http` | Prometheus scrape telemetry adapter |

`doctor` reports whether external tool binaries are present. `run` checks
required tools during preflight. Workload drivers include `oha`,
`h2load`, `iperf3`, `eggreplay-semantic`, `eggsec-load`, and feature-gated
`eggfetch-http`. `eggsec-waf` provides correctness checks. A network-path plan
needs a build with `eggstack-path`; the default build cannot run it.

## Examples and documentation

- [`examples/eggstack-path.json`](examples/eggstack-path.json): the quickstart plan
- [`examples/eggstack-replay.json`](examples/eggstack-replay.json): semantic replay
- [`examples/eggstack-diagnostics.json`](examples/eggstack-diagnostics.json): replay with diagnostics
- [`examples/security-profile.json`](examples/security-profile.json): qualification profile

Start with the [CLI guide](docs/cli.md), then see the [experiment plan
reference](docs/experiment-plan.md). Learn how to read [evidence
bundles](docs/evidence-bundle.md), [metrics](docs/metrics.md), and
[comparison results](docs/comparison.md). For capability selection, see
[driver capabilities](docs/driver-capabilities.md).

Specialized guides cover [network paths](docs/eggstack-http.md), [security
qualification](docs/security-qualification.md), [SynVoid
qualification](docs/synvoid-qualification.md), [external drivers](docs/external-drivers.md),
[telemetry](docs/gregg-telemetry.md), and [paired experiments](docs/paired-experiments.md).
The [architecture overview](architecture/overview.md) is intended for
contributors.
