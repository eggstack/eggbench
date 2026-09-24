# External measurement oracles (M002)

Independent load/capacity drivers on the M001 external-command substrate:
`oha` and `h2load` HTTP workloads plus the `iperf3` TCP throughput driver.
The tools own load generation and their machine-output semantics; Eggbench
owns binary resolution, version policy, argv construction from the plan
workload, bounded execution, raw retention, metric mapping, and evidence.

## Driver selection

`doctor` and `run` accept `--workload-driver <name>` to pin the workload
driver explicitly (`oha`, `h2load`, `iperf3`, `eggfetch-http`). Without the
flag, resolution uses the unique marked default (`eggfetch-http` when the
`eggstack-http` feature is compiled in); without a unique default the
resolution fails explicitly with `ambiguous_selection` listing the
candidates. Unknown names fail with `missing_driver`. Explicit selection
flows through the existing `ResolutionOptions::selections` seam — no plan
schema change.

External-process drivers additionally pin their resolved canonical binary
path into resolution (`executable_paths`), resolve the binary
synchronously in the executor factory, and probe `--version` in `run`
preflight. Missing binaries fail with `missing_executable_path` and
unsupported versions with `external_tool`, both exit code 3, before any
managed startup. `doctor` reports per-driver `binary_present` from
filesystem resolution only — it never spawns a tool (probed versions stay
in `run` preflight and trial evidence).

## oha (`oha` driver, parser `oha-json/v1`, requires oha ≥ 1.0.0)

Parses `--output-format json` against the upstream `schema.json` required
members (`summary`, `latencyPercentiles`, `statusCodeDistribution`,
`errorDistribution`); nullable timing fields (`slowest`, `fastest`,
`average`, percentiles) stay missing when no request completes.

| Plan workload | Tool invocation |
|---|---|
| ClosedLoop/FiniteCount count | `-n <requests> -c <concurrency>` |
| ClosedLoop/TimeBounded duration | `-z <dur> [-c <concurrency>]` |
| OpenLoop rate + count/duration | `-q <rps> --latency-correction` + `-n`/`-z` |

Always `--no-tui --output-format json`; the URL comes from the workload
target's `http_url` binding. Mutually exclusive count+duration pairs fail
closed (oha silently ignores `-n` under `-z`).

Normalized metrics (parity names with the native driver): `throughput`
(rps ← `requestsPerSec`), `error_rate` (ratio ← `1 − successRate`),
`latency_min/mean` (ms, when observed), `latency_p50/p95/p99` (ms, when
observed; p90/p999 stay missing). Error counts carry the
`errorDistribution` entries (`oha:<message>`); the status-code distribution
is retained as the `oha-status.json` diagnostic artifact; raw stdout as
`stdout.raw`.

Exit status is not a failure signal: oha exits 0 with every request
failed, so success is read from `successRate` and the error distribution.
Unachieved offered load stays visible (`error_rate`, error counts, raw
JSON) but never auto-invalidates the trial.

## h2load (`h2load` driver, parser `h2load-text/v1`, requires h2load ≥ 1.0.0)

h2load exposes no machine output, so the parser anchors on the
`finished in` marker and requires the `requests:` and `time for request:`
rows; progress chatter is ignored and anything else fails closed. The
probed version is the nghttp2 release from `h2load nghttp2/<release>`
(the generic token extractor cannot isolate it).

| Plan workload | Tool invocation |
|---|---|
| ClosedLoop/FiniteCount count | `-n <requests> -c <concurrency>` |
| ClosedLoop/TimeBounded duration | `--duration=<secs> [-c <concurrency>]` |
| OpenLoop | rejected (no rate limiter; capability-gated) |

Cleartext targets add `--h1`; `https` targets use the default (HTTP/2).
Only the capability-advertised subset is built — no timing scripts,
multi-URI, header, or H3/QUIC options.

Normalized metrics: `throughput` (rps ← `req/s` mean), `error_rate`
(ratio ← `failed/total`; missing when `total` is 0), `latency_min/mean`
(ms ← `time for request` min/mean). Nonzero failed/errored/timeout counts
become `h2load:<label>` error counts; status buckets are retained as
`h2load-status.json`; raw stdout as `stdout.raw`. Like oha, h2load exits 0
with every request failed — the counts, not the status, carry the failure.

## iperf3 (`iperf3` driver, parser `iperf3-json/v1`, requires iperf3 ≥ 3.1)

Runs `iperf3 -c <host> -p <port> -t <secs> -J [-P <streams>]` against an
explicit server host. Host/port come from the target's `http_url` binding
authority (scheme/path ignored and recorded as such; default port 5201).
Eggbench never manages an iperf3 server in M002.

Only duration-bound closed-loop workloads map (`-t` is ceiling'd whole
seconds, `-P` from concurrency). Request counts have no byte-stream
equivalent and fail closed with `unsupported_option` instead of being
coerced; OpenLoop rates have no TCP mapping and fail the same way. TCP
only; no UDP, reverse, or server-output options.

A top-level `error` member (the refused-connection shape) is a tool
failure, never a zero-throughput observation. Metrics keep their own
names (no common-denominator coercion with request-based drivers):
`bits_per_sec_sent/received` (bps), `bytes_sent/received` (bytes),
`retransmits` (count, TCP only, missing otherwise). Raw stdout as
`stdout.raw`.

## Evidence

Every trial stages `stdout.raw`, `stderr.raw`, and `command-metadata.json`
(tool identity, executable SHA-256, argc, exit code, truncation counters,
parser id) plus the tool status diagnostic where applicable. Exact binary
path, SHA-256, and probed version are therefore in every trial; no
credential, shell string, or full environment enters evidence. Cancellation
and trial timeouts propagate through the substrate; partial stdout is
still staged when the runner retains the invocation.
