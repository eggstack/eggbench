# External Oracles M001 — External Command Driver Substrate — Closure

Disposition: **closed**
Closed: 2026-09-23
Implementation commit: `7afa054` (feat(drivers): close External Oracles M001 command substrate and catalog ownership), on top of planning baseline `fde253dfcbc7b9263cee3df5802c84fa92b96e2a`.
Hosted qualification: not run in this pass; local verification only (see §5). Four-lane hosted CI remains required before release qualification claims.

## 1. Requirement-to-evidence matrix

| Plan § / requirement | Evidence | Outcome |
|---|---|---|
| §5 `crates/eggbench-drivers` (`lib/catalog/external/{resolver,version,command,parser,artifact,error}`), Rust 1.89, inherited lints, no HTTP/network deps | `crates/eggbench-drivers/`; `cargo tree` shows only core/runner/tokio/serde/sha2/thiserror (+`nix` on Unix); `forbid(unsafe_code)` | Pass |
| §6 production catalog ownership moves out of CLI; catalog empty; qual fake isolated; `main.rs` never imports runner `test_support`; CLI asks drivers for descriptors | `catalog.rs::DriverCatalog::production` (empty); `workload_registry.rs::production()` delegates with `debug_assert!(empty)`; `doctor`/`run` behavior unchanged; no re-export shim needed (no external consumers) | Pass |
| §7 feature policy `default=[]`, `external-command=[...]`; cheap default build | `eggbench-drivers/Cargo.toml`; `cargo tree -e features` shows no protocol client/server in default graph | Pass |
| §8 trusted resolver: explicit absolute only, PATH skips empty/relative, Unix exec bits, Windows `.exe`/`.com` only + `.bat`/`.cmd` rejected, canonical+hash identity | `resolver.rs::BinaryResolver`; unit tests (relative reject, empty/relative PATH skip, non-exec reject, batch reject, metacharacters literal) + integration symlink identity test | Pass |
| §9 SHA-256 streaming identity, hex, size, classification; diagnostic only | `hash_file()` bounded 8 KiB reads; `ResolvedExecutable{sha256_hex,file_size,executable_class}` | Pass |
| §10 version probe: argv[0] + tail, no shell, null stdin, bounded timeout, cancellable, nonzero = failure, truncation visible | `version.rs::VersionProbe::run`; integration tests (success `1.2.3`, nonzero fails, timeout typed) | Pass |
| §11 command spec: argv-only, no shell/glob, no inherited cwd/env, null stdin | `ExternalCommandSpec`; `env_clear` + explicit env; unit test asserts argv-literal contract | Pass |
| §12 environment policy: `env_clear`, explicit only, deterministic locale for probes | `command.rs` + `default_probe_env()` (`LC_ALL=C`/`LANG=C`); no whole-environment copy anywhere | Pass |
| §13 bounded capture: concurrent drain, independent caps, drain-past-cap, no UTF-8 assumption, no full-output clone | `drain_stream()` shared helper; integration tests (200 KiB > 4 KiB cap, concurrent 5 KiB+5 KiB) | Pass |
| §14 typed outcome: identity, redaction-safe argc, exit, captures, wall duration, cancel/timeout state, cleanup notes; no secrets in errors | `ExternalCommandOutcome`; errors carry category + redaction-safe detail only | Pass |
| §15 cancellation/timeout: token + deadline + exit observed; bounded TERM/KILL; drain continues; typed outcome; Unix process group reuse; Windows `direct_child_only` | `run_command()` + `terminate_child()` (safe `process_group(0)`, same mechanism as runner session; no second signal impl); integration tests (timeout, cancellation, Unix descendant cleanup) | Pass |
| §16 no duplicated process authority; dependency `core <- runner <- drivers <- cli`; runner does not depend on drivers | `cargo tree` confirms direction; drivers reuses runner's group mechanism via std API, not a copied supervisor | Pass |
| §17 raw artifact helper (`stdout.raw`, `stderr.raw`, `command-metadata.json` with identity/exit/truncation/parser id); safe single-component names; no metric normalization | `artifact.rs`; integration test asserts names + determinism | Pass |
| §18 parser contract independent of spawning; versioned; fixture-only parser (not a disguised oha parser); stable error categories | `ExternalOutputParser` + `FixtureParser` (`eggbench-fixture/v1`); unit tests (accept/malformed/nonzero/truncated) | Pass |
| §19 timing boundary: M002 measured interval untouched; substrate records child wall duration independently | No `orchestration.rs` change; `duration` is outcome-local only | Pass |
| §20 error taxonomy (14 stable categories) | `ErrorCategory::as_str()` + `Display`; every constructor maps to one category | Pass |
| §21 fixture executable (version/bytes/exit/sleep/malformed/child-spawn), argv-only, test-only, cross-platform | `src/bin/eggbench_fixture.rs`; `tests/substrate.rs` uses it on all platforms, no shell scripts | Pass |
| §22 security tests | Unit: relative/empty/relative-PATH/non-exec/batch/metachar; integration: symlink identity, truncation-no-deadlock | Pass |
| §23 lifecycle tests | Integration: success/nonzero/timeout/cancel/oversize-cap/concurrent-streams/Unix-descendant; cleanup precedence via typed errors | Pass |
| §24 catalog/CLI regressions | `production_catalog_remains_empty`; full workspace suite (184 passed) incl. existing CLI registry tests | Pass |
| Acceptance 1–15 | All hold; notably: no oha/h2load/iperf3/netem code, no `unsafe`, no shell interpolation (`grep` clean) | Pass |

