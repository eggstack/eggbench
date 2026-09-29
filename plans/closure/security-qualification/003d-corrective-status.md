# Security Qualification M003d Corrective — Closure Record

Disposition: **closed**. The Eggsec-load CI portability/integration repair is
accepted; the repository-wide qualification gate is restored on the corrective
head.

Plan:

- `plans/implementation/security-qualification/003d-corrective-eggsec-load-ci-portability-and-unblock.md`

Implementation commit:
`470333370e14c6a2ac31047f3f6836ce756f9a2a`
(`fix(security): repair Eggsec-load registration accounting and CI
portability`).

Source milestone implementation repaired:
`739f38285776c1ceb13df4fbfb9a04eb27a83077`
(`feat(security): add Eggsec strict-scope load workload`).

This record does **not** close M003d or M003. M003d remains open at
`plans/closure/security-qualification/003d-status.md`.

## 1. Why the corrective existed

The first Eggsec-load slice was architecturally usable but left two repository
regressions that only exact-head hosted qualification exposed. On Eggbench
implementation SHA `9d9d8b99d12127e959dab6ca895f278aa090b742`:

- live qualification run `36607614200` was green and executed the real
  SynVoid M002 stages;
- four-lane normal CI run `36607614262` was red;
- `linux-msrv` passed;
- `linux-stable` and `macos-stable` failed
  `workload_registry::tests::production_runtime_reports_no_driver`;
- `windows-stable` failed strict Clippy on
  `crates/eggbench-drivers/src/external/eggsec_load.rs`.

Both failures were introduced by M003d commit `739f382`. The intervening
C002 implementation `9d9d8b9` contains no `crates/*/src` change, so the
red four-lane verdict was never C002-owned.

Why local verification missed them:

1. the stale count assertion only fails under the compiled feature matrix that
   includes `eggstack-http`, and the local passes that mattered ran
   `-p eggbench-drivers` clippy plus an `eggbench-cli` test run that predated
   the final `eggsec-load` inventory addition;
2. the `unused_mut` defect is cfg-dependent: on a Linux host the
   `#[cfg(unix)]` `.mode(...)` calls consume the `mut` bindings, so the
   bindings are only unnecessary on non-Unix targets. No local run can
   produce the Windows finding.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Correct the stale runtime count assertion (§2.1, §5) | `production_runtime_reports_no_driver` now asserts the exact expected workload-name list and the exact expected full descriptor-name list for the compiled feature matrix, and derives both counts from those lists (`crates/eggbench-cli/src/workload_registry.rs`) |
| Non-magic-count derivation without a vacuous self-comparison (§2.1) | expectations are hand-maintained name lists, not catalog derivations; `production_runtime_reports_no_driver` additionally asserts `eggsec-load` is present in the descriptor set |
| Windows strict Clippy no longer reports the scope-builder mutation warning (§2.2, §6) | `scope_dir_builder`/`scope_file_options` are cfg-specific; Windows stable `-D warnings` Clippy is green in run `36640125422` |
| Unix private-mode behavior retained (§6) | `scope_dir_builder` (unix) applies mode `0700`; `scope_file_options` (unix) applies `0600`; new test `scope_file_is_private_exclusive_and_removed_on_drop` asserts `0o700`/`0o600` on the created directory and file |
| `create_new(true)` retained (§6) | same test asserts a second `open` fails with `AlreadyExists` |
| Collision retry and drop cleanup retained (§6) | `create_scope_file` retry loop unchanged; same test asserts both file and directory are gone after `ScopeFile::drop` |
| Focused driver/CLI/MSRV verification green (§7) | section 4 |
| Four-lane hosted CI green on the corrective source (§8, §12.5) | run `36640125422` |
| Exact-head live qualification revalidates C002's real SynVoid stages (§8, §12.6) | run `36640125453`, `live-synvoid-linux` pass=25 stopped=0 notexec=5 |
| C002 status records the new hosted evidence (§8, §12.7) | `plans/closure/security-qualification-m002-corrective/002-status.md` section 10 |
| M003d status records this corrective (§12.8) | `plans/closure/security-qualification/003d-status.md` reconciliation section |
| No Eggsec-load behavioral or security contract weakened (§4, §12.9) | section 3 |

## 3. Invariants preserved

The change is test-only in `eggbench-cli` plus a builder-construction refactor
in `eggbench-drivers`. Verified unchanged:

- `eggsec-load` remains explicit, non-default, and external-process;
- no fallback to native Eggfetch when Eggsec is unavailable;
- one reviewed digest-pinned `http_corpus` case per Eggsec-load trial;
- local/private strict-scope enforcement and the guarded no-network preflight;
- bounded requests, concurrency, headers, body, stdout, stderr, timeout;
- expected HTTP outcome mismatch stays distinct from transport error rate;
- Eggsec/Eggfetch transport is still recorded as non-independent
  corroboration;
- no public/wildcard target scope;
- no production SynVoid or Eggsec dependency enters Eggbench.

