# Security Qualification M002b — Umbrella M002 Closure

Disposition: conditionally closed (M002a conditionally closed alongside;
both share the same upstream/live conditions)

Implementation commit: `b74f861` (routine scope; profiles, scenarios, tests, harness,
guide). M002a closure: `plans/closure/security-qualification/002a-status.md`.

## What landed

`qualification/synvoid/v1/` suite (all M001 schema contracts, no new production
code beyond the M002a `command` compatibility):

- `smoke.profile.json` (`synvoid-smoke-v1`): correctness + small/large native c1
  proxy + direct-origin controls, absolute error gates only — 5/5 Pass locally.
- `perf.profile.json` (`synvoid-perf-v1`): correctness (absolute) + native
  c1/c8/c32 small/large proxy + controls, every performance scenario bound to an
  explicit baseline bundle with frozen v1 guardrails (throughput −15%,
  p95 +20%, error_rate absolute 0) and a 7-trial / 200-2000-request sample policy.
- 12 scenario plans: `perf-{small,large}-c{1,8,32}`, `smoke-{small,large}`,
  `control-{small,large}`, `oracle-{oha,h2load}-c8`.
- `stubs/fake_synvoid.py` (+ plan-invisible per-port delay sidecar for the
  performance-only proof), `stubs/fake_origin.py` (fixed-port deterministic
  origin for live SynVoid wiring), `baselines/` workflow doc, profile README
  (deviations D4-D6), `docs/synvoid-qualification.md` guide.
- Harness Stage C (smoke, 8-baseline materialization, same-source pair,
  delay proof, oracle procedure, Gregg probe) in
  `scripts/qualification/synvoid-m002/run-live-qualification.sh`.

## Requirement-to-evidence matrix

| # | Plan acceptance criterion | Evidence | Result |
|---|---|---|---|
| 1 | M002a correctness profile closed and green | 002a conditionally closed; routine scope green (6/6 tests, harness Stage A 5/5) | Conditional (shared) |
| 2 | SynVoid-owned upstream asset contract authoritative | No Eggbench translation of Detect/Pass; harness gates Stage B on the contract file; synthetic owner labels explicit | Pass (boundary honored) |
| 3 | Native Eggfetch proxy scenarios execute reproducibly | 6 perf + 2 smoke scenarios run via qualify; same-source pairs repeat without Fail | Pass (routine) |
| 4 | At least one independent external-oracle proxy scenario live-qualified | `oha` procedure green (run/run/compare, driver-owned trial observations); `h2load` procedure green after absolute-gate scoping | Pass (routine procedure; hosted condition) |
| 5 | Small and large response paths covered | 1024 B `/bench/small` + 65536 B `/bench/large` across native, control, oracle scenarios | Pass |
| 6 | Direct-origin controls retained as distinct subjects | Control scenarios target the adapter binding; smoke test asserts `workload.target == origin`; controls Pass in every suite run | Pass |
| 7 | Explicit baseline bundles drive relative comparison | Two-stage workflow (doc + harness + `perf_same_source_pair_never_fails` asserts frozen `baseline_bundle_identity` + receipt digests on all 8) | Pass |
| 8 | Throughput/p95/error gates frozen before interpretation | v1 guardrails in checked-in plans; §8 sample-policy revision recorded (5-trial policy failed same-source; 7-trial policy: zero Fail over repeated pairs); thresholds never widened | Pass |
| 9 | Correctness-only regression yields suite Fail despite perf Pass | `correctness_only_regression_fails_suite_despite_perf_pass` (exit 6; waf Fail, suite Fail) | Pass |
| 10 | Performance-only regression yields suite Fail despite correctness Pass | `performance_only_regression_fails_suite_despite_correctness_pass` (100 ms harness delay via plan-invisible sidecar; 6/6 perf Fail, waf Pass, controls Pass; exit 6) | Pass |
| 11 | Security/config drift fails closed | `workload_drift_compares_as_incomparable` (concurrency change → completed/Invalid → suite Invalid, exit 8); corpus tamper → Invalid (M002a) | Pass |
| 12 | No SynVoid-specific load generator or Prometheus scraper added | Reused eggfetch/oha/h2load/Gregg seams only; Prometheus/churn/mixed-load explicitly deferred | Pass |
| 13 | Host telemetry labeled truthfully | No collector declared in profiles (D5); Gregg role/limits documented; harness probes and reports NOT-EXECUTED | Pass |
| 14 | Four-lane Eggbench CI green | Local matrix green incl. MSRV; hosted runs pending push | CONDITION (hosted) |
| 15 | Linux live SynVoid qualification green | Harness Stage C green-in-form locally; real-SynVoid stages NOT-EXECUTED (contract open) | CONDITION |
| 16 | Umbrella M002 closure/reconciliation committed | This record + roadmap/registry reconciliation | Pass |

## Sample-policy (§8) evidence

- 5 measured trials / ≤400 requests: same-source `perf-small-c8` Fail
  (p95 degradation 0.67, CI entirely above the 0.20 threshold on loopback noise).
- Revised 7 measured trials (1 warmup) / 200-800-2000 requests, `min_trials` 5:
  two consecutive same-source pairs yield Pass/Inconclusive, zero Fail
  (worst CIs straddle thresholds honestly; e.g. large-c8 throughput CI
  [−0.37, 1.60] → Inconclusive, never converted to Pass).
- Thresholds frozen. Live-host same-build repeatability is a remaining
  condition; if it violates, M003 revises the policy with evidence.

## Oracle findings

- `oha` (local 1.x): full procedure green; trial observations producer-labeled.
- `h2load`: p95 resolution floor reports 0 ms on sub-ms loopback, which no
  relative gate can divide by (`nonpositive_relative_value` → Invalid).
  Oracle plans therefore carry absolute error gates plus a gateless
  diagnostic throughput metric; native scenarios keep the relative gates.
  This scoping is recorded, not hidden.

## Deferred to M003 (explicit, not forgotten)

Mixed malicious/benign load, request-body attack campaigns, explicit
connection churn, SynVoid Prometheus/event-loop/queue ingestion,
challenge/stall/tarpit semantics, HTTP/2/TLS variants, network-path/fault
injection, per-scenario qualify drivers (D4), generic binding
interpolation (D1), Gregg-gated resource policy (D5).

## Tests/guards and verification

- `synvoid_m002b` (6 tests): smoke Pass (5/5 + control-subject proof);
  perf same-source pair (all completed, Pass/Inconclusive, frozen baselines);
  correctness-only Fail (exit 6); performance-only Fail (exit 6, 6/6 perf
  Fail + waf Pass); drift Invalid (exit 8); oha+h2load procedure (driver-owned
  observations) — all pass, MSRV included.
- Full matrix green: fmt, workspace check, clippy all-features −D warnings,
  workspace test all-targets all-features, MSRV check + core/runner/cli tests,
  `git diff --check`.
- Live harness (local): 12 PASS (Stages A+C incl. both oracles), 2
  NOT-EXECUTED (Gregg daemon, upstream contract), 0 STOPPED, exit 0.

## Roadmap reconciliation

- M002 (M002a + M002b) is conditionally closed on the shared upstream/live
  conditions above.
- M003 becomes ready for research/planning with the deferred items carried
  explicitly; no M002 scope is silently dropped.
- External Oracles M003 netem remains a separate later boundary, unaffected.
