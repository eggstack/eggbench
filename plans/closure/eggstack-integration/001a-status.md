# Eggstack Integration M001a — EggServe Controlled Origin and Eggfetch Native HTTP Workload — Closure

Disposition: **closed**
Closed: 2026-09-24
Implementation commit: `8426e08` (feat(eggstack): close Eggstack M001a controlled origin and native HTTP workload), on top of planning baseline `852bf2dab6a266cc5043cf0817b429a8073d6339`.
Hosted qualification: not run in this pass; local verification only (see §5). Four-lane hosted CI remains required before release qualification claims. Windows/macOS loopback coverage is code-present (no OS-specific process ownership on this path) but locally unexercised.

## 1. Requirement-to-evidence matrix

| Plan § / requirement | Evidence | Outcome |
|---|---|---|
| §5 `eggstack-http` feature: `eggfetch-core/standard-http1` + `eggserve-primitives` + `eggserve-server` + hdrhistogram; CLI forwards; defaults stay empty; feature isolation in minimal graph | `crates/eggbench-drivers/Cargo.toml`, `crates/eggbench-cli/Cargo.toml`; `cargo tree --locked -p eggbench-drivers` shows no eggfetch/eggserve/hdrhistogram without the feature | Pass |
| §6 exact lockfile-resolved sibling versions in evidence, never hardcoded | `crates/eggbench-drivers/build.rs` parses `Cargo.lock` into `EGGBENCH_{EGGFETCH_CORE,EGGSERVE_SERVER,EGGSERVE_PRIMITIVES}_VERSION`; descriptors + method evidence report them; lock: eggfetch-core 0.2.0, eggserve-server 0.2.1, eggserve-primitives 0.2.0 | Pass |
| §7 generic named-service seam (`ManagedServiceAdapter`, `ManagedServiceHandle`, `ServiceAdapterRegistry`); core free of sibling crates | `crates/eggbench-runner/src/service.rs`; `eggbench-core` has no new dependencies | Pass |
| §8 mixed ownership (`RunningManagedService::Process/Adapter`), unchanged dependency order/reverse teardown, primary-failure preservation, every started adapter attempted, shutdown failure as cleanup evidence, externals unowned | `session.rs`; lifecycle tests (mixed order/reverse, adapter cleanup evidence) + orchestration tests (staging-failure cleanup, shutdown-failure vs workload-failure) | Pass |
| §9 adapter-owned readiness at `start` return; EggServe handle readiness; plan-level `Probe` on in-process services rejected; optional post-ready delay | `session.rs::apply_adapter_readiness`; test `adapter_probe_readiness_is_rejected_and_service_is_torn_down` | Pass |
| §10 protocol-neutral `RuntimeBindings` (`http_url` + `bound_addr` + `bound_port` for origin); bounded `Name` keys; immutable after startup; workloads receive post-readiness; retained through teardown | `service.rs::RuntimeBindings` (Name-validated insert); `session.runtime_bindings()`; lifecycle test asserts visibility + retention | Pass |
| §11 `InvocationContext.bindings` read-only snapshot; same snapshot every invocation; fakes ignore it | `orchestration.rs::InvocationRequest`; `execute_invocation` threads `session.runtime_bindings()` into warmups + trials | Pass |
| §12 versioned `lifecycle/runtime-topology.json` (schema v1, identities, ownership, provenance, bindings); staged post-teardown from retained state; preflight capacity updated | `bundle.rs::stage_runtime_topology`, `RUNTIME_TOPOLOGY_SCHEMA_VERSION = 1`; e2e asserts origin entry (`adapter`/`eggserve-origin`/loopback `http_url`) | Pass |
| §13 origin `eggserve-origin` (`path` default `/bench`, `body_bytes` default 1024 ≤ 1 MiB, `status` default 200 in 200–599); deterministic fixed-length body; no filesystem/timestamp/randomness | `eggstack/origin.rs`; route-discipline + determinism + config-rejection tests | Pass |
| §13 bind policy loopback-only `127.0.0.1:0`; address from server handle; `http_url` after readiness | Fixed bind constant; non-loopback refused; `bound_addr`/`bound_port` tests assert `127.0.0.1` + nonzero port | Pass |
| §14 direct `eggserve-server` supervision only (builder/bind/start_with_service/local_addr/shutdown/wait); cancellation-aware start; graceful shutdown within plan grace | `origin.rs` uses public API only; cancellation test; shutdown-timeout diagnostic | Pass |
| §15 `eggfetch-http` supports ClosedLoop±requests/duration, FiniteCount, TimeBounded-ClosedLoop; rejects OpenLoop/TimeBounded-OpenLoop before startup | `run_plan()`; resolution fails open-loop via missing capability (drivers test); defensive executor rejection | Pass |
| §16 one client per executor/run; warmups warm the pool; method provenance recorded | `EggfetchWorkload::new` + `eggfetch_workload()`; `eggfetch-method.json` (`client_reuse_policy`) per invocation | Pass |
| §17 GET vs `http_url` binding; full body consumption; no retries/redirects; transport/timeout/status errors categorized; no 2xx-masquerade | `issue_one`/`issue_timed`; `standard-http1` omits logical retry/redirect; categorized tests (transport, http_5xx, missing binding) | Pass |
| §18 closed-loop ≤N in-flight, next-on-finish, cancellation stops issuance, joined workers, exact counts, deadline stop | `run_closed_loop`/`worker` with atomic claim counter; cancellation timing test; exact-count assertion (20/20) | Pass |
| §19 dispatch-to-full-body latency; bounded HDR histogram (1–60M µs, 3 sigfig, saturating, no coordinated-omission claim) | `build_output`/`fold_outcomes`; hdrhistogram; no per-request vector retained | Pass |
| §20 same-trial `latency.hdr` (hdrhistogram-v2), bounded, referenced via `RawHistogramInput`, digest in manifest | `LATENCY_HISTOGRAM_ARTIFACT` + `serialize_histogram` (V2Serializer); e2e asserts per-trial hdr + histogram reference path | Pass |
| §21 raw metrics (throughput, latency_min/mean/p50/p90/p95/p99/p999, error_rate, timeout_rate, bytes_received) normalize through M001 schema | `push_volume_metrics`/`push_latency_metrics`; e2e asserts observed `throughput`/`latency_p99`/`error_rate` in `metrics.json` | Pass |
| §22 stable categories (transport/timeout/http_3xx/4xx/5xx/body_read/cancelled); no error-string identities | Category constants; error_counts tests | Pass |
| §23 saturation/method metadata per invocation (concurrency, attempted/completed, elapsed, max in-flight, reuse policy, exact version, H1-only) | `method_evidence` JSON; e2e + unit assertions | Pass |
| §24 catalog registers both descriptors with feature, empty without; `doctor` shows exact versions + capabilities | `catalog.rs::production()`; `DriverSummary` gains adapter/upstream/capability fields; CLI tests both modes | Pass |
| §25 generic production `run` (resolve → adapters → executor → prepare → session → execute_run → presentation); no adapter switch in `main.rs`; feature-off code 3 preserved | `commands/run.rs::run`; `production_service_adapters()` + `production_workload_executor()` in registry seam | Pass |
| §26 deterministic loopback fixture (label subject, managed named origin, closed loop, ≥1 warmup, 3 measured, throughput/latency_p99/error_rate) | `crates/eggbench-core/tests/fixtures/eggstack-loopback.json` (1 warmup, 3 measured, 50 requests × 4 concurrency) | Pass |
| §27 end-to-end acceptance (ready origin, ephemeral URL, warmup + measured, no errors, per-trial hdr, metrics.json, topology, shutdown, verified bundle, no remains) | `eggstack_loopback_end_to_end` + binary `production_loopback_run_exits_zero_in_both_modes` | Pass |
| §28 negatives (missing adapter, duplicate registration, probe rejection, open-loop rejection, missing binding, startup failure, transport failure, cancellation, staging-failure cleanup, shutdown-vs-workload failure) | lifecycle/orchestration/drivers/CLI tests, one per bullet | Pass |
| §29 methodological guard (many- vs few-requests runs expose exactly 3 trial observations; sample counts differ) | `trial_observation_count_is_independent_of_request_volume` (10- vs 50-request runs, 3 identical observation sets each) | Pass |
| Acceptance 1–12 | All hold; notably generic seam, mixed teardown, bindings-as-evidence, direct-API origin, no duplicate HTTP, closed-loop-only truthfulness, per-trial HDR, M001 normalization, feature-selected CLI path, feature-off fail-closed, verified bundle + shutdown, MSRV green | Pass |

