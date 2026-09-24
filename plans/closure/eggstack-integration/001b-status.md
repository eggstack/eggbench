# Eggstack Integration M001b — Gregg Host Telemetry — Closure

Disposition: **closed**
Closed: 2026-09-24
Implementation commit: `a0ff206` (feat(eggstack): close Eggstack M001b Gregg host telemetry), on top of planning baseline `46ebaa6baa13ec1b74512295a1886a54d2911ace`.
Hosted qualification: not run in this pass; local verification only (see §5). Four-lane hosted CI remains required before release qualification claims. Windows/macOS coverage is code-present (loopback HTTP only, no OS process ownership) but locally unexercised.

## 1. Requirement-to-evidence matrix

| Plan § / requirement | Evidence | Outcome |
|---|---|---|
| §5 `gregg` feature (`gregg-protocol` + shared `eggfetch-core/standard-http1`); CLI forwards; defaults telemetry-free | `eggbench-drivers/Cargo.toml`, `eggbench-cli/Cargo.toml`; feature-off tree carries no gregg/eggfetch/eggserve deps | Pass |
| §6 lockfile-exact `gregg-protocol` + `eggfetch-core` in evidence | `build.rs` emits `EGGBENCH_GREGG_PROTOCOL_VERSION`; descriptor, metric producer versions, per-trial provenance report it; lock: gregg-protocol 1.0.14, eggfetch-core 0.2.0 | Pass |
| §7 plan schema v1 unchanged; single external `gregg` named service; missing/multiple endpoint errors | `build_gregg_collector` (exactly-one rule, required-missing error); CLI tests | Pass |
| §8 loopback-only endpoints (`127/8`, `::1`, loopback-resolved localhost); reject public/LAN/HTTPS/credentials/query/fragment | `gregg/endpoint.rs`; unit matrix (accept/reject/not-loopback/invalid) | Pass |
| §9 generic seam (`TelemetryCollector`, `TelemetryRegistry`, preflight/start/stop/drain) | `crates/eggbench-runner/src/telemetry.rs`; duplicate-source rejection test | Pass |
| §10 protocol-neutral `TelemetryOutput` (shared artifacts, raw metrics, warnings); same normalization; no collector-written TrialMetrics | `TelemetryOutput`; `stage_trial` merges and `normalize_trial_metrics` validates | Pass |
| §11 phase order (start → timer → workload → elapsed → stop → stage workload → stage telemetry → normalize → metrics.json); stop after failure/cancellation; stop failure as cleanup; structural staging failure through cleanup tail | `execute_run` trial loop; `stop_trial_telemetry` on both arms; timing test proves 600 ms injected latency outside measured elapsed | Pass |
| §12 preflight (syntax/policy, healthz, status, protocol validation, identity/cadence); required blocks startup, optional disables + warns + missing | `preflight_telemetry` + `GreggCollector::preflight_inner`; required/optional regression tests at runner and CLI levels | Pass |
| §13 cadence `clamp(daemon, 250ms, 5s)`; immediate + cadenced + final snapshots; timestamp dedup | `MIN/MAX_POLL_INTERVAL`, start/final snapshots, `deduplicate`; dedup + poll-count tests | Pass |
| §14 owned polling task; bounded join then abort; final snapshot in budget; captured samples on failure; no leaks | `ActiveWindow` + `stop.cancel()` + timeout-join; second-stop/no-window behavior; cancellation test | Pass |
| §15 per-trial `gregg.ndjson` (exact validated wire bytes, else canonical reserialization); bounded; dropped/truncated counts; truncation marks metrics NaN | `serialize_series`, `MAX_SAMPLES_PER_TRIAL`/`MAX_NDJSON_BYTES`, `dropped`; e2e asserts byte-exact NDJSON lines | Pass |
| §16 unambiguous `host_*` names; explicit aggregation; custom under vocabulary v1; plan-declared units echoed | Eight documented metrics; unknown names fail closed; aggregation/unit tests | Pass |
| §17 mean CPU/frequency/disk/network; max memory; absent stays missing; measured zero observed; nonfinite rejected | `extract`/`aggregate_metrics`/`quantity_f64`; absent-frequency, zero-rate, nonfinite tests | Pass |
| §18 v2 capability/optional semantics respected; no recreated validation | `gregg-protocol` `validate()` on every payload; optional `None` → missing | Pass |
| §19 stable error categories (`endpoint_invalid/not_loopback`, `health/status_unavailable`, `schema_unsupported`, `payload_invalid`, `polling_failed/timeout`, `collector_cancelled`, `artifact_bound_exceeded`); bounded detail | `TelemetryError::new` (512-char cap); category assertions across tests | Pass |
| §20 duplicate observations stay invalid across producers | Core `normalize_one_request` duplicate arm + `cross_producer_duplicates_stay_invalid` | Pass |
| §21 provenance (`gregg` producer, protocol version, `v2.*` source field, `gregg.ndjson` ref); no daemon version fabricated | Per-observation producer override; e2e provenance assertions | Pass |
| §22 catalog telemetry descriptor; `doctor` config validation + capability report without live probing; `run` constructs collectors; feature-off explicit resolution failure | `gregg_telemetry_descriptor` (8 `TelemetryField` capabilities); `doctor` `telemetry_config` check; `production_telemetry_registry`; CLI tests all modes | Pass |
| §23 test-only HTTP fixture (ready/warming/invalid/malformed/delayed/absent-optionals/repeated timestamps/changing values); production uses wire types | `tests/gregg.rs` fixture server; 13 acceptance tests | Pass |
| §24 required/optional regression matrix (unavailable, malformed, unsupported schema, non-loopback, no credentials, absent-optional, zero-rate, dedup, cancel-stop) | One test per bullet | Pass |
| §25 timing regressions (start/stop delays outside `measurement_elapsed_ns`; failure preserves elapsed; trial-count invariant) | `telemetry_window_stays_outside_measured_elapsed` + M001a volume guard (unchanged) | Pass |
| §26 cleanup composition (workload+stop failure, cancel+stop timeout, staging failure, drain failure; precedence per M002) | Runner telemetry tests: stop-after-success primary, workload+stop keeps workload, staging-error tail, drain precedence | Pass |
| §28 interference accounting (interval, samples, bytes, outside-timer elapsed) + bounded poll-count test | `gregg-provenance.json`; `poll_count_stays_bounded` (≤5 polls per ~1.2 s trial) | Pass |
| Acceptance 1–14 | All hold; notably generic lifecycle, timing exclusion, failure/cancel cleanup, v2 parsing, loopback-only, required/optional truthfulness, bounded cadence, retained NDJSON, unambiguous aggregation, normalized host metrics with provenance, missing-not-zero, no task leaks, feature-off exclusion, MSRV green | Pass |

