# Eggstack Integration M004a — Eggsec Strict WAF Correctness Adapter — Closure

Disposition: **closed**

Closed: 2026-09-25

Implementation commit: `273e5b1` (`feat(eggstack): implement M004a Eggsec
strict WAF correctness adapter`), on top of planning baseline `aeed8f7`
with refinements `d1edf7b` and `f2910b6`, and predecessor live-tool C002
closure `4f71467` (implementation `98f16e6`).

Hosted qualification: four-lane CI plus the new Linux-only live Eggsec
qualification job run on the exact implementation candidate `273e5b1`
(see §3). The live job builds the exact pinned Eggsec source revision and
executes the committed qualification harness; the four lanes qualify
Eggbench's own platform/process behavior.

M004a closure unblocks M004b
(`plans/implementation/eggstack-integration/004b-security-correctness-gate-and-m004-closure.md`).

## 1. Requirement-to-evidence matrix

Acceptance criteria are
`plans/implementation/eggstack-integration/004a-eggsec-strict-waf-correctness-adapter.md`
§28.

| Plan requirement | Evidence | Outcome |
|---|---|---|
| Schema v6 security checks is additive | `EXPERIMENT_PLAN_SCHEMA_VERSION_6`; v6 round-trip (JSON+TOML); v1-v5 remain readable; explicit `security_checks` (even `[]`) on v1-v5 fails `unsupported_option`; diagnostics retained on v5+v6, SemanticReplay on v4-v6 | Pass |
| Correctness is a distinct category from Workload/Diagnostic/Telemetry | `DriverCategory::Correctness`, `Capability::SecurityCheck{family: waf_bypass}`, sibling-neutral `CorrectnessExecutor`/`CorrectnessRegistry` seam; no Eggsec type crosses into runner/core | Pass |
| Eggsec consumed as external strict-scope process | `crates/eggbench-drivers/src/external/eggsec.rs`; `eggsec --version` probe, guarded `preflight waf --target … --profile guarded`, bounded `waf … --bypass --test-type … --concurrency … --timeout …`; no shell; exact argv pinned by contract tests | Pass |
| No Eggsec Rust production dependency added | `cargo tree --locked` contains no `eggsec-*` crate in any graph; feature-isolation CI steps green | Pass |
| Only local/private runtime targets accepted | `confine_target_url`: `127.0.0.0/8`, `::1`, RFC1918, IPv6 ULA, `localhost`/`*.localhost`; public IPs, link-local, and other hostnames fail closed before spawn; unit matrix plus live public-target test | Pass |
| Generated scope is exact and digest-addressed | `generate_scope_manifest`: `require_explicit_scope = true`, exact host pattern plus host-covering `/32`/`/128` CIDR only, no credentials/wildcards; deterministic canonical TOML; SHA-256 is the security configuration identity; 0o600 file; removed after each check plus best-effort dir removal | Pass |
| Strict preflight succeeds before managed startup | Binary resolution + version probe run in CLI `run` preflight before startup (`security_driver_missing`/`security_contract_unsupported` fail before startup); the guarded scope preflight runs in the correctness phase with the generated scope (see recorded deviation D3) | Pass |
| No manual override flag is used | Exact argv contains no `--yes`/`--allow-*`/credentials/proxy flags; `waf_argv_tail` contract test forbids `--header-bypass`, `--smuggling`, `--evasion`, `--proxy`, `--auth`, `--bearer`; preflight rejects non-empty confirmation classes and honored overrides | Pass |
| Only bounded direct WAF bypass checks supported | One `SecurityCheckRequest` produces one `waf --bypass --test-type <family>` invocation; families `sqli/xss/ssrf/cmd/traversal` only (no `all`); `source` must be `eggsec-waf`; unsupported operations listed in doctor | Pass |
| At least one case required for a valid observation | Parser rejects zero-case output (Invalid, never vacuous Pass); `SecurityCheckResultV1` requires `evaluated_cases >= 1`; live safe fixture yields 22 cases | Pass |
| `bypass_successful` is the only initial semantic correctness signal | Parser consumes only the explicit boolean plus structural consistency; severity/title/payload/HTTP-status/WAF-name never gate correctness; threshold `successful <= max_successful_bypasses` applied to Eggsec-owned counts | Pass |
| Threshold Pass/Fail declared before execution | `max_successful_bypasses` validated in planu (bounded by `MAX_SECURITY_CASES`); stored disposition recomputed and rejected on mismatch at staging and (M004b) comparison | Pass |
| Security Fail does not fail execution or suppress performance trials | `run_correctness_phase`: valid Fail stages evidence with phase `Completed` and continues; lifecycle test asserts Completed + 2/2 trials + no primary failure; live safe run proves the path (Fail path proven by fake + live adapter observation) | Pass |
| Operational/security-tool failure remains distinguishable | `FailureCategory::CorrectnessFailed`; operational errors set `Invalid` (or `Cancelled`/`TimedOut` first-class) and skip workload with mandatory cleanup tail; lifecycle test asserts Invalid + teardown + no warmups | Pass |
| Raw payload bytes are not retained in bundles | Stdout parsed in memory; only `SanitizedSecurityCase` (technique/severity/status/bool/payload-SHA) staged; stderr never staged; bundle grep for payload markers empty; sentinel test proves unrelated secrets never enter security artifacts | Pass |
| Security checks run outside measured intervals | Lifecycle `startup → correctness → warmups → trials → drain → teardown`; no `MeasurementSignal` in `run_correctness_phase`; `TrialMetrics` carry workload observations only (no-contamination lifecycle test); phases proof in live bundle | Pass |
| Security configuration participates in comparability | `SecurityEvidenceIdentity` + `compare_security` in `compare_driver`; mismatch matrix (allowance/test-type/concurrency/tool-version/tool-digest/scope-digest) plus absent-vs-present tests; result values never participate | Pass |
| Paired/network_path combinations fail closed | `paired_security_not_supported` and `security_path_incompatible` ahead of generic gates (mirroring M003a/M003b precedence); schema matrix tests | Pass |
| Real pinned Eggsec safe/fail cases qualified | Harness 8/8 PASS plus `eggsec_live` 5/5: safe fixture 22 findings/0 bypasses/Pass; permissive fixture 22 findings/9 bypasses/Fail; both valid observations; provenance below | Pass |
| Rust 1.89 remains green | `cargo +1.89.0` check default/all-features plus core (152), runner (118), drivers (147) all-feature tests green | Pass |
| Normal four-lane CI is green | Hosted run on `273e5b1` (see §3) | Pass |
| Live Eggsec qualification is green | Hosted `live-eggsec-linux` job on `273e5b1` (see §3) | Pass |
| Closure record is committed | This file plus plan-status/roadmap/registry reconciliation | Pass |

