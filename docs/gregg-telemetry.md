# Gregg host telemetry (M001b)

Optional trial-synchronized telemetry backed by a loopback Gregg daemon,
behind the `gregg` cargo feature:

```toml
eggbench-drivers/gregg
  -> gregg-protocol
  -> eggfetch-core/standard-http1   # shared with M001a, never duplicated
```

The CLI forwards with its own `gregg` feature
(`eggbench-cli/gregg`). Default builds stay telemetry-free: without the
feature the catalog carries no telemetry driver and required Gregg
telemetry fails resolution explicitly (`missing_driver`, exit code 3).

Exact sibling versions come from the workspace `Cargo.lock` via the
`eggbench-drivers` build script (currently `gregg-protocol 1.0.14`,
`eggfetch-core 0.2.0`); descriptors, method provenance, and per-trial
provenance artifacts report those values.

## Ownership

- Gregg owns telemetry collection and v2 wire semantics.
- Eggbench never scrapes OS counters remotely when Gregg is configured, and
  never manages, installs, or starts `greggd` (external lifecycle only).
- Eggbench owns scheduling, aggregation, and evidence.

## Plan representation (schema v1, unchanged)

```json
"services": [
  {"name": "gregg", "kind": {"kind": "named", "service_type": "gregg"},
   "lifecycle": "external", "config": {"endpoint": "http://127.0.0.1:11310"}}
],
"telemetry": [
  {"source": "gregg", "fields": ["host_cpu_percent"], "required": true}
]
```

Exactly one external named `gregg` service is allowed when Gregg telemetry
is requested; zero or multiple endpoints fail required telemetry before
startup. The endpoint is part of resolved-plan evidence.

## Endpoint policy

Loopback HTTP only: `127.0.0.0/8`, `::1`, or `localhost` with
loopback-only deterministic resolution. Rejected: public or private-LAN
addresses, HTTPS, embedded credentials, and query/fragment forms. This
matches Gregg's unauthenticated private-network model; `doctor` validates
config syntax without dialing, `run` preflight dials.

## Collector lifecycle

The generic runner seam (`TelemetryCollector`, `TelemetryRegistry`) runs
each collector strictly outside measured workload timing per trial:

```text
telemetry.start_trial
measurement_start = Instant::now()
workload.execute
measurement_elapsed = ...
telemetry.stop_trial
stage workload raw artifacts
stage telemetry raw artifacts (trials/NNN/telemetry/...)
normalize combined metric inputs
stage metrics.json
```

Preflight (before managed startup) probes `GET /v2/healthz` then
`GET /v2/status`, validates with `gregg-protocol`, and derives the poll
cadence as `clamp(daemon_sample_interval_ms, 250ms, 5s)`. Required
failures prevent measurement; optional failures disable the collector for
the run with an explicit `telemetry_disabled` warning, and requested
metrics normalize as missing — never fabricated zeroes. Stop is attempted
after workload failure/cancellation when start succeeded; stop failures
attach as cleanup diagnostics without rewriting the primary cause. No
polling task survives its trial (bounded join, abort on expiry, drain in
the cleanup tail).

## Polling and retention

One snapshot immediately after start, cadenced polling (never faster than
250 ms), one final snapshot at stop within budget. Identical
`observed_at_unix_ms` snapshots deduplicate. At most 256 snapshots and
256 KiB of raw series bytes are retained per trial; aggregation always
runs over exactly the retained set, overflow counts as dropped samples
plus a warning, and truncation marks affected metrics invalid (NaN) rather
than inventing values.

## Metrics

Custom `host_*` names (vocabulary v1 reserves nothing; plans declare
unit/direction/intent explicitly):

| Metric | Unit | Source | Aggregation |
|---|---|---|---|
| `host_cpu_percent` | percent | `v2.cpu.usage_pct` | mean |
| `host_memory_used_bytes` | bytes | `v2.memory.used_bytes` | maximum |
| `host_memory_percent` | percent | `v2.memory.usage_pct` | maximum |
| `host_cpu_frequency_hz` | hertz | `v2.cpu_frequency_hz` | mean of present values |
| `host_disk_read_bytes_per_sec` | bytes_per_sec | `v2.disk_io.aggregate_read_bytes_per_sec` | mean |
| `host_disk_write_bytes_per_sec` | bytes_per_sec | `v2.disk_io.aggregate_write_bytes_per_sec` | mean |
| `host_network_rx_bytes_per_sec` | bytes_per_sec | `v2.network.aggregate_rx_bytes_per_sec` | mean |
| `host_network_tx_bytes_per_sec` | bytes_per_sec | `v2.network.aggregate_tx_bytes_per_sec` | mean |

Absent optional fields stay missing (measured zero stays observed zero).
All arithmetic rejects nonfinite values. Observations carry
`producer: gregg`, the protocol version, the `v2.*` source field, and a
`gregg.ndjson` artifact reference, so a future workload emitting a
colliding name still fails normalization as a duplicate rather than
selecting silently.

## Evidence

Per measured trial: `trials/NNN/telemetry/00-00-gregg.ndjson` (exact
retained wire lines) and `trials/NNN/telemetry/00-01-gregg-provenance.json`
(endpoint host/port, paths, wire schema, exact crate versions, system
identity, cadence, sample/drop/error counts, no credentials).
Interference accounting (poll interval, sample count, response bytes) is
part of the provenance artifact; cadence tests bound the poll count.
