# External Oracles M002 — HTTP and Capacity Oracles (oha/h2load/iperf3)

Status: ready (2026-09-24; author and implementer are the same agent pass —
review checklist §14 was worked explicitly before implementation)

Source roadmap: `plans/subsystems/external-oracles-roadmap.md` M002.
Long-term: `plans/002-long-term-roadmap.md` Phase 6 (substrate already
landed; this plan adds the three tool adapters, not netem).
Relevant ADRs: ADR-0001 (typed boundaries, capability failures),
ADR-0004 (independent oracles; no duplicated protocol behavior).
Baseline: commit `69707fd` (Eggstack M001 complete).

## 1. Objective

Implement `oha`, `h2load`, and `iperf3` workload drivers on the M001
external-command substrate (`crates/eggbench-drivers/src/external/`):
trusted resolution, version probes, argv execution, raw retention, and the
versioned parser contract. Each adapter records exact executable identity
and version, parses machine output (JSON for oha/iperf3, anchored text
stats for h2load), retains raw output beside normalized metrics, maps to
parity metric names where the tool supports them, and fails explicitly
(missing binary, unsupported version/option, unachieved load visible, no
silent fallback). Netem (M003) is out of scope.

## 2. Current implementation evidence (verified 2026-09-24)

- Substrate: `external/{resolver,command,version,parser,artifact,error}.rs`
  (`BinaryResolver::resolve`, `run_command`, `VersionProbe::run`,
  `ExternalOutputParser`, `artifact_candidates`, `DriverError/ErrorCategory`).
- Catalog: `DriverCatalog::production()` registers feature-gated drivers;
  external-process adapters need no cargo feature (absence is a runtime
  capability error, never a build configuration).
- Executor seam: `WorkloadExecutor::execute(InvocationContext) ->
  WorkloadOutput { artifacts, metrics, histograms, error_counts }`
  (`eggbench-runner/src/orchestration.rs`); CLI factory
  `production_workload_executor` in
  `crates/eggbench-cli/src/workload_registry.rs`.
- Plan model: `Workload::{ClosedLoop{target,concurrency,requests?,duration_ms?},
  OpenLoop{target,rate_milli_rps,requests?,duration_ms?}, FiniteCount{...}}`
  (schema v1, unchanged by this plan). Targets resolve to an `http_url`
  runtime binding for managed services.
- Live tool ground truth (loopback, this host):
  - `oha 1.16.0`: `--output-format json` keys exactly match upstream
    `schema.json` (draft 2020-12): `summary`, `responseTimeHistogram`,
    `latencyPercentiles`, `firstByteHistogram`, `firstBytePercentiles`,
    `rps`, `details`, `statusCodeDistribution`, `errorDistribution`
    (plus optional `metrics`). All-fail run: exit **0**, nullable
    `slowest/fastest/average/sizePerRequest` (JSON null), `successRate 0`,
    `errorDistribution {"Connection refused (os error 111)": 5}`.
  - `h2load nghttp2/1.59.0`: human-readable stats only. All-fail run:
    exit **0** with `requests: 5 total, 0 started, 0 done, 0 succeeded,
    5 failed, 5 errored, 0 timeout`. Success run: `finished in …`,
    `requests:`, `status codes:`, `traffic:`, `time for request/connect/1st
    byte:`, `req/s:` rows. Exit code is NOT a failure signal; counts are.
  - `iperf 3.16`: `-J` top-level `{start, intervals, end}` with
    `end.{sum_sent,sum_received}.{seconds,bytes,bits_per_second,…}`;
    refused connection: exit **1**, stdout JSON `{"start":…,"intervals":[],
    "end":{}, "error":"…"}`. `-s -1` single-test server mode exits 0.

## 3. Invariants

- Exact executable path + SHA-256 + version in every trial's
  `command-metadata.json`; machine output used where available (oha/iperf3
  JSON; h2load has no machine format, so anchored line parsing with
  required-row validation).
- No load-model fallback: count-bound plans on iperf3 fail closed (it has
  no request-count semantic); OpenLoop on h2load fails closed (no rate
  limiter); unsupported flags are never silently dropped.
- Parsers are versioned (`oha-json/v1`, `h2load-text/v1`, `iperf3-json/v1`),
  fixture-tested, and reject nonfinite/domain-invalid numbers.
