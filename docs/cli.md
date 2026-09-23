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
```

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
  "command": "validate|doctor|run|inspect",
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

Exit codes are stable and locked by subprocess tests in both JSON and
human modes: the same outcome yields the same numeric code regardless of
presentation. JSON failures still emit exactly one envelope document on
stdout; human diagnostics go to stderr. `--quiet` suppresses optional prose
but never changes the exit status. A finalized run with `Failed`, `Cancelled`,
or `Invalid` execution status retains its `run` result (including the bundle
path) alongside a stable `run_non_success` error and exits `4`. New
categories must be added through planning review.

## Driver registry and unsupported workloads

Production `eggbench` has no workload adapter yet: the production registry
is empty, `doctor` truthfully reports `has_workload_driver=false`, and `run`
fails before managed startup with the stable `missing_driver` /
`unsupported_workload` category. No service process is started and no bundle
is published on that path.

A deterministic `fake-load` adapter exists only as explicit test injection
for qualification harnesses. It is not a production traffic generator, has
no public `--fake-workload` (or similar) switch, and never appears in the
production driver inventory. Production adapters (for example `oha`,
`h2load`, `Eggfetch`) belong to External Oracles / Eggstack Integration
milestones.

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
