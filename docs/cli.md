# CLI guide

The `eggbench` command validates experiment plans, checks local driver
capabilities, runs experiments, inspects evidence, and compares completed runs.
It also has a separate command family for security qualification profiles.

## Commands

```text
eggbench validate <plan> [--input-format json|toml]
eggbench doctor <plan> [--input-format json|toml] [--workload-driver <name>]
eggbench run <plan> <new-bundle.eggb> [--input-format json|toml] [--workload-driver <name>]
eggbench inspect <bundle.eggb> [--manifest-json]

eggbench compare <baseline.eggb> <candidate.eggb> [--output <receipt.json>] [--seed <u64>]
eggbench compare --alias <baseline.eggbaseline.json> <candidate.eggb> [--output <receipt.json>] [--seed <u64>]
eggbench compare --absolute-only <candidate.eggb> [--output <receipt.json>] [--seed <u64>]
eggbench compare --paired <bundle.eggb> [--output <receipt.json>] [--seed <u64>]

eggbench qualify validate <profile>
eggbench qualify expand <profile>
eggbench qualify run <profile> --output <new-directory>
eggbench qualify inspect <qualification-receipt.json>
```

`--json` and `--quiet` are global options. `validate`, `doctor`, and `run`
accept JSON or TOML plans. To read from standard input, use `-` and specify
`--input-format`; the format is not inferred. Network route, fault, and seed
settings are declared in the plan rather than selected with CLI flags.

## Validate, check, and run

`validate` parses a plan and checks its schema and semantics. `doctor` also
resolves driver capabilities and checks required tools and environment facts.
Neither starts managed services. If diagnostics are requested, `doctor` performs
the `eggprobe` schema handshake.

`run` resolves the plan and checks features and external tools before preparing
the bundle or starting managed workloads. The output must end in `.eggb`, its
parent directory must exist, and the destination must be new. Preflight and
path capability failures occur before startup. A run that starts but finishes
with a non-success execution status is finalized with its evidence and exits
with code `4`.

`--workload-driver` on `doctor` or `run` selects a workload driver by name.
Registered drivers include `oha`, `h2load`, `iperf3`, `eggreplay-semantic`,
`eggsec-load`, and, when built with `eggstack-http`, `eggfetch-http`. The option
selects a driver, but does not install or add its executable to `PATH`.

For path plans, the build must include `eggstack-path`, which also enables
`eggstack-http`. Route and fault capability requirements are checked against
the selected drivers. A requested proxy never falls back to a direct route.
See [Eggstack HTTP and network paths](eggstack-http.md) for supported path
combinations and evidence.

`inspect` verifies a finalized bundle and summarizes it. It is read-only.
`--manifest-json` emits the normalized manifest inline.

## Compare runs

Comparison is offline and does not modify bundles. Give it a baseline and a
candidate, an alias file and candidate, one candidate for absolute thresholds,
or one bundle for paired analysis. `--output` writes a JSON receipt, and
`--seed` sets the deterministic bootstrap seed. Comparison policies and
compatibility rules are described in [Comparison](comparison.md).

## Machine-readable output

For the five plan-chain commands (`validate`, `doctor`, `run`, `inspect`, and
`compare`), `--json` writes one JSON envelope to stdout. Human progress and
diagnostics go to stderr. The envelope includes `schema_version`, `command`,
`ok`, and either a command result or a stable error category and detail. JSON
mode writes one document; `--quiet` suppresses optional prose without changing
the result or exit code.

`qualify` has its own JSON payloads and does not use this envelope. `--quiet`
is accepted but does not suppress its output. Its `inspect` argument is the
receipt file inside the published output directory.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Command succeeded, or comparison verdict is Pass |
| `1` | Internal or unclassified failure |
| `2` | Parse, schema, or plan-validation error |
| `3` | Capability, preflight, or unsupported-path error |
| `4` | Run finalized with Failed, Cancelled, or Invalid execution status |
| `5` | Bundle I/O or verification failure |
| `6` | Comparison or qualification verdict is Fail |
| `7` | Comparison or qualification verdict is Inconclusive |
| `8` | Comparison or qualification verdict is Invalid |

A comparison or qualification command can therefore complete its work and
return a nonzero exit code to report the measured verdict. See [security
qualification](security-qualification.md) for profile workflows and receipts.

## Platform support

Managed `run` is currently unsupported on Windows. `validate`, `doctor`, and
`inspect` remain available and report the capabilities of the current build
and environment.