## 2. Sibling seam audit — implementation time

- Audited Eggsec default-branch HEAD `0509ac668adfd78e9899cd3428a807d0b3c9f27b`
  (workspace 0.1.0, Rust 1.89; no immutable release selected for this
  machine contract), matching the M004 planning audit exactly.
- Live qualification builds that revision fresh from git with the plan's
  exact command (`cargo build --locked --release -p eggsec-cli
  --no-default-features`) and records bounded provenance:
  - source SHA `0509ac668adfd78e9899cd3428a807d0b3c9f27b`;
  - Cargo.lock SHA-256
    `66010e5380d39d1ad0bcf2f1013c080f70146df1987c13848d0d0bb47224e1bb`;
  - binary SHA-256
    `ef7675ab5b164457ee465a919b20322311ecf12fcca3069d1f43b14dfca545a7`
    (16,663,896 bytes; build-environment-specific, recorded as observed);
  - `eggsec --version` → `eggsec 0.1.0`;
  - rustc 1.98.1 / cargo 1.98.1 (host build toolchain; MSRV 1.89
    qualified separately for Eggbench-owned code).
- Selected seam confirmed live: `eggsec waf <url> --json` emits
  `ScanResults` (`target`, `timestamp`, `duration_ms`, optional
  `waf_detection`, `findings[]`, `summary`) with per-finding
  `bypass_successful`; `summary.total_findings` matches `findings.len()`
  and `bypass_success_rate` matches counted booleans. `waf_detection`
  carries `request_error: null` on valid observations.
