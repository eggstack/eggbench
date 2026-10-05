# External load tools: oha, h2load, iperf3

## Purpose

`oha`, `h2load`, and `iperf3` are the external load generators. Their
architectural reason for existing is **independent measurement**: each is a
separate process, built from a different codebase, driving the subject under
test over its own client stack. Nothing they share with the in-process
`eggfetch-http` workload driver is shared *implementation* — only the plan
workload and the metric vocabulary. Agreement between an oracle and the native
driver is therefore evidence about the subject, not a tautology about a shared
client.

Each module states the ownership split in its own header, identically in all
three ([oha.rs](../crates/eggbench-drivers/src/external/oha.rs):1-8,
[h2load.rs](../crates/eggbench-drivers/src/external/h2load.rs):1-9,
[iperf3.rs](../crates/eggbench-drivers/src/external/iperf3.rs):1-11):

| The tool owns | Eggbench owns |
|---|---|
| Load generation; the semantics of its own report | Binary resolution, version policy, argv construction, bounded execution, raw artifact retention, metric mapping, evidence |

The tool decides what a request was, how long to run, and what its numbers mean.
Eggbench decides *whether it may run*, *under what version floor*, *with which
argv*, *for how long at most*, *what raw bytes are kept*, and *how the report is
spelled in Eggbench's vocabulary*. No tool output format is redefined: each
parser validates the documented upstream shape and fails closed on anything else,
so upstream drift surfaces as a parse failure rather than a silently
reinterpreted measurement.

This document covers the three adapters only. The substrate is owned by
[drivers-external-substrate.md](drivers-external-substrate.md), normalization by
[core-metrics.md](core-metrics.md), the in-process path by
[drivers-network-path.md](drivers-network-path.md).

## Source layout

| File | Lines | Role |
|---|---|---|
| [`external/oha.rs`](../crates/eggbench-drivers/src/external/oha.rs) | 989 | HTTP load; JSON report; the only one of the three with a rate limiter and with HTTP-corpus support |
| [`external/h2load.rs`](../crates/eggbench-drivers/src/external/h2load.rs) | 843 | HTTP load; human-readable text report, no machine format |
| [`external/iperf3.rs`](../crates/eggbench-drivers/src/external/iperf3.rs) | 720 | TCP byte-stream throughput; JSON report; not HTTP |

All three are registered unconditionally by the production catalog
([catalog.rs](../crates/eggbench-drivers/src/catalog.rs):59-62) and re-exported
through [`external/mod.rs`](../crates/eggbench-drivers/src/external/mod.rs):51-60
and the crate root ([lib.rs](../crates/eggbench-drivers/src/lib.rs):58-59). None
is behind a Cargo feature: the substrate they need is the only optional part.

Each file has the same internal shape: constants → executor struct and its four
construction entry points → `WorkloadExecutor` impl → argv builder(s) →
validated report struct → parse shapes → `ExternalOutputParser` adapter →
`parse_*_report` and its row/token helpers → `*_output` → `*_descriptor` → unit
tests. The `observation` helper is written out three times with identical bodies
([oha.rs](../crates/eggbench-drivers/src/external/oha.rs):661-679,
[h2load.rs](../crates/eggbench-drivers/src/external/h2load.rs):587-605,
[iperf3.rs](../crates/eggbench-drivers/src/external/iperf3.rs):447-465); it is
not shared in `common.rs`. Each file also has its own exhaustive
`workload_target_name` match over the six `Workload` variants
([oha.rs](../crates/eggbench-drivers/src/external/oha.rs):199-208), so adding a
workload variant is a compile error in three places at once.

## Key types

### Identity, floors, and bounds

| Constant | oha | h2load | iperf3 |
|---|---|---|---|
| Driver name | `oha` ([oha.rs:39](../crates/eggbench-drivers/src/external/oha.rs)) | `h2load` ([h2load.rs:37](../crates/eggbench-drivers/src/external/h2load.rs)) | `iperf3` ([iperf3.rs:39](../crates/eggbench-drivers/src/external/iperf3.rs)) |
| Parser id | `oha-json/v1` ([oha.rs:43](../crates/eggbench-drivers/src/external/oha.rs)) | `h2load-text/v1` ([h2load.rs:41](../crates/eggbench-drivers/src/external/h2load.rs)) | `iperf3-json/v1` ([iperf3.rs:43](../crates/eggbench-drivers/src/external/iperf3.rs)) |
| Minimum version | `(1, 0, 0)` ([oha.rs:45](../crates/eggbench-drivers/src/external/oha.rs)) | `(1, 0, 0)` ([h2load.rs:43](../crates/eggbench-drivers/src/external/h2load.rs)) | `(3, 1, 0)` ([iperf3.rs:45](../crates/eggbench-drivers/src/external/iperf3.rs)) |
| Extra diagnostic artifact | `oha-status.json` ([oha.rs:47](../crates/eggbench-drivers/src/external/oha.rs)) | `h2load-status.json` ([h2load.rs:45](../crates/eggbench-drivers/src/external/h2load.rs)) | none |
| stdout cap | 4 MiB ([oha.rs:49](../crates/eggbench-drivers/src/external/oha.rs)) | 1 MiB ([h2load.rs:47](../crates/eggbench-drivers/src/external/h2load.rs)) | 4 MiB ([iperf3.rs:53](../crates/eggbench-drivers/src/external/iperf3.rs)) |
| stderr cap | 256 KiB (all three) | | |
| Probe timeout | 10 s (all three) | | |
| Tool-specific constant | — | — | `U64_MAX_AS_F64` ([iperf3.rs:51](../crates/eggbench-drivers/src/external/iperf3.rs)) |

The stdout caps sit far above the real report sizes — "JSON reports are small;
the cap only guards pathologies"
([oha.rs:48](../crates/eggbench-drivers/src/external/oha.rs)) — so they bound
retention, not content. `U64_MAX_AS_F64` makes the byte-counter range check a
named constant with a parity test instead of a lossy cast at the comparison site
([iperf3.rs:46-51](../crates/eggbench-drivers/src/external/iperf3.rs),
[iperf3.rs:631-639](../crates/eggbench-drivers/src/external/iperf3.rs)).

### Executors

