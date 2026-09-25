# `eggbench` CLI

The `eggbench` binary is a thin presentation adapter over `eggbench-core` and `eggbench-runner`. It does not duplicate orchestration: every command either calls core contracts (`parse` / `validate` / `resolve` / `inspect`) or delegates to the runner's bundle-preparation and execution seams.

## Commands and existing flags

```text
eggbench validate <plan> [--input-format toml|json]
eggbench doctor   <plan> [--input-format toml|json] [--workload-driver <name>]
eggbench run      <plan> <bundle> [--input-format toml|json] [--workload-driver <name>]
eggbench inspect  <bundle> [--manifest-json]
eggbench compare <baseline.eggb> <candidate.eggb> [--output <comparison.json>] [--seed <u64>]
eggbench compare --alias <baseline.eggbaseline.json> <candidate.eggb> [--output <comparison.json>] [--seed <u64>]
eggbench compare --absolute-only <candidate.eggb> [--output <comparison.json>] [--seed <u64>]
eggbench compare --paired <bundle.eggb> [--output <comparison.json>] [--seed <u64>]
```

`--json` and `--quiet` are global options. `validate`, `doctor`, and `run` accept a `.toml` or `.json` path, or `-` for stdin; stdin requires `--input-format` because content alone cannot determine the format. `run` requires a new destination ending in `.eggb`; `inspect` requires an existing finalized bundle. There is no route, fault, network-path, or plan-seed CLI flag: those values are declared in the plan.

`--workload-driver` on `doctor` and `run` pins the workload driver explicitly (`oha`, `h2load`, `iperf3`, or the feature-gated `eggfetch-http`). Without it, the resolver uses a unique marked default or fails with `ambiguous_selection`; unknown names fail with `missing_driver`.

## Schema-v3 path behavior

`validate` parses and semantically validates schema v3, including route shape, credential policy, fault bounds/IDs, the explicit seed requirement for non-empty faults, and the paired/external-subject restrictions. It does not compile a route, establish a connection, or start load. Core validation is independent of the optional path feature, so a feature-disabled binary can validate a valid path plan; execution remains feature-gated.

`doctor` performs validation, resolution, and environment/driver preflight without starting a managed process or making a route connection. Its `network_path` payload reports whether the `eggstack-path` feature is compiled, whether a path was requested, route mode, selected route/fault driver names and upstream versions, supported capabilities, and explicit M002 exclusions. Its `diagnostics` payload reports whether diagnostics were requested, `eggprobe` binary presence, the schema-handshake outcome, supported/explicitly-unsupported probe families, and the diagnostic-only timing policy. It reports exact adapter/sibling provenance and truthful platform/environment facts. A schema-v3 plan with a path can resolve only when the workload advertises `NetworkPath`, a matching `Route` descriptor with `ProxyRouting` is available, and—when `stream_faults` is present—a matching `Fault` descriptor with `StreamFaultPlan` is available. External oracles do not advertise that custom-dialer capability.

`run` checks feature availability, resolves the plan, lowers the route/fault configuration, and probes external tools before environment collection, bundle preparation, or managed startup. With `eggstack-path`, the supported execution combination is `eggfetch-http` with the EggServe origin. The route is established first and the Eggchaos wrapper is applied second; a requested proxy failure is returned as a request failure and never falls back to Direct. One Eggfetch client is retained for the whole run, so warmups and measured trials may reuse pooled physical connections.

Path preflight failures are reported before startup and use capability-preflight exit code `3` where applicable. Stable categories include `unsupported_network_path`, `unsupported_stream_fault_plan`, `missing_route_driver`, `missing_fault_driver`, `unsupported_route`, `invalid_route`, `invalid_fault_plan`, `missing_fault_seed`, `route_credentials_not_supported`, `workload_path_incompatible`, and `paired_network_path_not_supported`. A `Subject::External` or external-oracle workload cannot own this path. No new broad exit-code family is introduced.

`inspect` verifies the bundle before reading any evidence. A feature-enabled binary decodes and validates the manifest-listed `network-path.json` and reports its schema, route/fault provenance, ordering/layer markers, fault count, and bounded dial counters. A feature-disabled binary still reports whether the artifact is present but marks detailed path evidence unavailable. Inspection is read-only and never establishes a route. `--manifest-json` emits the normalized manifest inline.

## Feature isolation and driver inventory

The production catalog is owned by `eggbench-drivers`. The external-process drivers (`oha`, `h2load`, `iperf3`, `eggreplay-semantic`, `eggprobe`) always register. `eggstack-http` adds `eggserve-origin` and `eggfetch-http` (the unique native workload default). `eggstack-path` implies `eggstack-http` and adds `eggress-route` and `eggchaos-stream`; default and `eggstack-http`-only builds do not link Eggress or Eggchaos. `gregg` is orthogonal and uses its own feature.

`eggstack-path` changes the driver inventory, capability resolution, runtime path execution, and retained evidence. It does not add a public route/fault selection flag: the CLI consumes the catalog and the plan. Missing tool binaries, unsupported tool versions, missing native adapters, and invalid path capabilities fail before managed startup. With the path feature, `doctor` shows adapter/sibling versions, per-driver `binary_present` for external tools, supported load modes, and path capabilities; it spawns no tool except the `eggprobe` handshake when the plan requests diagnostics. `run` executes the selected native path and retains its evidence.

A deterministic `fake-load` adapter exists only through explicit test/qualification injection. It is not a production traffic generator, has no public fake-workload switch, and never appears in the production inventory. See [Eggstack HTTP and network path](eggstack-http.md) and [driver capabilities](driver-capabilities.md).

## Machine output and exit codes

Pass `--json` to emit one JSON envelope on stdout. Human progress and diagnostics go to stderr. Envelope schema v1 has this shape:

```json
{
  "schema_version": 1,
  "command": "validate|doctor|run|inspect|compare",
  "ok": true,
  "result": { "kind": "command-specific payload" },
  "error": { "category": "<stable>", "detail": "<human prose>" },
  "warnings": [{ "category": "...", "detail": "..." }]
}
```

`ok` is true when the command succeeded. On failure, `error` carries a stable category and `result` is absent. JSON mode writes exactly one document to stdout; `--quiet` suppresses optional prose but never changes the exit status.

| Code | Meaning |
|---|---|
| `0` | Command completed successfully. |
| `1` | Internal/unclassified CLI failure. |
| `2` | Parse, schema, or ordinary plan-validation error. |
| `3` | Capability, doctor, preflight, or network-path unsupported/invalid. |
| `4` | Run finalized with `Failed`/`Cancelled`/`Invalid` execution status. |
| `5` | Evidence/bundle I/O or verification failure. |
| `6` | Comparison aggregate verdict is `Fail`. |
| `7` | Comparison aggregate verdict is `Inconclusive`. |
| `8` | Comparison aggregate verdict is `Invalid`. |

A finalized non-success run retains its run result and bundle path alongside `run_non_success` and exits `4`. Network-path preflight failures do not publish a bundle or start a service.

## Cancellation and boundaries

One SIGINT/Ctrl-C signal requests cancellation through the existing runner token. In-flight route/fault work follows the normal drain and teardown path; the path adds no listener, process, or force-kill path.

Remote execution, schedulers, credential machinery for route authentication, and automatic repository discovery remain outside this milestone. Windows managed `run` remains explicitly unsupported; `validate`, `doctor`, and `inspect` continue to report truthful environment facts. See [comparison](comparison.md) and [evidence bundles](evidence-bundle.md) for receipt and artifact behavior.
