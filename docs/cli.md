# `eggbench` CLI

The `eggbench` binary is the local-runner command surface shipped in M003. It
is a thin presentation adapter over `eggbench-core` and `eggbench-runner`. The
binary does not duplicate orchestration; every command either calls into core
contracts (`parse` / `validate` / `resolve` / `inspect`) or delegates to the
runner’s bundle-preparation and execution seams.

## Commands

```text
eggbench validate <plan>     Parse and validate an experiment plan.
eggbench doctor <plan>       Validate plus driver/capability/environment preflight.
eggbench run <plan> <bundle> Validate, resolve, prepare, and execute the experiment.
eggbench inspect <bundle>    Open, verify, and summarize a finalized bundle.
eggbench compare <baseline.eggb> <candidate.eggb>
eggbench compare --alias <baseline.eggbaseline.json> <candidate.eggb>
eggbench compare --absolute-only <candidate.eggb>
```

`compare` never modifies either bundle. It emits the standalone versioned
comparison receipt as machine JSON (stdout, or `--output <comparison.json>`)
with `--seed <u64>` available for explicit seeding. Aggregate `Fail`,
`Inconclusive`, and `Invalid` retain the compare result alongside a stable
error and exit 6, 7, or 8; passing, descriptive-only, and no-verdict
comparisons exit 0. See [comparison](comparison.md) and
[baselines](baselines.md).

`validate`, `doctor`, and `run` accept `<plan>` as a path to a `.toml` or
`.json` file, or `-` for stdin. Stdin requires `--input-format toml|json`
because the parser cannot infer the format from content.

`run` requires an explicit destination bundle path (must end in `.eggb` and
must not already exist). `inspect` requires an existing finalized bundle.

## Machine output contract

Pass `--json` to emit a single JSON envelope on stdout. Human progress and
diagnostics go to stderr and are intentionally outside the compatibility
surface.

Envelope schema v1:

```jsonc
{
  "schema_version": 1,
  "command": "validate|doctor|run|inspect|compare",
  "ok": true,
  "result": { /* command-specific payload */ },
  "error": { "category": "<stable>", "detail": "<human prose>" },
  "warnings": [{ "category": "...", "detail": "..." }]
}
```

`ok` is `true` when the command succeeded. On failure, `error` carries a
stable `category` plus human-readable detail; `result` is absent. The
machine envelope is the compatibility surface; human prose is not.

JSON mode writes exactly one JSON document to stdout. Stderr receives human
diagnostics unless `--quiet` is set.

## Exit codes

The CLI uses a compact stable mapping:

| Code | Meaning |
|------|---------|
| `0` | Command completed successfully. |
| `1` | Internal/unclassified CLI failure. |
| `2` | Parse, schema, or plan validation error. |
| `3` | Capability / doctor / preflight unsupported or invalid. |
| `4` | Run completed with `Failed`/`Cancelled`/`Invalid` execution status. |
| `5` | Evidence/bundle I/O or verification failure. |
| `6` | Comparison aggregate verdict is `Fail`. |
| `7` | Comparison aggregate verdict is `Inconclusive`. |
| `8` | Comparison aggregate verdict is `Invalid`. |

Exit codes are stable and locked by subprocess tests in both JSON and
human modes: the same outcome yields the same numeric code regardless of
presentation. JSON failures still emit exactly one envelope document on
stdout; human diagnostics go to stderr. `--quiet` suppresses optional prose
but never changes the exit status. A finalized run with `Failed`, `Cancelled`,
or `Invalid` execution status retains its `run` result (including the bundle
path) alongside a stable `run_non_success` error and exits `4`. New
categories must be added through planning review.

## Driver registry and unsupported workloads

Production `eggbench` resolves against the production catalog owned by
`eggbench-drivers` (`DriverCatalog::production`). Without the
`eggstack-http` feature the catalog is empty, `doctor` truthfully reports
`has_workload_driver=false`, and `run` fails before managed startup with
the stable `missing_driver` / `unsupported_workload` category. No service
process is started and no bundle is published on that path. With the
feature, the catalog registers the `eggserve-origin` service adapter and
the `eggfetch-http` workload driver, plus the `gregg` telemetry driver
with its own feature; `doctor` shows exact adapter/sibling versions and
supported load-mode capabilities, and `run` executes the native loopback
path. The CLI consumes the drivers catalog rather than owning
registration. `doctor` validates declared Gregg endpoint config syntax
(loopback policy) without dialing; live health/status probing stays in
`run` preflight.

A deterministic `fake-load` adapter exists only as explicit test injection
for qualification harnesses. It is not a production traffic generator, has
no public `--fake-workload` (or similar) switch, and never appears in the
production driver inventory. Independent external adapters (for example
`oha`, `h2load`) belong to External Oracles milestones. See
[Eggstack HTTP](eggstack-http.md).

## Cancellation

`run` wires one SIGINT/Ctrl-C signal into the existing M002
`CancellationToken`: the signal only requests cancellation, and the normal
drain/teardown path remains authoritative. Cancellation produces a finalized
`Cancelled` bundle with mandatory cleanup, retains the bundle in the code-4
result, and leaves no detached signal-listener task after completion. A
second "force kill everything immediately" path is intentionally absent.

## Inspection

`eggbench inspect` verifies the bundle before summarizing. The default
summary includes the manifest schema, run identity, execution status,
comparison verdict (when present), legacy v1 status (when applicable),
subject summary, driver inventory, environment fingerprint summary, trial
identities, artifact count, and total bytes.

Pass `--manifest-json` to emit the normalized manifest JSON inline.

## Environment fingerprint

`eggbench doctor` and `eggbench inspect` surface a summary of the collected
`EnvironmentFingerprint`. Each field is classified as
`comparison_critical`, `warning_only`, or `informational`. Missing optional
fields remain absent; the collector never fabricates `unknown` placeholders.
See [`environment-fingerprint.md`](environment-fingerprint.md) for the
authoritative field table.

## Boundaries

- No production traffic generator, comparison engine, daemon, or TUI.
- No remote execution, scheduler, or credential machinery.
- No automatic Git crawl or repository discovery for subject identity.
- Windows managed `run` remains explicitly unsupported; `validate`,
  `doctor`, and `inspect` continue to work and report truthful
  environment facts.
