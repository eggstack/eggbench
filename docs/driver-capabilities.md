# Driver capabilities and plan resolution

`DriverDescriptor` records the canonical adapter name, adapter version, upstream identity/version, one category, structured capabilities, supported platforms, optional output schema, process-backed status, default status, and compatible service types. Capabilities are typed, including HTTP version, load model, corrected latency, proxy routing, network-path dialing, stream-fault plans, telemetry fields, and external-binary support.

`resolve_plan` performs no I/O. Callers supply descriptors, selections/default policy, platform, executable paths, and extra requirements. The resolver always checks the requested workload load model, required telemetry, service/workload compatibility, and platform support. Unsupported, missing, mismatched, ambiguous, or incompatible behavior is an error before execution. Optional missing telemetry produces a warning only when the request marks it optional. The resolver does not search PATH or contact external services.

Default selection is deterministic: use one marked category default, or the only available driver in the category. More than one candidate without a unique default is an error. Explicit selection removes ambiguity. A schema-v3 path does not introduce a second selection mechanism: the route and fault names in the plan must match the selected Route/Fault descriptors.

## Network-path capability matrix

A requested `network_path` requires all of the following before resolution succeeds:

| Category | Required capability | Production descriptor |
|---|---|---|
| Workload | `NetworkPath` plus the plan's load mode | `eggfetch-http` only when `eggstack-path` is enabled |
| Route | `ProxyRouting` | `eggress-route` / `eggress-outbound` |
| Fault, when `stream_faults` is present | `StreamFaultPlan` | `eggchaos-stream` / `eggchaos-core` |

The external-process oracles (`oha`, `h2load`, `iperf3`) do not advertise `NetworkPath`, so selecting one for a path plan fails before startup. A path requires a transport-owning workload and is incompatible with `Subject::External`. It is also incompatible with the paired design: both arms remain live while one run-scoped client/pool can reuse physical connections. These combinations fail with stable preflight categories rather than weakening pool or trial semantics.

## ResolvedPlan compatibility

New resolutions use **ResolvedPlan schema v3**. It retains the source plan version, selected workload/service/route/fault descriptors, exact upstream versions, normalized intent, seed, warnings, and optional resolved network-path provenance. ResolvedPlan v1 and v2 remain accepted on read for legacy bundles and are not rewritten. Unknown fields and unknown capability variants require an explicit compatibility decision; concrete adapter and upstream versions remain in every resolved snapshot.

A resolved network path records the route request, selected route descriptor, `route-first-fault-second-v1` semantics, and—when configured—the ordered fault request, selected fault descriptor, and `splitmix64-v1` RNG identity. It is not a callable runtime object and contains no credentials or runtime handles.

## Feature and route boundaries

The `eggstack-path` feature is opt-in and implies `eggstack-http`. Default and `eggstack-http`-only builds do not link Eggress or Eggchaos; `gregg` is orthogonal. The feature adds the route/fault descriptors and custom dialer, not a new CLI flag. The native workload path supports:

- explicit listener-free Direct TCP;
- native single- or multi-hop HTTP CONNECT and SOCKS routes (SOCKS4/SOCKS4a/SOCKS5 forms accepted by the schema);
- the seven static accepted-byte stream faults: `latency`, `bandwidth`, `blackhole`, `limit_data`, `slow_close`, `slice`, and graceful `disconnect`.

Fault plans preserve order, use probability 1.0, and require an explicit plan seed when either direction is non-empty. `slice.variation` must be smaller than `average_size`; fault IDs are unique within each direction, with at most 128 entries per direction. Durations and positive byte/rate fields use the core bounded types. There is no hard reset, live mutation, scenario, packet claim, or datagram operation. **Eggchaos faults are user-space accepted byte-stream impairments, never packet/datagram loss.**

The route is established before the stream wrapper (`route-first/fault-second`). Faults affect client-to-final-target and final-target-to-client application bytes, not DNS, proxy handshakes, or individual hops. A failed requested route never falls back to Direct. Unsupported route features include SSH, QUIC/H3, UDP, extended protocols, insecure TLS, reverse/listener-bound routing, pproxy syntax, credentials, and retries invented by Eggbench.

## One-client and evidence semantics

The `eggfetch-http` adapter owns one Eggfetch client per workload executor/run, not one client per request or trial. Warmups may populate its pool; measured trials may reuse physical connections, and a static fault policy stays attached to a reused connection. Physical dials are evidence distinct from request count.

A path run stages `network-path.json` with redacted route/fault provenance, seed/RNG identity, static semantics, and bounded diagnostics. Existing `eggfetch-method.json` evidence is extended additively with a bounded `network_path` object. Neither artifact introduces a new normalized metric, and neither records secrets, payload bytes, or ephemeral socket addresses as comparison identity.

See [Eggstack HTTP](eggstack-http.md), [experiment plans](experiment-plan.md), [evidence bundles](evidence-bundle.md), and [comparison](comparison.md).
