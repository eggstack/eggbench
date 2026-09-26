# Eggstack HTTP and network path

The Eggstack-native path has two layers:

```text
ExperimentPlan
  -> EggServe named managed origin (`eggserve-origin`)
  -> runtime `http_url` binding
  -> Eggfetch WorkloadExecutor (`eggfetch-http`)
  -> [optional Eggress route -> Eggchaos stream wrapper]
  -> measured trials and immutable `.eggb` evidence
```

Eggfetch owns outbound HTTP semantics, EggServe owns inbound HTTP/runtime semantics, and Eggbench owns scheduling, adapter configuration, metric mapping, lifecycle, and evidence. Eggbench does not implement a Hyper client or server.

## Feature boundary

`eggstack-http` contains the M001 origin/workload path:

```text
eggfetch-core/standard-http1
eggserve-primitives
eggserve-server
hdrhistogram
```

`eggstack-path` is a separate opt-in feature and implies `eggstack-http`. It additionally links the narrow `eggress-outbound`, `eggress-uri`, `eggress-core`, and `eggchaos-core` crates and registers:

- `eggress-route` (`Route`, `ProxyRouting`), using `eggress-outbound`;
- `eggchaos-stream` (`Fault`, `StreamFaultPlan`), using `eggchaos-core`;
- the custom `NetworkPath` dialer capability on `eggfetch-http`.

Default and `eggstack-http`-only builds do not link Eggress or Eggchaos. `gregg` is orthogonal. The CLI forwards `eggstack-path` to the drivers; route and fault names come from the plan, not from a new CLI flag. Exact sibling versions are taken from `Cargo.lock` at build time and are reported in descriptors and evidence.

Without `eggstack-path`, the native drivers remain unregistered for path use, while the external oracles remain catalogued. A run declaring `network_path` fails closed with `unsupported_network_path` before environment collection, bundle preparation, or managed startup.

## Native workload semantics

`eggfetch-http` supports closed-loop count, closed-loop duration, finite-count, and closed-mode time-bounded workloads. Open-loop and open-mode time-bounded plans fail resolution before startup because the driver does not advertise that capability. Requests are HTTP/1.1 GETs against the target's `http_url`, consume the full response body, and use a 30-second per-request timeout inside the runner safety deadline. Eggbench adds no retries and Eggfetch does not follow redirects. Latency spans dispatch through full-body consumption; non-2xx responses still retain timing and byte evidence while counting as HTTP-status errors.

Raw observations include throughput, latency minimum/mean and percentiles, error/timeout rates, and received bytes. Each measured invocation retains an `hdrhistogram` V2 `latency.hdr` artifact and the existing `eggfetch-method.json`; stable error categories include `transport`, `timeout`, `http_3xx`, `http_4xx`, `http_5xx`, `body_read`, and `cancelled`. Raw samples never become extra comparison units.

## Controlled origin

`eggserve-origin` is an in-process EggServe H1 runtime bound to `127.0.0.1:0`; the bind is not configurable and non-loopback binds are refused. Service configuration is bounded:

| Key | Default | Bounds |
|---|---|---|
| `path` | `/bench` | visible ASCII, starts with `/`, no query/fragment/whitespace, at most 256 bytes |
| `body_bytes` | `1024` | integer `0..=1048576` |
| `status` | `200` | integer `200..=599` |

The exact path returns the configured status and a deterministic fixed-length body filled with `0x42`; every other target returns `501`. Startup publishes `http_url`, `bound_addr`, and `bound_port`. Adapter readiness is complete when the server handle exists; the plan does not configure a probe for this in-process service.

With the `eggstack-http` feature, the same Eggfetch transport also serves the `eggbench-http-corpus` correctness source. It runs fixed corpus cases serially outside measured intervals, confines targets to local/private runtime bindings, and records sanitized status-only outcomes. Redirect and retry support are absent from the compiled Eggfetch feature set.

## Route-first, fault-second

The resolved path is lowered before managed startup. For each physical Eggfetch dial, the dialer:

1. connects through the explicitly selected Eggress route;
2. returns one logical stream, or fails the dial;
3. only on success wraps that stream with Eggchaos when faults are configured.

