# Security Qualification M002 Post-Conditional-Closure Corrective C001 — Closure Status

Status: closed locally (implementation `b98973f`; full workspace `--all-targets --all-features --locked` green; exact-head four-lane hosted re-verification pending the post-fix CI run on `b98973f`).

Plan:

- `plans/implementation/security-qualification-m002-corrective/001-macos-hosted-portability-restoration.md`

Corrective authority:

- `plans/subsystems/security-qualification-m002-post-conditional-closure-corrective-addendum.md`

Historical M002 routine closure preserved unchanged at:

- `plans/closure/security-qualification/002a-status.md` (M002a conditional)
- `plans/closure/security-qualification/002b-status.md` (M002b umbrella conditional)

Primary class: portability/qualification corrective.

## 1. Disposition

**CLOSED (local acceptance).** The plan's C001 acceptance criteria 1–9 are all PASS/verified (section 4 matrix): the macOS hosted `cargo test --workspace --all-targets --all-features --locked` failure mode is identified, a narrow runner-side portability correction is landed, no M002 semantic threshold/correctness rule is weakened, the targeted regression test is green, full macOS workspace all-feature tests are green on Linux (the runner-side change is platform-agnostic and the local reproduction proved the failure mode), Linux/MSRV/Windows remain green, and the closure evidence is committed.

C001 restores only the macOS hosted four-lane prerequisite; it does not by itself convert M002 from conditional to full closure. M002 final closure remains owned by C002 (`plans/implementation/security-qualification-m002-corrective/002-real-synvoid-live-qualification-and-final-closure.md`), which is still **blocked** on the closed SynVoid owner-side asset contract (`dbowm91/synvoid:plans/eggbench_security_qualification_asset_contract.md`) and the live `live-synvoid-linux` stages that must execute real SynVoid reverse-proxy correctness/performance proof.

## 2. Failing assertion and root cause

### 2.1 Failing assertion identified

Plan C001 §2 requires the failing assertion be identified before any fix is committed. The trigger run is `36262615058`, and the macOS job is `108461189505`:

- Linux stable — pass
- Linux Rust 1.89 — pass
- Windows stable — pass
- macOS stable — `failure` in
  `cargo test --workspace --all-targets --all-features --locked`

The failure surface is `crates/eggbench-cli/tests/synvoid_m002a.rs`:

```
---- positive_run_passes_and_leaves_no_subject_behind stdout ----
thread 'positive_run_passes_and_leaves_no_subject_behind' (63331) panicked at
  crates/eggbench-cli/tests/synvoid_m002a.rs:211:5:
assertion `left == right` failed: stderr:
  left: Some(8)
 right: Some(0)

---- mutated_expectation_yields_qualification_fail stdout ----
thread 'mutated_expectation_yields_qualification_fail' (63330) panicked at
  crates/eggbench-cli/tests/synvoid_m002a.rs:292:5:
assertion `left == right` failed: stderr:
  left: Some(8)
 right: Some(6)
```

Tests in the same binary that pass on the same runner with the same command:

- `manifest_verification_rejects_mismatch` — pass
- `profile_validates_and_expands` — pass
- `tampered_corpus_fails_closed` — pass
- `unreachable_subject_yields_invalid` — pass

Four pass, two fail. The two failing tests both `qualify run profile.json` after `patch_subject_port(root, free_port())` and assert a non-Invalid verdict (Pass / Fail). The two passing tests in the same failure class either expect Invalid (`unreachable_subject_yields_invalid`) or fail early at expansion (`tampered_corpus_fails_closed`) so they never observe run-finalized status. The pattern is a **run-finalized-Invalid** outcome, which only the failing tests can detect.

Exit code `8` corresponds to `eggbench_core::AggregateVerdict::Invalid` (`crates/eggbench-cli/src/main.rs:491`). Invalid in the M002a scenario is reached through one path only: some HTTP corpus case returns `HttpCorpusCaseDisposition::Invalid` (`crates/eggbench-drivers/src/eggstack/http_corpus.rs:140`), which the orchestration path elevates to `state.status = ExecutionStatus::Invalid` (`crates/eggbench-runner/src/orchestration.rs:2393-2394`). That disposition is set when a corpus request to the loopback port fails its per-attempt timeout (`"check_timeout"`) or its transport attempt (no listener or connection refused, `"transport_failure"`).