- Unachieved offered load is visible (oha `successRate<1` + error
  distribution, h2load failed/errored/timeout counts, iperf3 retransmits
  and `error` member) but never auto-invalidates the trial: warnings and
  error categories carry it; verdicts stay with gates (ADR-0003).
- Absence of a binary is a capability error (`binary_not_found`) in
  `doctor` and pre-execution resolution, never a fallback to another
  workload. Minimal build has no new cargo dependencies.
- argv only, stdin null, `LC_ALL=C`, bounded capture, cancellation and
  trial-timeout propagation through `run_command`.

## 4. Non-goals

- Linux netem (M003), Eggstack route/fault work (Eggstack M002),
  UDP/iperf3 server management (iperf3 server is an explicit configured
  host; loopback tests spawn `iperf3 -s -1` as a test fixture only),
  oha `--stats-success-breakdown` / CSV / DB / redirect-chasing / auth /
  proxy / body-upload options, h2load timing-script/multi-URI/H3/QUIC,
  adaptive trial extension, cross-tool common-denominator metrics.

## 5. Production changes

`crates/eggbench-drivers/src/external/` (new files `oha.rs`, `h2load.rs`,
`iperf3.rs` + shared `mod` wiring; no substrate changes expected):

- oha (`oha` driver): resolve `oha`; probe `--version` (require ≥1.0.0 for
  `--output-format json`; record `oha-json/v1`). ClosedLoop count →
  `-n/-c`, duration → `-z`; OpenLoop rate+count/duration → `-q <rps>
  --latency-correction` + `-n/-z`; always `--no-tui --output-format json`,
  target URL from the workload target's `http_url` binding. Parse required
  schema.json members; accept nullable timing fields (all-fail runs);
  metrics: `throughput` (rps, Rate ← requestsPerSec), `error_rate` (ratio
  ← 1−successRate), `latency_min/mean/p50/p95/p99` (ms ← fastest/average/
  percentiles×1000; p90/p999 stay missing), `timeout_rate` missing;
  `error_counts` ← errorDistribution; status codes → `oha-status.json`
  diagnostic artifact; raw stdout → `oha.json`.
- h2load (`h2load` driver): resolve `h2load`; probe `--version` (record
  `h2load-text/v1`; version token is the nghttp2 release). ClosedLoop and
  FiniteCount count → `-n/-c` (+`-t` threads=1, `-m` streams=1 defaults
  overridable? No: fixed minimal mapping `-n/-c/--h1` for cleartext,
  default h2c otherwise chosen by URL scheme); duration → `--duration`;
  OpenLoop rejected (capability). Parse anchored rows after the `finished
  in` marker; require `requests:` + `time for request:` rows; metrics:
  `throughput` (rps ← req/s mean), `error_rate` (ratio ←
  failed/total), `latency_min/mean` (ms ← time-for-request min/mean);
  failed/errored/timeout → `error_counts` + `offered_load_shortfall`
  warning when nonzero; raw stdout → `h2load.txt`.
- iperf3 (`iperf3` driver): resolve `iperf3`; probe `--version` (require
  ≥3.1 for `-J`; record `iperf3-json/v1`). Only duration-bound workloads
  (ClosedLoop/OpenLoop with `duration_ms`, no `requests`): `-c <host> -p
  <port> -t <secs> -J [--get-server-output]`; concurrency → `-P`;
  count-bound plans fail closed (`unsupported_load_model`). Host/port come
  from the target's `http_url` binding authority section (scheme/path
  ignored and recorded as such). Parse `-J`: top-level `error` member →
  `WorkloadFailed` with message; require `end.sum_sent/sum_received`;
  metrics: `bits_per_sec_sent/received` (bps, Rate),
  `bytes_sent/received` (bytes, Sum), `retransmits` (count, Sum; TCP only,
  missing for UDP); raw stdout → `iperf3.json`.
- Descriptors (`external_process: true`, `default: false`): `oha`
  (HttpVersion 1.1/2, LoadMode closed+open, CorrectedLatency when `-q`
  with `--latency-correction`, ExternalBinary), `h2load` (HttpVersion
  1.1/2, LoadMode closed, ExternalBinary), `iperf3` (LoadMode closed —
  duration-bound only, documented; ExternalBinary). Registered
  unconditionally in `production()`; `doctor` reports binary presence,
  probed version, and capability matrix without executing load.
