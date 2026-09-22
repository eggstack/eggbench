# Driver architecture

Drivers are described by a stable identity and one independent category: service, workload, telemetry, fault, diagnostic, or execution provider. Categories are kept separate so each future adapter can own a narrow boundary. Core stores descriptors and typed capability values; it does not instantiate runtime traits or probe executables.

Resolution consumes a validated ExperimentPlan, an in-memory descriptor set, explicit selections or the deterministic default policy, a platform label, and caller-supplied executable paths. Default selection accepts a single marked default or a sole category candidate. Ambiguous selection fails. A selected driver's platform and every required capability are checked before resolution succeeds.

The resolver derives the required load-model capability from the plan and verifies required telemetry fields. Missing optional telemetry is retained as a structured warning; required telemetry fails closed. A workload driver may advertise compatible service types, which are checked against the selected target service. External-process drivers require an explicit non-empty executable path and the ExternalBinary capability. Eggbench does no PATH search here.

ResolvedPlan schema v1 freezes the source-plan version, resolved schema version, exact driver descriptors and upstream versions, paths supplied for external drivers, normalized topology/workload, trial defaults, environment/comparison requests, seed, and warnings. It contains no process or runtime handles. Unknown v1 fields and unknown capability variants are rejected; adding serialized capability variants requires an explicit compatibility/version decision.

Future Eggstack integrations should first use stable sibling-owned crates or process/protocol seams, as ADR-0004 directs. Independent external measurement tools remain separate workload adapters where they provide an independent oracle.

For local orchestration, runtime adapter instances remain in
`eggbench-runner`, not in `eggbench-core`. The mutable `WorkloadExecutor`
contract accepts one warmup or measured invocation and exposes a separate
bounded drain step. Reset behavior uses an explicit runner `ResetRegistry`;
the core's `ResetPolicy::Service` value does not imply process supervision or
restart semantics. See [runner ownership](runner.md) and
[trial orchestration](../docs/trial-orchestration.md).
