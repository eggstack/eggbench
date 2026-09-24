# Post-M003 Hosted Qualification Corrective C002 — Driver Stable-Clippy Debt and Final Hosted Qualification

Status: closed — implementation `a8fbcea` + `0e32ff0`; four-lane green hosted run `36029547565`; see plans/closure/post-m003-hosted-qualification-corrective/002-status.md

Repository baseline: 327eaab513b162897388b0e7ca15565b052feb4e

Source corrective:

- plans/subsystems/post-m003-hosted-qualification-corrective-addendum.md

Predecessor corrective:

- plans/implementation/post-m003-hosted-qualification-corrective/001-current-tip-ci-and-closure-reconciliation.md
- C001 implementation: 808c35f23b941495aad6ee17ee62015957fd7f19
- C001 stop record: 327eaab513b162897388b0e7ca15565b052feb4e

Hosted evidence:

- initial red run: 36014465034
- C001 rerun: 36017662684
  - Linux Rust 1.89: pass
  - Linux stable: check pass, all-feature Clippy fail
  - macOS stable: check pass, all-feature Clippy fail
  - Windows stable: check pass, all-feature Clippy fail

Primary class: closure qualification / driver code hygiene.

## 1. Objective

Clear the stable-Clippy debt exposed in eggbench-drivers by C001's hosted rerun without changing External Oracles M001/M002 behavior, emitted workload semantics, parser acceptance, normalized metric meaning, driver capability matrices, or evidence schemas.

Then rerun the same four-lane hosted matrix and use the resulting green run as the additive hosted qualification for:

- Measurement M002/M003;
- External Oracles M001/M002;
- Eggstack M001a/M001b;
- the C001 portability fixes.

C002 is a follow-up corrective, not External Oracles M003 and not a feature milestone.

## 2. Why a separate C002 is required

C001 deliberately froze External Oracles adapter/parser code. Its stop condition required planning review if the hosted rerun exposed failures inside those frozen contracts.

Run 36017662684 did exactly that: the original C001 defects are fixed, but stable Clippy 1.98 reports pre-existing warnings-as-errors in eggbench-drivers.

C002 therefore authorizes narrowly bounded edits to the affected driver implementation while preserving behavior through equivalence tests.

## 3. Observed lint inventory

Linux/macOS stable report the following Clippy classes in eggbench-drivers:

- map_unwrap_or;
- needless_pass_by_value;
- missing_fields_in_debug;
- doc_markdown;
- manual_is_multiple_of;
- cast_precision_loss;
- map_identity;
- redundant_closure_for_method_calls;
- manual_div_ceil;
- uninlined_format_args;
- float_cmp.

Windows reports the same set plus:

- needless_return in the Windows branch of executable classification.

Observed affected files include:

- crates/eggbench-drivers/src/external/common.rs;
- crates/eggbench-drivers/src/external/h2load.rs;
- crates/eggbench-drivers/src/external/oha.rs;
- crates/eggbench-drivers/src/external/iperf3.rs;
- crates/eggbench-drivers/src/external/resolver.rs on Windows.

The implementation agent MUST re-run Clippy and inventory the complete current set before editing. The list above is controlling evidence, not permission to ignore additional warnings in the same crate/configuration.

## 4. Corrective invariants

1. oha/h2load/iperf3 argv produced for every currently supported workload shape remains semantically identical.
2. Driver capability descriptors remain identical.
3. Minimum-version policy remains identical.
4. Parser accepted/rejected fixture classes remain identical.
5. Raw artifact names/content policy remain identical.
6. Metric names, units, aggregations, source fields, and values remain identical for existing fixtures.
7. No workload load-model fallback is added.
8. Missing binaries/version failures still fail before managed startup.
9. Debug output remains redaction-safe and MUST NOT begin exposing executable paths/digests merely to satisfy missing_fields_in_debug.
10. No plan/evidence/receipt schema changes.
11. No dependency additions.
12. Rust 1.89 remains the MSRV.
13. No crate/module-wide Clippy allow is introduced.
14. Any targeted lint allow must document why the representation boundary makes the cast intentional and must be narrower than a module.

## 5. Non-goals

Do not:

- add new oracle features;
- add netem;
- change supported oha/h2load/iperf3 versions;
- change command-line option mappings;
- make h2load parsing more permissive or restrictive;
- change iperf3 float-to-integer truncation/range semantics unless a current behavior cannot be preserved safely;
- change normalized metric schema from f64;
- introduce decimal/big-number dependencies;
- expose executable identity through Debug;
- change External Oracles closure history;
- touch Measurement M003 semantics;
- start Eggstack M002 implementation;
- weaken workspace pedantic lints or -D warnings.