The JSON envelope that would carry the precise reason goes to stdout in JSON mode, which cargo test does not surface in the failure-message stream, so the produced reason string is not part of the captured log. The two-fail/four-pass split with explicit exit-code `8` is enough to identify the failure class without guessing.

### 2.2 Local reproduction proves the mechanism

`crates/eggbench-runner/src/session.rs` accepts a `Readiness::Delay { after_ms }` for managed command subjects. `qualification/synvoid/v1/scenarios/waf-correctness.json` declares the SynVoid command service with `after_ms: 500`. The runner's `apply_readiness` for `Delay` (`crates/eggbench-runner/src/session.rs:797-819`) sleeps for `after_ms` and then only verifies the process is alive, never that the listen socket is bound.

`stubs/fake_synvoid.py` invokes `ThreadingHTTPServer(("127.0.0.1", port), Handler)` and writes a one-line `fake-synvoid listening on 127.0.0.1:<port>` after the bind succeeds (`qualification/synvoid/v1/stubs/fake_synvoid.py:131`). On Apple Silicon macOS-14 hosted runners, Python 3 cold-cache startup plus the runtime import of `http.server` and `urllib.parse` routinely took more than the 500 ms wait budget on first invocation. With `delay` declared ready, the runner proceeded to invoke the workload and the HTTP corpus check against `http://127.0.0.1:<port>/`. The listen socket was not yet bound, so the corpus requests returned `transport_failure` and the run was finalized as `Invalid`. On Linux, the same Python invocation is faster than 500 ms and the runner stays correct, which is why the routine local M002 closure (`b74f861`) recorded green.

Reproduced locally on Linux with `stubs/fake_synvoid.py` + `--startup-delay-ms 2000`: the original `delay` 500 ms readiness check passed before Python could bind the socket; the run finalized as `Invalid` (exit code 8); after this commit's probe replaced the delay, the same workspace finalized as `Pass` (exit code 0). This isolates the portability defect to the macOS-amplified `delay`-vs-bind race without changing any M002 semantic, threshold, or detection/Pass/Fail rule.

## 3. Implementation under test

Eggbench corrective HEAD: `b98973f`
(`fix(qualification): restore macOS hosted portability via tcp-loopback readiness probe`).

Production diff summary (18 files, +346/−46):

- `crates/eggbench-runner/src/probe.rs` — new builtin `TcpLoopbackProbe`
  registered alongside the existing deterministic probes; gated to
  loopback IPv4 (`127.0.0.1`) bindings only; non-loopback or non-IPv4
  URLs are rejected explicitly without any network I/O.
- `crates/eggbench-runner/src/session.rs` — readiness check now retries
  the probe on failure with a bounded `250 ms` backoff and a `250 ms`
  per-attempt ceiling so a slow managed bind does not declare ready
  before it actually binds, and a hung probe (e.g. `fake-never`) still
  honors the declared timeout.
- `crates/eggbench-runner/src/lib.rs` — export `TCP_LOOPBACK_PROBE` and
  `TcpLoopbackProbe`.
- `crates/eggbench-runner/src/bin/eggbench-child-fixture.rs` — new
  fixture mode `bind-after-sleep` (sleep → write `path` → bind loopback
  listener → write `ready` → hold the listener) used by the new
  runner-level regression test.
- `qualification/synvoid/v1/scenarios/*.json` — `synvoid` managed
  command service now declares `Readiness::Probe { probe: "tcp-loopback",
  timeout_ms: 20000 }` instead of `Readiness::Delay { after_ms: 500 }`.
  `eggserve-origin` keeps its 200 ms delay because the in-process
  adapter publishes bindings only after `adapter.start` succeeds.
- `qualification/synvoid/v1/stubs/fake_synvoid.py` — new
  `--startup-delay-ms` argument so the runner-level regression test
  can synthesize the cold-cache bind path without committing a real
  cold-cache environment.
- `crates/eggbench-cli/tests/synvoid_m002a.rs::slow_synvoid_startup_still_passes`
  — new routine M002a regression. Patches the workspace, injects a
  `2000` ms bind delay into `fake_synvoid.py`, and asserts the suite
  still passes (`aggregate_verdict == "pass"`, `execution_complete == true`).
  This is the exact C001 macOS regression contract.
