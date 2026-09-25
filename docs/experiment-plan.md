# Experiment plans

An ExperimentPlan is a versioned request, not an execution script. JSON is the canonical machine representation and TOML is supported for hand editing. Both decode to the same typed model, reject unknown fields, and are semantically validated before use.

## Schema versions

- **v1** describes ordinary workloads and service topology. It remains supported unchanged.
- **v2** adds the optional predeclared `paired` baseline/candidate design. It remains supported unchanged.
- **v3** adds the optional first-class `network_path`. A v3 plan may omit it. v1 and v2 plans must omit it; a network path in either legacy schema fails validation rather than being reinterpreted as Direct.
- **v4** adds the `semantic_replay` workload (`target` plus relative workspace `fixture`). v1-v3 plans must omit it; a replay intent in a legacy schema fails validation. v4 plans must omit `network_path`; replay uses `--route direct` explicitly and never composes with the M002 path.
- **v5** adds the optional `diagnostics` array of pre/post workload diagnostic requests. v1-v4 plans must omit the field entirely (an explicit field, even empty, fails validation). v5 keeps the v4 replay workload and rejects `network_path` whenever diagnostics are requested; diagnostics bypass the benchmark path by design.

A workload target must name a declared service or the explicitly named external subject. A closed/open workload specifies exactly one of request count or duration. Time-bounded closed-loop plans require concurrency; open-loop plans require an offered rate. Services have stable names, managed/external lifecycle intent, acyclic dependencies, and bounded typed fields. Metric direction, unit, intent, and gates are explicit; diagnostic or informational metrics cannot gate. Secret material is referenced, never embedded.

A v2/v3 paired design requires a label subject naming the comparison, two distinct live arm services, an even measured trial count of at least two, and a workload target equal to the baseline arm. A v3 plan may still use paired design when `network_path` is absent.

## Schema-v3 `network_path`

`network_path` is optional and is not a managed service. Its shape is:

```json
{
  "route": {
    "driver": "eggress-route",
    "mode": { "kind": "direct" }
  },
  "stream_faults": {
    "driver": "eggchaos-stream",
    "upstream": [],
    "downstream": []
  }
}
```

`route` is required. `stream_faults` is optional; when present, its two ordered vectors may be empty. Route and fault driver names are explicit and must resolve to the selected production descriptors. The plan does not contain a route or fault CLI selector.

### Routes

The supported route modes are:

- `direct`: an explicit listener-free TCP connection to the final target; it is not an implicit fallback.
- `proxy_chain`: one or more native hops separated by `__`, with explicit `protocol://host:port` endpoints. HTTP CONNECT, SOCKS4/SOCKS4a, and SOCKS5 chains are supported by the current base TCP profile.

The chain is bounded and credential-free. Userinfo, encoded credentials, query strings, fragments, control characters, and unsupported schemes are rejected. If a requested route cannot be established, the request fails; Eggbench never silently retries Direct or changes the route. SSH, QUIC/H3, UDP, extended Eggress protocols, insecure TLS, reverse/listener-bound routing, pproxy syntax, credentials, and invented retries are unsupported.

### Stream faults

Each direction contains ordered `{ "id", "kind" }` entries. IDs are unique within a direction, with at most 128 entries per direction. Durations use the existing bounded `DurationMs` type; buffer, rate, burst, byte-limit, and average-size values are positive. The route chain is limited to 1,024 bytes. The seven supported kinds are:

| JSON kind | Fields | Meaning |
|---|---|---|
| `latency` | `delay_ms`, `jitter_ms`, `max_buffer_bytes` | Delay accepted bytes with bounded symmetric jitter and buffering. |
| `bandwidth` | `bytes_per_second`, `burst_bytes` | Rate-limit accepted bytes with a token burst. |
| `blackhole` | optional `close_after_ms` | Discard accepted bytes, optionally closing after a delay. |
| `limit_data` | `bytes` | Stop forwarding after the bounded byte count. |
| `slow_close` | `delay_ms` | Delay the shutdown phase. |
| `slice` | `average_size`, `variation`, `delay_ms` | Split accepted writes into bounded slices; variation is less than `average_size`. |
| `disconnect` | `after_ms` | Graceful disconnect; hard reset is not exposed. |

