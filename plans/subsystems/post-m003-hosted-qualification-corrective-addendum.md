# Post-M003 Combined Hosted Qualification Corrective Addendum

Status: closed — C001 stopped after fixing its original defects; C002 closed by four-lane green hosted run 36029547565 (see plans/closure/post-m003-hosted-qualification-corrective/002-status.md)

Repository audit baseline: `5ffe87ba9bb352822f85b7780cd15745097dc230`

Hosted qualification trigger:

- GitHub Actions CI run `36014465034`
- Linux stable: failed
- Linux Rust 1.89 MSRV: passed
- macOS stable: failed
- Windows stable: failed

Historical implementation/closure records covered by this combined qualification gate:

- Measurement M002:
  - implementation `80ff6d14ecb9f997278bc4efbb6edb2a26ed0384`
  - closure `plans/closure/measurement-comparison/002-status.md`
- External Oracles M001:
  - implementation `7afa054ce908972907144a935cdbab21f29668ea`
  - closure `plans/closure/external-oracles/001-status.md`
- Eggstack M001a:
  - implementation `8426e08cea9f9d534eef212f83501a4cffd99312`
  - closure `plans/closure/eggstack-integration/001a-status.md`
- Eggstack M001b:
  - implementation `a0ff20686926bc57b64cdf4c277199010c1abb2b`
  - closure `plans/closure/eggstack-integration/001b-status.md`
- External Oracles M002:
  - implementation `3384a89c997fd58f9ced7b571e6225f6bfb85397`
  - closure `plans/closure/external-oracles/002-status.md`
- Measurement M003:
  - implementation `49a41053e960a46b8da731c2163774938db850e8`
  - closure `plans/closure/measurement-comparison/003-status.md`

Controlling governance:

- `plans/003-planning-process.md#25-closure-records`
- `plans/003-planning-process.md#8-corrective-passes`

## 1. Corrective trigger

The implementation sequence advanced through Measurement M002, External Oracles M001, Eggstack M001a/M001b, External Oracles M002, and Measurement M003 using strong local verification. Their closure records correctly disclose that hosted four-lane qualification had not yet run.

The first current-tip hosted qualification, CI run `36014465034`, exposed three narrow repository defects:

1. **Measurement M003 stable-Clippy failure on Linux/macOS**
   - `crates/eggbench-runner/src/orchestration.rs:1840`
   - current code:
     `(arm, (number + 1) / 2)`
   - current stable Clippy reports `clippy::manual_div_ceil` under `-D warnings`.
   - intended correction: `number.div_ceil(2)`, preserving the exact 1-based pair-id mapping.

2. **Windows all-target compile regression from ResolvedPlan schema v2**
   - `crates/eggbench-runner/tests/platform.rs:25`
   - the Windows-only fixture directly constructs `ResolvedPlan` but was not updated with the additive `paired` field.
   - compiler error: E0063 missing field `paired`.
   - intended correction: `paired: None` for the unpaired platform fixture.

3. **Feature-configuration warning debt in CLI tests**
   - `crates/eggbench-cli/src/workload_registry.rs:665-666`
   - `expected_workload` and `expected_descriptors` are `mut` only under feature combinations that add adapters.
   - the no-feature/default build reports `unused_mut`.
   - this did not cause the first Clippy failure because runner Clippy stopped earlier, but it is expected to fail the same `-D warnings` lane once the runner lint is fixed unless corrected.

No evidence currently indicates a defect in:

- comparison policy v1/v2 math;
- pair identity semantics;
- driver command execution;
- oha/h2load/iperf3 parsers;
- EggServe/Eggfetch integration semantics;
- Gregg telemetry semantics;
- lifecycle cleanup;
- evidence immutability;
- CLI exit-code contracts.

## 2. Why prior verification missed these defects

The closures record local verification, but the new combined implementation state had not yet been exercised by the full hosted matrix.

Specifically:

- the M003 local stable toolchain did not emit the current hosted stable `manual_div_ceil` lint;
- the Windows-only `platform.rs` fixture is cfg-gated and therefore was not compiled by Unix local verification;
- feature-dependent `mut` usage is configuration-sensitive and can remain useful under all-features while warning in the default/no-feature build.

This is exactly the class of portability/configuration defect that the four-lane hosted gate exists to catch.

## 3. Work classification

Primary class: closure qualification / portability corrective.

This is not a new capability milestone.

## 4. Frozen contracts

C001 MUST NOT reopen or change:

