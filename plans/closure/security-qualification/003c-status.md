# Security Qualification M003c — Status

Disposition: **conditionally closed — Eggbench-owned scope complete and
verified; one terminal gate is blocked by a named upstream defect.**

Implementation commits: this record accompanies the M003c/M003d work on this
branch (see the repository history for the exact SHAs); the original collector
landed at `2742e0e`.

The single unmet gate is *required subject series carry live owner values*.
Everything Eggbench owns is closed: the collector, the probe-point seam, the
fail-closed negatives, the routine tests, and the live harness that executes
against the real pinned SynVoid.

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

## Unresolved finding (blocks terminal closure)

**Severity: high (upstream, blocks a terminal gate).**

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

Disposition: the live harness reports this as a distinct stopped gate
(`m003c-13b`) rather than weakening the assertion to "present at zero". Eggbench
consumed the owner contract exactly as published, and every fail-closed path
behaves correctly. Closing this gate requires an owner revision whose supervisor
dispatches the unified-server heartbeat; it is not Eggbench work and must not be
worked around in the consumer.

## Remaining condition

Terminal M003c closure requires: an owner revision that populates the required
series, the same live harness executed against that revision (hosted evidence
preferred), and a green hosted four-lane run plus the `live-m003-linux` job on
the exact source revision.

## Relationship to M003d and M003

M003d's own profile, baseline, negative-demonstration, and live stages are
verified (see `plans/closure/security-qualification/003d-status.md`). Overall M003
closure inherits the same single upstream condition, because the M003 claim
"subject telemetry participates in measured evidence" cannot be asserted while
the pinned owner publishes zeros.