`OhaWorkload`, `H2loadWorkload`, and `Iperf3Workload` share a two-field shape — a
`ResolvedExecutable` and an `Option<String>` version
([oha.rs:61-64](../crates/eggbench-drivers/src/external/oha.rs),
[h2load.rs:54-57](../crates/eggbench-drivers/src/external/h2load.rs),
[iperf3.rs:60-63](../crates/eggbench-drivers/src/external/iperf3.rs)) — and a
four-entry construction surface:

| Method | Role |
|---|---|
| `resolve()` | Synchronous trusted-path resolution via `BinaryResolver::resolve(tool, None, None)`; runs in the executor factory, before managed startup |
| `probe(exe, cancel)` | One bounded `--version` spawn returning a `ToolVersion` |
| `new(exe, version)` | Binds a probed version and enforces the floor via `check_min_version` |
| `from_resolved(exe)` | Binds without a version; the first `execute` self-probes and enforces the floor before any load runs |

The `version: None` state lets a direct `execute_run` consumer get the same floor
as the CLI preflight path
([preflight.rs](../crates/eggbench-drivers/src/external/preflight.rs):1-8); the
lazy branch is `ensure_probed`
([oha.rs:128-138](../crates/eggbench-drivers/src/external/oha.rs),
[h2load.rs:159-169](../crates/eggbench-drivers/src/external/h2load.rs),
[iperf3.rs:125-135](../crates/eggbench-drivers/src/external/iperf3.rs)). Once
set, the version is never re-probed: it is pinned for the whole run and lands in
each trial's command metadata. `Debug` renders only driver name and version,
redacting the executable path and digest
([oha.rs:141-148](../crates/eggbench-drivers/src/external/oha.rs), asserted at
[oha.rs:970-988](../crates/eggbench-drivers/src/external/oha.rs)).

### Validated report structs

| Type | Fields | Where |
|---|---|---|
| `OhaReport` | `requests_per_sec`, `success_rate`, `fastest_ms: Option`, `average_ms: Option`, `percentile_ms: BTreeMap<&'static str, f64>`, `status_codes`, `error_counts` | [oha.rs:462-470](../crates/eggbench-drivers/src/external/oha.rs) |
| `H2loadReport` | `total`, `failed`, `errored`, `timed_out`, `req_per_sec_mean`, `request_min_ms`, `request_mean_ms`, `status_codes: Option` | [h2load.rs:331-340](../crates/eggbench-drivers/src/external/h2load.rs) |
| `Iperf3Report` | `sent_bps`, `received_bps`, `sent_bytes: u64`, `received_bytes: u64`, `retransmits: Option<u64>` | [iperf3.rs:275-281](../crates/eggbench-drivers/src/external/iperf3.rs) |

The `Option` fields carry one policy: *a measurement the tool could not take
stays missing and is never zero-filled*. For oha that is an all-fail run with
`successRate: 0.0` and null timings
([oha.rs:496-503](../crates/eggbench-drivers/src/external/oha.rs), fixture at
[oha.rs:801-829](../crates/eggbench-drivers/src/external/oha.rs)).

### Descriptors

All three descriptors are pure functions with no I/O and
`category: DriverCategory::Workload`, `external_process: true`,
`default: false`, empty `supported_platforms`, empty
`compatible_service_types`, and `machine_output_schema: Some(SchemaVersion(1))`
([oha.rs:686-716](../crates/eggbench-drivers/src/external/oha.rs),
[h2load.rs:612-637](../crates/eggbench-drivers/src/external/h2load.rs),
[iperf3.rs:472-491](../crates/eggbench-drivers/src/external/iperf3.rs)).
`upstream_version` is `None` in all three
([oha.rs:707](../crates/eggbench-drivers/src/external/oha.rs),
[iperf3.rs:482](../crates/eggbench-drivers/src/external/iperf3.rs)): the
descriptor is static, and the pinned tool version lives in the executor and in
trial evidence.

| Capability | oha | h2load | iperf3 |
|---|---|---|---|
| `HttpVersion(Http11)` | yes ([oha.rs:688](../crates/eggbench-drivers/src/external/oha.rs)) | yes ([h2load.rs:614](../crates/eggbench-drivers/src/external/h2load.rs)) | — (not HTTP) |
| `HttpVersion(Http2)` | yes ([oha.rs:691](../crates/eggbench-drivers/src/external/oha.rs)) | yes ([h2load.rs:617](../crates/eggbench-drivers/src/external/h2load.rs)) | — |
| `LoadMode(ClosedLoop)` | yes ([oha.rs:694](../crates/eggbench-drivers/src/external/oha.rs)) | yes ([h2load.rs:620](../crates/eggbench-drivers/src/external/h2load.rs)) | yes ([iperf3.rs:474](../crates/eggbench-drivers/src/external/iperf3.rs)) |
| `LoadMode(OpenLoop)` | yes ([oha.rs:698](../crates/eggbench-drivers/src/external/oha.rs)) | no | no |
| `CorrectedLatency` | yes ([oha.rs:701](../crates/eggbench-drivers/src/external/oha.rs)) | no | no |
| `HttpCorpus` | yes ([oha.rs:697](../crates/eggbench-drivers/src/external/oha.rs)) | no | no |
| `ExternalBinary` | yes ([oha.rs:702](../crates/eggbench-drivers/src/external/oha.rs)) | yes ([h2load.rs:623](../crates/eggbench-drivers/src/external/h2load.rs)) | yes ([iperf3.rs:477](../crates/eggbench-drivers/src/external/iperf3.rs)) |
| `NetworkPath` / `StreamFaultPlan` | no | no | no |

Two descriptor facts deserve emphasis. `supported_platforms` is an empty set for
all three, so no platform restriction is declared for any external oracle. And
`machine_output_schema: Some(SchemaVersion(1))` is set for h2load as well, even
though h2load has no machine output — the field is the versioned parser
contract, not a claim that the tool emits machine-readable bytes.

## How it works

### Preflight and version policy

`probe_external_workload` is the single preflight entry point for external
workload drivers; for these three it resolves, probes, and constructs the
executor purely to apply the version floor
([preflight.rs:80-106](../crates/eggbench-drivers/src/external/preflight.rs)).
Resolution is synchronous in the executor factory, so a missing binary fails
before managed startup, and the CLI pins the resolved canonical path into the
plan's `executable_paths`
([preflight.rs:54-73](../crates/eggbench-drivers/src/external/preflight.rs)).
`doctor` reports `binary_present` from filesystem resolution only, never spawning
a tool ([preflight.rs:36-52](../crates/eggbench-drivers/src/external/preflight.rs)).