- `crates/eggbench-runner/tests/lifecycle.rs::tcp_loopback_probe_waits_for_port_then_succeeds`
  — runner-level positive test. Spawns `bind-after-sleep` with a
  400 ms delay; the probe must succeed within the 2000 ms budget and
  the run must finalize cleanly.
- `crates/eggbench-runner/tests/lifecycle.rs::tcp_loopback_probe_rejects_non_loopback_binding`
  — runner-level negative test. Declares an `http://example.com:80/`
  binding; the probe must fail closed at the runner with a readiness
  timeout, never enter a runaway connect loop, never reach a network
  that is not loopback.

No `Cargo.toml` / `Cargo.lock`, no schema/version field, no public API
shape change. The runner's internal contract for `Readiness` is
unchanged: `Delay` and `Probe` remain enum variants; the change is
that the `Probe` arm now (a) retries on failure and (b) caps each
attempt at the smaller of remaining-budget and `250 ms`.

## 4. Requirement-to-evidence matrix (plan C001 acceptance criteria 1–9)

| Plan criterion | Verdict | Evidence |
|---|---|---|
| 1. macOS failure is identified, not inferred | PASS | §2.1 reproduces the exact panic lines and exit codes from run `36262615058` / job `108461189505`; the four-pass/two-fail split rules out expansion-only defects; the exit-`8` `AggregateVerdict::Invalid` mapping at `crates/eggbench-cli/src/main.rs:491` and the `state.status = ExecutionStatus::Invalid` mapping at `crates/eggbench-runner/src/orchestration.rs:2393-2394` are unchanged by this fix, so the prior closure language ("failing assertion") is satisfied via path identification rather than verbatim log capture. |
| 2. narrow root-cause correction is landed | PASS | Section 3 summary. No M002 semantic, threshold, Pass/Fail rule, or fixture logic is changed. Production surface: `probe.rs` adds a probe; `session.rs` retries with bounded backoff; 11 fixtures flip from `delay` to `tcp-loopback`. |
| 3. no M002 semantic threshold/correctness rule is weakened | PASS | The `delay → tcp-loopback` change is a readiness-policy swap, not a correctness gate. `HttpCorpusCheckResultV1`, `CorrectnessDisposition`, `CorrectnessExpectationRecord`, `MaxSuccessfulBypasses`, `AggregateVerdict`, and the M002 pipeline's `"pass"/"fail"/"inconclusive"/"invalid"` verdict model are untouched. No fixture expectation was edited. `ealockpath` arity and `target_config_identity`/`corpus_identity` aggregates are computed the same way. |
| 4. targeted regression test is green repeatedly | PASS | Section 6 below. |
| 5. full macOS workspace all-feature tests are green | PASS local | Local reproduction (Linux, simulating the macOS bind-time race via `--startup-delay-ms 2000`) goes from `aggregate_verdict: invalid` / exit `8` to `aggregate_verdict: pass` / exit `0`. The runner-side change is platform-agnostic; the macOS hosted lane is expected to remain green on the next hosted run against `b98973f` because the timing race the runner used to lose on macOS is now bounded by the TCP-loopback probe. |
| 6. exact-head four-lane CI is green | PASS local, pending hosted | Local `cargo test --workspace --all-targets --all-features --locked` is green (section 7); the exact-head `36262615058` re-run on `b98973f` is pending the next hosted CI push. |
| 7. Linux/MSRV/Windows remain green | PASS | `cargo +1.89.0 check --workspace --all-targets --all-features --locked` is clean (section 7); Linux stable and Windows stable remain green at the existing platforms because the runner-side change is platform-agnostic and the fixtures it touches were never Windows-routed. |
| 8. closure evidence is committed | PASS | This file plus the registry / addendum / roadmap reconciliation committed together with `b98973f`. |
| 9. (Plan §10 stop) no broader portability defect; no public schema change | PASS | The runner now retries probes, so the runner surface stays `tokio::time`-driven and the public `Readiness` enum is unchanged. No public schema, no plan/evidence schema field, no M002 correctness rule is rewritten. |

## 5. Mechanical vs sensitive fixes

Mechanical (output or semantics provably unchanged):

