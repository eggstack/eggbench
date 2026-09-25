# Driver architecture

Drivers are described by a stable identity and one independent category: service, workload, telemetry, fault, diagnostic, execution provider, or route. Categories are kept separate so each adapter can own a narrow boundary. Core stores descriptors and typed capability values; it does not instantiate runtime traits, probe executables, open sockets, or perform adapter-specific runtime lowering.

## Resolution and resolved-plan provenance

Resolution consumes a validated ExperimentPlan, an in-memory descriptor set, explicit selections or the deterministic default policy, a platform label, and caller-supplied executable paths for external adapters. Default selection accepts a single marked default or a sole category candidate. Ambiguous selection fails. A selected driver's platform and every required capability are checked before resolution succeeds.

The resolver derives the required load-model capability from the plan and verifies required telemetry fields. Missing optional telemetry is retained as a structured warning; required telemetry fails closed. A workload driver may advertise compatible service types, which are checked against the selected target service. External-process drivers require an explicit non-empty executable path and the `ExternalBinary` capability. The resolver does no PATH search or external I/O.

Current **ResolvedPlan schema v3** freezes the source-plan version, resolved schema version, exact driver descriptors and upstream versions, caller-supplied external paths, normalized topology/workload, trial defaults, environment/comparison requests, seed, warnings, and optional resolved `network_path` provenance. ResolvedPlan v1 and v2 remain readable for legacy evidence; new resolutions write v3. Unknown fields and unknown capability variants are rejected under the active schema. Driver upgrades can change behavior without a wire-schema change, so concrete adapter/upstream versions remain in every snapshot. Secret values and callable/runtime objects do not belong in this contract.

A path request adds independent `Route` and, when faults are present, `Fault` selections. These are not workload or service aliases. The selected `eggfetch-http` workload must advertise `NetworkPath`; the route must advertise `ProxyRouting`; a requested fault plan must advertise `StreamFaultPlan`. The resolved path records the selected route/fault descriptors, the original credential-free request, the stable route-first/fault-second semantics version, and the Eggchaos RNG identity. A non-empty fault plan also retains the explicit experiment seed.

## Production catalog ownership and feature isolation

`eggbench-drivers` is the sole production adapter/catalog ownership crate (`DriverCatalog::production`). The CLI consumes the catalog rather than owning registration; the qualification fake remains test/qualification-only and is never linked into the production path. Dependency direction stays `core <- runner <- drivers <- cli`.

The catalog registers external-process oracles (`oha`, `h2load`, `iperf3`) unconditionally. With `eggstack-http`, it registers the `eggserve-origin` service adapter and the `eggfetch-http` workload driver. With `eggstack-path`, which implies `eggstack-http`, it additionally registers the listener-free `eggress-route` and deterministic `eggchaos-stream` descriptors and enables the custom network-path dialer on Eggfetch. `gregg` remains orthogonal and adds only its telemetry descriptor when enabled.

The feature boundary is intentional: default and `eggstack-http`-only builds do not link Eggress or Eggchaos; `eggstack-path` adds only the narrow route/fault dependencies needed for the path. The CLI has a corresponding forwarding feature. Route and fault identities come from the schema-v3 plan, not a new CLI flag. Exact Eggress/Eggchaos/Eggfetch/EggServe versions are extracted from the lockfile at build time and retained in descriptors and evidence.

## Listener-free path ownership

The production path is lowered before managed startup. For each physical Eggfetch dial, the `eggbench-drivers` adapter first asks Eggress to establish the explicitly selected Direct or native HTTP/SOCKS proxy-chain route. Only a successful route receives the Eggchaos bidirectional wrapper. The route is therefore first and the fault wrapper second. Upstream is client to final target; downstream is final target to client.

A failed route is a failed dial. There is no shell, subprocess, Eggbench listener, second route decision, or direct fallback. Credential-bearing or extended route options fail before runtime. The route adapter produces bounded diagnostics and versioned `network-path.json`; the Eggfetch adapter adds a bounded `network_path` object to its existing method evidence without changing normalized metrics.

One Eggfetch client is owned by one workload executor for the whole run. Its pool and physical connection state survive warmups and measured trials; a static fault policy remains attached to a reused physical connection. The concrete Eggfetch and path adapter implementations live in `eggbench-drivers`; `eggbench-runner` owns the executor/session contracts and orchestration. The core `ResetPolicy::Service` value does not imply process supervision or restart semantics. See [runner ownership](runner.md) and [Eggstack HTTP](../docs/eggstack-http.md).

## External command substrate and tool adapters

Reusable machinery for optional external benchmark tools remains separate from native Eggstack drivers:

- trusted executable resolution (`BinaryResolver`): explicit absolute paths only; `PATH` search skips empty/relative components;
- bounded argv-only version probes and command execution with explicit timeout, cancellation, output caps, and environment policy;
- raw artifact helpers and versioned parser contracts independent of spawning.

The tool adapters (`oha`, `h2load`, `iperf3`) use that substrate and do not advertise `NetworkPath` or `StreamFaultPlan`. See [external drivers](../docs/external-drivers.md) and [external oracles](../docs/external-oracles.md).

Future integrations should prefer stable sibling-owned crates or process/protocol seams, as ADR-0004 directs. Independent external measurement tools remain separate workload adapters where they provide an independent oracle.