## 6. Work package A — Reproduce and freeze the lint inventory

Before editing, run:

~~~text
cargo +stable clippy -p eggbench-drivers --all-targets --all-features --locked -- -D warnings
cargo +stable clippy -p eggbench-drivers --all-targets --locked -- -D warnings
~~~

Record:

- stable rustc/Clippy version;
- every lint category;
- every file/location;
- whether each finding is library, test-only, feature-gated, or Windows-only.

Compare the inventory with hosted run 36017662684.

If a new warning indicates a behavioral bug rather than style/representation debt, stop for planning review instead of silently folding it into C002.

## 7. Work package B — Mechanical no-semantic-change Clippy fixes

Fix map_unwrap_or with map_or/equivalent while preserving evaluation behavior.

Remove identity map/map_err operations.

Replace redundant closures with method references where inference remains explicit.

Backtick domain identifiers such as OpenLoop in documentation.

Use inline format captures only where output bytes remain identical.

Use is_multiple_of and div_ceil only after confirming Rust 1.89 support for the concrete integer type.

Rewrite resolver.rs classify Windows control flow to remove needless_return while preserving:

- .com -> windows-com;
- every other accepted Windows executable -> windows-exe;
- non-Windows -> unix-executable.

No platform policy change.

## 8. Work package C — Error-category borrowing without lifetime/API drift

Current shared helper takes a DriverError by value even though it inspects only the category.

Preferred correction:

~~~text
failure_category(&DriverError) -> FailureCategory
~~~

and similarly pass by reference from probe_failure_category where practical.

Requirements:

- callers may still consume/drop the original error after categorization;
- cancellation/timed-out classification remains exact;
- no error detail is accidentally retained longer or exposed;
- public surface changes stay crate-internal unless already required across modules.

Regression tests must cover:

- Cancelled -> Cancelled;
- TimedOut -> TimedOut;
- parse/version/spawn/resolution -> WorkloadFailed;
- cancelled token during probe -> Cancelled.

## 9. Work package D — Preserve Debug redaction

Stable Clippy reports missing_fields_in_debug on manual Debug impls because the executable field is intentionally omitted.

Do NOT fix this by printing ResolvedExecutable, selected/canonical paths, SHA-256, or other identity fields.

Preferred correction:

- retain current driver/version fields;
- terminate debug builders with finish_non_exhaustive or another Clippy-clean pattern that explicitly communicates omission.

Apply consistently to OhaWorkload, H2loadWorkload, Iperf3Workload, and any equivalent affected adapter.

Add tests proving Debug output:

- contains driver name;
- contains version state where currently present;
- does not contain selected path;
- does not contain canonical path;
- does not contain executable SHA-256.

## 10. Work package E — Duration formatting without floating-point conversion

Duration strings are command syntax, not floating-point measurements. Avoid u64-as-f64 formatting.

For h2load/oha helpers that currently format millisecond durations through floating-point division:

- use integer quotient/remainder;
- preserve exact current textual output.

Lock examples where applicable:

| ms | expected existing text |
|---:|---|
| 1 | 0.001 |
| 999 | 0.999 |
| 1000 | 1 if current exact-second branch emits 1 |
| 1001 | 1.001 |
| 1500 | 1.500 |
| 2501 | 2.501 |

Use pre-C002 behavior as authority. Do not trim trailing fractional zeroes if the current adapter emitted them.

For iperf3 whole-second ceiling:

- use duration_ms.div_ceil(1000).max(1);
- prove argv output unchanged for the accepted duration domain.

Do not use a float merely to silence a lint.

## 11. Work package F — Intentional integer-to-f64 metric boundary

RawMetricObservation v1 stores numeric values as f64. Some source values are integer counts/bytes, so a conversion is intrinsic to the existing schema.

C002 MUST preserve current cast semantics rather than invent a new metric schema.

Preferred approach:

1. centralize conversion in a tiny helper such as metric_u64_as_f64;
2. document that v1 metric representation is f64;
3. place a function-local allow for clippy::cast_precision_loss on this helper only;
4. use the helper for h2load count ratios/values and iperf3 byte/retransmit observations as needed.

This is the one explicitly authorized representation-boundary lint allowance.

Do not add crate-wide, module-wide, or workspace lint suppression.

Regression tests must lock exact previous Rust as-f64 behavior for:

- 0;
- 1;
- a realistic count;
- 2^53;
- 2^53 + 1.