Fault order is preserved, activation is static with probability 1.0, and there is no live mutation, scenario, probability, or hard-reset field. If either direction is non-empty, the plan must provide an explicit non-null `seed`; the same seed is the deterministic Eggchaos namespace. An explicitly empty fault plan does not require a seed.

Eggchaos faults are **user-space accepted byte-stream impairments, never packet/datagram loss**. They wrap the established end-to-end stream: upstream is workload client to final target, and downstream is final target to workload client. They do not independently fault DNS, proxy connection setup, SOCKS/HTTP handshakes, or individual proxy hops. The route is established first and the fault wrapper is applied second (`route-first/fault-second`).

### Subject and design restrictions

`network_path` requires a transport-owning workload. It is incompatible with `Subject::External` (including an external oracle) and with the v2/v3 paired design; both fail during validation before startup. This is deliberate: paired arms share a run-scoped client/pool, so a physical connection cannot be assigned unambiguously to an arm. The current supported execution combination is the native `eggfetch-http` workload with an `eggserve-origin` target; external oracles do not advertise the custom network-path capability.

Resolved snapshots currently use ResolvedPlan schema v3 and add selected Route/Fault provenance when a path is present. ResolvedPlan v1 and v2 remain readable for legacy evidence; new path resolution always records v3. Semantic-replay resolution records the same v3 envelope with a `SemanticReplay` workload capability and an external `eggreplay-semantic` descriptor; fixture digest identity lives in `semantic-replay.json`, not in the resolved path. Diagnostic resolution adds the required `DiagnosticProbe` capabilities and selects the external `eggprobe` descriptor with its pinned executable path. See [driver capabilities](driver-capabilities.md), the complete [schema-v3 example](../examples/eggstack-path.json), the [schema-v4 replay example](../examples/eggstack-replay.json), and the [schema-v5 diagnostics example](../examples/eggstack-diagnostics.json). The paired rejection example is [intentionally invalid](../examples/eggstack-path-paired-unsupported.json).

## Schema-v4 `semantic_replay`

A v4 replay workload names a target service (or external target) publishing an `http_url` binding and a relative workspace fixture directory:

```json
{ "kind": "semantic_replay", "target": "origin", "fixture": "fixtures/replay" }
```

Fixture paths are relative, forward-slash, bounded (512 bytes, 16 components, 128 bytes per component), and free of absolute prefixes, parent traversal, and control characters. Existence, symlink-escape, directory, traversal-bound, and digest checks run in driver preflight against `RunnerOptions.workspace_root` before managed startup.

One complete fixture replay is one trial observation. `semantic_findings` (`count`, lower-is-better) is the correctness metric; `semantic_flows` is diagnostic-only. Process wall-clock time never becomes latency. Only absolute gates are supported for `semantic_findings`; relative/statistical gates fail validation with `unsupported_gate`.

## Schema-v5 `diagnostics`

A v5 plan may request bounded one-shot network diagnostics that run outside every measured trial interval: `pre_workload` after readiness/before warmups, `post_workload` after workload drain/before teardown, or `both`:

```json
{ "id": "pre-check", "source": "eggprobe", "phase": "pre_workload",
  "target": "origin", "probes": ["dns", "tcp", "http"],
  "required": true, "timeout_ms": 5000 }
```

At most 32 requests with at most 16 probes each; IDs are unique and artifact-path safe; only the `eggprobe` source and the `dns`/`tcp`/`tls`/`http` families are supported in M003b, and duplicate probes are rejected. Each timeout is nonzero and capped at ten minutes. Targets must name a declared service or the external subject target. Diagnostics never compose with `network_path` (`diagnostic_path_incompatible`): generated Eggprobe plans always use route Direct, so direct diagnostics must not be presented as evidence about a routed path.

A required pre diagnostic that reports a negative outcome (or fails operationally) invalidates the run before any warmup begins; an optional one records evidence and continues. A required post negative after an otherwise completed workload invalidates the run; it never masks an earlier workload/cancellation failure. Cancellation skips post diagnostics (`skipped_due_to_cancellation`) and teardown always runs. Probe timings are diagnostic evidence only: they never enter `TrialMetrics`, satisfy metrics, or gate acceptance. See the [schema-v5 example](../examples/eggstack-diagnostics.json).