| Tool | Probe mechanism | Version token | On unsupported version |
|---|---|---|---|
| oha | `VersionProbe::run` with `["--version"]` ([oha.rs:82-98](../crates/eggbench-drivers/src/external/oha.rs)) | generic first `N.N` token, parsed by `parse_version_tuple` ([common.rs:162-188](../crates/eggbench-drivers/src/external/common.rs)) | `unsupported_version` from `check_min_version` ([common.rs:138-160](../crates/eggbench-drivers/src/external/common.rs)) |
| h2load | hand-rolled `run_command` on `["--version"]` ([h2load.rs:77-129](../crates/eggbench-drivers/src/external/h2load.rs)) | the nghttp2 release after the `nghttp2/` marker | same, compared against the nghttp2 release |
| iperf3 | `VersionProbe::run` with `["--version"]` ([iperf3.rs:79-95](../crates/eggbench-drivers/src/external/iperf3.rs)) | generic first `N.N` token | same |

`check_min_version` compares only the leading numeric `major.minor.patch`
components, ignores extras, and treats an unparseable token as unsupported rather
than accepting it on trust
([common.rs:132-160](../crates/eggbench-drivers/src/external/common.rs)). Probe
errors map through `probe_failure_category`, which re-checks the cancellation
token first so a cancelled probe is `Cancelled`, never a workload failure
([common.rs:116-129](../crates/eggbench-drivers/src/external/common.rs)).

**h2load is the version-policy outlier.** It prints `h2load nghttp2/<release>` —
the digits a generic extractor would key on precede the `/`, so the release
cannot be isolated generically ([h2load.rs:70-72](../crates/eggbench-drivers/src/external/h2load.rs)).
It therefore bypasses `VersionProbe::run`, runs its own `run_command`, checks the
exit code itself
([h2load.rs:107-112](../crates/eggbench-drivers/src/external/h2load.rs)), and
extracts the token with `parse_h2load_version`, which requires an `nghttp2/`
marker and a token holding both a `.` and a digit
([h2load.rs:448-460](../crates/eggbench-drivers/src/external/h2load.rs)). So
`H2LOAD_MIN_VERSION = (1, 0, 0)` is a floor on the **nghttp2** release, not on an
h2load release. It also folds `TimedOut | Cancelled` into `VersionProbeTimeout` by
hand ([h2load.rs:97-106](../crates/eggbench-drivers/src/external/h2load.rs)) —
the same folding `VersionProbe` performs, reproduced locally.

### Execution pipeline

Identical in all three; only the argv builder, parser, and output assembler
differ.

1. `ensure_probed` — enforce the floor before any load runs.
2. Resolve the target URL from the invocation bindings snapshot via
   `target_http_url` (key `http_url`); a missing binding is
   `FailureCategory::WorkloadFailed`
   ([common.rs:29-39](../crates/eggbench-drivers/src/external/common.rs)).
3. Build argv from the plan workload; the URL is appended last.
4. Run one bounded child through `run_command` with `driver_env()` (`LC_ALL=C`,
   `LANG=C` only), `stdin_null`, no cwd override, and the invocation's timeout
   ([oha.rs:171-181](../crates/eggbench-drivers/src/external/oha.rs)).
5. Parse retained stdout into the validated report, then assemble artifacts and
   raw observations.

`drain` is a no-op for all three: every invocation spawns and reaps one child, so
there is no persistent state to quiesce
([oha.rs:190-196](../crates/eggbench-drivers/src/external/oha.rs),
[h2load.rs:211-216](../crates/eggbench-drivers/src/external/h2load.rs),
[iperf3.rs:177-182](../crates/eggbench-drivers/src/external/iperf3.rs)).

### argv and the load-model knobs the plan controls

**oha** ([oha.rs:216-318](../crates/eggbench-drivers/src/external/oha.rs)) always
emits `--no-tui --output-format json` first
([oha.rs:223](../crates/eggbench-drivers/src/external/oha.rs)):

| Plan workload | argv | Knob source |
|---|---|---|
| `ClosedLoop` with `requests` | `-n <requests> -c <concurrency>` | count, concurrency |
| `ClosedLoop` with `duration_ms` | `-z <humantime> -c <concurrency>` | duration, concurrency |
| `FiniteCount` | `-n <requests> -c <concurrency>` | count, concurrency |
| `OpenLoop` | `-q <rate> --latency-correction` plus exactly one of `-n` / `-z` | rate, count **or** duration |
| `TimeBounded{ClosedLoop}` | `-z <humantime> [-c <concurrency>]` | duration, optional concurrency |
| `TimeBounded{OpenLoop}` | `-q <rate> --latency-correction -z <humantime>` | rate, duration |
| `SemanticReplay`, `HttpCorpus` on the generic path | rejected — needs a specialized invocation | — |

`--latency-correction` is unconditional on every open-loop path, which is what
makes `CorrectedLatency` true rather than aspirational. Rates trim trailing zeros
from milli-rps (`2500` → `2.5`,
[oha.rs:439-449](../crates/eggbench-drivers/src/external/oha.rs)); durations
emit as `1500ms`, not `1.5s`
([oha.rs:451-458](../crates/eggbench-drivers/src/external/oha.rs), pinned by
[oha.rs:955-967](../crates/eggbench-drivers/src/external/oha.rs)). A count **and**
duration together fails closed rather than being coerced, because oha silently
ignores `-n` under `-z` — a silent load-model change is exactly what the parser
contract forbids ([oha.rs:243-247](../crates/eggbench-drivers/src/external/oha.rs),
[oha.rs:277-281](../crates/eggbench-drivers/src/external/oha.rs)).

