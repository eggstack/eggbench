# Driver architecture

Drivers are described by a stable identity and one independent category: service, workload, telemetry, fault, diagnostic, or execution provider. Categories are kept separate so each future adapter can own a narrow boundary. Core stores descriptors and typed capability values; it does not instantiate runtime traits or probe executables.

Resolution consumes a validated ExperimentPlan, an in-memory descriptor set, explicit selections or the deterministic default policy, a platform label, and caller-supplied executable paths. Default selection accepts a single marked default or a sole category candidate. Ambiguous selection fails. A selected driver's platform and every required capability are checked before resolution succeeds.

The resolver derives the required load-model capability from the plan and verifies required telemetry fields. Missing optional telemetry is retained as a structured warning; required telemetry fails closed. A workload driver may advertise compatible service types, which are checked against the selected target service. External-process drivers require an explicit non-empty executable path and the ExternalBinary capability. Eggbench does no PATH search here.

ResolvedPlan schema v1 freezes the source-plan version, resolved schema version, exact driver descriptors and upstream versions, paths supplied for external drivers, normalized topology/workload, trial defaults, environment/comparison requests, seed, and warnings. It contains no process or runtime handles. Unknown v1 fields and unknown capability variants are rejected; adding serialized capability variants requires an explicit compatibility/version decision.

## Production catalog ownership (External Oracles M001)

`eggbench-drivers` is the sole production adapter/catalog ownership crate
(`DriverCatalog::production`, currently empty). The CLI consumes the catalog
rather than owning registration; the qualification fake remains
test/qualification-only and is never linked into the production path.
Dependency direction stays `core <- runner <- drivers <- cli`.

## External command substrate (External Oracles M001)

Reusable machinery for optional external benchmark tools (no oha/h2load/
iperf3 adapter ships in M001):

- trusted executable resolution (`BinaryResolver`): explicit absolute paths
  only; `PATH` search skips empty/relative components (no implicit cwd);
  Unix requires executable mode bits; Windows accepts only direct `.exe`/
  `.com` targets and rejects `.bat`/`.cmd` shell wrappers; canonical target
  is SHA-256 hashed for provenance;
- bounded argv-only version probes (`VersionProbe`) with explicit timeout,
  output caps, cancellation, and a parsed version token;
- argv-only command execution (`run_command`) with `env_clear` plus explicit
  driver environment, null stdin, concurrent bounded stdout/stderr draining
  (draining continues past the cap so children never block), cancellation/
  timeout cleanup via the runner's process-group semantics on Unix and
  explicit `direct_child_only` reporting on Windows;
- raw artifact helpers (`stdout.raw`, `stderr.raw`, `command-metadata.json`)
  with no metric normalization;
- versioned parser contract (`ExternalOutputParser`) independent of
  spawning; M001 ships only a trivial fixture parser, not a tool parser.

See [external drivers](../docs/external-drivers.md).

Future Eggstack integrations should first use stable sibling-owned crates or process/protocol seams, as ADR-0004 directs. Independent external measurement tools remain separate workload adapters where they provide an independent oracle.

For local orchestration, runtime adapter instances remain in
`eggbench-runner`, not in `eggbench-core`. The mutable `WorkloadExecutor`
contract accepts one warmup or measured invocation and exposes a separate
bounded drain step. Reset behavior uses an explicit runner `ResetRegistry`;
the core's `ResetPolicy::Service` value does not imply process supervision or
restart semantics. See [runner ownership](runner.md) and
[trial orchestration](../docs/trial-orchestration.md).
