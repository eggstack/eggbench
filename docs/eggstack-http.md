# Eggstack HTTP (M001a)

The first real Eggstack-native experiment path, behind the `eggstack-http`
cargo feature:

```text
ExperimentPlan
  | EggServe named managed origin (`eggserve-origin`)
  | runtime HTTP binding (`http_url`)
  | Eggfetch WorkloadExecutor (`eggfetch-http`)
  | M002 trial lifecycle
  | M001 TrialMetrics + raw HDR evidence
  | immutable .eggb
```

## Ownership

- Eggfetch owns outbound HTTP semantics.
- EggServe owns inbound HTTP/runtime semantics.
- Eggbench owns experiment lifecycle, adapter configuration, metric mapping,
  and evidence only. No Hyper client/server implementation is created in
  Eggbench.

## Feature boundary

```toml
eggbench-drivers/eggstack-http
  -> eggfetch-core/standard-http1
  -> eggserve-primitives
  -> eggserve-server
  -> hdrhistogram (V2 serialization for retained histograms)
```

The CLI forwards with its own `eggstack-http` feature
(`eggbench-cli/eggstack-http`). Without the feature the native drivers stay
unregistered (the external-process oracles still register, with an explicit
`--workload-driver` selection required) and default `run` fails closed
before managed startup (`ambiguous_selection`/`missing_driver`, exit
code 3).

Exact sibling versions are resolved from the workspace `Cargo.lock` by the
`eggbench-drivers` build script and embedded as compile-time values; driver
descriptors and method evidence report those values, never a hardcoded patch
version. Current lock: `eggfetch-core 0.2.0`, `eggserve-server 0.2.1`,
`eggserve-primitives 0.2.0`.

## Controlled origin (`eggserve-origin`)

An in-process EggServe H1 runtime bound to loopback only: the bind address
is fixed to `127.0.0.1:0` and is not configurable. The actual bound address
comes from the EggServe server handle; non-loopback binds are refused.

Service `config` keys:

| Key | Default | Bounds |
|---|---|---|
| `path` | `/bench` | must start with `/`, visible ASCII, no query/fragment/whitespace, ≤ 256 bytes |
| `body_bytes` | `1024` | integer `0..=1048576` (1 MiB) |
| `status` | `200` | integer `200..=599` |

Response discipline: the exact configured path returns the configured status
with a deterministic fixed-length body (fill byte `0x42`, generated once at
startup); every other target returns `501 Not Implemented`. No filesystem,
timestamp, or random body.

Startup publishes non-secret runtime bindings for the service identity:
`http_url` (full origin URL including the route path), `bound_addr`, and
`bound_port`. Adapter `start` returns only after the server handle exists
(adapter-owned readiness); plan-level `Readiness::Probe` on an in-process
named service is rejected; an optional post-ready delay remains supported
outside measurement. Graceful shutdown honors the plan grace; shutdown
timeouts become cleanup diagnostics.

## Native workload (`eggfetch-http`)

Capability matrix — supported: `ClosedLoop` with requests, `ClosedLoop`
with duration, `FiniteCount`, `TimeBounded { mode: ClosedLoop }`. Not
supported: `OpenLoop`, `TimeBounded { mode: OpenLoop }`, H2/H3, proxy/TLS
semantics. Only `LoadMode::ClosedLoop` and `HttpVersion::Http11` are
advertised, so open-loop plans fail resolution explicitly before startup.

One Eggfetch client per workload executor (per run), never per request:
warmups establish pool/connection state and measured trials reflect a warmed
run when warmups are configured. Connection reuse is part of the method
provenance retained per invocation (`eggfetch-method.json`).

Request semantics: HTTP GET against the target service's `http_url` binding,
full response-body consumption, per-request 30 s timeout inside the runner
safety deadline, no Eggbench-level retries, no redirect following. Latency
spans dispatch to full-body consumption; normalization is never included.
Non-2xx responses still complete the round trip: timing and byte evidence
are retained while the response counts as an HTTP-status error.

Closed-loop schedule: at most N active requests; each worker issues the next
request when its previous one finishes. Finite counts issue exactly the
planned count unless the trial fails or cancels; time-bounded mode stops
issuance at the deadline. Cancellation stops issuance; worker tasks are
joined before return.

### Raw metrics and histogram

Emitted raw observations (only plan-requested names become gate-eligible
normalized metrics): `throughput` (rps/Rate), `latency_min`/`latency_mean`
(ms/Minimum/Mean), `latency_p50/p90/p95/p99/p999` (ms/Percentile),
`error_rate`/`timeout_rate` (ratio/Ratio), `bytes_received` (bytes/Sum).
Source fields use `eggfetch.*` labels.

One same-trial `latency.hdr` artifact per invocation: hdrhistogram V2 binary
encoding of dispatch-to-full-body latencies in integer microseconds
(range 1–60,000,000 µs, 3 significant figures, saturating, no
coordinated-omission correction claim). Referenced through
`RawHistogramInput` (`latency`/`hdrhistogram-v2`/`us`).

Stable error categories: `transport`, `timeout`, `http_3xx`, `http_4xx`,
`http_5xx`, `body_read`, `cancelled`. Eggfetch error strings never become
category identities.

## Example

Build with the feature, then run the deterministic loopback fixture (closed
loop to the origin, 1 warmup, 3 measured trials, throughput/latency_p99/
error_rate gates):

```sh
cargo build -p eggbench-cli --features eggstack-http
./target/debug/eggbench run \
  crates/eggbench-core/tests/fixtures/eggstack-loopback.json loopback.eggb
```

Each measured trial stages `trials/NNN/metrics.json`,
`trials/NNN/artifacts/001-latency.hdr`, and
`trials/NNN/artifacts/002-eggfetch-method.json`; `lifecycle/runtime-topology.json`
records the adapter-owned origin and its ephemeral `http_url`.
