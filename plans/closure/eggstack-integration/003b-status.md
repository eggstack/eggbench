# Eggstack Integration M003b — Eggprobe Pre/Post Diagnostics — Closure

Disposition: **closed**

This record is also the umbrella Eggstack M003 closure evidence: M003a
(EggReplay semantic replay workload, implementation `adc3c15`, closure
`plans/closure/eggstack-integration/003a-status.md`, planning closure
`b4e823a`) plus M003b (Eggprobe pre/post diagnostics) together close M003.

Closed: 2026-09-25

Implementation commit: `7712995` (`feat(eggstack): implement M003b Eggprobe
pre/post diagnostics and M003 closure proof`), on top of planning baseline
`46e7aa0` with M003a closure `b4e823a` as predecessor.

Hosted qualification: four-lane CI run `36140143375` on the exact
implementation candidate `7712995` — Linux stable, Linux Rust 1.89 MSRV,
macOS stable, and Windows stable all green (see §3). No live `eggprobe` or
`eggreplay` binary is installed on hosted runners or was present on the
implementation host, so closure distinguishes hosted
absence-safe/parser/fixture-contract evidence from live-binary evidence
(none claimed), exactly as M003a did.

## 1. Requirement-to-evidence matrix

Acceptance criteria are
`plans/implementation/eggstack-integration/003b-eggprobe-pre-post-diagnostics-and-m003-closure.md`
§30.

| Plan requirement | Evidence | Outcome |
|---|---|---|
| Schema v5 diagnostics additive, v1-v4 supported | `EXPERIMENT_PLAN_SCHEMA_VERSION_5`; v5 round-trip; v1-v4 compat; explicit-field rejection incl. empty (`plan.rs` presence-sensitive `Option`, `deserialize_present_optional`) | Pass |
| Diagnostic is a distinct category, not Telemetry/Workload | `DriverCategory::Diagnostic`, `Capability::DiagnosticProbe{probe}`, `DiagnosticExecutor`/`DiagnosticRegistry` seam separate from `WorkloadExecutor`/`TelemetryCollector`; no Eggprobe type crosses into runner/core | Pass |
| Eggprobe consumed externally via JSON, not as Rust dependency | `crates/eggbench-drivers/src/external/eggprobe.rs`; `eggprobe run -` with stdin plan; `cargo tree` contains no `eggprobe-*`/`eggreplay-*` crate | Pass |
| Qualified schema 0.3 enforced | `parse_probe_report` requires `schema_version == "0.3"`, `tool.name == "eggprobe"`, direct route, bounded producer/execution/target/status/probe/findings/warnings fields | Pass |
| Same-SemVer schema-0.4 binary fails the handshake | Parser rejects `"0.4"` explicitly; `handshake_eggprobe` fails closed with `diagnostic_contract_unsupported`; regression test covers 0.4-with-0.1.1-version | Pass |
| Pre diagnostics run after readiness, before workload | `execute_run_with_diagnostics` ordering; `DiagnosticsPre` phase events; lifecycle test asserts pre-before-warmup | Pass |
| Post diagnostics run after drain, before teardown | `DiagnosticsPost` after drain while services alive; lifecycle test asserts post-after-trials and post-before-teardown | Pass |
| No diagnostic time enters measured duration | No `MeasurementSignal` in `run_diagnostic_phase`; `TrialMetrics` test proves only workload metrics normalize; docs label timings evidence-only | Pass |
| Exit 1 is valid negative evidence | `classify_probe_exit`: exit 1 + valid report is `Negative`, never `WorkloadFailed`; exit-1 fixture test | Pass |
| Required pre negative prevents workload, still tears down | `Invalid` + `DiagnosticFailed`, zero workload invocations, mandatory teardown tail; lifecycle test | Pass |
| Optional negatives warn/continue | Evidence + warnings recorded, `Completed` preserved; lifecycle test | Pass |
| Required post negative invalidates Completed | `Invalid` + `DiagnosticFailed`; lifecycle test | Pass |
| Diagnostic never masks earlier failure | Prior `Failed`/`Cancelled` status preserved, diagnostic is secondary evidence; lifecycle test | Pass |
| Direct route explicit | Generated plans pin `route: {kind: direct}`; parser rejects non-direct; no route input exists in M003b | Pass |
| Only DNS/TCP/TLS/HTTP claimed | Descriptor advertises exactly the four families; `eggprobe_supported_family_names` + `EGGPROBE_UNSUPPORTED_FAMILIES` (icmp/udp/trace/pmtu); catalog/doctor tests | Pass |
| Raw evidence bounded and versioned | `diagnostics/<pre\|post>/<id>.json` + `diagnostics.json` v1; 4 MiB/256 KiB/64 KiB caps; 128 KiB index cap; `validate_contract` | Pass |
| Config/provenance in comparability | `DiagnosticsEvidenceIdentity` + `compare_diagnostics` in `compare_driver`; mismatch matrix test (timeout/probes/required/version/digest/schema) | Pass |
| M003a semantic replay intact | Full workspace suites green incl. M003a semantic/comparison tests and M002 path suite (13 passed); goldens updated by suffix only | Pass |
| Combined EggServe+EggReplay+Eggprobe path tested | Orchestration-level combined lifecycle proof (pre+both+post around workload with evidence index); parser/handshake unit contracts; live-binary combination deferred for lack of qualified binaries (known limitation) | Pass |
| No Eggprobe/EggReplay Rust production dependency | `cargo tree --locked` clean; feature-isolation CI steps green | Pass |
| Clippy all-feature -D warnings | Default and all-features Clippy clean locally; CI linux-stable/macos/windows Clippy steps green | Pass |
| Rust 1.89 green | Local `cargo +1.89.0` check/tests green; CI linux-msrv lane green | Pass |
| Four hosted lanes pass | Run `36140143375`: linux-stable, linux-msrv, macos-stable, windows-stable all `success` | Pass |
| M003 closure record + reconciliation committed | This file plus roadmap/registry updates (M003 closed/hosted-qualified; M004 plan-authorable) | Pass |

