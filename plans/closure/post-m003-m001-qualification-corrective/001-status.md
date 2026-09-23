# Post-M003/M001 Qualification Corrective C001 — Closure

Disposition: **closed**
Closed: 2026-09-23
Implementation commits: `e924f2a` (CLI truthfulness, runtime split, SIGINT,
portability) plus `a5f8260` (hosted Clippy follow-up: cfg-gated imports,
truthful macOS `os_version`) against plan baseline `c73c00a` (via plans-only
`d80fdc3`, no code delta).
Hosted qualification: CI run `35808371805` — success on Linux stable, Linux
Rust 1.89 MSRV, macOS stable, and Windows stable jobs at HEAD `a5f8260`.
A first corrective push (`e924f2a`, run `35808047844`) passed both Linux
lanes and both `cargo check` steps but exposed two further hosted-only
Clippy defects; `a5f8260` resolved them and the fresh four-lane run is
fully green.

## 1. Why a corrective closure rather than a new milestone

Historical M003 closure (`plans/closure/local-runner-lifecycle/003-status.md`)
and Measurement M001 closure (`plans/closure/measurement-comparison/001-status.md`)
were based on local verification. Post-implementation audit plus hosted run
`35803742746` (Linux pass, macOS `cargo check` fail, Windows Clippy fail)
found four confined defect classes at the M003 CLI/environment boundary —
production execution of the qualification fake, incorrect exit-code routing,
unwired SIGINT, and cfg-specific collector failures — with no Measurement
M001 semantic defect. The corrective restores the planned invariants without
rewriting either historical closure.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Outcome |
|---|---|---|
| Production registry contains no synthetic driver | `WorkloadRegistry::production()` returns empty; legacy `with_builtin()` is now a production-empty alias; `with_qualification_fake()` is qualification-only and never used by `main.rs`. Tests `production_registry_contains_no_fake_driver`, `legacy_builtin_constructor_is_production_empty`, `production_runtime_reports_no_driver`. | Pass |
| Production `run` fails before startup with no service started | `commands::run::run` resolves against the empty production inventory (`MissingDriver`) before environment/bundle/session work, with a defense-in-depth `unsupported_workload` guard. Test `production_run_fails_before_startup_without_starting_services` asserts code 3, `missing_driver`/`unsupported_workload`, no result payload, bundle path absent. Binary tests assert exit 3 and no bundle in both modes. | Pass |
| Deterministic fake remains injectable without a public flag | `QualificationRuntime::new()` + `workload_executor(FakeWorkload)`; `run_with_qualification` and `doctor::run_with_registry` seams; no `--fake-workload` or similar CLI switch. Tests `qualification_registry_injects_fake_load`, `doctor_qualification_registry_resolves`, qualification run tests. | Pass |
| JSON failures return nonzero; human/JSON codes identical | `PresentedCommandResult { envelope, exit_code }`; `present()` returns the attached code in both modes; `main()` is the only process-status site. `tests/binary_exit_codes.rs` (7 tests) locks codes 0/2/3/5 in both modes; `exit_code_matrix_is_locked` pins 0/1/2/3/4/5. | Pass |
| Finalized Failed/Cancelled/Invalid retain bundle, exit 4 | `CliEnvelope::run_non_success` / `PresentedCommandResult::run_non_success`: `ok=false`, `result=Some(Run{...})`, `error=run_non_success`, code 4. Tests `qualification_run_failed_finalized_run_exits_four_with_bundle` (fail_on) and `qualification_run_cancellation_exits_four_with_bundle` (pending + signal) assert bundle exists, `execution_status` truthful, `error.category == run_non_success`. | Pass |
| Evidence/bundle failures exit 5 | `failure_from_bundle_error` and `execute_run` evidence errors map to `ExitCode::EvidenceIo`. Binary tests: corrupt and missing bundle `inspect` exit 5 in both modes; library test `inspect_rejects_missing_bundle`. | Pass |
| Parse/validation → 2, capability/preflight → 3 | `CliError::into_failure` mapping unchanged; `doctor` resolution errors map `InvalidPlan` → 2, driver/capability/platform → 3. Binary + library tests cover both modes. | Pass |
| Ctrl-C cancels the M002 token; drain/teardown authoritative | `forward_signal` + `spawn_signal_forwarder` around `execute_run`; listener aborted/joined after completion; no `process::exit` in the handler, no direct child killing. Test `signal_forwarder_cancels_token` plus the cancelled-bundle regression above. | Pass |
| Platform-label `unknown` fallback removed | `platform_name()` in `run.rs` and `Name::new` mapping in `doctor.rs` propagate `CliError::Internal` instead of coercing to `unknown`. | Pass |
| macOS compiles/lints/tests clean | Root causes: `usize::from_str` without trait import (2 sites), cfg-ignored `key` param, Linux-only `fs`/`Path` imports, always-`Some` macOS `os_version`. Fixes: `parse::<usize>()`, cfg-scoped parameter name, cfg-gated imports, `kernel_release().map(...)`. Run `35808371805` macos-stable: success. | Pass |
| Windows Clippy clean without broad suppressions | Root causes: cfg-leaked `cpuinfo_logical_count`, `parse_cpu_range_count`, `read_trimmed` dead on Windows; `os_version_label` always-`Some` with `unknown` fallback. Fixes: `#[cfg(target_os = "linux")]` gating (tests cfg-aligned), Windows `os_version` omitted when unavailable. Run `35808371805` windows-stable: success. | Pass |
| Optional facts absent, never `unknown` | Windows/macOS `os_version` return `None` when the source is unavailable; collector test `windows_missing_version_stays_absent` (hosted); `docs/environment-fingerprint.md` contract unchanged. | Pass |
| Linux stable + Rust 1.89 green | Same run: linux-stable and linux-msrv jobs success; local `cargo +1.89.0 test -p eggbench-core` 41 passed. | Pass |
| Measurement M001 schemas/semantics unchanged | No edits under `metrics.rs`, trial schemas, manifest v1/v2, or CLI envelope schema v1. Only M003 CLI/environment code changed. Hosted core tests pass on all lanes. | Pass |

