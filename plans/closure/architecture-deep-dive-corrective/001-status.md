# C001/C002 Closure Status — Architecture Deep-Dive Audit Corrective

Corrective: `plans/subsystems/architecture-deep-dive-corrective-addendum.md`
Implementation:
`plans/implementation/architecture-deep-dive-corrective/001-contract-truthfulness.md`
and `002-evidence-integrity-and-observability.md`

Status: **closing** — C001 closed, C002 landing with a named verification
condition (see "Verification actually run").

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

### Named verification conditions

1. **Heavy qualification integration suite — narrowed, not cleared.** The
   `synvoid_m002b` suite runs real paired qualification against Python stub
   origins and serializes on a shared lock, so it takes tens of minutes in a
   debug build. It was still executing at commit time, with **9 test groups
   green and 0 failures**. Critically, the two cases that assert on comparison
   verdicts — the ones most exposed to the C002 semantics change — both
   **passed**: `workload_drift_compares_as_incomparable` and
   `smoke_profile_passes_with_absolute_gates`, alongside
   `perf_same_source_pair_never_fails`,
   `performance_only_regression_fails_suite_despite_correctness_pass`, and
   `correctness_only_regression_fails_suite_despite_perf_pass`.

   This is consistent with the analysis: both `check_metric_semantics` call
   sites convert a rejection into a per-metric `Invalid` disposition, the same
   shape the pre-existing `metric_semantics_mismatch` path used, and a real
   run's baseline is semantically coherent, so no verdict changes. The
   remaining telemetry-drift cases were still running and are unrelated to the
   comparison change. **A complete local run and a hosted CI run are still
   required to clear this condition** — the partial result is recorded as
   evidence, not as completion.
2. **Platform-conditional fixes.** C002 items 2.1–2.3 (deadline flags, cleanup
   notes, drain timeout) matter most on non-Linux, and
   `evidence.rs`'s stale-lock path is `#[cfg(not(target_os = "linux"))]`. The
   Linux lane does not exercise the `direct_child_only` note or the
   finalize-lock path. **macOS and Windows lanes required.**
3. **No hosted CI run is claimed by this pass.** Clippy and `cargo fmt --check`
   must pass in the hosted lanes before C002 is marked closed.

## Reclassified, not closed

C003-a … C003-o remain open, decision-gated. The most consequential are the
`doctor`/`run` disagreement for `prometheus-http` fields (C003-d), stale
`finalize-lock` reporting (C003-a), and whether the v2 manifest should be a
closed contract (C003-b). None blocks C001 or C002.