The ordering is stable and is recorded as `route_first_fault_second` (semantics identity `route-first-fault-second-v1`). A failed proxy connection is returned as an error; there is no direct fallback, alternate route, shell command, listener, or second route decision. A directly reachable origin does not make an unavailable requested proxy route succeed directly.

Faults therefore act on accepted application bytes carried over the established end-to-end route. They do not independently impair DNS, proxy TCP setup, HTTP CONNECT/SOCKS negotiation, or individual proxy hops. `upstream` means client to final target; `downstream` means final target to client.

Supported route modes are explicit `direct` TCP and native `proxy_chain` routes using ordinary HTTP CONNECT or SOCKS routes (including single- or multi-hop chains). Unsupported route features include SSH, QUIC/H3, UDP, extended Eggress protocols, insecure TLS, reverse/listener-bound routing, pproxy syntax, credentials, and retries/fallbacks invented by Eggbench.

Route chains are credential-free. URI userinfo, encoded credentials, query, and fragment data fail preflight with `route_credentials_not_supported`; length, malformed host/port, and control-character failures use `invalid_route`. Rejected credential-bearing input is never copied into an accepted resolved plan or path evidence.

## Stream faults

The supported static stream-fault kinds are:

| Kind | Configuration | Stream effect |
|---|---|---|
| `latency` | `delay_ms`, `jitter_ms`, `max_buffer_bytes` | Delay accepted bytes with bounded jitter. |
| `bandwidth` | `bytes_per_second`, `burst_bytes` | Rate-limit accepted bytes. |
| `blackhole` | optional `close_after_ms` | Discard accepted bytes, optionally close later. |
| `limit_data` | `bytes` | Stop forwarding after a bounded byte count. |
| `slow_close` | `delay_ms` | Delay shutdown. |
| `slice` | `average_size`, `variation`, `delay_ms` | Split accepted writes; variation is smaller than the average. |
| `disconnect` | `after_ms` | Graceful disconnect; hard reset is not exposed. |

Every lowered fault has probability 1.0, preserves plan order, and is static for the run. There are no scenarios, live mutations, hard resets, packet claims, or datagram operations. **Eggchaos faults are user-space accepted byte-stream impairments, never packet/datagram loss.** Non-empty upstream or downstream plans require an explicit experiment `seed`; the seed namespace and RNG version are retained in evidence.

## Pooling and evidence

One Eggfetch client is created per workload executor for the whole run, never one client per request or trial. Warmups can establish pool state, and measured trials can reuse physical connections. A static fault policy remains attached to a reused physical connection; the runner does not force a reconnect per trial. Physical dial attempts are consequently distinct from request counts.

A path run stages run-level `network-path.json` (schema v1, redacted) with selected route/fault provenance, canonical credential-free route identity, seed/RNG identity, static policy, and bounded dial diagnostics. Existing per-invocation `eggfetch-method.json` is extended additively with a `network_path` object containing bounded dial, failure, hop, wrapping, and ordinal facts. Network-path evidence is not a new normalized metric.

The service lifecycle remains in `lifecycle/runtime-topology.json`; the route is not represented as a fake managed service. Cancellation drops in-flight work, uses the normal drain/teardown path, and leaves no Eggbench-owned route listener or detached route task.

## Example

Build with the feature, then validate or run the complete schema-v3 example:

```sh
cargo build -p eggbench-cli --features eggstack-path
./target/debug/eggbench validate examples/eggstack-path.json
./target/debug/eggbench run examples/eggstack-path.json path.eggb
```

The example uses a Direct route with deterministic upstream/downstream Eggchaos faults, so it runs without an external proxy fixture. Set `route.mode` to a credential-free native HTTP/SOCKS `proxy_chain` when testing a routed deployment. The paired-plus-path example is intentionally rejected before startup; see [`examples/eggstack-path-paired-unsupported.json`](../examples/eggstack-path-paired-unsupported.json). See also the [plan schema](experiment-plan.md), [CLI behavior](cli.md), and [evidence bundles](evidence-bundle.md).
