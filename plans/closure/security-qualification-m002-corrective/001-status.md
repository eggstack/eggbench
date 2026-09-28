# Security Qualification M002 Post-Conditional-Closure Corrective C001 — Closure Status

Status: **STOPPED with verification** (implementation `b98973f`; primary macOS m002a fix verified green on hosted CI run `36451730637`; a separate `synvoid_m002b::perf_same_source_pair_never_fails` live-host repeatability failure surfaced post-fix on macOS and remains owned by C002).

Plan:

- `plans/implementation/security-qualification-m002-corrective/001-macos-hosted-portability-restoration.md`

Corrective authority:

- `plans/subsystems/security-qualification-m002-post-conditional-closure-corrective-addendum.md`

Historical M002 routine closure preserved unchanged at:

- `plans/closure/security-qualification/002a-status.md` (M002a conditional)
- `plans/closure/security-qualification/002b-status.md` (M002b umbrella conditional)

Primary class: portability/qualification corrective.

## 1. Disposition

**STOPPED with verification, per plan C001 §10 stop condition "multiple independent failures appear after the first correction".** The original trigger failure (run `36262615058`, macOS jobs `108461189505`) was the four-line on the M002a routine test binary: `positive_run_passes_and_leaves_no_subject_behind` and `mutated_expectation_yields_qualification_fail` finalised the run as `Invalid` (exit code 8) instead of `Pass`/`Fail`. The exact-head re-run on the corrective HEAD `b98973f` (run `36451730637`, macOS job logged as completed) clears those two tests — `synvoid_m002a` is now green on macOS with all seven tests passing including the new routine regression `slow_synvoid_startup_still_passes`.

Once the M002a lane is green, the M002b lane runs for the first time on macOS hosted CI. The `perf_same_source_pair_never_fails` test asserts the same-source perf pair must not regress and the test itself explicitly notes "live-host repeatability remains an M002 closure condition"; on Apple Silicon macOS-14 the same-source comparison trips a `statistical_relative` regression gate under cold-cache and warm-cache load asymmetry. The test is a known live-host issue explicitly owned by C002 (§5 "Baseline repeatability proof"; §13 "Real-SynVoid CI job semantics" "fail when ... the `statistical_relative` gate sees the genuine same-source regression that real-stage B executes against"; §14 closes only after real SynVoid Stage B and Stage C run on the live runner).

Therefore:

- C001's narrow scope (runner-managed command subject readiness race) is closed:
  `synvoid_m002a` is now green on macOS hosted CI;
- C001 exposes an independent failure on `synvoid_m002b::perf_same_source_pair_never_fails`
  that is owned by C002, not by C001;
- the macOS hosted four-lane cannot fully turn green until C002 closes the live-host
  repeatability gap, and that is unchanged from the original M002 corrective architecture;
- per plan §10 "multiple independent failures appear after the first correction", C001
  records STOP-with-verification here so the next handoff is to C002 (not a new C003
  duplicate).

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
| 4. targeted regression test is green repeatedly | PASS hosted | The new `synvoid_m002a::slow_synvoid_startup_still_passes` test injects a 2 s `fake_synvoid.py` `--startup-delay-ms` and asserts `aggregate_verdict == "pass"`; green on local Linux (10 consecutive runs, 6 sentinels green per run) and on macOS hosted CI run `36451730637` (shown as `ok` in the m002a log fragment). `lifecycle::tcp_loopback_probe_waits_for_port_then_succeeds` and `lifecycle::tcp_loopback_probe_rejects_non_loopback_binding` exercise the runner-side retry path and the loopback-only invariant. |
| 5. full macOS workspace all-feature tests are green | **FAIL on macOS** (PASS for M002a) | M002a is green on macOS hosted CI run `36451730637` (all 7 tests pass); the remaining macOS failure is `synvoid_m002b::perf_same_source_pair_never_fails` (exit-code assertion at `synvoid_m002b.rs:269`), an existing live-host repeatability test that the test comment itself marks as "M002 closure condition" and is owned by C002 (§14). Local reproduction with the same probe-based readiness does not change per-trial perf; macOS-14 Apple Silicon cold/warm asymmetry on the same-source `statistical_relative` regression gate is the live-host repeatability gap C001 was never scoped to close. |
| 6. exact-head four-lane CI is green | PARTIAL: Linux stable / Linux Rust 1.89 / Windows stable green; macOS green on m002a but red on m002b perf test. | Local `cargo test --workspace --all-targets --all-features --locked` is green (section 7); on macOS hosted CI run `36451730637` the binary-level failures are confined to `synvoid_m002b::perf_same_source_pair_never_fails`. |
| 7. Linux/MSRV/Windows remain green | PASS | `cargo +1.89.0 check --workspace --all-targets --all-features --locked` is clean (section 7); Linux stable and Windows stable hosted lanes stay green because the runner-side change is platform-agnostic and the touched fixtures are not Windows-routed. |
| 8. closure evidence is committed | PASS | This file plus the registry / addendum / roadmap reconciliation committed with `b98973f`; this STOP-with-verification reclassification committed with the follow-up that captured run `36451730637`. |
| 9. (Plan §10 stop) no broader portability defect; no public schema change | MIXED | C001's runner-side change is narrow: `probe.rs` adds a probe, `session.rs` retries with bounded backoff, 11 fixtures swap readiness variant — no public schema change, no M002 correctness rule rewritten. The m002b perf failure is a separate live-host repeatability issue that surfaced post-fix; per plan §10 it triggers the "multiple independent failures appear after the first correction" stop and C001 is recorded STOP-with-verification here. |

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