oha also has the only second argv builder, `oha_http_corpus_argv`
([oha.rs:321-436](../crates/eggbench-drivers/src/external/oha.rs)), adding
`--method`, one `-H` per case header, one `-H` per non-conflicting default
header, `--disable-keepalive` for `HttpConnectionPolicy::FreshPerRequest`
([oha.rs:388-390](../crates/eggbench-drivers/src/external/oha.rs)), and `-D` for
a body. It enforces: exactly one scheduled case
([oha.rs:344-346](../crates/eggbench-drivers/src/external/oha.rs)); a
digest-verified corpus load
([oha.rs:347-348](../crates/eggbench-drivers/src/external/oha.rs)); no default
header contradicting a case header
([oha.rs:355-361](../crates/eggbench-drivers/src/external/oha.rs)); and an
absolute HTTP target, since it rebinds `origin + path_and_query` rather than
forwarding the configured path
([oha.rs:411-428](../crates/eggbench-drivers/src/external/oha.rs)). An inline
body is spilled to a `NamedTempFile` **returned alongside the argv** and flushed
before return, so the handle outlives argv construction and the file cannot
vanish before the child reads it
([oha.rs:391-406](../crates/eggbench-drivers/src/external/oha.rs),
[oha.rs:429-435](../crates/eggbench-drivers/src/external/oha.rs), rebound at
[oha.rs:159-170](../crates/eggbench-drivers/src/external/oha.rs)); a file body is
canonicalized under the corpus body root
([oha.rs:400-405](../crates/eggbench-drivers/src/external/oha.rs)).

**h2load** ([h2load.rs:235-311](../crates/eggbench-drivers/src/external/h2load.rs))
switches on the URL scheme first: `http` gets `--h1`, `https` gets nothing (the
HTTP/2 default), anything else fails closed
([h2load.rs:242-248](../crates/eggbench-drivers/src/external/h2load.rs)).

| Plan workload | argv | Knob source |
|---|---|---|
| `ClosedLoop` with `requests` | `-n <requests> -c <concurrency>` | count, concurrency |
| `ClosedLoop` with `duration_ms` | `--duration=<secs> [-c <concurrency>]` | duration, concurrency |
| `FiniteCount` | `-n <requests> -c <concurrency>` | count, concurrency |
| `TimeBounded{ClosedLoop}` | `--duration=<secs> [-c <concurrency>]` | duration, optional concurrency |
| `OpenLoop`, `TimeBounded{OpenLoop}` | rejected — no rate limiter | — |
| `SemanticReplay`, `HttpCorpus` | rejected | — |

`--duration` keeps sub-second precision as trimmed decimals (`1500 ms` → `1.5`,
[h2load.rs:318-328](../crates/eggbench-drivers/src/external/h2load.rs), pinned by
[h2load.rs:805-821](../crates/eggbench-drivers/src/external/h2load.rs)). No
timing scripts, header, multi-URI, or H3/QUIC options are built: only the
capability-advertised subset reaches argv.

**iperf3** ([iperf3.rs:200-266](../crates/eggbench-drivers/src/external/iperf3.rs))
takes host and port from the target URL authority via `authority_host_port`,
defaulting to 5201
([iperf3.rs:207-208](../crates/eggbench-drivers/src/external/iperf3.rs),
[common.rs:46-80](../crates/eggbench-drivers/src/external/common.rs)). Scheme and
path are discarded; the helper rejects an empty authority, a malformed IPv6
bracket, and a non-`u16` port rather than guessing.

| Plan workload | argv | Knob source |
|---|---|---|
| `ClosedLoop` with `duration_ms` | `-c <host> -p <port> -t <secs> -J -P <concurrency>` | duration, concurrency → parallel streams |
| `TimeBounded{ClosedLoop}` | `-c <host> -p <port> -t <secs> -J [-P <concurrency>]` | duration, optional concurrency |
| `ClosedLoop` with `requests`, `FiniteCount` | rejected — no byte-stream equivalent | — |
| `OpenLoop`, `TimeBounded{OpenLoop}` | rejected — no TCP rate mapping | — |
| `SemanticReplay`, `HttpCorpus` | rejected | — |

`-t` is **ceiling**-divided to whole seconds with a floor of 1
([iperf3.rs:268-271](../crates/eggbench-drivers/src/external/iperf3.rs)), so a
1500 ms workload runs 2 s: the window is longer than requested, never shorter, and
`div_ceil` avoids the `ms + 999` overflow near `u64::MAX`
([iperf3.rs:611-629](../crates/eggbench-drivers/src/external/iperf3.rs)). There
is no `-R`, `-u`, `-s`, or server-spawning option: Eggbench never manages an
iperf3 server ([iperf3.rs:4-7](../crates/eggbench-drivers/src/external/iperf3.rs)).

### Report → metric mapping

The built-in vocabulary is `builtin_metric` in
[`metrics.rs`](../crates/eggbench-core/src/metrics.rs):345-409; the last column
marks whether the name resolves there. A blank present-column means the adapter
emitted no observation and normalization must record `missing`, not zero (see
[core-metrics.md](core-metrics.md)).

**oha — `--output-format json`**
([oha.rs:588-659](../crates/eggbench-drivers/src/external/oha.rs)). The parser
requires the four upstream `schema.json` members `summary`,
`latencyPercentiles`, `statusCodeDistribution`, `errorDistribution`
([oha.rs:472-481](../crates/eggbench-drivers/src/external/oha.rs), enforced at
[oha.rs:534-535](../crates/eggbench-drivers/src/external/oha.rs)):

| Report path | Metric | Unit | Aggregation | `source_field` | Present | Built-in |
|---|---|---|---|---|---|---|
| `summary.requestsPerSec` | `throughput` | `rps` | `Rate` | `oha.summary.requests_per_sec` | always | yes |
| `1 − summary.successRate` | `error_rate` | `ratio` | `Ratio` | `oha.summary.success_rate` | always | yes |
| `summary.fastest` × 1000 | `latency_min` | `ms` | `Minimum` | `oha.summary.fastest` | when observed | yes |
| `summary.average` × 1000 | `latency_mean` | `ms` | `Mean` | `oha.summary.average` | when observed | yes |
| `latencyPercentiles.p50` × 1000 | `latency_p50` | `ms` | `Percentile{5000}` | `oha.latency_percentiles` | when observed | yes |
| `latencyPercentiles.p95` × 1000 | `latency_p95` | `ms` | `Percentile{9500}` | `oha.latency_percentiles` | when observed | yes |
| `latencyPercentiles.p99` × 1000 | `latency_p99` | `ms` | `Percentile{9900}` | `oha.latency_percentiles` | when observed | yes |
| `summary.slowest` | *no metric* | — | — | — | — | — |
| `errorDistribution` | error counts `oha:<message>` | — | — | — | per entry | n/a |
| `statusCodeDistribution` | `oha-status.json` artifact | — | — | — | always | n/a |

