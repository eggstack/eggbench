# Local Runner M003 — Environment Fingerprint and CLI Lifecycle — Closure

Disposition: **conditionally closed**
Closed: 2026-09-23
Implementation commit: `59d53774dc6015e385d705af4dee5f063f4aefff` against plan baseline `9d143584c18cc3d7a49e0271f452587736b118d9`.
Hosted qualification: not run in this pass; local verification only (see §5).

## 1. Requirement-to-evidence matrix

| Plan § / requirement | Evidence | Outcome |
|---|---|---|
| §6 environment collector in runner, core owns DTO | `crates/eggbench-runner/src/environment.rs`: `LocalEnvironmentCollector::collect()`; no shell strings, no env/`PATH` inspection | Pass |
| §7 fingerprint v1 field policy, stable names/classes, absence-not-placeholder | `docs/environment-fingerprint.md` field table; collector trims/omits empty facts; `environment.rs` classification tests | Pass |
| §8 subject snapshot `subject.json` + digest match, external/label no fabricated digest | `crates/eggbench-runner/src/subject.rs`: `SubjectSnapshot::build()`, bounded 64 KiB streaming SHA-256, `declared_digest_matches`, `MAX_SUBJECT_SNAPSHOT_BYTES`; `doctor`/`run` fail closed on mismatch | Pass |
| §10 `prepare_bundle` pre-start helper staging plan/resolved-plan/environment/subject | `crates/eggbench-runner/src/prepare.rs`: `BundlePreparation`, `prepare_bundle`, `build_subject_snapshot`, `collect_local_environment`; unit tests stage all four artifacts and reject invalid env/unsafe subject | Pass |
| §5/§11–§15 CLI crate with validate/doctor/run/inspect | `crates/eggbench-cli` (`lib.rs`, `main.rs`, `commands/`, `plan_input.rs`, `workload_registry.rs`, `envelope.rs`, `error.rs`); binary `eggbench`; `.toml`/`.json` inference, stdin requires `--input-format`, unknown extensions rejected | Pass |
| §12 validate parses + semantic check, no drivers/processes | `commands/validate.rs`; CLI integration tests good/bad/unknown-extension/stdin | Pass |
| §13 doctor validates + resolves + collects env + preflights, starts nothing | `commands/doctor.rs`; integration test asserts fingerprint fields present and no process start | Pass |
| §14 run pipeline parse→validate→resolve→preflight→collect→prepare→session→execute→result; explicit bundle destination; injected fake proves plumbing, no `--fake-workload` production flag | `commands/run.rs` delegates to `LocalSession::prepare` + `execute_run` via `prepare_bundle`; `BuiltinWorkloadExecutor` wraps `FakeWorkload`; docs state fake is qualification-only | Pass |
| §15 inspect verifies via `BundleReader` before summarizing | `commands/inspect.rs`; integration tests verify v2 bundle and reject missing bundle | Pass |
| §16 machine envelope v1, one JSON doc on stdout, stderr-only diagnostics, no secrets | `envelope.rs` (`schema_version`, `command`, `ok`, `result`, `error`, `warnings`); `main.rs::present` prints JSON only on stdout, human lines on stderr; `PathBufPayload` refuses lossy non-UTF8 paths | Pass |
| §17 exit-code table locked by tests | `ExitCode::{Success=0, Internal=1, ParseValidation=2, CapabilityPreflight=3, RunCompletedNonSuccess=4, EvidenceIo=5}`; `CliError::into_failure` mapping covered by tests | Partial — mapping exists and is tested, but `main.rs::present()` exits `3` for every `ok:false` envelope instead of the envelope's own code (see §7) |
| §18 cancellation via M002 token, no second force-kill path | `run.rs` creates a `CancellationToken` and passes it to `execute_run`; docs record single-cancel semantics | Partial — no OS SIGINT handler is wired to that token in this pass (see §7) |
| §19 Windows compiles; validate/doctor/inspect work; managed run unsupported | `cfg` use is limited to `environment.rs` field sources; `UnixPlatform` reports support state; no Job Objects added | Pass (local Linux only; Windows/macOS lanes not run here) |
| §20 ARM/SBC: bounded buffers, no scans, no mandatory commands | 64 KiB hash buffer, `/proc`+`sysctl` best-effort reads, `cpu_model` omitted when absent | Pass |
| §21 security/privacy: no env dump, no hostname/MAC/IP, bounded hashing, no symlink following | Collector never reads process env; fingerprint docs exclude hostname/net; `BundleReader` verifies | Pass |
| Acceptance 12: no manifest/trial schema change | No change to manifest v2 or trial-result v1; only additive runner/CLI modules | Pass |