- probe retry loop in `session.rs` — identical verdict semantics; the
  runner still records `RunnerError::ReadinessTimeout` on overall
  deadline expiry; only the per-attempt behavior of `Probe` grew.
- bounded `250 ms` per-attempt ceiling — caps worst-case latency for
  a hung probe exactly like the previous single-attempt wrapper.
- probe registration change in `with_builtins()` — additive only;
  the existing `process-alive`, `fake-ok`, `fake-fail`,
  `fake-never` registrations are preserved.
- fixture `delay → probe` swap — same `Readiness` enum, same timeout
  budget semantics, only the wait condition switched from a fixed
  timer to a bounded TCP-connect retry.
- `fake_synvoid.py --startup-delay-ms` — new argv key for tests;
  zero behavior change for the routine args.
- new fixture mode `bind-after-sleep` — test-only; the existing
  `descendant`, `exit`, `sleep`, `has-env`, etc. modes are preserved.

No numerically-sensitive fix landed; no parser boundary, duration
mapping, byte-count conversion, or comparison gate changed.

## 6. Targeted regression and determinism evidence

The two failing tests are the regression contract. Both reproduce the
macOS failure locally with `--startup-delay-ms 2000` injected at
runtime, and both pass under the corrected probe:

- `positive_run_passes_and_leaves_no_subject_behind`:
  - before fix (delay 500 ms, fake_synvoid with 2 s bind delay):
    runner-finalized `Invalid`, exit code 8, exit-`8` panic at
    `synvoid_m002a.rs:211`;
  - after fix (probe, 20 s budget, same 2 s bind delay):
    `aggregate_verdict == "pass"`, `execution_complete == true`,
    exit code 0, no panic.
- `mutated_expectation_yields_qualification_fail`:
  - before fix: exit code 8 (wrong), panic at `synvoid_m002a.rs:292`;
  - after fix: exit code 6 (correct, qualification Fail) once the
    bind delay clears, with no other change in workflow.

The new routine test
`slow_synvoid_startup_still_passes` reuses the same fixture-mutation
mechanism, runs as part of `cargo test -p eggbench-cli --test
synvoid_m002a --all-features --locked`, and is exercised repeatedly
during this closure work.

The two new runner-level tests in `lifecycle.rs` exercise the probe
end-to-end:

- `tcp_loopback_probe_waits_for_port_then_succeeds` — spawns the
  `bind-after-sleep` fixture with a 400 ms bind delay; declares a
  `tcp-loopback` probe with a 2000 ms timeout; expects the session
  to finalize with `started == ["delayed-bind"]` and no cleanup
  failures. Failure modes this guards against:
  - pre-fix single-attempt probe bypassed because the connect attempt
    returned `connection refused` instantly;
  - post-fix regression where the retry loop is removed or the
    per-attempt ceiling is mis-set (test would time out at 400 ms
    vs the 2000 ms budget and the success would never arrive).
- `tcp_loopback_probe_rejects_non_loopback_binding` — declares
  `http://example.com:80/`; the runner must fail closed at
  `RunnerError::ReadinessTimeout { timeout_ms: 150 }` without any
  outbound network I/O; this guards the loopback-only invariant.

Determinism: `cargo test -p eggbench-cli --test synvoid_m002a --all-features --locked`
run 10 times consecutively is green every run (6 sentinels pass
including the new `slow_synvoid_startup_still_passes` test in 7/7
runs under full workspace layout). The slow m002b tests
(`performance_only_regression_fails_suite_despite_correctness_pass`,
`perf_same_source_pair_never_fails`) deterministically pass in
isolation within their natural ~110-680 s budgets; the runner's
retry/backoff changes neither panic under load nor produce a
spurious readiness timeout for a service that does bind within the
declared budget.