- Recorded deviations from the plan (§6 handoff authority: invariants
  preserved, smallest coherent adjustment, recorded not hidden):
  - D1 — preflight operation identity is `waf-detect` (command dispatch
    maps the `waf` subcommand), not `waf`. The adapter accepts both
    spellings and requires an `allow` outcome with an allowed decision.
  - D2 — Eggsec emits line-delimited tracing JSON records to stdout
    ahead of the pretty-printed result document (including an
    enforcement-audit record). The adapter strips single-line log
    records (payload starts at the first bare-`{` line) instead of
    trusting raw stdout as JSON.
  - D3 — the guarded strict-scope preflight runs in the correctness
    phase after readiness, not before managed startup: the generated
    scope needs the startup-established runtime target binding, which
    does not exist pre-startup. CLI `run` preflight before startup
    covers binary resolution and the version probe. The safety
    invariant holds — Eggsec sends no security traffic before a strict
    allow for the generated scope.
  - D4 — the generated scope carries a host-covering `/32` (IPv4) or
    `/128` (IPv6) CIDR alongside the exact pattern for IP literals
    (required for CIDR evaluation); no broader wildcard is allowed.
- Rejected seams honored: no `eggsec-report-model` dependency, no generic
  `scan --json`/`PipelineReport`, no `ci`, no stress/flood/raw-packet/
  NSE/db-proxy/C2/remote/credential/proxy behavior.

## 3. Verification record

Local (implementation host, `EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process`):

- `cargo fmt --all -- --check` — pass.
- `cargo check --workspace --all-targets --locked` — pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — pass.
- `cargo test --workspace --all-targets --locked` — pass: **430 tests across 21 suites**.
- `cargo check --workspace --all-targets --all-features --locked` — pass.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass.
- `EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-targets --all-features --locked` — pass: **489 tests across 21 suites**.
- `cargo +1.89.0 check --workspace --all-targets --locked` — pass.
- `cargo +1.89.0 check --workspace --all-targets --all-features --locked` — pass.
- `cargo +1.89.0 test -p eggbench-core --all-features --locked` — pass (152).
- `cargo +1.89.0 test -p eggbench-runner --all-features --locked` — pass (118).
- `cargo +1.89.0 test -p eggbench-drivers --all-features --locked` — pass (147).
- `cargo tree --locked` — pass; no `eggsec-*` crate in any graph.
- `git diff --check` — pass.
- Live harness `scripts/qualification/m004a-eggsec/run-live-qualification.sh` — **8/8 PASS** (fresh-clone pinned build, `eggsec_live` 5/5, full safe production run with 22 evaluated cases / 0 bypasses / execution completed / 2 trials intact / lifecycle placement `startup_readiness > correctness_checks > measured_trial > drain > teardown > finalization` / no raw payload in bundle).
- CLI spot checks: `validate` accepts schema v6 and rejects `security_checks` on v5; `doctor` reports the `eggsec-waf` descriptor, live `0.1.0` probe, families, unsupported operations, and strict-scope note; `inspect` shows the sanitized per-check summary without payloads.

Hosted (on the exact implementation candidate `273e5b1`):

- CI run `36197530270`: macos-stable, windows-stable, and linux-msrv
  (1.89.0) green; linux-stable failed at `cargo fmt --all -- --check`.
  Root cause (self-inflicted, non-semantic): post-`fmt` Clippy-driven
  edits in `resolved.rs` were committed without re-running `cargo fmt`.
  No test, semantic, or platform failure was involved.
- Live run `36197530332`: `live-tools-linux` and the new
  `live-eggsec-linux` job green — the M004a harness passed 8/8 against
  the exact pinned Eggsec build on hosted Linux.

Superseding full-green qualification on `b2de53e` (which contains the
identical M004a implementation plus formatting and the M004b gate
family, which does not alter M004a semantics):

- CI run `36200879518`: linux-stable, linux-msrv, macos-stable,
  windows-stable all green.
- Live run `36200879546`: `live-tools-linux`, `live-eggsec-linux`, and
  `live-m004b-linux` all green.

## 4. Schema, compatibility, and provenance

- Plan schema v6 is additive; v1-v5 remain readable. An explicit
  `security_checks` field (even `[]`) on v1-v5 fails with
  `unsupported_option`; `security_checks + paired` fails with
  `paired_security_not_supported` and `security_checks + network_path`
  fails with `security_path_incompatible`, both ahead of the generic
  schema gates (mirroring M003a/M003b precedence).