## 2. Sibling seam audit — implementation time

- Eggprobe qualified release of record remains `v0.1.1` at `53ea53d`
  with machine schema 0.3 (planning audit). No local Eggprobe checkout
  exists in `~/projects`; no `eggprobe` binary on PATH (or hosted
  runners). Package version alone is not trusted: the handshake enforces
  the machine schema and rejects 0.4 explicitly.
- EggReplay planning HEAD `29ce133f`, envelope schema 1,
  `RegressionReport` schema 2 — unchanged from the M003a audit; M003b
  does not modify EggReplay semantics.
- The generated schema-0.3 plan shape (`target`/`route`/`probes`/
  `execution`/`assertions`) is the documented M003b lowering contract.
  It was validated by parser/handshake unit contracts and absence-safe
  CLI paths, not against a live binary. If the qualified `v0.1.1`
  binary rejects the exact shape, the handshake fails closed with
  `diagnostic_contract_unsupported` rather than silently adapting, and
  a corrective pass will reconcile the shape against the real contract.
- Substrate extension: `ExternalCommandSpec.stdin_bytes` (64 KiB cap,
  one-shot write then EOF) was added for `eggprobe run -` plan
  delivery; all nine pre-existing construction sites set
  `stdin_bytes: None` with unchanged behavior. No temporary plan file.

## 3. Verification record

Local (implementation host, `EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process`):

- `cargo fmt --all -- --check` — pass.
- `cargo check --workspace --all-targets --locked` — pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — pass.
- `cargo test --workspace --all-targets --locked` — pass: **396 tests across 20 suites**.
- `cargo check --workspace --all-targets --all-features --locked` — pass.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass.
- `cargo test --workspace --all-targets --all-features --locked` — pass: **454 tests across 20 suites**.
- `cargo +1.89.0 check --workspace --all-targets --locked` — pass.
- `cargo +1.89.0 check --workspace --all-targets --all-features --locked` — pass.
- `cargo +1.89.0 test -p eggbench-core --all-features --locked` — pass (121 + 23).
- `cargo +1.89.0 test -p eggbench-runner --all-features --locked` — pass (incl. 43 orchestration).
- `cargo +1.89.0 test -p eggbench-drivers --all-features --locked` — pass.
- `cargo test -p eggbench-drivers --all-features --test eggstack_path` — pass (13; M002 no-regression).
- `cargo tree --locked` — pass; no `eggprobe-*` or `eggreplay-*` crate in any graph.
- `git diff --check` — pass.
- `eggbench validate examples/eggstack-diagnostics.json` — pass.
- `eggbench doctor examples/eggstack-diagnostics.json --json` — truthful `missing-binary` handshake with full family/timing summary (no binary installed).

Hosted (run `36140143375` on `7712995`):

