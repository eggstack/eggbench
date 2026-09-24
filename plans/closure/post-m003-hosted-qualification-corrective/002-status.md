# Post-M003 Hosted Qualification Corrective C002 — Closure Record

Disposition: **closed**. Combined post-M003 hosted qualification: **closed**.

This record is additive qualification evidence for the stopped C001 sequence.
C001 remains stopped historical work; no C001 success record is created here.

## 1. Lineage

- C001 implementation: `808c35f` (`fix(qualification): repair current-tip CI portability defects (C001)`)
- C001 stop record: `327eaab` (plans: record C001 §20 stop after rerun `36017662684`)
- First hosted red run: `36014465034` (C001 trigger)
- C001 rerun (red, stopped): `36017662684` — three C001 defects fixed, all-feature
  Clippy failed on pre-existing `eggbench-drivers` debt (lib 20/21 errors,
  lib test 32/33 incl. lib; stable rustc 1.98.1)
- C002 implementation:
  - `a8fbcea` (`fix(qualification): clear driver stable-Clippy debt and masked workspace lints (C002)`)
  - `0e32ff0` (`fix(qualification): enforce LF goldens for Windows checkout (C002)`)
- Intermediate hosted run on `a8fbcea`: `36028965856` — Linux stable / Linux 1.89 /
  macOS stable green; Windows stable failed only on two core golden tests
  (CRLF checkout conversion, no semantic difference; §6)
- Final hosted qualification run on `0e32ff0`: `36029547565` — all four lanes green (§7)

## 2. Complete C002 stable-Clippy inventory (before)

Local stable rustc/Clippy 1.98.1, `eggbench-drivers`:

All-features `--all-targets` lib (20 errors):

| Lint | Locations |
|---|---|
| `map_unwrap_or` | `external/common.rs:47` |
| `needless_pass_by_value` | `external/common.rs:93` (`failure_category`) |
| `missing_fields_in_debug` | `external/h2load.rs:171`, `external/iperf3.rs:132`, `external/oha.rs:139` |
| `doc_markdown` | `external/h2load.rs:227`, `external/oha.rs:197` (`OpenLoop`) |
| `manual_is_multiple_of` | `external/h2load.rs:305`, `external/oha.rs:313` |
| `cast_precision_loss` | `external/h2load.rs:308` (duration float), `external/h2load.rs:546` ×2 (error-rate ratio), `external/iperf3.rs:337,351` (`u64::MAX as f64` bounds), `external/iperf3.rs:372` (count closure) |
| `map_identity` | `external/h2load.rs:462` |
| `redundant_closure_for_method_calls` | `external/iperf3.rs:209,217` |
| `manual_div_ceil` | `external/iperf3.rs:249` |
| `uninlined_format_args` | `external/oha.rs:305` |

Windows-only: `needless_return` at `external/resolver.rs:279` (cfg(windows) branch;
verified fixed via `--target x86_64-pc-windows-msvc` Clippy, green).

Lib-test-only (12 errors, all `float_cmp`): `h2load.rs:691-693,700,721`;
`iperf3.rs:517-518`; `oha.rs:634-635,637,673,682`.

Default-features additionally exposed `vec_init_then_push` at
`src/catalog.rs:48` (cfg-gated extends vanish without features, leaving
`Vec::new` + pushes). No new warning indicated a behavioral bug; all findings
were style/representation debt, so no planning-review stop was needed.

## 3. Newly exposed debt after the driver crate cleared (same masking class)

Cargo stops the Clippy unit graph at the first failing crate, so clearing
`eggbench-drivers` exposed pre-existing stable-Clippy debt that prior runs
never reached. All of it predates C002 (last touched by the M002/M003/oracle/
Eggstack milestone commits, never by C001/C002) and all fixes are
behaviorally neutral:

- `eggbench-cli` lib: `needless_borrow` (`commands/compare.rs:129`,
  `&&ComparisonReceipt` passed where `&` was meant); `result_large_err` on
  private `parse_workload_driver` in `commands/doctor.rs:55` and
  `commands/run.rs:48` plus their closures (400-byte `PresentedCommandResult`
  boxed at the private boundary; callers unbox with `Ok(*presented)`);
  `too_many_arguments` on private `run_impl` (`commands/run.rs:179`, 8 args
  grouped into a `RunPlan` triple; both call sites updated).
- `eggbench-cli` all-feature tests: `doc_markdown` (`tests/cli.rs:1510`, `EggServe`).
- `eggbench-drivers` integration tests: `useless_conversion`
  (`tests/substrate.rs:231`); `float_cmp` ×4 (`tests/oracles.rs:244,285,315,343`,
  live `error_rate` assertions now compare `to_bits`); `used_underscore_binding`
  (`tests/oracles.rs:360,385`, guard renamed since it is explicitly dropped).