No schema, workload semantics, scope rules, result parsing, metric names,
thresholds, qualification policy, or security expectation changed.

## 4. Verification

Local, on the implementation tree:

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo check --workspace --all-targets --locked` | pass |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | pass |
| `cargo test -p eggbench-cli --all-features --locked` | pass (91 tests, nine suites) |
| `cargo test -p eggbench-drivers --all-features --locked` | pass (167 tests, nine suites) |
| `cargo test --workspace --all-targets --all-features --locked` (with the CI parent sentinel) | pass |
| `cargo +1.89.0 check --workspace --all-targets --all-features --locked` | pass |
| `git diff --check` | clean |

Feature-isolation checks equivalent to the `linux-stable` workflow all pass:
`eggbench-drivers` with `--no-default-features` and each of `eggstack-http`,
`gregg`, `eggstack-path`, `eggstack-path,gregg`; `eggbench-cli --no-default-features
--features eggstack-path`; and `validate examples/eggstack-path.json`
(`eggbench: validate ok`).

Portability evidence for the Windows finding: the `cfg(not(unix))` builder
shapes were compiled under `-D warnings` on the Linux host by temporarily
selecting the non-Unix variants, which is green. Cross-compiling
`--target x86_64-pc-windows-msvc` is not possible on this host because
`ring v0.17.14` has no buildable Windows C toolchain here, so the authoritative
Windows evidence is hosted lane `windows-stable` in run `36640125422`.

Note on the workspace test run: the `managed_environment_is_hermetic_and_explicit_secrets_stay_redacted`
lifecycle test requires `EGGBENCH_PARENT_SENTINEL_26CE` to be set in the parent
process. It fails identically on the unmodified baseline without that variable
and passes with it; it is not related to this corrective.

## 5. Hosted evidence

Both runs are on the exact corrective head
`470333370e14c6a2ac31047f3f6836ce756f9a2a`.

### 5.1 Normal CI — run `36640125422` (green, four lanes)

| Lane | Result |
|---|---|
| `linux-stable` | success (fmt, check, feature isolation, all-feature Clippy, workspace all-target tests, `eggbench-cli --no-default-features --test cli`) |
| `linux-msrv` | success (Rust 1.89.0 checks plus `eggbench-core`/`eggbench-drivers` tests) |
| `macos-stable` | success (all-feature Clippy, workspace all-target tests, process-group cleanup, symlink confinement) |
| `windows-stable` | success (all-feature Clippy `-D warnings`, core contract tests, `platform` suite, `eggstack_path` suite) |

This is the direct comparison against the red run `36607614262` that triggered
the corrective.

### 5.2 Live qualification — run `36640125453` (green, four jobs)

| Job | Result |
|---|---|
| `live-tools-linux` | success |
| `live-eggsec-linux` | success |
| `live-m004b-linux` | success |
| `live-synvoid-linux` | success, `pass=25 stopped=0 notexec=5` |

`live-synvoid-linux` executed the real Stage B/C stages on the corrective head
rather than NOT-EXECUTED: pinned SynVoid revision `ae045481…`, closed upstream
asset contract, minimal release `--no-default-features` build, owner export
materialization, export `check` + `configtest`, pinned provenance, real
workspace construction, real positive correctness, blocked requests never
reaching the origin, real negative mutation failing with exit 6, teardown
leaving no children, and the real Stage C smoke/same-source scenarios. The five
NOT-EXECUTED entries are the optional oha/h2load oracles and the Gregg host
probe absent from the hosted runner, which C002 records as permitted optional
absence.

## 6. Downstream effect

- M002 corrective C002 is no longer gated on this repair. Its exact-head
  four-lane (`36640125422`) and live (`36640125453`) evidence is green, so C002
  performed its terminal roadmap/registry reconciliation at
  `plans/closure/security-qualification-m002-corrective/002-status.md`.
  C002's frozen thresholds were not re-run or weakened.
- M003c's terminal closure precondition ("close on a source revision whose
  normal hosted CI is green") is now satisfiable. Its own live/current-head
  evidence gates remain.
- M003d profile, baseline, live, and negative-demonstration work is unblocked
  and remains gated only on terminal M003c and terminal M002 C002 closure.

## 7. Known limitations

- The Windows `unused_mut` regression is guarded by the cfg-specific builder
  shape plus the hosted `windows-stable` Clippy lane, not by a committed
  cross-target test. No local non-Unix target is buildable on this host.
- Hosted `live-synvoid-linux` still reports oha/h2load/Gregg as NOT-EXECUTED
  because those tools are absent from the runner. That is a harness/environment
  fact, not a regression, and C002's own record carries the local oracle proof.

## 8. Unresolved findings

None owned by this corrective. No stop condition in plan §13 was reached: the
descriptor count was the only accounting drift, non-Unix builds need no scope
confidentiality change, the platform behavior is identical, the four-lane rerun
exposed no different semantic failure, and C002 live proof did not regress.