- `ResolvedPlan` schema advances v3 → v4 (`security_checks` carried
  alongside `diagnostics`); v1-v3 remain readable via `#[serde(default)]`.
  The `Correctness` driver resolves against the `waf_bypass` family
  capability with the pinned `eggsec` executable path
  (`MissingExecutablePath` before startup when absent). Upstream tool
  provenance is intentionally not frozen in the descriptor (evolving 0.1
  tree): the observed version plus executable SHA-256 are recorded per
  run in `security-checks.json` and per check in `security/<id>.json`.
- `security-checks.json` schema v1 records driver/adapter/tool
  version/SHA, the audited `waf --json --bypass` operation, scope digest,
  lifecycle placement, and per-check request/disposition/artifact
  identity. Comparison loads and verifies it; security runs without the
  index (or vice versa) fail closed.
- Comparison goldens changed by exactly one suffix per file
  (`security checks match (absent)` in `driver_detail`); no verdict changed.
- `ResolvedPlan` snapshot fixture gained `"security_checks": []` with
  schema version 4.
- Removal gate (plan §9): once Eggsec publishes an immutable release
  carrying the strict-preflight + WAF JSON contract, future
  qualification should pin that release/tag instead of source revision
  `0509ac66`.

## 5. Security and lifecycle evidence

- No shell, argv-only Eggsec invocation, bounded stdout/stderr (4 MiB /
  256 KiB), bounded JSON fields, targets from declared runtime bindings
  only with local/private confinement enforced before spawn, generated
  exact scope with restrictive permissions and post-check removal, no
  manual override flags, no credentials/proxies, bounded
  cancellation/timeout with mandatory drain/teardown tail.
- A valid security Fail never becomes `ExecutionStatus::Failed` or
  `Invalid` and never suppresses warmups/trials; operational correctness
  errors use `CorrectnessFailed` → `Invalid` (or first-class
  `Cancelled`/`TimedOut`).
- Two defects found during implementation were fixed with regression
  coverage: schema-v6 plans carrying `diagnostics` were rejected (v5
  feature not retained in v6; now accepted on v5+v6 with SemanticReplay
  on v4-v6), and the evidence-capacity preflight reserved security
  bytes even for security-free plans (now reserves nothing when empty).
- Sentinel test proves unrelated secret-like plan content cannot enter
  security artifacts; per-check results are `Sensitivity::Redacted`.

## 6. Known limitations and follow-up boundaries

- Full production runs observe Fail only through a reflecting managed
  service, which M004a deliberately does not introduce (no new attack
  surface): the live Fail observation is qualified at the adapter level
  against the permissive echo fixture with the real pinned binary, plus
  fake-driven lifecycle proof that Fail continues into trials. The
  permissive echo fixture plus the live adapter suite are the reusable
  substrate for M004b combined-verdict qualification.
- External-service (non-runtime-bound) targets fail closed with
  `security_target_incompatible` at execution: M004a supports only
  startup-established runtime HTTP bindings.
- Link-local IPv6 targets are rejected (zone-ambiguous scope matching);
  non-`localhost` hostnames that could resolve publicly are rejected
  without DNS (fail closed).
- Child stderr is bounded but never staged (payload-leak avoidance);
  Eggsec `duration_ms` remains diagnostic-only evidence.
- All M004a non-goals from plan §4 remain future work (generic Eggsec
  commands/profiles, public targets, stress/packet/NSE/db-proxy/C2/
  remote/daemon/REST/MCP/agent, credentials, custom routes, M002 path
  translation, paired checks, severity heuristics, baseline-derived
  expectations, performance-verdict combination — the last owned by
  M004b).
- M004b owns ComparisonReceipt v3, the correctness gate family, and the
  umbrella M004 closure; Security Qualification M001 remains gated on
  M004b closure.

## 7. Disposition

**Closed.** M004a delivers schema-v6 security-check intent, the distinct
`Correctness` execution category with a sibling-neutral runner seam, the
external strict-scope Eggsec WAF adapter with generated scope and
guarded preflight, one bounded bypass observation per requested check
with Eggsec-owned `bypass_successful` semantics and a predeclared
threshold, sanitized versioned evidence outside measured timing with
payload non-retention, comparison-critical security configuration
identity, CLI validate/doctor/run/inspect coverage, and local plus
hosted plus live-binary qualification. M004b is unblocked.
