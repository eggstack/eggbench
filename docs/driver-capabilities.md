# Driver capabilities and plan resolution

`DriverDescriptor` records the canonical adapter name, adapter version, upstream identity/version, one category, structured capabilities, supported platforms, optional output schema, and process-backed status. Capabilities are typed (for example, HTTP version, load model, corrected latency, proxy routing, fault family, telemetry field, or external binary support), not arbitrary strings.

`resolve_plan` performs no I/O. Callers supply available descriptors, driver selections/default policy, platform, optional executable paths, and extra requirements. The resolver always checks the requested workload's load model. It checks required telemetry and service/workload compatibility. Unsupported, missing, mismatched, ambiguous, or platform-incompatible behavior is an error before execution. Optional missing telemetry produces a warning only when the source request marks it optional.

Default selection is deterministic: use one marked category default, or the only available driver in the category. More than one candidate without a unique default is an error. Use explicit selection to remove ambiguity. The resolver does not search PATH or contact external services.

ResolvedPlan schema v1 captures driver and upstream provenance and is serializable. Unknown fields and capability variants are rejected under v1. Driver upgrades can change behavior without a schema change, so concrete adapter/upstream versions remain in every resolved snapshot. Secret values and callable/runtime objects do not belong in this contract.

