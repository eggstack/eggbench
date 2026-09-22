# Local Runner M001 Closure

Disposition: **closed**

Implementation commit: `9387a45e1bbf9c1f9a55fb8ad875b07f6f880d21` (`feat(runner): add managed process and readiness lifecycle`).

## Requirement-to-evidence matrix

| Requirement | Evidence | Outcome |
|---|---|---|
| Runtime/process-owning crate depending inward on core | `crates/eggbench-runner` depends on `eggbench-core`; `cargo tree -p eggbench-core` shows no process, Tokio, or network dependency | Pass |
| Narrow local session API for `ResolvedPlan` | `LocalSession::prepare/startup/shutdown/run`, ordered specs, bounded logs, diagnostics | Pass |
| Spawn argv directly, never a shell | `tokio::process::Command` with program plus args, `env_clear`, no shell | Pass |
| Start only managed commands; never own external services | `prepare` skips `External` lifecycle; `external_services_are_observed_but_never_owned` asserts no spawn, no events, pre-existing process left running | Pass |
| Dependency order startup, reverse teardown | Topological sort in `spec::prepare`; `dependency_order_startup_and_reverse_teardown` asserts spawn `[a b c]` and stop `[c b a]` | Pass |
| Readiness within declared timeout; unknown probes fail | `Delay`/`Probe` dispatch with caller-side timeout; `readiness_delay_and_fake_ok_succeed` and `unknown_probe_fails_before_spawn` | Pass |
| Spawn failure before readiness | `spawn_failure_before_readiness` with a nonexistent program; empty cleanup | Pass |
| Readiness timeout and early exit | `readiness_timeout_tears_down_started_process` (`fake-never`, 150 ms) and `child_failure_before_readiness` (exit 3 during delay) | Pass |
| Cancellation during startup/readiness and before spawn | `cancellation_during_readiness_tears_down` and `cancellation_before_spawn_reports_cleanly` | Pass |
| Graceful shutdown then forced cleanup | `graceful_shutdown_path` (`term-exit`) and `forced_cleanup_after_ignored_termination` (`term-ignore`, SIGKILL after grace) | Pass |
| Descendant cleanup on advertised platforms | `descendant_cleanup_reaches_process_group`: grandchild pid file, alive before teardown, reaped after; Linux tested | Pass |
| Cleanup failure preserves the original failure | `cleanup_failure_preserves_primary_failure` with an injected failing platform adapter: primary early exit plus one teardown record | Pass |
| Continuous pipe draining with bounded/truncated output | `bounded_output_truncates_but_keeps_draining`: 128 KiB per stream against a 4 KiB cap; totals prove draining continued | Pass |
| Missing secrets fail before spawn; values never leak | `missing_secret_fails_before_spawn_and_values_stay_redacted`: `MissingSecret`, redacted `Debug`, logs and bundle bytes asserted free of the value | Pass |
| Lifecycle artifacts reuse the M003 bundle writer | `stage_lifecycle_logs`/`stage_lifecycle_metadata` with `Stdout`/`Stderr`/`Other(lifecycle)` roles, `Redacted` sensitivity | Pass |
| Zero-trial lifecycle evidence is inconclusive | `zero_trial_lifecycle_evidence_is_inconclusive`: finalized with zero trials, status `Inconclusive`, bundle verifies | Pass |
| Working-directory confinement | `working_directory_escapes_are_rejected` (`../escape`, `/tmp`, `../../etc`); relative paths resolve against the explicit root | Pass |
| Unsupported kinds and platforms fail explicitly | `unsupported_service_and_platform_fail_explicitly` (managed `Named` kind; `UnsupportedPlatform` adapter) | Pass |
| Managed subject spawns first, stops last | `subject_managed_command_spawns_first` | Pass |
| Platform matrix is truthful | `supported_platform_matrix_is_truthful`; Unix supported, Windows unsupported-capability error | Pass |

## Verification

- `cargo fmt --all -- --check` — pass
- `cargo check --workspace --all-targets --locked` — pass
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass
- `cargo test --workspace --all-features --locked` — pass (27 core + 20 runner)
- `cargo +1.89.0 check --workspace --all-targets --locked` — pass
- `cargo tree --locked` — pass; runner adds `tokio`, `tokio-util`, `nix`; core keeps filesystem/hash support with no runtime, process, or concrete network dependency
- `git diff --check` — pass

## Runner/core dependency boundary

`eggbench-core` owns only runtime-free contracts and the M003 bundle writer. All process ownership (`tokio::process`), signal delivery (`nix` process groups), readiness dispatch, secret resolution at the spawn boundary, and the child fixture live in `eggbench-runner`, which depends inward on core. No core schema migration was required; no runner-local serialized fields were invented.

## Supported platform matrix

| Platform | Status | Evidence |
|---|---|---|
| Linux | Supported, descendant cleanup tested | Process-group SIGTERM/SIGKILL; grandchild reaping test passes |
| macOS | Supported by the same process-group contract | Same code path, no procfs assumptions; qualification awaits platform CI |
| Windows | Unsupported capability | `UnsupportedPlatform` adapter; managed spawn fails explicitly before any spawn |

## Log-bound behavior

Per-stream retention keeps leading bytes up to `log_limit_bytes` (subject default 1 MiB) while draining continues past the cap with dropped-byte accounting. The 128 KiB emission test retains exactly 4 KiB per stream with `truncated` set and full totals recorded.

## Evidence bundle example

The inconclusive zero-trial test stages `lifecycle/logs/<service>.stdout/.stderr` plus `lifecycle/lifecycle.json` (order, events with diagnostic pids, cleanup, observed externals) alongside the required plan, resolved-plan, and environment artifacts, then finalizes with `RunStatus::Inconclusive` and zero trials. The bundle verifies and carries no secret values.

## Findings and disposition

- No load generator, trial scheduler, measurement clock, comparison engine, or CLI was added, per the milestone non-goals.
- Managed `Named` service kinds, unknown probes, absolute/escaping working directories, missing secrets, and Windows managed execution all fail explicitly before or during startup with redaction-safe errors.
- Known limitations: macOS descendant cleanup shares the Unix implementation but is qualified only by code-path equivalence until platform CI runs; Windows managed execution is an explicit capability error, not a degraded mode; zombie reaping relies on the owned handle poll plus signal-zero liveness.
- No unresolved correctness findings.
- No material deviation from the plan.

M001 is closed. Its hard-dependency gate for Local Runner M002 is satisfied; M002 still needs its implementation plan written before handoff.