No stop condition (§32) triggered: `gregg-protocol` v2 deserializes/validates current payloads publicly; no schema change; telemetry composes outside M002 timing; `TrialMetrics` v1 extended only additively (per-observation producer override); loopback suffices; polling meets cadence without push.

## 2. Tests/guards run and outcomes

- `cargo fmt --all -- --check` — pass
- `cargo check --workspace --all-targets --locked` — pass
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass (pedantic-clean)
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — pass (feature-off clean)
- `EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-targets --all-features --locked` — pass: **273 passed (17 suites)** — 10 new runner telemetry tests, 13 new drivers Gregg tests, 3 new core producer-override tests, 5 new CLI Gregg tests (e2e, required/optional, feature-off, doctor)
- Feature-off `cargo test --workspace --all-targets --locked` — pass: **234 passed (17 suites)**
- `cargo +1.89.0 check --workspace --all-targets --all-features --locked` — pass
- `cargo +1.89.0 test --workspace --all-targets --all-features --locked` — pass: 273 passed (17 suites), 0 failures
- `cargo tree --locked` (on: gregg-protocol 1.0.14 + shared eggfetch-core 0.2.0; off: absent) — pass
- `git diff --check` — pass
- Live loopback: fixture-backed e2e completes with `host_cpu_percent = 25.0`, `host_memory_used_bytes = 8e9`, byte-exact NDJSON, verified bundle.

## 3. Schema/migration/compatibility evidence

- Additive only: `RawMetricObservation.producer/producer_version` (serde-defaulted, old payloads parse); `TrialExecutionFailure::TelemetryFailed`; `FailureCategory::TelemetryFailed`; `telemetry` plan-timeout key (optional, defaults to `measurement`); `trials/NNN/telemetry/` artifacts. Plan v1, `ResolvedPlan` v1, manifest v2, `TrialMetrics` v1, envelope v1 untouched; exit codes unchanged.
- `execute_run` gains a trailing `&mut TelemetryRegistry` parameter (internal seam; production passes the catalog-built registry, qualification an explicit/empty one).
- `doctor` resolves against the full production catalog (all categories) instead of the workload-only inventory; qualification seam `run_with_registry` behavior unchanged.

## 4. Security and lifecycle evidence

- Loopback-only endpoints by validation (public/LAN/HTTPS/credentials/query rejected); no credentials exist to record; provenance carries host/port without secrets (`Redacted` role).
- No Eggbench HTTP parsing/dialing outside sibling public APIs; protocol validation owns acceptance.
- Polling tasks always joined/aborted within trial/drain budgets; windows pair start/stop; drain closes orphans.
- Required failures block before startup; optional gaps warn and stay missing; no fabricated zeroes anywhere.

## 5. Documentation/operational evidence

- Added `docs/gregg-telemetry.md` (feature, ownership, plan shape, endpoint policy, lifecycle, cadence, metric table, evidence).
- Updated `architecture/drivers.md` (catalog with `gregg`), `architecture/runner.md` (collector seam), `docs/trial-orchestration.md` (window placement), `docs/metrics.md` (producer override + duplicates), `docs/evidence-bundle.md` (telemetry artifacts), `docs/cli.md` (registry + doctor behavior), `README.md` (feature + doc links).

## 6. Known limitations

- H1 polling only; no push/stream protocol, no TLS/auth (daemon is private-network by design), no private-LAN endpoints yet.
- No `greggd` management/installation/startup; daemon binary version unexposed by design.
- Hosted four-lane CI not run in this pass; Windows/macOS loopback unexercised locally.
- Descriptor `TelementryField` capabilities fix the eight M001b names; vocabulary v2 promotion explicitly deferred.
- Per-request timeout fixed at 10 s; trial sample cap 256 / 256 KiB with dropped-count accounting.

## 7. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| None | No unresolved M001b correctness, security, lifecycle, or portability finding | No corrective work required |
| Note | Hosted CI qualification outstanding (Linux stable, Linux 1.89, macOS, Windows) | Tracked as follow-up; local matrix is green incl. MSRV 1.89 tests |

## 8. Disposition

**Closed.** Generic telemetry lifecycle, Gregg v2 loopback collection with bounded polling and exact-byte retention, unambiguous host aggregation through the shared normalization contract, required/optional truthfulness, production wiring with fail-closed minimal builds, verified end-to-end evidence, and documentation are landed and regression-tested. Closing M001b together with M001a closes Eggstack Integration M001.