The last case intentionally demonstrates the existing f64 representation limitation rather than hiding it.

## 12. Work package G — Preserve iperf3 f64-to-u64 parser behavior

iperf3 JSON represents byte/retransmit counters as JSON numbers deserialized to f64. Current code rejects nonfinite/negative values, compares against u64::MAX as f64, then casts with existing narrow truncation/sign-loss allowances.

The u64::MAX as f64 comparison itself triggers cast_precision_loss.

Do not casually tighten or loosen the accepted boundary.

Preferred correction:

- define a named f64 constant equal to the exact rounded value currently produced by u64::MAX as f64;
- compare against that constant with the same operator currently used;
- retain existing cast semantics and existing narrow cast allowances.

Tests must prove parity around:

- 0.0;
- ordinary integer values;
- non-integral positive values if currently accepted/truncated;
- current rounded upper-bound value;
- next representable value above that bound;
- negative/NaN/infinity rejection.

If exact behavioral parity cannot be expressed clearly, stop for planning review instead of silently changing parser acceptance.

## 13. Work package H — Floating-point parser tests

Stable Clippy rejects strict assert_eq on f64.

Do not weaken parser tests to vague tolerances by default.

For values where current tests require exact IEEE-754 equality, compare to_bits:

~~~text
assert_eq!(actual.to_bits(), expected.to_bits())
~~~

This preserves the strictness of the prior assertion without float_cmp.

For genuinely arithmetic/approximate values, use a small test-only tolerance helper with an explicitly chosen tolerance and document why exact bits are not the contract.

Use bit equality for existing literal/parsed golden values unless there is a concrete reason not to.

Apply to h2load/oha/iperf3 tests found by Clippy.

## 14. Work package I — Oracle semantic equivalence matrix

Before and after changes, lock existing fixture behavior for each driver.

### oha

Verify supported argv for finite count, duration, and open-loop rate; duration text; requests/sec; success/error rates; latency percentiles; status/error counts; malformed/truncated JSON rejection; version floor behavior.

### h2load

Verify count-bound and duration-bound argv; H1 selection; duration text; anchored parser rows; request rate; latency min/mean and existing percentile subset; failed/errored/timeout counts; malformed/truncated text rejection; version floor behavior.

### iperf3

Verify duration ceiling and -P mapping; default/explicit port; sent/received bps; sent/received bytes; retransmits; top-level error rejection; malformed/missing-section rejection; version floor behavior.

Where feasible, capture a compact test-only semantic snapshot/DTO and compare expected values. Do not change production evidence schemas to create a test snapshot.

## 15. Work package J — Feature and platform matrix

Run locally where supported:

### Driver-only

~~~text
cargo +stable check -p eggbench-drivers --all-targets --locked
cargo +stable clippy -p eggbench-drivers --all-targets --locked -- -D warnings
cargo +stable check -p eggbench-drivers --all-targets --all-features --locked
cargo +stable clippy -p eggbench-drivers --all-targets --all-features --locked -- -D warnings
cargo +stable test -p eggbench-drivers --all-targets --all-features --locked
~~~

### Workspace

~~~text
cargo fmt --all -- --check
cargo +stable check --workspace --all-targets --locked
cargo +stable check --workspace --all-targets --all-features --locked
cargo +stable clippy --workspace --all-targets --locked -- -D warnings
cargo +stable clippy --workspace --all-targets --all-features --locked -- -D warnings
EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo +stable test --workspace --all-targets --all-features --locked
cargo +stable test --workspace --all-targets --locked
~~~

### MSRV

~~~text
cargo +1.89.0 check --workspace --all-targets --locked
cargo +1.89.0 check --workspace --all-targets --all-features --locked
cargo +1.89.0 test -p eggbench-core --all-features --locked
cargo +1.89.0 test -p eggbench-drivers --all-features --locked
cargo tree --locked
git diff --check
~~~

No new dependency is expected.

## 16. Work package K — Cross-platform compile concerns

The C001 rerun proved all three stable platforms pass cargo check. C002 must preserve that.

Pay special attention to:

- resolver.rs classify Windows cfg branch;
- any integer helper introduced in C002;
- Debug tests using platform paths;
- no Unix-only fixture leaking into Windows compilation.

Do not broaden Windows external-process ownership claims.

## 17. Work package L — Fresh hosted qualification

After local/default/all-feature/MSRV checks are green, push C002 and require a fresh CI run on that exact implementation commit.

Required jobs on the same commit:

- Linux stable — pass;
- Linux Rust 1.89 — pass;
- macOS stable — pass;
- Windows stable — pass.