- linux-stable — success (fmt, check, feature isolation, all-feature Clippy, all-feature tests, no-default CLI tests).
- linux-msrv (1.89.0) — success (check default/all-features, core + drivers all-feature tests).
- macos-stable — success (check, Clippy, all-feature tests, process-group cleanup, symlink confinement).
- windows-stable — success (check, Clippy, core contracts, runner platform tests, portable Eggress route tests).

Live-binary evidence: none claimed. Hosted lanes execute
absence-safe/parser/fixture-contract tests. A fake-`eggprobe` shell
script was deliberately not used: parser/handshake/exit-code/target
contracts are proven by JSON-fixture unit tests plus CLI absence-safe
tests without inventing upstream process semantics, and process
spawning/cancellation/timeout is proven by the shared substrate suite.

## 4. Schema, compatibility, and provenance

- Plan schema v5 is additive; v1-v4 remain readable. An explicit
  `diagnostics` field (even `[]`) on v1-v4 fails with
  `unsupported_option`; `diagnostics + network_path` fails with
  `diagnostic_path_incompatible` ahead of the generic schema gate
  (mirroring the M003a replay-gate precedence).
- `ResolvedPlan` schema stays v3; `ResolvedPlan.diagnostics` carries the
  normalized requests and `Diagnostic` required capabilities
  (`DiagnosticProbe` per family) resolve against the pinned `eggprobe`
  executable path (`MissingExecutablePath` before startup when absent).
- `diagnostics.json` schema v1 records driver/adapter/tool
  version/SHA, machine schema 0.3, and per-execution
  request/disposition/artifact identity. Comparison loads and verifies
  it; diagnostic runs without the index (or vice versa) fail closed.
- Comparison goldens changed by exactly one suffix per file
  (`diagnostics match (absent)` in `driver_detail`); no verdict changed.
- `ResolvedPlan` snapshot fixture gained explicit `"diagnostics": []`.

## 5. Security and lifecycle evidence

- No shell, no arbitrary caller plan, no routed credential input, Direct
  route only, bounded stdin/stdout/stderr, bounded report fields,
  targets from declared runtime bindings only, no automatic public
  target, report-safe route summaries, bounded cancellation, mandatory
  teardown. Sentinel tests prove secret-like binding values cannot enter
  generated plans and route secrets cannot enter evidence (no route
  input exists).
- Target URL must be loopback; TLS requires an explicit `https_url`
  binding and is never inferred from the port (required fails closed,
  optional records `unavailable`); DNS for literal-IP hostnames is
  deterministically `not_applicable`.
- Two seam defects found during M003b implementation were fixed with
  regression coverage: all-skipped diagnostic runs failed index
  validation (accepted with empty provenance), and cancelled runs with
  zero diagnostic records hit a staging error (early return, no index).
- Diagnostics run outside measured intervals by construction; the
  timing-policy test proves `TrialMetrics` carry workload metrics only.

## 6. Known limitations and follow-up boundaries

- No live `eggprobe`/`eggreplay` binary was available locally or on
  hosted runners. Live matching/mismatch qualification (real EggServe
  origin with passing/failing replay plus pre/post diagnostics, zero-
  gate failure path, multi-trial unit proof against real tools) remains
  to be recorded when qualified binaries are installed. Parser,
  handshake, exit-code, lowering, lifecycle, evidence, and
  comparability contracts are hosted and green.
- Recording, `serve` as managed origin, append/re-record, fixture
  mutation, Python bindings, interception, WebSocket/SSE controls,
  timeline scheduler, `network_path` composition, Eggprobe compare
  statistics, schema-0.4 native families (ICMP/UDP/trace/PMTU), routed
  diagnostics, telemetry polling, per-trial probes, retries, and
  security scanning are explicitly non-goals and remain future work.
- M003b unblocks M003 closure. Eggstack M004/Eggsec becomes
  plan-authorable; no M004 implementation plan exists yet.

## 7. Disposition

**Closed.** M003b delivers schema-v5 diagnostics, the `Diagnostic`
runner/driver category with the external `eggprobe` adapter, trusted
provenance plus the schema-0.3 handshake, deterministic Direct plan
lowering, exit-code semantics with exit 1 as negative evidence,
pre/post lifecycle execution with required/optional policy and
cleanup precedence, bounded versioned evidence, comparison-critical
diagnostic identity, CLI validate/doctor/run/inspect coverage,
feature isolation, documentation, and local plus four-lane hosted
qualification. Together with M003a, Eggstack M003 is closed and
hosted-qualified.