Latency conversion is a single `× 1000` from oha's seconds
([oha.rs:545-549](../crates/eggbench-drivers/src/external/oha.rs)), applied to
timings and percentiles alike. `successRate > 1.0` is a parse failure, not a clamp
([oha.rs:536-542](../crates/eggbench-drivers/src/external/oha.rs)), which is what
keeps `error_rate = 1 − successRate` inside the `[0, 1]` domain `Ratio` requires.
`slowest` is deserialized only to validate the report shape; there is no
`latency_max` in the vocabulary and oha does not invent one
([oha.rs:489-491](../crates/eggbench-drivers/src/external/oha.rs)). p90 and p999
stay missing by construction
([oha.rs:636-640](../crates/eggbench-drivers/src/external/oha.rs), asserted at
[oha.rs:791](../crates/eggbench-drivers/src/external/oha.rs)).

**h2load — human-readable text** ([h2load.rs:521-585](../crates/eggbench-drivers/src/external/h2load.rs)).
The parser skips every line up to and including the first one starting with
`finished in `, then requires three rows
([h2load.rs:364-401](../crates/eggbench-drivers/src/external/h2load.rs)):

| Report row | Metric | Unit | Aggregation | `source_field` | Present | Built-in |
|---|---|---|---|---|---|---|
| `req/s` — third numeric cell (mean) | `throughput` | `rps` | `Rate` | `h2load.req_per_sec.mean` | always | yes |
| `time for request:` — cell 0 (min) | `latency_min` | `ms` | `Minimum` | `h2load.time_for_request.min` | always | yes |
| `time for request:` — cell 2 (mean) | `latency_mean` | `ms` | `Mean` | `h2load.time_for_request.mean` | always | yes |
| `requests:` `failed` / `total` | `error_rate` | `ratio` | `Ratio` | `h2load.requests.failed` | only when `total > 0` | yes |
| `requests:` `failed` / `errored` / `timeout` | error counts `h2load:<label>` | — | — | — | per nonzero count | n/a |
| `status codes: N 2xx, …` | `h2load-status.json` artifact | — | — | — | row present | n/a |
| `finished in … req/s`, `time for connect`, `time to 1st byte`, `traffic`, `req/s` max/sd, `time for request` max/sd | *discarded* | — | — | — | — | — |

Three properties of this table are easy to get wrong.

- **`throughput` is the per-client `req/s` mean, not the summary rate.** The
  same stdout carries both — the fixture shows `finished in 4.87ms, 2055.50
  req/s` in the summary line and `req/s … 1072.78` in the row
  ([h2load.rs:673-681](../crates/eggbench-drivers/src/external/h2load.rs)) — and
  the parser takes the row
  ([h2load.rs:482-495](../crates/eggbench-drivers/src/external/h2load.rs),
  asserted at [h2load.rs:708](../crates/eggbench-drivers/src/external/h2load.rs)).
  The two are not interchangeable; the row is anchored after the marker and
  positionally stable.
- **h2load has no percentiles.** No `latency_p50/p95/p99` appears in its mapping,
  so a plan requesting them normalizes as `missing` against an h2load trial — a
  real capability gap between the two HTTP oracles, not a parser omission.
- **`error_rate` is absent, not zero, when `total == 0`**
  ([h2load.rs:558-567](../crates/eggbench-drivers/src/external/h2load.rs)) — the
  opposite of oha, which always emits it. A zero-request h2load run yields no
  ratio at all.

h2load's durations are unit-suffixed and converted to ms by suffix, accepting
`us`, `ms`, `s`, and `m`
([h2load.rs:462-479](../crates/eggbench-drivers/src/external/h2load.rs)); the
`requests:` row is comma-separated `count label` cells, of which only `total`,
`failed`, `errored`, and `timeout` are consumed
([h2load.rs:405-432](../crates/eggbench-drivers/src/external/h2load.rs)).

**iperf3 — `-J`** ([iperf3.rs:390-445](../crates/eggbench-drivers/src/external/iperf3.rs)):

| Report path | Metric | Unit | Aggregation | `source_field` | Present | Built-in |
|---|---|---|---|---|---|---|
| `end.sum_sent.bits_per_second` | `bits_per_sec_sent` | `bps` | `Rate` | `iperf3.end.sum_sent.bits_per_second` | always | **no** |
| `end.sum_received.bits_per_second` | `bits_per_sec_received` | `bps` | `Rate` | `iperf3.end.sum_received.bits_per_second` | always | **no** |
| `end.sum_sent.bytes` | `bytes_sent` | `bytes` | `Sum` | `iperf3.end.sum_sent.bytes` | always | yes |
| `end.sum_received.bytes` | `bytes_received` | `bytes` | `Sum` | `iperf3.end.sum_received.bytes` | always | yes |
| `end.sum_sent.retransmits` | `retransmits` | `count` | `Sum` | `iperf3.end.sum_sent.retransmits` | member present | **no** |
| top-level `error` | *parse failure* | — | — | — | — | — |
| `start.*`, `intervals.*` | *ignored* | — | — | — | — | — |

iperf3 is a TCP byte-stream tool, and that changes the vocabulary work in three
ways, all visible in the table:

1. **No `throughput` name, deliberately.** The unit is bits per second, not
   requests per second; emitting `throughput` would let a plan compare a byte rate
   against a request rate as one measurement. A unit test asserts the absence
   ([iperf3.rs:549](../crates/eggbench-drivers/src/external/iperf3.rs)).
2. **No latency names at all.** TCP has no request latency, so there is no
   `latency_*` observation; every `latency_*` request against an iperf3 trial
   normalizes as `missing`.
3. **Only two of its five names are in the built-in vocabulary.**
   `bytes_sent`/`bytes_received` are built in with exactly the emitted unit and
   aggregation ([metrics.rs](../crates/eggbench-core/src/metrics.rs):392-396);
   `bits_per_sec_sent`, `bits_per_sec_received`, and `retransmits` are not, so
   they get no vocabulary unit check, no vocabulary direction, and an
   `Aggregation::Direct` fallback when a trial is missing
   ([metrics.rs](../crates/eggbench-core/src/metrics.rs):789-796,
   [metrics.rs](../crates/eggbench-core/src/metrics.rs):407). A plan comparing
   throughput across oracles must declare its own direction for the iperf3 names
   rather than inheriting `HigherIsBetter`. This needs no special handling
   *inside the adapter*, but it is the one place a reader of the metrics contract
   can be surprised.