Hosted run `36028965856` then proved Linux/macOS/MSRV green including Clippy,
and Windows check + Clippy green, but Windows core tests failed on
`comparison::tests::golden_receipts_are_stable` and
`paired_golden_receipts_are_stable`. Local CRLF reproduction (convert golden
JSONs to CRLF, rerun) produced the identical failure signature at
`comparison.rs:3056` with byte-identical JSON modulo `\r`: the golden files had
no `.gitattributes` eol protection, and Windows fresh checkouts convert LF to
CRLF under the runner image default. Commit `0e32ff0` enforces
`crates/eggbench-core/tests/golden/*.json text eol=lf` (same rationale class as
the existing `*.eggb` rule). No golden content changed; the test stays
byte-exact.

## 4. Mechanical versus numerically sensitive fixes

Mechanical (output bytes provably unchanged): `map_or`, identity-map removal,
method-reference closures, doc backticks, inline format captures,
`is_multiple_of` (MSRV 1.89 verified), `needless_return` rewrite (`.com` →
`windows-com`, other executables → `windows-exe`, non-Windows →
`unix-executable`), `vec` tail-`extend`, `needless_borrow`, error boxing,
`RunPlan` grouping, `into_iter` removal, guard rename, LF enforcement.

Numerically sensitive (equivalence-tested, semantics preserved):

- `failure_category(&DriverError)` / `probe_failure_category(&DriverError, …)`
  borrow instead of consume; callers borrow temporaries and still drop the
  original error afterwards.
- `H2loadWorkload`/`OhaWorkload`/`Iperf3Workload` Debug keeps driver + version
  fields and terminates with `finish_non_exhaustive`; executable paths/digests
  stay redacted.
- h2load `duration_secs` uses integer quotient/remainder with the same
  trim-trailing-zeros rule (`1500ms` → `1.5`, matching pre-C002 float output).
- iperf3 `duration_secs` uses `div_ceil(1000).max(1)` (identical over the
  accepted domain; additionally avoids `ms + 999` overflow at `u64::MAX`).
- `metric_u64_as_f64` centralizes the TrialMetrics-v1 `u64 → f64` boundary with
  exact historical `as f64` semantics.
- iperf3 parser compares against named `U64_MAX_AS_F64 = 18_446_744_073_709_551_616.0`
  (bit-equal to pre-C002 `u64::MAX as f64`) with the same `>` operator and the
  existing narrow truncation/sign-loss allowances retained.
- All `float_cmp` sites (unit + integration) compare `to_bits`, preserving
  strict IEEE-754 equality.

## 5. Targeted lint allowances (exact)

1. `crates/eggbench-drivers/src/external/common.rs` — `#[allow(clippy::cast_precision_loss)]`
   on `metric_u64_as_f64` only. This is the one representation-boundary allowance
   authorized by the plan; it documents that v1 metrics are `f64`.
2. Test-only `#[allow(clippy::cast_precision_loss)]` on
   `metric_u64_as_f64_preserves_historical_cast_semantics` (common.rs tests) and
   `u64_max_constant_matches_historical_cast` (iperf3.rs tests), required to lock
   the historical `as f64` bits in assertions.
3. Retained pre-existing `#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]`
   on the iperf3 `f64 → u64` casts (preconditions preserved).
4. Removed the now-unneeded `#[allow(unused_mut)]` in `catalog.rs`.

No crate-level, module-level, or workspace lint suppression was introduced;
workspace pedantic lints and `-D warnings` are untouched.

## 6. Changed files

`a8fbcea` (12 files, +389/−91): `eggbench-drivers` `catalog.rs`,
`external/common.rs`, `external/h2load.rs`, `external/iperf3.rs`,
`external/oha.rs`, `external/resolver.rs`, `tests/oracles.rs`,
`tests/substrate.rs`; `eggbench-cli` `commands/compare.rs`, `commands/doctor.rs`,
`commands/run.rs`, `tests/cli.rs`. `0e32ff0` (1 file, +4): `.gitattributes`.
No `Cargo.toml`/`Cargo.lock`, schema, or dependency change (`cargo tree --locked`
clean, `git diff --check` clean).

## 7. Verification evidence

Local stable 1.98.1 (all with `--locked`):

