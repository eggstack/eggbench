# C001/C002 Closure Status — Architecture Deep-Dive Audit Corrective

Corrective: `plans/subsystems/architecture-deep-dive-corrective-addendum.md`
Implementation:
`plans/implementation/architecture-deep-dive-corrective/001-contract-truthfulness.md`
and `002-evidence-integrity-and-observability.md`

Status: **closing** — C001 closed, C002 implemented with a complete local
verification pass. Two hosted-lane conditions remain open (see "Named
verification conditions"); no hosted CI run is claimed by this record.

## What this milestone is

A post-closure corrective pass triggered by the 21 component deep dives added
in `13eb443`. Those dives surfaced 38 findings; every one was re-verified
against source before entering this plan, because a deep dive is a lead, not a
verdict. The verification changed the plan in three places, recorded in the
addendum rather than quietly dropped:

- `docs/equivalence.md` **does not exist and never has**. The finding that
  motivated a corrective row was reported against a non-existent file, and
  its substance is also wrong: `ComparisonReceipt` already sets
  `#[serde(deny_unknown_fields)]` and `ComparisonRequest` is not serialized.
  No edit made.
- The catalog-versus-CLI driver-name divergence is **refuted** — the two sets
  are identical in all four feature combinations. Only the comment claiming
  otherwise was wrong. C001 fixed the comment; C002 added the guard test.
- The histogram/scalar unresolvable-reference asymmetry is a **stated design
  decision** at `metrics.rs:1028-1029`, not an oversight. C002's original
  item 1.5 proposed changing it; that proposal was withdrawn in favour of
  documenting the reasoning.

## C001 — Contract truthfulness (closed)

Zero behavior change. Every place a document or code comment contradicted the
code is corrected.

Version claims brought current: `ResolvedPlan` v3 → **v6** with v1–v5
readable; comparison receipt v2 → **v4**; `TrialExecutionResult` v1 → **v2**;
`TrialPolicy` timeout allowlist four names → **five** (`telemetry` was already
accepted and defaults to the measurement bound).

Behavioral claims corrected: unconditional workload drain (the "only if the
executor was reached" text described a superseded gate); partial stdout is
**not** staged on cancellation; `Debug` on a command spec is **not** redacted;
`None` working directory **does** inherit the parent cwd; the
`external-command` cargo feature gated nothing; percentile basis points are a
naming convention and are **not** validated against the expected percentile;
`UnsupportedByDriver` is in the vocabulary but unconstructed; manifest
field tolerance is blanket top-level, not a curated additive set; the
unconditional external driver set is **seven**, not three; `eggstack-http`
also registers the HTTP corpus driver; the `qualify` command family is
absent from the README; and the SynVoid v1 upstream asset contract is closed,
not open.

Code comments corrected: five stale schema comments in `comparison.rs`, a
misattached doc/`#[must_use]` block that had been orphaned above
`correctness_section`, the unreachable `StrictSameTestbed` arm (now an
explicit `unreachable!` with its derivation), `execution_status`'s false
"missing in the normalized view" claim, the inverted pipeline order and
wrong entry-point name in `run.rs`, the catalog module doc's under-reported
registration list, the misleading "only catalog-registered drivers resolve"
comment, and the "sibling-neutral" claims in `correctness.rs` and
`diagnostics.rs` (no sibling *types* cross; sibling *evidence vocabulary*
deliberately does).

Dead-but-public API is **documented, not deleted** — `MissingReason::UnsupportedByDriver`,
`HistogramReference::source_metric`, `ErrorCategory::OutputTruncated`, and
`ExternalParseError` each now state at the declaration that they are reserved
and unconstructed. Deletion is an API change, not a corrective.

## C002 — Evidence integrity and observability (landing)

No serialized-schema change. Every fix reuses an existing schema field or
changes only internal behavior.

**Evidence integrity.** Comparison now records a `metric_receipt_truncated`
warning instead of truncating silently, using the existing
`ComparisonReceipt.warnings` field. Per-trial metric semantics are validated
across the whole baseline rather than first-wins, and a baseline whose trials
disagree on unit, direction, or aggregation produces
`baseline_metric_semantics_divergent` — which both callers convert to a
per-metric `Invalid` disposition, the same outcome shape the pre-existing
`metric_semantics_mismatch` path already used, so coherent evidence is
unaffected. `BundleReader::trial_metrics` bounds its read before allocating,
mirroring the manifest's existing correct idiom. The comparison manifest
re-read is delegated to a new `BundleReader::manifest_bytes`, so it inherits
the reader's no-follow path resolution and byte cap instead of using a plain
`File::open` on a joined path with a different limit.

**Observability that could not fire.** `cancelled`/`timed_out` now derive from
the real deadline outcome, and a deadline that elapses during output draining
is no longer reported as a clean success. `cleanup_notes` are computed before
the cancel/timeout early returns, so the Windows `direct_child_only`
orphan-risk note is finally emitted. `join_pipes` returns an explicit cleanup
failure instead of `unwrap_or_default`, which used to turn a drain timeout
into empty stdout with `truncated == false` and a misleading "malformed
output" diagnosis. `CapturedStream::collect`'s dead `limit` parameter is
removed. `workload_entered` — written four times, never read — is deleted.
`FakeDiagnosticExecutor` reports `DiagnosticFailed` instead of
`WorkloadFailed`, which was being serialized verbatim into
`runner-phases.json`. `TelemetryError`'s fields are private so the documented
512-character bound cannot be bypassed by a struct literal. Both
`CorrectnessRegistry::register` and `DiagnosticRegistry::register` now return
`Result` and reject a duplicate, matching the service and telemetry registries;
the four CLI call sites surface a rejection as a preflight failure rather than
silently keeping the second executor.

**Bounds, coverage, and dead code.** The phase-event reservation is the exact
required count (`FIXED_PHASE_EVENTS = 3`, net of the surplus the `3 * trials`
term already carries) rather than one short — the short count could fail
`stage_phase_artifacts` *after* all measurement work. The value is exact
rather than generous on purpose: the bound sizes the phase-artifact size
floor, so over-reserving would reject plans that fit. `is_external_workload`
now derives from one shared list covering all seven registered external
drivers instead of a five-name subset. The catalog gained
`route`/`fault`/`execution_provider`/`by_name` accessors so it can serve every
category it registers. The inert `external-command` feature is removed. The
unreachable path-validation arm is deleted.

## Findings that were fixed by documenting rather than changing code

- `build.rs` returns the first matching `[[package]]`. Currently harmless (the
  lockfile holds one version of every scraped crate), and the ambiguity fix is
  safe, but the `hdrhistogram`/exposition-label portion changes evidence bytes.
  Left to **C003-e**.
- `created_by_version` is now documented as a build version, so the
  reproducibility claim is correctly scoped to a fixed build.

## Newly discovered during implementation

- **C003-n**: a failing diagnostic executor can fail the whole run with an
  evidence error, because the failed record carries an empty
  `producer_version`/`executable_sha256` and `DiagnosticsIndex::validate_contract`
  requires both unless every record is `skipped`. Pre-existing, not caused by
  this corrective.
- **C003-o**: `doctor` does not construct telemetry collectors, so it cannot
  observe the `prometheus-http` field rejection that `run` enforces. This is
  the mechanism behind C003-d and constrains any fix.

## Item-by-item audit of the C002 plan

Every item in
`plans/implementation/architecture-deep-dive-corrective/002-evidence-integrity-and-observability.md`
was re-verified against the source tree, and each was checked for the named
regression test the plan requires. The plan's test obligation is the substance
of this milestone, so the test column is the one that decides closure.

| Item | Implementation | Named regression test |
|---|---|---|
| 1.1 truncation warning | `push_metric_truncation_warning` | `metric_truncation_is_recorded_as_a_receipt_warning`, `metric_set_within_bound_emits_no_truncation_warning` |
| 1.2 per-trial metric semantics | rejects `baseline_metric_semantics_divergent` | `baseline_trials_with_divergent_metric_semantics_are_rejected`, `baseline_trials_with_coherent_metric_semantics_are_accepted` |
| 1.3 bounded artifact read | `read_bounded` | **was missing — added `bounded_read_rejects_past_the_cap_and_accepts_exactly_the_cap`** |
| 1.4 secure manifest re-read | `BundleReader::manifest_bytes`, `read_manifest_bytes` | **was missing — added `manifest_reread_shares_the_open_time_manifest_cap`** |
| 2.1 real deadline flags | deadline-derived, not success-derived | `deadline_during_drain_reports_timed_out_outcome` |
| 2.2 cleanup notes before early returns | `platform_cleanup_notes`, `with_cleanup_notes` | `timed_out_invocation_carries_cleanup_notes`, `cancelled_invocation_carries_cleanup_notes` |
| 2.3 drain timeout is an error | `join_pipes_within`, `join_pipe` | `stalled_drain_is_a_cleanup_failure_not_empty_output`, `failed_drain_task_is_a_cleanup_failure` |
| 2.4 dead `limit` parameter removed | `from_parts` owns the bound | `captured_stream_truncation_follows_total_minus_retained` |
| 2.5 duplicate registration rejected | both registries return `Result` | `duplicate_correctness_source_registration_rejected`, `duplicate_diagnostic_source_registration_rejected` |
| 2.6 diagnostic failure category | `DiagnosticFailed` | `diagnostic_operational_failure_is_not_reported_as_a_workload_failure` |
| 2.7 `TelemetryError` bound | fields private, `new` sole constructor | `telemetry_error_detail_truncates_at_evidence_bound`, `telemetry_error_detail_keeps_short_text_verbatim` |
| 3.1 exact phase bound | `FIXED_PHASE_EVENTS = 3` | `maximal_phase_shape_stays_within_the_reserved_phase_bound` |
| 3.2 `workload_entered` deleted | field and four assignments gone | pre-existing `assert!(workload.drained)` guard, which the plan names as this item's guard |
| 3.3 shared external-driver list | one list drives all three predicates | `external_predicates_agree_over_the_registered_set`, `shared_list_matches_the_registered_external_drivers` |
| 3.4 catalog accessors | `route`, `fault`, `execution_provider`, `by_name` | `every_registered_category_is_reachable_through_its_accessor`, `by_name_resolves_any_category_and_rejects_unknown_names`, `route_and_fault_descriptors_resolve_under_the_path_feature` |
| 3.5 catalog/dispatch guard | `every_catalog_workload_driver_has_a_dispatch_arm` | same-named test in `crates/eggbench-cli/src/workload_registry.rs` |
| 3.6 inert `external-command` feature | absent from the manifest; no `cfg` referent remains in `crates/` | no test — the plan asks for a `cargo check` with and without all features, which the feature matrix covers, plus a source grep |
| 3.7 unreachable path arm | `ends_with(':')` deleted | behavior-preserving deletion of a subsumed predicate; no test required |

### Two items shipped without their required test

1.3 and 1.4 are the two Group 1 items whose entire purpose is a bound that must
hold on adversarial input, and both landed carrying only the code change. 1.3
inlined the bounded read at its call site instead of extracting the
`read_bounded` helper the plan specifies — and specifies it precisely *so the
property is testable without a 256 MiB fixture*. 1.4's `manifest_bytes` had no
negative test at all, so nothing would have caught a regression that restored
the plain `File::open` re-read with its own larger cap.

Both are now covered. The three copies of the bound-then-check idiom — `open`,
`trial_metrics`, and `manifest_bytes` — are collapsed into the single
`read_bounded` helper, so the cap exists once and the two tests exercise that
one implementation. The tests assert rejection past the cap, acceptance at
exactly the cap, and that a trusted capacity hint cannot relax the bound; the
manifest test asserts `open` and the re-read report the *same*
`BoundExceeded` for the same oversized bytes, which is the property item 1.4
was written for.

The guard test this milestone's headline property deserves is the negative one:
it proves the cap is applied by the read rather than by a check after the read,
which is the distinction the plan draws and the one no positive-path test in
the existing suite could observe.

## Verification actually run

Local, Linux only, at the implementation commit:

| Check | Result |
|---|---|
| `cargo check --workspace --all-targets --locked` | clean, no warnings |
| `cargo test -p eggbench-core --lib` | 157 passed, 0 failed (includes 4 new) |
| `cargo test -p eggbench-runner` | 128 passed, 0 failed (includes 6 new) |
| `cargo test -p eggbench-drivers` | lib + integration green (includes 8 new) |
| `cargo test -p eggbench-cli --lib workload_registry` | 8 passed (includes 1 new) |
| `cargo check -p eggbench-drivers --all-features --all-targets` | clean |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |
| `python3 scripts/verify-architecture-docs.py` | 26 files, 3,557 citations, 0 errors |
| `cargo test --workspace --all-features` (partial) | 9 groups green, 0 failures; still running at commit |

The runner suite requires `EGGBENCH_PARENT_SENTINEL_26CE` to be set; without it
`lifecycle.rs` intentionally panics. All runs set it.

### Complete local verification pass (2026-10-05/06)

Re-run on the final tree, which includes the `read_bounded` extraction and the
two added regression tests for items 1.3/1.4. Linux, stable 1.99.

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo check --workspace --all-targets --locked` | clean |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | clean |
| `cargo test --workspace --all-targets --all-features --locked` | **615 passed, 0 failed across 27 test binaries** |
| `python3 scripts/verify-architecture-docs.py` | 26 files, 3,532 citations, 0 errors |
| Drivers isolation: `--no-default-features` | clean |
| Drivers isolation: `+ eggstack-http` | clean |
| Drivers isolation: `+ gregg` | clean |
| Drivers isolation: `+ eggstack-path` | clean |
| Drivers isolation: `+ eggstack-path,gregg` | clean |
| Drivers isolation: `+ prometheus-http` | clean |
| `cargo check -p eggbench-cli --no-default-features --features eggstack-path` | clean |
| `cargo tree -p eggbench-drivers --no-default-features --edges normal \| grep -E 'eggress\|eggchaos'` | empty — isolation holds |
| same, with `--features eggstack-http` | empty — isolation holds |
| `cargo test -p eggbench-cli --no-default-features --test cli` | passed (feature-off fail-closed path) |
| `cargo +1.89.0 check --workspace --all-targets --locked` (MSRV) | clean |
| `cargo +1.89.0 check --workspace --all-targets --all-features --locked` (MSRV) | clean |
| `cargo run -p eggbench-cli --features eggstack-path -- validate examples/eggstack-path.json` | `eggbench: validate ok` |

**The heavy-suite condition is now cleared.** The earlier partial result is
superseded: the complete `--all-targets --all-features` run finished with 615
tests passing and no failures across 27 binaries, including the whole
`synvoid_m002b` group. The cases that assert on comparison verdicts — the ones
most exposed to C002's semantics change — all passed:
`workload_drift_compares_as_incomparable`,
`smoke_profile_passes_with_absolute_gates`,
`perf_same_source_pair_never_fails`,
`performance_only_regression_fails_suite_despite_correctness_pass`, and
`correctness_only_regression_fails_suite_despite_perf_pass`.

That is the expected result rather than a lucky one: both `check_metric_semantics`
call sites convert a rejection into a per-metric `Invalid` disposition, the same
shape the pre-existing `metric_semantics_mismatch` path used, and a real run's
baseline is semantically coherent, so no verdict changes.

`synvoid_m002b` took roughly 70 minutes wall-clock, serializing on a shared lock
and running real paired qualification against Python stub origins. The host was
also heavily contended for part of that window (load average above 30), so treat
the duration as an environment property, not a code property.

One gate failed for an environmental reason and was re-run clean: the
`validate` smoke check first aborted with `No space left on device` while
compiling `ring`/`eggchaos-core` (the filesystem was at 100%, with `target/`
alone at 9.9 GB). It passed on retry once space was available. This is a disk
condition, not a code defect, and it did not affect any test result.

### Named verification conditions

1. ~~**Heavy qualification integration suite.**~~ **CLEARED** — see the complete
   local verification pass above. 615 passed, 0 failed.
2. **Platform-conditional fixes — Windows cleared, macOS blocked by an
   unrelated defect.** C002 items 2.1–2.3 (deadline flags, cleanup notes, drain
   timeout) matter most on non-Linux, and `evidence.rs`'s stale-lock path is
   `#[cfg(not(target_os = "linux"))]`. `windows-stable` is **green**, which
   covers its half of this condition.

   `macos-stable` is **red**, but not on anything C002 changed. It fails on
   `perf_same_source_pair_never_fails` in `crates/eggbench-cli/tests/synvoid_m002b.rs`
   with `primary_failure: "TimedOut"` — a pre-existing condition that has been
   red since `13eb443` and affects every commit since, including this one. The
   three `perf-large` scenarios move ~786 MB per trial at 12000 requests and a
   65536-byte body, which exceeded the 300000 ms measurement budget on the
   `macos-14` runner.

   That sample policy is deliberate and measured (identical-build p95 spread
   fell from 77.9% to 0.2–11.4% against unchanged 15%/20% allowances; see
   `plans/closure/security-qualification-m002-corrective/002-status.md`), so the
   fix raises the measurement budget on the three `perf-large` scenarios to
   600000 ms rather than reverting the request count. The budget is a safety
   bound and enters no comparison, so this cannot change any verdict. It is not
   verified: the correction is unproven until a hosted `macos-stable` run is
   green, and none is claimed here.
3. **No hosted CI run is claimed by this pass.** Clippy and `cargo fmt --check`
   must pass in the hosted lanes before C002 is marked closed.

## Reclassified, not closed

C003-a … C003-o remain open, decision-gated. The most consequential are the
`doctor`/`run` disagreement for `prometheus-http` fields (C003-d), stale
`finalize-lock` reporting (C003-a), and whether the v2 manifest should be a
closed contract (C003-b). None blocks C001 or C002.