- CLI: `production_workload_executor` handles the three names (binary
  resolution deferred to execution so `doctor` stays side-effect-free
  beyond probes); capability-gated plan rejection flows through existing
  resolution (`missing_driver`/`unsupported_*`).

## 6. Schema/storage/protocol effects

Additive only: three workload-driver names, three parser ids, three raw
artifact names, `offered_load_shortfall` warning label, bps unit in
plan-declared descriptors. Plan v1, ResolvedPlan v1, manifest v2,
TrialMetrics v1, envelope v1, exit codes unchanged. No core/runner changes
expected; if the `http_url`-authority reuse for iperf3 needs a helper, it
lands in drivers, not core.

## 7. Work packages

1. oha adapter + parser + descriptor + unit tests (live-verified here).
2. h2load adapter + anchored-text parser + descriptor + unit tests
   (live-verified here).
3. iperf3 adapter + JSON parser + descriptor + unit tests (live-verified
   here, incl. `-s -1` fixture server tests).
4. Catalog/CLI wiring + doctor matrix + capability-gated rejection tests.
5. Fixture corpus (per tool: valid, all-fail/partial, malformed, truncated,
   version-mismatch, nonzero/unreachable shapes) + live loopback e2e
   (oha+h2load always; iperf3 via fixture server) + docs
   (`docs/external-oracles.md`, `docs/cli.md`, README driver list).

## 8. Failure/cancellation/restart

Tool failure inside a trial → `WorkloadFailed` with stable category
(`binary_not_found` at resolution; `unsupported_version/unsupported_*` at
probe/argv build; `parse_failed` on bad machine output; tool-reported
errors via `error` member / error distributions into `error_counts`).
Cancellation/timeout propagate via substrate; partial stdout still staged
as raw artifacts when the runner retains the invocation. No retries, no
resume; rerun is a new trial.

## 9. Verification

- Focused: per-tool parser matrices (valid/partial/malformed/truncated/
  version/empty), argv-mapping tests (incl. rejection of count-bound
  iperf3, OpenLoop h2load, unknown flags never built), resolver
  missing-binary tests, doctor matrix test.
- Broad: `cargo fmt --check`; `check`/`clippy -D warnings` workspace
  all-targets (+all-features); full test suite default and all-features;
  MSRV 1.89 check+test; live loopback e2e producing verified bundles for
  all three drivers; `git diff --check`.
- Static guards: no new dependencies (substrate + serde_json only);
  `external_process: true` on all three descriptors; warning-label test.

## 10. Acceptance

1. All three drivers execute loopback trials with retained raw output,
   provenance, and parity/diagnostic metrics; bundles verify.
2. Missing binary → explicit capability error before startup (doctor+run).
3. Malformed/truncated/unsupported outputs → `parse_failed`, never partial
   silent metrics.
4. Unachieved load visible in evidence (counts/distributions + warning),
   trial not auto-invalidated.
5. No common-denominator coercion (iperf3 keeps bps names; h2load keeps
   missing percentiles missing).
6. Full local matrix green incl. MSRV.

## 11. Stop conditions

Stop and record if: a tool's machine output contradicts §2 ground truth on
the pinned minimum version (re-pin or reject the version explicitly); a
required mapping needs a plan-schema change (defer to a schema plan, do
not smuggle fields); iperf3 server orchestration proves necessary for a
loopback trial (defer server management, keep explicit-host design).

## 12. Closure evidence required

Requirement-to-evidence matrix over §§3/10, test outcomes (incl. live
tool versions), schema-compatibility note (additive), security/lifecycle
(argv-only, bounded, redacted, cancellation), docs, limitations (option
coverage §4; iperf3 explicit-host design), findings by severity.

## 13. Planning review checklist (§003 §11)

1. Long-term refs correct (Phase 6, ADR-0001/0004). 2. Hard deps closed
   (External M001 substrate). 3. No duplicated protocol behavior (tools
   own load semantics; Eggbench owns invocation/evidence). 4. Tool runtime
   is the timed interval; resolution/probe outside. 5. Trial is the unit;
   primary metrics explicit per tool (§5). 6. No status/verdict conflation
   (§3: visibility ≠ invalidity). 7. Lifecycle explicit (§8). 8. No
   filesystem/platform claims beyond trusted resolution + POSIX/Windows
   argv. 9. No security semantics touched. 10. Additive-only (§6).
   11. Closure bar is end-to-end live evidence (§9/§12).