Stable jobs must get past Clippy and execute remaining workspace/platform tests previously skipped.

A rerun of 36017662684 is not sufficient because code changes are required.

## 18. Work package M — Final corrective closure

After four-lane green hosted CI, create:

plans/closure/post-m003-hosted-qualification-corrective/002-status.md

The closure MUST record:

- C001 implementation 808c35f;
- C001 stop record 327eaab;
- failed run 36017662684;
- complete C002 stable-Clippy inventory;
- implementation commit(s);
- mechanical versus numerically sensitive fixes;
- exact targeted lint allowances and justification;
- oracle semantic-equivalence tests;
- default/all-feature test counts;
- Rust 1.89 results;
- dependency tree result;
- new hosted run ID;
- all four job conclusions;
- whether any new failures appeared after Clippy;
- unresolved findings/severity;
- final disposition.

C002 closure is also additive closure evidence for the stopped C001 sequence. Do not create a false C001 success record.

## 19. Work package N — Planning reconciliation after green CI

Only after C002 hosted CI is green:

### Combined corrective

Record C001 as stopped after fixing its original defects, C002 as closed, and the combined post-M003 hosted qualification as closed.

### Measurement/comparison

Mark M002 hosted-qualified by the C002 current-tip run and M003 fully closed/hosted-qualified. Preserve historical closure records.

### External Oracles

Mark M001/M002 hosted-qualified and record C002 as lint/qualification corrective. M003 netem remains future.

### Eggstack

Mark M001a/M001b current integrated build/test state hosted-qualified. M002 implementation becomes unblocked subject to its own authored implementation plan.

### Registry

Remove stale C001-as-next-handoff language, mark C002 closed, restore truthful closed/qualified statuses, and do not invent an Eggstack M002 implementation plan if none exists.

## 20. Static guards

C002 must not introduce:

- crate-level allow(clippy::...);
- module-level broad lint allows;
- workspace lint downgrades;
- a dependency solely to avoid numeric casts;
- Debug output of executable paths/digests;
- schema/version changes.

A targeted function-level cast_precision_loss allow is permitted only for the documented u64 -> f64 TrialMetrics-v1 representation boundary.

Existing targeted cast_possible_truncation/cast_sign_loss allowances in the validated iperf3 f64 -> u64 path may remain if their preconditions are preserved.

## 21. Acceptance criteria

C002 closes only when:

1. complete driver lint inventory is recorded;
2. eggbench-drivers default Clippy is green;
3. eggbench-drivers all-feature Clippy is green;
4. workspace default Clippy is green;
5. workspace all-feature Clippy is green;
6. Debug redaction remains intact;
7. oha argv/parser/metric fixtures are behaviorally unchanged;
8. h2load argv/parser/metric fixtures are behaviorally unchanged;
9. iperf3 argv/parser/metric fixtures are behaviorally unchanged;
10. duration textual mappings are locked;
11. integer/f64 representation behavior is explicitly tested;
12. no parser boundary changes without a stop/replan;
13. Linux stable hosted CI passes;
14. Linux 1.89 hosted CI passes;
15. macOS stable hosted CI passes;
16. Windows stable hosted CI passes;
17. stable jobs execute post-Clippy tests successfully;
18. no new dependency/schema change lands;
19. C002 closure and registry reconciliation are committed;
20. no unresolved correctness/security/portability finding remains.

## 22. Stop conditions

Stop for planning review if:

- satisfying Clippy requires changing an oracle's supported workload mapping;
- a parser's accepted/rejected source-data domain must change;
- emitted metric values change for existing fixtures;
- h2load/oha duration argv text cannot be preserved;
- Debug compliance appears to require exposing executable identity;
- Rust 1.89 lacks a required warning-free integer API and no simple equivalent exists;
- hosted CI exposes a substantive runtime/lifecycle/parser failure after Clippy is cleared;
- a schema or dependency change appears necessary.

A newly exposed mechanical lint in the same driver crate may be folded into C002 if behaviorally neutral and recorded in closure. A correctness defect may not.

## 23. Closure evidence required

Record:

- implementation SHA(s);
- stable/Clippy version;
- full before lint inventory;
- full after lint result;
- changed files;
- Debug redaction regression;
- duration mapping table results;
- integer/f64 conversion boundary results;
- oracle semantic-equivalence fixture results;
- local driver/workspace test counts;
- MSRV results;
- dependency tree;
- failed run 36017662684;
- fresh green hosted run ID;
- four job conclusions;
- combined qualification disposition;
- planning reconciliation commit;
- unresolved findings/severity;
- final disposition.