## 2. Tests/guards run and outcomes

- `cargo fmt --all -- --check` — pass
- `cargo check --workspace --all-targets --locked` — pass
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass
- `EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-features --locked` — pass: **112 passed (12 suites)**
- `cargo +1.89.0 check --workspace --all-targets --locked` — pass
- `git diff --check` — pass
- Without the parent sentinel one pre-existing lifecycle test fails by design (`managed_environment_is_hermetic...` requires the sentinel); not an M003 regression.
- New coverage: 6 environment unit tests + 6 subject unit tests + 5 prepare unit tests + 9 CLI integration tests in `crates/eggbench-cli/tests/cli.rs`.

## 3. Schema/migration/compatibility evidence

- `EnvironmentFingerprint` schema v1 unchanged; M003 only adds real producers.
- New `SubjectSnapshot` schema v1 (`subject.json`, `ArtifactRole::Subject`); additive, no manifest migration.
- New CLI envelope schema v1 (`CLI_OUTPUT_SCHEMA_VERSION = 1`); machine fields are the compatibility surface, human stderr prose explicitly not.
- `BundleWriter` still requires source plan/resolved plan/environment before finalization; `prepare_bundle` satisfies that ordering before startup.

## 4. Security and lifecycle evidence

- Collection runs before `LocalSession::prepare`/startup, so collection failure cannot strand processes.
- No shell interpolation, no ambient-`PATH` resolution, no secret refs resolved into artifacts.
- Declared-digest mismatch fails `doctor`/`run` before startup with `subject_digest_mismatch`.
- `run` stages evidence through M002 `execute_run`, preserving the C001 mandatory drain/teardown tail; structural evidence errors still return `Evidence` without publishing a partial bundle.

## 5. Documentation/operational evidence

- Added `docs/environment-fingerprint.md` (field/class table, privacy rules, subject snapshot).
- Added `docs/cli.md` (commands, envelope example, exit codes, fake-load boundary, cancellation, inspection).
- Updated `architecture/runner.md` (M003 pre-start preparation), `architecture/core.md` (fingerprint/subject ownership), `README.md` (command examples + no-production-generator notice).

## 6. Known limitations

- Production workload adapters remain absent by design; `run` executes only the deterministic `fake-load` qualification path.
- Hosted CI (Linux stable, Linux 1.89, macOS, Windows subset) was not run in this pass; cross-platform qualification is outstanding.
- `inspect` summarizes execution/subject/driver/environment/trials but performs no metric interpretation (deferred to Measurement M001).

## 7. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| Medium | `main.rs::present()` maps every `ok:false` envelope to exit code 3, so documented codes 4 (`Failed`/`Cancelled`/`Invalid` with bundle) and 5 (evidence/verify failure) are not observable from the binary despite the `ExitCode` enum and `into_failure` mapping being correct. | Corrective required: route the envelope's failure exit code through `present()` and add binary-level exit-code tests. |
| Medium | No OS signal handler wires SIGINT/Ctrl-C to the `CancellationToken` created in `commands/run.rs`; cancellation works only if the token is cancelled internally. Plan §18 is therefore only structurally staged, not functionally delivered. | Corrective required: add `tokio::signal` (unix) / console handler (windows) wiring with a regression test proving cancellation reaches `execute_run` cleanup. |
| Low | `run` synthesizes a `Name("unknown")` platform fallback via `unwrap_or_else` if `UnixPlatform.label()` ever fails validation; current label validates, so the path is dead, but the fallback masks future label regressions. | Clean up in corrective pass: propagate a typed error instead of falling back. |

## 8. Disposition

**Conditionally closed.** The M003 vertical slice (fingerprint, subject snapshot, pre-start bundle preparation, four-command CLI, envelope contract, docs, local verification) is landed and regression-tested. Full closure requires (a) the exit-code routing fix, (b) real SIGINT→token wiring, and (c) a green hosted CI run across the four lanes. Measurement/Comparison M001 may proceed against the landed `WorkloadOutput`/trial-evidence seam; it must not depend on the CLI crate.