## 3. Verification

- `cargo fmt --all -- --check` — pass
- `cargo check --workspace --all-targets --locked` — pass
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass
- `EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-features --locked` — pass: 154 passed (14 suites)
- `cargo +1.89.0 check --workspace --all-targets --locked` — pass
- `cargo +1.89.0 test -p eggbench-core --all-features --locked` — pass (41 core tests)
- `cargo tree --locked` — pass; no new runtime or process dependency (`tokio` signal feature already present)
- `git diff --check` — pass
- Focused subprocess tests in JSON and human modes — pass (`tests/binary_exit_codes.rs`, 7 tests; code 4 via the injected harness exercising the same presentation/exit path)
- Hosted CI run `35808371805` — success: linux-stable, linux-msrv, macos-stable, windows-stable

Intermediate hosted run `35808047844` is retained as evidence that the first
push fixed `cargo check` on both hosted targets while surfacing the final
two Clippy defects; it is superseded by the green run above, not a separate
qualification basis. The plans-only closure commit itself is likewise green
(CI run `35808813055`, four lanes success), so closure HEAD carries the same
qualification.

## 4. Compatibility, security, and limitations

- CLI envelope schema v1, manifest v2, trial execution/result schemas, metric
  vocabulary/normalization, and the M002 lifecycle contract are unchanged.
- `execute()` now returns `PresentedCommandResult` instead of
  `Result<CliEnvelope, CliError>`; in-repo callers and tests were migrated.
  External consumers of the CLI library crate must read
  `.envelope`/`.exit_code`.
- `WorkloadRegistry::with_builtin()` is preserved as a production-empty alias
  so existing call sites fail closed; new code should prefer
  `production()` / `with_qualification_fake()` / the runtime structs.
- Windows managed `run` remains explicitly unsupported and Windows
  managed-process cancellation is not qualified, per plan §17. Production
  `run` on Windows fails with the same stable capability category before
  startup.
- No second-signal hard termination, no Job Objects, no real workload
  adapter, no plugin registry — all per plan non-goals.
- Docs updated: `docs/cli.md` (empty production registry, locked matrix,
  code-4 bundle retention, graceful Ctrl-C), `README.md` (substrate-only
  production `run`).

## 5. Dependency disposition

- **Local Runner M003** — fully closed. The conditionally-closed status is
  lifted; no further M003 corrective is registered.
- **Measurement M001** — hosted qualification now supplied by the corrected
  combined HEAD (run `35808371805`); treated as closed for qualification.
  Schemas and normalization semantics untouched by this corrective.
- **Measurement M002** — unblocked; plan authoring/implementation may proceed
  under ADR-0003.
- **Eggstack Integrations M001** — planning/implementation re-enabled against
  the truthful CLI/runner substrate.
- **External Oracles M001** — permitted to introduce the first real workload
  adapter; no competing production fake remains.
- **Security Qualification / Distributed execution** — unchanged (still
  blocked/deferred per registry gates F–H).

## 6. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| None | No unresolved correctness, security, lifecycle, or portability finding | No further corrective work required |
| Info | Windows managed-process cancellation remains unqualified by design | Accepted per plan §17; production `run` fails before startup there |

## 7. Disposition

**Closed** with the production fake path removed, exit codes truthful and
locked at binary level in both presentation modes, SIGINT routed into M002
cancellation with mandatory cleanup, macOS/Windows collectors cfg-correct
without suppressions or fabricated values, and a fresh four-lane hosted CI
run green on the corrective HEAD. Future plan authoring may proceed.