`retransmits` is emitted only when the member exists, and only from `sum_sent`
([iperf3.rs:428-437](../crates/eggbench-drivers/src/external/iperf3.rs)), so a
non-TCP-shaped report leaves it missing rather than zero. Byte and retransmit
counters cross an integer→`f64` boundary through `metric_u64_as_f64`, which
documents the precision property above 2^53
([common.rs](../crates/eggbench-drivers/src/external/common.rs):101-114). Those
integers come from `f64` JSON fields and are range-checked against
`U64_MAX_AS_F64` before the cast, so a value one representable step above the
bound is rejected rather than silently wrapping
([iperf3.rs:353-380](../crates/eggbench-drivers/src/external/iperf3.rs),
[iperf3.rs:641-698](../crates/eggbench-drivers/src/external/iperf3.rs)).

### Raw observations and artifacts

All three emit `RawMetricObservation` — protocol-neutral, un-normalized — and
never write normalized metric JSON; only `core::metrics` does
([overview.md](overview.md):183-185). `producer` and `producer_version` are left
`None` ([oha.rs:675](../crates/eggbench-drivers/src/external/oha.rs),
[h2load.rs:601](../crates/eggbench-drivers/src/external/h2load.rs),
[iperf3.rs:461](../crates/eggbench-drivers/src/external/iperf3.rs)): tool identity
travels in `command-metadata.json` and the resolved plan, not per observation.
`histograms` is empty and `measurement_elapsed` is `None` throughout — no adapter
claims a measurement window it did not observe
([iperf3.rs:438-444](../crates/eggbench-drivers/src/external/iperf3.rs)).
Every observation cites exactly one raw artifact, `stdout.raw`
([oha.rs:597](../crates/eggbench-drivers/src/external/oha.rs)), so each number
traces to the retained bytes. The `*_Parser` adapters satisfy the versioned
`ExternalOutputParser` contract independent of spawning; each reports `parser_id`
plus a `truncated` flag and leaves `tool_version` empty, because the version comes
from the preflight probe, not the report
([oha.rs:506-524](../crates/eggbench-drivers/src/external/oha.rs),
[h2load.rs:343-358](../crates/eggbench-drivers/src/external/h2load.rs),
[iperf3.rs:310-325](../crates/eggbench-drivers/src/external/iperf3.rs)).

Artifacts per trial are `stdout.raw`, `stderr.raw`, and `command-metadata.json`
from `artifact_candidates`
([artifact.rs](../crates/eggbench-drivers/src/external/artifact.rs):13-34), plus
`oha-status.json` unconditionally
([oha.rs:590-596](../crates/eggbench-drivers/src/external/oha.rs)) or
`h2load-status.json` only when the row was present
([h2load.rs:523-530](../crates/eggbench-drivers/src/external/h2load.rs)); iperf3
adds none. Both status artifacts restate a distribution already in `stdout.raw`,
and a live test asserts byte-equality of the two JSON views
([oracles.rs](../crates/eggbench-drivers/tests/oracles.rs):467-481).

### Why these three advertise neither `NetworkPath` nor `StreamFaultPlan`

`NetworkPath` means the driver owns a custom dialer for the HTTP transport;
`StreamFaultPlan` means it produces a static deterministic fault plan
([resolved.rs](../crates/eggbench-core/src/resolved.rs):92-97). Both are
properties of an **in-process** client Eggbench constructs. These three are
`argv[0]` child processes connected through the tool's own socket stack, so the
route-first/fault-second lowering in
[drivers-network-path.md](drivers-network-path.md) has nothing to wrap: no
Eggbench-owned dial to intercept, no transport to wrap after the route.

This is enforced, not merely omitted. The descriptors carry
`external_process: true` and no such capability, and the resolver rejects an
external-process workload for a network path on the capability, on
`external_process`, and on a third condition that no `executable_path` may be set
([resolved.rs](../crates/eggbench-core/src/resolved.rs):964-974), with the same
structure on the fault driver
([resolved.rs](../crates/eggbench-core/src/resolved.rs):1028-1032). Evidence
validation repeats the rule ([evidence.rs](../crates/eggbench-core/src/evidence.rs):1641-1646,
[evidence.rs](../crates/eggbench-core/src/evidence.rs):1710-1713). Descriptor and
resolver contracts agree; the statement in [drivers.md](drivers.md):39 is
accurate.

## Invariants

1. **Exit status is not a success signal for oha or h2load.** Both exit 0 with
   every request failed; oha reads success from `successRate` and the error
   distribution ([oha.rs:526-530](../crates/eggbench-drivers/src/external/oha.rs)),
   h2load from the request counts
   ([h2load.rs:1-9](../crates/eggbench-drivers/src/external/h2load.rs)). Neither
   parser reads `exit_code`; an unreachable target is a *completed trial with
   `error_rate = 1`*
   ([oracles.rs](../crates/eggbench-drivers/tests/oracles.rs):268-290,
   [oracles.rs](../crates/eggbench-drivers/tests/oracles.rs):328-352).
2. **iperf3 inverts that rule**: a top-level `error` member — the
   refused-connection shape — is a tool failure, never a zero-throughput
   observation ([iperf3.rs:327-338](../crates/eggbench-drivers/src/external/iperf3.rs)).
   A zero-byte transfer is legitimate; a refused connection is not a measurement.
3. **A missing measurement stays missing.** No percentile, timing, or
   `retransmits` value is ever zero-filled
   ([oha.rs:496-503](../crates/eggbench-drivers/src/external/oha.rs),
   [h2load.rs:558](../crates/eggbench-drivers/src/external/h2load.rs),
   [iperf3.rs:428](../crates/eggbench-drivers/src/external/iperf3.rs)).
4. **Domain checks are per field, not per document.** Every scalar that becomes a
   metric passes `finite_non_negative`
   ([common.rs](../crates/eggbench-drivers/src/external/common.rs):190-197);
   `successRate` must additionally be ≤ 1
   ([oha.rs:536-542](../crates/eggbench-drivers/src/external/oha.rs)); byte and
   retransmit counters are range-bounded before the `u64` cast.