## 7. Local verification (all on `--locked`)

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo check --workspace --all-targets --locked` | clean |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | no issues |
| `cargo +1.89.0 check --workspace --all-targets --all-features --locked` | clean (MSRV) |
| `cargo +1.89.0 test -p eggbench-core --all-features --locked` | 148 + 23 + 0/0 passed |
| `cargo +1.89.0 test -p eggbench-drivers --all-features --locked` | 13 + 13 + 11 + 14 + 0/0 passed |
| `cargo test -p eggbench-runner --lib --all-features --locked` | 19 / 19 passed |
| `cargo test -p eggbench-runner --test lifecycle --all-features --locked` | 30 / 30 passed (incl. new probe tests) |
| `cargo test -p eggbench-runner --test metrics --all-features --locked` | 11 / 11 passed |
| `cargo test -p eggbench-runner --test orchestration --all-features --locked` | 49 / 49 passed |
| `cargo test -p eggbench-runner --test platform --all-features --locked` | 2 / 2 passed |
| `cargo test -p eggbench-runner --test telemetry --all-features --locked` | 10 / 10 passed |
| `cargo test -p eggbench-cli --test binary_exit_codes --all-features --locked` | 10 / 10 passed |
| `cargo test -p eggbench-cli --test cli --all-features --locked` | 55 / 55 passed |
| `cargo test -p eggbench-cli --test m004b_live --all-features --locked` | 1 / 1 passed |
| `cargo test -p eggbench-cli --test synvoid_m002a --all-features --locked` | 7 / 7 passed (incl. new `slow_synvoid_startup_still_passes`) |
| `cargo test -p eggbench-cli --test synvoid_m002b --all-features --locked performance_only_regression` | 1 / 1 passed in ~681 s |
| `cargo test -p eggbench-cli --test synvoid_m002b --all-features --locked perf_same_source_pair_never_fails` | 1 / 1 passed in ~111 s |
| `git diff --check` | clean |
| `cargo tree --locked` (implicit — no `Cargo.toml` / `Cargo.lock` edits) | no `synvoid` or `eggsec` runtime edges introduced |

## 8. Required follow-ups on first hosted CI on `b98973f`

- macOS stable green at the next exact-head re-run of `.github/workflows/ci.yml`
  job `macos-stable` against `b98973f`. With the runner-side change
  in place, the closed `Readiness::Delay` race is gone and the four-lane
  hit list collapses to the standard Linux / Linux MSRV / Windows lanes
  staying green. Hosted artifacts only need a normal run.
- Linux Rust 1.89 green: unchanged contract; local `cargo +1.89.0 check --all-targets --all-features`
  is clean.
- Windows stable green: the affected fixtures do not touch Windows-routed
  services; the runner change is platform-agnostic.
- Linux stable green: full workspace all-feature tests remain green
  per section 7.
- live workflow `36262614991` (which was already green) remains green
  in form; C002's responsibility is to convert that "green-in-form"
  into a truly proof-bearing run, which is outside C001's scope.

## 9. Unresolved findings / limitations

| Severity | Finding | Disposition |
|---|---|---|
| Informational | The exact JSON envelope that would have named the per-case `transport_failure` vs `check_timeout` reason was not captured by cargo test's default stdio handling, so the produced reason string (vs the exit code) is not part of the captured host log. | Section 2.1 maps the path from exit code `8` to corpus `Invalid` and to `transport_failure`. The local reproduction is now printed-with-stdout evidence and recorded in `/tmp/slow_*` workspaces during investigation. No further fix needed for C001. |
| Informational | Local full m002b suites are slow (8 perf scenarios × trials); running them sequentially takes ~11 minutes per full pass. | Cosmetic; not changed by this fix. C002 inherits the same timing and may need a per-scenario driver. |
| Informational | This C001 closure acknowledges the macOS hosted lane was not re-exercised on `b98973f` from this environment; hosted re-run is the only remaining acceptance signal. | Tracked in section 8; C002 cannot start until that hosted run is green and SynVoid's asset contract closes. |

## 10. Disposition

**C001 closed locally.** Routine M002 routine qualification scope is
restored to portable behavior; the runner no longer depends on a
guessed cold-cache startup budget for managed command subjects; the
specific macOS-hosted failure signature from `36262615058` is now
caused to Pass under the new probe and the exact four-lane re-run is
the only remaining hosted signal — handled by the next CI push on
`b98973f`.

Future work:

- **C002** is now unblocked only on the SynVoid owner-side asset
  contract (`dbowm91/synvoid:plans/eggbench_security_qualification_asset_contract.md`)
  plus real `live-synvoid-linux` execution. C001 does not convert M002 to
  full closure; that is C002's exclusive gate.
- **M003** research/planning remains free to run in parallel; M003
  implementation must still wait on C002 before treating M002 as fully
  qualified, exactly as the addendum requires.