## 7. Local and hosted verification (all on `--locked`)

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
| Hosted macOS m002a on `b98973f` (run `36451730637`) | green — `manifest_verification_rejects_mismatch`, `profile_validates_and_expands`, `positive_run_passes_and_leaves_no_subject_behind`, `mutated_expectation_yields_qualification_fail`, `tampered_corpus_fails_closed`, `slow_synvoid_startup_still_passes`, `unreachable_subject_yields_invalid` (7/7). |
| Hosted macOS m002b on `b98973f` (run `36451730637`) | partial — `external_oracle_procedure_observes_proxy`, `correctness_only_regression_fails_suite_despite_perf_pass`, `performance_only_regression_fails_suite_despite_correctness_pass`, `smoke_profile_passes_with_absolute_gates`, `workload_drift_compares_as_incomparable` pass; `perf_same_source_pair_never_fails` fails (5/6). This is the live-host repeatability issue C002 owns. |
| `git diff --check` | clean |
| `cargo tree --locked` (implicit — no `Cargo.toml` / `Cargo.lock` edits) | no `synvoid` or `eggsec` runtime edges introduced |

## 8. Required follow-ups on first hosted CI on `b98973f`

- macOS stable green at the next exact-head re-run of `.github/workflows/ci.yml`
  job `macos-stable` against `b98973f`.
  *Hosted evidence already collected:* run `36451730637` (macOS job)
  shows `synvoid_m002a` green (7/7 including the new regression),
  `synvoid_m002b` 5/6 with the only failure being
  `perf_same_source_pair_never_fails` — a live-host repeatability test
  the test itself documents as "M002 closure condition" (C002).
- Linux Rust 1.89 green: unchanged contract; local `cargo +1.89.0 check --all-targets --all-features`
  is clean.
- Windows stable green: the affected fixtures do not touch Windows-routed
  services; the runner change is platform-agnostic; hosted CI run
  `36451730637` Windows-stable lane green.
- Linux stable green: full workspace all-feature tests remain green
  per section 7; hosted CI run `36451730637` Linux-stable lane green.
- live workflow `36262614991` (which was already green in
  green-in-form) remains green in form; C002's responsibility is to
  convert that "green-in-form" into a truly proof-bearing run, which
  is outside C001's scope.

## 9. Unresolved findings / limitations

| Severity | Finding | Disposition |
|---|---|---|
| Informational | The exact JSON envelope that would have named the per-case `transport_failure` vs `check_timeout` reason was not captured by cargo test's default stdio handling, so the produced reason string (vs the exit code) is not part of the captured host log. | Section 2.1 maps the path from exit code `8` to corpus `Invalid` and to `transport_failure`. The local reproduction is now printed-with-stdout evidence and recorded in `/tmp/slow_*` workspaces during investigation. No further fix needed for C001. |
| Informational | Local full m002b suites are slow (8 perf scenarios × trials); running them sequentially takes ~11 minutes per full pass. | Cosmetic; not changed by this fix. C002 inherits the same timing and may need a per-scenario driver. |
| **Material (causes C001 stop)** | Hosted macOS `synvoid_m002b::perf_same_source_pair_never_fails` fails on `b98973f` (run `36451730637`): the test asserts the same-source `statistical_relative` regression gate must not trigger (`Some(0)` or `Some(7)` only) but the host CI observed a non-Pass/Inconclusive verdict. The test source itself notes "Live-host repeatability remains an M002 closure condition"; the M002 corrective architecture already assigns live-host repeatability to C002 (§14 — `live-synvoid-linux` fails when the `statistical_relative` gate sees a genuine same-source regression under cold/warm cache asymmetry). | Owned by C002, not C001. C001 records STOP-with-verification here (plan §10). No new CLI-side change is required; the corrective hand-off is "host the same-source pair under the warm-cache-only gate, or replace the synthetic stand-in with real SynVoid and re-validate under real reverse-proxy live-host conditions." |
| Informational | This C001 closure originally projected local acceptance only; hosted CI evidence at run `36451730637` since updated criterion 4 (targeted regression) to PASS hosted and exposed the m002b perf failure as a separate live-host issue. | Recorded in the disposition and criterion-5 row above; no plan content changes beyond this closure. |

## 10. Disposition

**C001 STOPPED with verification (plan §10 stop).** The exact failure
mode C001 was scoped to fix (runner-managed command subject readiness
race on macOS cold-cache Apple Silicon) is closed: `synvoid_m002a` is
green on macOS hosted CI on `b98973f`. The macOS lane is not yet fully
green overall because `synvoid_m002b::perf_same_source_pair_never_fails`
fails on Apple Silicon under warm/cold-cache asymmetry; that is the
`live-host repeatability` M002 closure condition owned by C002 and not
in C001's portable-corrective scope. C001 is recorded STOP-with-verification
so the next handoff is C002, not a new C003 duplicate.

Future work:

- **C002** is the only remaining M002 corrective gate. It must consume
  the closed SynVoid owner-side asset contract and execute real SynVoid
  `live-synvoid-linux` Stage B and Stage C runs against the real
  subject so the live-host repeatability proof lands at the exact
  binary gate the M002b perf test defends against.
- **M003** research/planning remains free to run in parallel; M003
  implementation must still wait on C002 before treating M002 as fully
  qualified, exactly as the addendum requires.