5. **A workload the tool cannot express fails closed, never approximately.**
   Count-plus-duration for oha, any rate for h2load, any count for iperf3, and
   every corpus/replay workload outside oha's dedicated builder produce
   `ErrorCategory::UnsupportedOption` before a process is spawned.
6. **argv is the only interface.** No shell, no glob, no inherited cwd, no
   environment beyond `LC_ALL=C`/`LANG=C`; the URL is the last argument in the
   HTTP adapters.
7. **The version is probed once per executor and never re-probed**, so every
   trial in a run carries the same tool version.
8. **Raw bytes are retained before parsing**, so a parse failure still leaves the
   tool's own output in evidence.
9. **One child per invocation**; `drain` is a no-op because nothing persists.
10. **The plan cannot select the HTTP version.** Both HTTP adapters advertise
    `HttpVersion(Http11)` and `HttpVersion(Http2)`, but no argv flag chooses
    between them: for h2load the scheme alone decides
    ([h2load.rs:242-248](../crates/eggbench-drivers/src/external/h2load.rs)), and
    oha passes no version flag. The capability states what the tool can do against
    the bound URL, not what the plan can dial.

## Failure model

| Condition | Category | Trial outcome |
|---|---|---|
| No trusted binary | resolution / `binary_not_found` | Fails in the executor factory, before managed startup |
| Probe timeout | `version_probe_timeout` | Preflight failure; no trial |
| Probe nonzero exit (h2load path) | `version_probe_failed` | Preflight failure ([h2load.rs:107-112](../crates/eggbench-drivers/src/external/h2load.rs)) |
| Version token unparseable | `version_probe_failed` | Preflight failure; h2load requires an `nghttp2/` release ([h2load.rs:114-119](../crates/eggbench-drivers/src/external/h2load.rs)) |
| Version below the floor | `unsupported_version` | Preflight failure; also on the lazy self-probe path |
| Workload the tool cannot express | `unsupported_option` | No process is spawned |
| Missing `http_url` binding | `FailureCategory::WorkloadFailed` | No spawn ([common.rs:38](../crates/eggbench-drivers/src/external/common.rs)) |
| Driver timeout | `TimedOut` | Trial fails |
| Cancellation token | `Cancelled` | Distinct from workload failure via `failure_category` / `probe_failure_category` ([common.rs:92-99](../crates/eggbench-drivers/src/external/common.rs)) |
| Malformed, incomplete, or domain-invalid report | `parse_failed` | Trial fails; raw bytes already retained |
| iperf3 `error` member | `parse_failed` | Trial fails, not a zero observation |
| Every request failed (oha, h2load) | none | Trial **succeeds** with `error_rate = 1` and error counts |
| Parse failure under a cancelled token | `Cancelled` | Never downgraded to a workload failure |

A stdout stream that hits its cap is reported as `truncated` through the parser
contract ([oha.rs:521](../crates/eggbench-drivers/src/external/oha.rs)) rather
than being silently accepted. A parse failure never mutates the invocation into a
different load or protocol semantic — the parser contract's explicit prohibition
([parser.rs](../crates/eggbench-drivers/src/external/parser.rs):44-50).

## Boundaries

**These adapters own:** argv construction from the plan workload, report
validation, metric-name mapping, error-count labeling, one status diagnostic
artifact, and the capability/descriptor declaration.

**They do not own:** process lifecycle (spawning, draining, capping,
process-group cleanup, and cancellation belong to
[drivers-external-substrate.md](drivers-external-substrate.md)); normalization
(`core::metrics` writes `TrialMetrics`, see
[core-metrics.md](core-metrics.md)); target lifecycle (the subject belongs to the
runner, and iperf3's peer server is always an externally configured host — the
adapter never spawns one
([iperf3.rs:4-7](../crates/eggbench-drivers/src/external/iperf3.rs)), the live
test spawning `iperf3 -s` as a fixture
([oracles.rs](../crates/eggbench-drivers/tests/oracles.rs):97-108)); routing and
faults (above); the installed toolchain; and corpus validation (oha calls
`load_http_security_corpus` for digest verification
([oha.rs:347-348](../crates/eggbench-drivers/src/external/oha.rs)) while the
corpus schema is a core contract).

**Capability gating is the outer ring.** The resolver derives the required
load-mode capability from the plan workload
([resolved.rs](../crates/eggbench-core/src/resolved.rs):438-445) with `OpenLoop`
mapping to `LoadMode::OpenLoop`
([resolved.rs](../crates/eggbench-core/src/resolved.rs):1044-1047), and
`HttpCorpus` additionally requires `Capability::HttpCorpus`
([resolved.rs](../crates/eggbench-core/src/resolved.rs):431-437). Since only oha
advertises those, an open-loop or corpus plan resolves to oha or fails; the
argv-level rejections in the other two are defense in depth, not the primary gate.
All three are `default: false`, so without explicit selection the marked default
workload driver wins.

## Tests and qualification

**Unit tests, next to each adapter.** Fixture-driven and spawning nothing: they
build an `ExternalCommandOutcome` with retained bytes and assert parse, mapping,
and argv behavior.