No stop condition (§34) triggered: direct APIs supplied typed bind/readiness/shutdown; lean profile required no retry/redirect semantics; no plan-schema change; bindings stayed protocol-neutral (`String` maps, `Name`-validated keys); histogram is bounded; feature isolation verified.

## 2. Tests/guards run and outcomes

- `cargo fmt --all -- --check` — pass
- `cargo check --workspace --all-targets --all-features --locked` — pass
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass (pedantic-clean)
- `EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-targets --all-features --locked` — pass: **240 passed (15 suites)** — 15 new drivers eggstack tests, 3 new runner lifecycle adapter tests, 2 new orchestration cleanup tests, 4 new CLI e2e/guard tests (loopback, volume guard, startup failure, no-binding failure), 1 new binary loopback test, feature-gated production/doctor/run expectations
- `cargo check --workspace --all-targets --locked` (feature-off) — pass; `cargo test` feature-off — pass: **220 passed (15 suites)**
- `cargo +1.89.0 check --workspace --all-targets --all-features --locked` — pass
- `cargo +1.89.0 test --workspace --all-targets --all-features --locked` — pass: 15 suites, 0 failures
- `cargo tree --locked` (on and off) — pass; minimal graph free of eggfetch/eggserve/hdrhistogram
- `git diff --check` — pass
- Live loopback: origin binds `127.0.0.1:<ephemeral>`, 50-request trials complete error-free with `throughput > 1 rps`, bundle verifies.

