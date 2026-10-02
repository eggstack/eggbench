 # Security Qualification M003c — Status

Disposition: **CLOSED** — the upstream live-value defect is resolved by owner
revision `1338ce7b60f3793701091b4c329f80eb542f802d`, and the previously stopped
gate `m003c-13b` now passes with the harness semantics byte-identical.

Implementation commits: this record accompanies the M003c/M003d work on this
branch (see the repository history for the exact SHAs); the original collector
landed at `2742e0e`. The owner-pin advance to `1338ce7b...` lands with this
record (see the repository history).

## Implemented

- Generic feature-gated `prometheus-http` telemetry source: workspace-confined,
  digest-pinned mapping, exact metric/label selection, private/loopback
  endpoint authority bound to a declared service binding, bounded scalar
  exposition parsing, gauge aggregation, monotonic counter deltas with reset
  detection, required/optional field behavior, and bounded provenance.
- Bounded drain: the polling task is cancelled and awaited within
  `DrainContext.timeout`; cancellation, poll failure, and drain timeout are
  distinct reported reasons instead of a silent abort.
- Probe-point seam: `TelemetryPreflightTiming` (`BeforeStartup` by default,
  `AfterReadiness` for a managed subject's own endpoint) in
  `crates/eggbench-runner/src/telemetry.rs`, re-exported from the runner.
  Orchestration performs the deferred probe immediately after readiness and
  strictly before diagnostics, correctness checks, warmups, or measured trials.
- Required failures at the deferred point set the run `Failed` with
  `FailureCategory::TelemetryFailed` and record the per-source reason
  (`source`, failure kind, observed contract state) in `RunState` and
  `RunOutcome`; the CLI surfaces `telemetry_preflight_failed`. No measurement
  traffic is generated. `OrchestrationError::TelemetryPreflight(String)` carries
  the dynamic reason without a `'static` string leak.
- Preflight retries on the declared cadence until the required contract is
  observable or the run's telemetry bound expires: an owner publishes its
  series on its own refresh cadence, so a listening subject can lack its worker
  series for several seconds. The first successful sample carries the contract
  decision; a permanently absent contract reports its last observed failure.
- M003 workspace `qualification/synvoid/v2`: the owner's mapping and contract
  byte-for-byte, Eggbench corpora (separate correctness corpus), synthetic
  subject and controlled origin with fault injection, smoke and perf profiles,
  13 scenarios, and a README that states ownership and the repin procedure.
- Live harness `scripts/qualification/synvoid-m003/run-live-qualification.sh`
  plus `build-real-m003-workspace.py`, `controlled-origin-m003.py`,
  `assert-subject-telemetry.py`, `resolve-content-digests.py`.

## Verification

Local (host: 16 logical CPUs, shared with other workspaces):

- `cargo fmt --all -- --check`: clean.
- `cargo check --workspace --all-targets --locked`: clean.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`: clean.
- `cargo test -p eggbench-drivers --all-features --locked`: 106 lib tests green.
- `cargo test -p eggbench-runner --all-features --locked`: 122 tests green.
- `cargo test -p eggbench-cli --all-features --locked --test synvoid_m003`:
  12/12 green (463 s), covering telemetry evidence, optional absence, drain,
  bounded overhead, mapping/TYPE/counter/disappearance fail-closed, explicit
  baselines, correctness-only, performance-only, and the bounded external
  oracle and Eggsec plans.
- Full-workspace suite is green except `synvoid_m002b::perf_same_source_pair_never_fails`,
  which also fails on the pristine baseline commit `1b79f62` on this host while
  another workspace runs a parallel test suite (load average 40–60 on 16 CPUs).
  It is host contention, not a regression from this work.

Live local qualification against the real pinned binaries
(`KEEP_WORK=1`, work dir `/tmp/m003-live-qual.95LoW0`):

- `pass=29 stopped=1 notexec=0`.
- Owner pin `739e7ba6f02c5e3f83fe9ff5321b09213182b193`; owner materializer
  export plus `cargo xtask eggbench-qualification check --configtest` against
  the real minimal binary; provenance, contract identity, and mapping bytes
  asserted; loopback-only metrics endpoint with admin disabled.
- `m003c-12` real smoke profile: `pass` (6 scenarios).
- `m003c-13` owner contract evidence: 3 trials, 20 in-window samples each at a
  200 ms cadence, required gauges and counters observed, no poll errors, no
  dropped samples, pinned mapping identity recorded.
- `m003c-14` optional owner samples stay absent and warned; no fabricated zero.
- `m003c-15` the listener set is unchanged across a suite run.
- `m003c-16` renamed required metric fails closed with no measured trials
  published; `m003c-17` TYPE drift fails closed with `sample_type_mismatch`.

The pre-corrective run above is retained as the precise downstream proof of
the defect. It is superseded for the terminal claim by the corrected-owner
run below.

## Terminal live qualification (corrected owner pin 1338ce7b)

Harness, corpora, profile, and assertion script unchanged; only the owner pin
advanced from `739e7ba6f02c5e3f83fe9ff5321b09213182b193` to
`1338ce7b60f3793701091b4c329f80eb542f802d` (no mapping, contract, threshold, or
gate-semantics change — the pin is the only diff). Owner closeout with the
proof-bearing implementation SHAs:
`dbowm91/synvoid@...` → `architecture/eggbench_security_qualification_m003_telemetry_heartbeat_dispatch_corrective_closeout.md`
in the SynVoid tree.

Local run (`KEEP_WORK=1`, work dir `/tmp/m003-live-qual.DLzHSu`):

- `pass=30 stopped=0 notexec=0`, harness exit code 0.
- Owner pin `1338ce7b60f3793701091b4c329f80eb542f802d`; owner materializer
  export plus `cargo xtask eggbench-qualification check --configtest` against
  the real minimal binary (`--locked --release --no-default-features`,
  `sha256 251ac1d2e0c45be399570b3e2abcbb2925589bc01102eb2869585bcd7f8030f7`);
  provenance, contract identity, and mapping bytes asserted; loopback-only
  metrics endpoint with admin disabled.
- Contract identity unchanged: `synvoid.eggbench-telemetry.v2`; mapping digest
  unchanged: `622f6a13c4353cc7465cce39a57ed86fa0db2fe4114258e6f06226c1748d2d99`.
- `m003c-12` real smoke profile: `pass` (6 scenarios).
- `m003c-13` owner contract evidence: 3 trials, 20 in-window samples each at a
  200 ms cadence, required gauges and counters observed, no poll errors, no
  dropped samples, pinned mapping identity recorded
  (`dd7d58dd204a691ab0b83b82a83d49f3438b67e3ea6d0e14c7c1fba3832584b0`).
- **`m003c-13b` required subject series carry live owner values: PASS.**
  Recorded per-trial observations (all three trials identical):
  `subject_active_connections = 8.0`, `subject_event_loop_lag_ms = 1.0`,
  `subject_body_buffering_bytes_total = 0.0`,
  `subject_offload_rejections_total = 0.0`, each with 20 in-window samples,
  `poll_error_count = 0`, `dropped_sample_count = 0`, endpoint authority
  `127.0.0.1:43633`. The gauges the baseline run observed at zero for every
  trial now carry live worker-backed values; the two required counters are
  observed with valid counter semantics, and no counter value was
  manufactured by the consumer.
- `m003c-14` optional owner samples stay absent and warned; no fabricated zero.
- `m003c-15` the listener set is unchanged across a suite run.
- `m003c-16`/`m003c-17` fail-closed negatives unchanged.
- All `m003d-*` stages unchanged and green; teardown leaves no listener or
  subject process.

The owner-side root causes Eggbench asked for (recorded here so the closure
is traceable from the consumer side):

1. `src/supervisor/ipc.rs` dispatched every worker message except
   `Message::UnifiedServerWorkerHeartbeat` (fell through `_ => {}`), so
   `ProcessManager` kept default metrics.
2. The same message was absent from worker-ID classification (global limiter,
   no per-worker peer-PID path).
3. `src/worker/unified_server/startup_plan.rs` rebound `metrics` to a second
   fresh `WorkerMetrics` (Iteration 93 decomposition leftover), so the
   heartbeat serialized an instance no request had ever touched and every
   request-derived counter stayed zero.
4. `src/worker/unified_server/lifecycle.rs` seeded the lag accumulator one
   cadence ahead of the immediate first `interval.tick()`, clamping
   `event_loop_lag_ms` to a structural zero.

None of these touched the contract, mapping, names, units, cadence, WAF
semantics, or any Eggbench assertion. Hosted evidence (four-lane CI +
`live-m003-linux`) is recorded below once observed on the exact closing
revision.

## Unresolved finding (was blocking; now resolved downstream)

**Severity: high (upstream) — RESOLVED by owner revision
`1338ce7b60f3793701091b4c329f80eb542f802d`.** The record below is retained
verbatim as the baseline that the corrected run closes.

At `dbowm91/synvoid@739e7ba6f02c5e3f83fe9ff5321b09213182b193` the minimal
runtime exports the twelve `synvoid_subject_*` series of
`synvoid.eggbench-telemetry.v2` but never populates them: under sustained load
through the subject, every series — including `synvoid_subject_active_connections`
with eight in-flight requests and `synvoid_subject_worker_memory_bytes` for a
live worker — reads exactly `0`. The values do not change after more than one
owner refresh interval.

Root cause in the pinned tree:

- `src/worker/unified_server/lifecycle.rs:220` sends
  `Message::UnifiedServerWorkerHeartbeat`;
- `crates/synvoid-ipc/src/manager.rs:931`
  `ProcessManager::handle_unified_server_worker_heartbeat` stores the payload;
- `src/supervisor/ipc.rs` has **no dispatch arm** for
  `Message::UnifiedServerWorkerHeartbeat` (the match ends in `_ => {}` at the
  same place that handles `Message::UnifiedServerWorkerReady`), so the payload
  never reaches the store;
- `src/supervisor/telemetry_bridge.rs:713`
  `run_telemetry_bridge_loop` therefore aggregates default payloads and
  publishes zeros.

The owner's own plan `plans/eggbench_security_qualification_m003_telemetry_contract.md:109`
states that `handle_unified_server_worker_heartbeat` is the source of truth for
these payloads; at this pin that is not true of the shipped dispatch path. The
same gap is present on the current closeout head `0dc1f7fb21a5df60e72fc7f2cd60b7cb73bc9f35`.

The hosted live qualification runs `36897109527` and `36972886077`
(`live-m003-linux`) reproduced the gate independently of the local runs, both at
`pass=28 stopped=1 notexec=1`, with the single stop being `m003c-13b` reporting
that required gauge `subject_event_loop_lag_ms` was published at zero for every
trial. Contract-shape
evidence held there as well (`m003c-13` samples [20, 20, 20], `m003c-14` no
fabricated zero, `m003c-15` no listener leak, `m003c-16`/`m003c-17` fail closed
on a renamed and a TYPE-drifted owner metric). The pinned binary's hosted digest
was `sha256 07766ca02e129c9abe1c1b32e978e6de0cb710ed6e916b6cb5ae77bd615f8b1f`.

Disposition: the live harness reports this as a distinct stopped gate
(`m003c-13b`) rather than weakening the assertion to "present at zero". Eggbench
consumed the owner contract exactly as published, and every fail-closed path
behaves correctly. Closing this gate requires an owner revision whose supervisor
dispatches the unified-server heartbeat; it is not Eggbench work and must not be
worked around in the consumer.

## Remaining condition

Local terminal evidence is complete (`pass=30 stopped=0 notexec=0` on the exact
pin above). Hosted evidence is pending: a green hosted four-lane run plus the
`live-m003-linux` job on the exact source revision of this closing commit.
Once observed, their run IDs are appended here and this clause is struck.

## Relationship to M003d and M003

M003d's own profile, baseline, negative-demonstration, and live stages are
verified (see `plans/closure/security-qualification/003d-status.md`). Overall M003
closure inherits the same single upstream condition, because the M003 claim
"subject telemetry participates in measured evidence" cannot be asserted while
the pinned owner publishes zeros.