| Area | oha | h2load | iperf3 |
|---|---|---|---|
| Happy-path metric names and values | [oha.rs:774-798](../crates/eggbench-drivers/src/external/oha.rs) | [h2load.rs:696-725](../crates/eggbench-drivers/src/external/h2load.rs) | [iperf3.rs:536-550](../crates/eggbench-drivers/src/external/iperf3.rs) |
| All-fail run stays a valid observation | [oha.rs:800-829](../crates/eggbench-drivers/src/external/oha.rs) | [h2load.rs:727-749](../crates/eggbench-drivers/src/external/h2load.rs) | [iperf3.rs:552-563](../crates/eggbench-drivers/src/external/iperf3.rs) |
| Malformed / missing / domain-invalid fails | [oha.rs:831-840](../crates/eggbench-drivers/src/external/oha.rs) | [h2load.rs:751-762](../crates/eggbench-drivers/src/external/h2load.rs) | [iperf3.rs:565-576](../crates/eggbench-drivers/src/external/iperf3.rs) |
| argv matrix and rejections | [oha.rs:842-890](../crates/eggbench-drivers/src/external/oha.rs) | [h2load.rs:774-803](../crates/eggbench-drivers/src/external/h2load.rs) | [iperf3.rs:578-609](../crates/eggbench-drivers/src/external/iperf3.rs) |
| Corpus argv (oha only) | [oha.rs:892-953](../crates/eggbench-drivers/src/external/oha.rs) | — | — |
| Duration / number text parity | [oha.rs:955-967](../crates/eggbench-drivers/src/external/oha.rs) | [h2load.rs:805-821](../crates/eggbench-drivers/src/external/h2load.rs) | [iperf3.rs:611-629](../crates/eggbench-drivers/src/external/iperf3.rs) |
| Version token extraction | — | [h2load.rs:764-772](../crates/eggbench-drivers/src/external/h2load.rs) | — |
| Numeric boundary parity | — | — | [iperf3.rs:631-698](../crates/eggbench-drivers/src/external/iperf3.rs) |
| `Debug` redaction | [oha.rs:969-988](../crates/eggbench-drivers/src/external/oha.rs) | [h2load.rs:823-842](../crates/eggbench-drivers/src/external/h2load.rs) | [iperf3.rs:700-719](../crates/eggbench-drivers/src/external/iperf3.rs) |

The three "text parity" tests are not cosmetic: each pins a byte-exact argv
fragment against a documented pre-refactor authority (`humantime`,
`duration_secs`, and the `u64::MAX` ceil), so a refactor cannot silently change
the emitted number while every behavioral test still passes.

**Integration tests** in
[`tests/oracles.rs`](../crates/eggbench-drivers/tests/oracles.rs). Descriptors and
preflight helpers are covered unconditionally
([oracles.rs](../crates/eggbench-drivers/tests/oracles.rs):160-219); live loopback
runs only when the tool binary is present, otherwise printing a skip note and
returning, so a machine without these tools still runs the suite green without
asserting anything false
([oracles.rs](../crates/eggbench-drivers/tests/oracles.rs):43-50). Live coverage:
parity metrics against `python3 -m http.server`
([oracles.rs](../crates/eggbench-drivers/tests/oracles.rs):223-266,
[oracles.rs](../crates/eggbench-drivers/tests/oracles.rs):294-326), the all-fail
cases
([oracles.rs](../crates/eggbench-drivers/tests/oracles.rs):268-290,
[oracles.rs](../crates/eggbench-drivers/tests/oracles.rs):328-352), iperf3 against
a fixture server and the refused-connection failure
([oracles.rs](../crates/eggbench-drivers/tests/oracles.rs):356-408), cancellation
propagating as `Cancelled`
([oracles.rs](../crates/eggbench-drivers/tests/oracles.rs):412-437), and
raw-evidence determinism
([oracles.rs](../crates/eggbench-drivers/tests/oracles.rs):441-481). The
missing-binary path is qualified through resolution instead: the adapter tests
never exercise "binary absent", and the suite is explicit that these are different
things ([oracles.rs](../crates/eggbench-drivers/tests/oracles.rs):1-7).

## Review focus

Ordered by how expensive a mistake would be to catch downstream.

1. **The report→metric mapping tables above.** A wrong `source_field`, unit, or
   `Aggregation` is indistinguishable from a real regression in the evidence.
   `Aggregation` is checked against the vocabulary at normalization time
   ([metrics.rs](../crates/eggbench-core/src/metrics.rs):965-974), so a mismatch
   surfaces as `invalid` — but only for built-in names; `bits_per_sec_*` and
   `retransmits` have no vocabulary entry to catch them.
2. **h2load's `req/s` mean vs the `finished in` summary rate.** Two throughput
   numbers exist in one report and only one is mapped; a change to
   `parse_rate_row`'s cell index would silently alter `throughput` with no failing
   test if the fixture's two numbers ever converged.
3. **h2load's `throughput` having no percentile family.** A plan shared between
   oha and h2load gets `latency_p99` from one and `missing` from the other, and no
   descriptor advertises the asymmetry.
4. **`error_rate` presence differs**: oha always emits it, h2load only when
   `total > 0`. Both satisfy the vocabulary, but a cross-oracle comparison must
   treat "absent" and "0" differently.
5. **Exit-status handling.** The oha and h2load parsers deliberately never read
   `exit_code`; gating on it would convert visible all-fail trials into hard
   failures — a change in what the evidence means.
6. **The three duplicated `observation` bodies and three `workload_target_name`
   matches.** A vocabulary change must be applied three times, not
   compiler-enforced as one unit.
7. **`machine_output_schema: Some(SchemaVersion(1))` on a text parser** — correct
   as "parser contract version", misleading as "the tool emits machine output".
8. **h2load's local version probe**
   ([h2load.rs:97-112](../crates/eggbench-drivers/src/external/h2load.rs)) is the
   one place these adapters touch the substrate directly instead of through
   `VersionProbe`, re-implementing exit-code and timeout folding. Its
   `H2LOAD_MIN_VERSION` floor applies to the nghttp2 release, easily misread as an
   h2load version.
9. **iperf3's concurrency → `-P` parallel streams.** The same plan field means
   in-flight requests for the HTTP adapters and parallel byte streams here; only
   the driver name records which meaning was used.
10. **iperf3's `-t` ceiling.** A 1500 ms workload measures 2 s — bounded and
    always upward, but not the requested window.

## Related

- [External command substrate](drivers-external-substrate.md) — resolution,
  probes, bounded execution, artifact retention, `ExternalOutputParser`.
- [Metric vocabulary and normalization](core-metrics.md) — built-in names, units,
  directions, and the `missing` / `invalid` states these mappings feed.
- [Network path](drivers-network-path.md) — the route-first / fault-second
  lowering these three cannot participate in.
- [Driver architecture](drivers.md) — descriptor and resolution contracts, and
  the external-adapter statement this document verifies.
- [Driver resolution](core-driver-resolution.md) — capability derivation and the
  `executable_paths` pin.
- [Overview](overview.md) — the normalize-once and no-silent-fallback rules.
- [External drivers](../docs/external-drivers.md) — catalog ownership and
  resolution policy.
- [External measurement oracles](../docs/external-oracles.md) — the M002 narrative.
- [Baselines](../docs/baselines.md) — how trial observations become baseline
  comparison inputs.