## 2. Tests/guards run and outcomes

- `cargo fmt --all -- --check` — pass
- `cargo check --workspace --all-targets --locked` — pass (lockfile updated for the new crate + `nix` Unix dep)
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass (pedantic-clean)
- `EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-features --locked` — pass: **184 passed (18 suites)** — 30 new drivers tests (unit + `tests/substrate.rs` integration)
- `cargo +1.89.0 check --workspace --all-targets --locked` — pass
- `cargo tree --locked` — pass; dependency direction `core <- runner <- drivers <- cli` confirmed; `nix` Unix-only
- `git diff --check` — pass

## 3. Schema/migration/compatibility evidence

- No bundle/plan/metric schema change. `TrialMetrics` v1, manifest v2, `ResolvedPlan` v1 untouched.
- CLI behavior is byte-equivalent: production `doctor`/`run` fail-closed paths unchanged; only catalog ownership moved (with `debug_assert!(empty)` tripwire).
- No deprecated shim was needed: `WorkloadRegistry::production/with_builtin` signatures are unchanged.

## 4. Security and lifecycle evidence

- No shell, glob, implicit cwd, or environment inheritance in any new code path.
- Relative/empty-PATH cwd-confusion attacks are tested shut.
- Cancellation/timeout always reaps the owned child (process-group TERM/KILL on Unix, `kill_on_drop(true)` everywhere) and drains pipes to completion.
- Parser cannot promote truncated output to complete unless the required version token is in retained bytes.
- `forbid(unsafe_code)` holds across the new crate.

## 5. Documentation/operational evidence

- Added `docs/external-drivers.md` (ownership, resolution policy, execution, tests; states no tool adapter ships).
- Updated `architecture/drivers.md` (catalog ownership + substrate sections), `docs/driver-capabilities.md`, `docs/cli.md` (catalog consumption), `README.md` (substrate status + doc link).
- Representative version/artifact JSON observable via `tests/substrate.rs` assertions and `command-metadata.json` helper.

## 6. Known limitations

- No oha/h2load/iperf3/netem adapter (explicitly External Oracles M002 scope).
- Windows cleanup is direct-child-only; tool adapters that spawn descendants must not advertise Windows support until qualified (recorded in code + docs).
- Hosted four-lane CI not run in this pass; portability beyond local Linux relies on the argv-only fixture design and review.
- `workload_output_from_outcome` helper is currently unused by production code (available for M002 adapters).

## 7. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| None | No unresolved M001 correctness, security, lifecycle, or portability finding | No corrective work required |
| Note | Hosted CI qualification outstanding (Linux stable, Linux 1.89, macOS, Windows) | Tracked as follow-up; local matrix is green incl. MSRV check |

## 8. Disposition

**Closed.** Shared `eggbench-drivers` crate, production catalog ownership, trusted resolution, bounded command capture, parser contract, fixture qualification, and documentation are landed and regression-tested.

**Unblocks:** External Oracles M002 tool adapters, and Eggstack Integration M001a/M001b which consume the shared `eggbench-drivers` crate without creating a competing registry.
