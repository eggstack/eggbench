# Driver capabilities and plan resolution

`DriverDescriptor` records the canonical adapter name, adapter version, upstream identity/version, one category, structured capabilities, supported platforms, optional output schema, and process-backed status. Capabilities are typed (for example, HTTP version, load model, corrected latency, proxy routing, fault family, telemetry field, or external binary support), not arbitrary strings.

`resolve_plan` performs no I/O. Callers supply available descriptors, driver selections/default policy, platform, optional executable paths, and extra requirements. The resolver always checks the requested workload's load model. It checks required telemetry and service/workload compatibility. Unsupported, missing, mismatched, ambiguous, or platform-incompatible behavior is an error before execution. Optional missing telemetry produces a warning only when the source request marks it optional.

Default selection is deterministic: use one marked category default, or the only available driver in the category. More than one candidate without a unique default is an error. Use explicit selection to remove ambiguity. The resolver does not search PATH or contact external services.

ResolvedPlan schema v1 captures driver and upstream provenance and is serializable. Unknown fields and capability variants are rejected under v1. Driver upgrades can change behavior without a schema change, so concrete adapter/upstream versions remain in every resolved snapshot. Secret values and callable/runtime objects do not belong in this contract.

Production catalog ownership lives in `eggbench-drivers` (External Oracles
M001); the CLI consumes it. The catalog always registers the
external-process oracles — `oha` (`HttpVersion` 1.1/2, `LoadMode`
closed+open, `CorrectedLatency`, `ExternalBinary`), `h2load` (`HttpVersion`
1.1/2, `LoadMode` closed, `ExternalBinary`), `iperf3` (`LoadMode` closed
duration-bound, `ExternalBinary`) — and with the `eggstack-http` feature it
additionally registers the
`eggserve-origin` service descriptor (`HttpVersion::Http11`) and the
`eggfetch-http` workload descriptor (`HttpVersion::Http11`,
`LoadMode::ClosedLoop`, compatible service type `eggserve-origin`) with
exact lockfile-resolved sibling versions. Without a unique marked default
an explicit `--workload-driver` selection is required
(`ambiguous_selection` otherwise); unknown names fail with
`missing_driver`, and missing tool binaries fail with
`missing_executable_path` before startup. No open-loop capability is
advertised by the native driver, so open-loop plans fail resolution
explicitly unless an oracle advertises it. The
external-command substrate (trusted resolution, bounded argv execution,
versioned parsers) is documented in [external drivers](external-drivers.md);
the tool adapters are documented in [external oracles](external-oracles.md).
See [Eggstack HTTP](eggstack-http.md).