## 3. Schema/migration/compatibility evidence

- New additive schemas only: `RuntimeTopology` v1, `lifecycle/runtime-topology.json` artifact; plan v1, `ResolvedPlan` v1, manifest v2, `TrialMetrics` v1 untouched.
- `InvocationContext` gains additive `bindings` field (fakes ignore it; `FakeWorkload` untouched).
- `DriverSummary` gains additive `adapter_version`/`upstream_name`/`upstream_version`/`capabilities` display fields; envelope schema version stays 1; exit codes 0–5 semantics unchanged.
- `eggserve-server` resolves to 0.2.1 within the plan's `0.2` requirement (§2 notes 0.2.1 current); exact resolved versions recorded per §6.

## 4. Security and lifecycle evidence

- Origin binds loopback only by construction (fixed `127.0.0.1:0`, non-loopback refused); no public bind, no TLS/proxy surface in M001a.
- Bindings carry non-secret connection facts only; secret values never enter topology evidence (`Sensitivity::Redacted`).
- No Eggbench-owned HTTP parsing/dialing: Hyper stays inside sibling crates behind their public APIs.
- Cancellation races every request path; worker tasks joined; shutdown within plan grace; teardown failures are cleanup diagnostics, never primary-cause rewrites.
- `doctor` performs no startup; `run` fails before managed startup whenever resolution, preflight, or adapter construction fails.

## 5. Documentation/operational evidence

- Added `docs/eggstack-http.md` (ownership, feature boundary, origin config/bindings, workload semantics/metrics/histogram, example run).
- Updated `architecture/drivers.md` (catalog with feature), `architecture/runner.md` (adapter seam + topology), `docs/trial-orchestration.md` (bindings snapshot), `docs/evidence-bundle.md` (runtime-topology artifact), `docs/cli.md` (registry + feature behavior), `docs/driver-capabilities.md` (descriptors), `README.md` (real-run example with feature requirement).

## 6. Known limitations

- H1 only; no H2/H3, TLS, proxy, POST/upload, scripting, open-loop (explicit non-goals).
- Hosted four-lane CI not run in this pass; Windows/macOS loopback unexercised locally.
- `eggserve-server` 0.2.1 vs planning-pass audit 0.2.0: within the `0.2` requirement; exact version recorded in evidence.
- Per-request timeout fixed at 30 s; histogram saturates above 60 s (documented, saturating).
- Gregg telemetry explicitly deferred to M001b.

## 7. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| None | No unresolved M001a correctness, security, lifecycle, or portability finding | No corrective work required |
| Note | Hosted CI qualification outstanding (Linux stable, Linux 1.89, macOS, Windows) | Tracked as follow-up; local matrix is green incl. MSRV 1.89 tests |

## 8. Disposition

**Closed.** Generic named-service seam, mixed lifecycle ownership, runtime bindings as workload input and versioned evidence, loopback EggServe origin on the direct public API, native Eggfetch closed-loop workload with bounded per-trial histograms through the M001 metric schema, feature-gated production CLI path with fail-closed minimal build, deterministic fixture with verified end-to-end bundle, and documentation are landed and regression-tested. M001 remains open pending Gregg telemetry M001b.