- `cargo fmt --all -- --check`: clean.
- Drivers Clippy default + all-features `-D warnings`: green.
- Workspace `check` default + all-features: green (stable and `+1.89.0`).
- Workspace Clippy default + all-features `-D warnings`: green.
- Workspace tests all-features (sentinel set): **349 passed / 0 failed**.
- Workspace tests default (sentinel set): **309 passed / 0 failed**.
  (One full-default run showed 4 live-oracle loopback failures from parallel-load
  server-startup flakiness; isolated rerun 11/11 green and full rerun 309/0 green.)
- MSRV 1.89: `check` default + all-features green; `test -p eggbench-core
  --all-features` **90 passed**; `test -p eggbench-drivers --all-features`
  **99 passed** (46 lib + 15 eggstack + 13 gregg + 11 oracles + 14 substrate).
- Windows cross-check from Linux (`--target x86_64-pc-windows-msvc`):
  drivers `check` and all-feature Clippy green.
- Oracle semantic-equivalence coverage: existing argv/parser/metric fixture
  suites all green plus new regression tests — failure-category mapping (3),
  Debug redaction (3), h2load/oha/iperf3 duration tables, `u64→f64` boundary
  (`0/1/counts/2^53/2^53+1`), iperf3 byte-counter boundary
  (`0.0`/integers/truncation/upper-bound/next-above/negative/NaN/infinity).

Hosted CI (`.github/workflows/ci.yml`, same four lanes as C001):

- Run `36028965856` on `a8fbcea`: Linux stable success; Linux 1.89 success;
  macOS stable success; Windows stable failure only at core golden tests
  (CRLF checkout conversion, §3). Check + Clippy passed on all lanes,
  including Windows.
- Run `36029547565` on `0e32ff0`: **Linux stable success; Linux 1.89 success;
  macOS stable success; Windows stable success.** Post-Clippy workspace tests
  executed and passed on Linux/macOS; Windows core + platform subsets passed.

## 8. Requirement-to-evidence matrix (plan acceptance criteria 1–20)

1. Driver lint inventory recorded — §2 above + `/tmp` Clippy transcripts.
2. Drivers default Clippy green — local + hosted Linux/macOS/Windows Clippy steps.
3. Drivers all-feature Clippy green — same.
4. Workspace default Clippy green — local + hosted.
5. Workspace all-feature Clippy green — local + hosted.
6. Debug redaction intact — 3 redaction tests; no identity fields emitted.
7–9. oha/h2load/iperf3 argv/parser/metric fixtures unchanged — fixture suites
   green; semantic-equivalence tests added; live loopback suites green locally.
10. Duration mappings locked — h2load/oha/iperf3 duration-table tests.
11. Integer/`f64` representation tested — `metric_u64_as_f64` + `U64_MAX_AS_F64` tests.
12. No parser boundary change without stop — boundaries bit-preserved and tested.
13–16. Four hosted lanes pass — run `36029547565`, all `success`.
17. Stable jobs execute post-Clippy tests — Linux/macOS workspace tests,
   Windows core + platform tests all ran green.
18. No new dependency/schema change — file list (§6), `cargo tree`, MSRV intact.
19. Closure + registry reconciliation committed — this file plus the
   reconciliation commit.
20. No unresolved correctness/security/portability finding — §9.

## 9. Unresolved findings / limitations

- Severity-bearing findings: **none**.
- Windows/macOS verification is hosted-only for execution (local Windows
  coverage is cross-target check + Clippy; no local Windows test execution
  exists in this environment).
- Live oracle loopback tests require installed `oha`/`h2load`/`iperf3` +
  `python3`; they ran green locally and skip gracefully when absent. Hosted
  lanes do not install third-party tools, so hosted qualification covers
  fixture/parser plus absence-safe paths, as in the original milestone closures.
- One full-default local run showed parallel-load flakiness in live loopback
  server startup (4 failures, `wait_for_tcp` timeouts); full rerun and isolated
  rerun were green. No code defect; no retry logic added (out of scope).

## 10. Qualification disposition

- Measurement M002 (`80ff6d1`): **hosted-qualified** by run `36029547565`.
- Measurement M003 (`49a4105`): **fully closed / hosted-qualified** by run
  `36029547565` (promoted from conditionally closed).
- External Oracles M001 (`7afa054`) / M002 (`3384a89`): **hosted-qualified**;
  C002 recorded as lint/qualification corrective only.
- Eggstack M001a (`8426e08`) / M001b (`a0ff206`): **current integrated
  build/test state hosted-qualified**; M002 implementation unblocked subject to
  its own authored implementation plan. No M002 plan is invented here.
- Historical closure records stand unchanged. C001 stays stopped.