- ExperimentPlan paired-design semantics;
- ResolvedPlan schema v2 field meanings;
- TrialExecutionResult v2 arm/pair semantics;
- manifest v2;
- comparison receipt v1/v2 schemas;
- comparison policy v1/v2 math;
- bootstrap seeds/resampling behavior;
- CLI comparison exit codes 6/7/8;
- External Oracles driver capabilities or parser semantics;
- Eggstack M001 service/telemetry semantics;
- local runner timing/cleanup behavior;
- driver catalog ownership;
- evidence path/layout semantics.

The corrective may change only test fixtures, lint-equivalent expressions, configuration-safe test code, CI/closure metadata, and any directly exposed portability defect discovered by the required rerun.

## 5. Corrective milestone

### C001 — Current-tip portability fixes and combined hosted qualification

Implementation handoff:

- `plans/implementation/post-m003-hosted-qualification-corrective/001-current-tip-ci-and-closure-reconciliation.md`

Status: implementation attempted at `808c35f`; stopped for planning review — see the §22 stop notice in the implementation plan. Rerun CI `36017662684` (2026-09-24) confirms the three known defects fixed (fmt/check green all lanes, MSRV green) but fails all-feature Clippy on pre-existing `eggbench-drivers` findings in frozen oracle-adapter code (lib 20/21 errors, lib test 32/33 errors; includes `float_cmp` parser goldens, `cast_precision_loss` parser math, and one Windows-only `needless_return` at `resolver.rs:279`). These cannot be folded into C001 per §20; a follow-up corrective must disposition them first.

## 5.1 Follow-up corrective C002

C001 correctly stopped when rerun 36017662684 exposed warnings-as-errors inside the frozen External Oracles adapter code.

Follow-up implementation handoff:

- plans/implementation/post-m003-hosted-qualification-corrective/002-drivers-stable-clippy-and-final-hosted-qualification.md

C002 scope is limited to stable-Clippy cleanup and semantic-equivalence qualification in eggbench-drivers. It explicitly protects oracle argv mappings, parser domains, metric values, Debug redaction, schemas, dependencies, and Rust 1.89.

C002 closed at implementation `a8fbcea` + `0e32ff0`, qualified by hosted run
`36029547565` (Linux stable, Linux 1.89, macOS stable, Windows stable all
green; closure at
`plans/closure/post-m003-hosted-qualification-corrective/002-status.md`).
Beyond the driver lint class, C002 also repaired narrowly masked,
behavior-neutral findings exposed by progress: `eggbench-cli` Clippy debt
(private helpers only), driver/cli test-only lints, and CRLF checkout
conversion of comparison golden files (durable `.gitattributes` LF rule; no
golden content change). C001 remains a stopped historical attempt and must not
be re-executed.

## 6. Qualification disposition

The combined corrective closed through C002 (hosted run `36029547565`):

- Measurement M002 is hosted-qualified;
- Measurement M003 is fully closed/hosted-qualified (promoted from conditionally closed);
- External Oracles M001/M002 are hosted-qualified;
- Eggstack M001a/M001b current integrated build/test state is hosted-qualified;
- the combined current-tip feature set is release-qualified to the extent of the
  hosted build/test matrix (third-party tool execution remains covered by local
  live tests and fixture/parser suites, as in the original closures);
- Eggstack M002 implementation is unblocked subject to its own authored
  implementation plan; External Oracles M003 netem remains future.

Historical closure records remain immutable evidence of the local closure state at the time they were written. C001/C002 supply additive qualification evidence; they do not rewrite history.

## 7. Completion definition

The combined corrective closed when C002 supplied (all met; evidence in the
C002 closure record):

1. C001's three original source/test defects remain corrected;
2. eggbench-drivers default and all-feature Clippy are warning-free;
3. driver semantic-equivalence fixtures prove oracle behavior unchanged;
4. Rust 1.89 remains green;
5. the Windows platform test fixture and driver cfg paths compile/run under the supported subset;
6. fresh CI run `36029547565` on the C002 implementation HEAD passes Linux stable, Linux 1.89, macOS stable, and Windows stable;
7. the corrective closure explicitly records that this run supplies the previously outstanding hosted qualification for the combined M002/M003/oracle/Eggstack implementation state;
8. registry/roadmaps stop claiming unconditional closure where hosted qualification was still outstanding;
9. stale forward-looking registry text for already-closed M001/M002 work is reconciled.

## 8. Dependency disposition

C001 is stopped and is not re-executable. C002 is closed (implementation
`a8fbcea` + `0e32ff0`, hosted run `36029547565`, closure at
`plans/closure/post-m003-hosted-qualification-corrective/002-status.md`).

The final four-lane green qualification is landed. Eggstack M002
route/stream-fault topology may proceed to implementation once its own
implementation plan is authored; External Oracles M003 netem remains the later
milestone.
