# C002 — Evidence Integrity and Observability

Status: **implemented — local verification complete, hosted-lane conditions
open** (closure record
`plans/closure/architecture-deep-dive-corrective/001-status.md`)
Corrective classification: post-closure corrective
Blocked by: nothing
Compatibility: **no serialized-schema change.** Every fix either reuses an
existing schema field, changes only internal behavior, or changes public Rust
signatures that no serialized contract depends on.

## Purpose

Enforce one property across the workspace:

> No input may cause Eggbench to emit a self-description that is more
> confident than the evidence supports.

The three headline findings are instances of it: a comparison receipt that
looks complete after silent truncation, a semantics check that passes on a
heterogeneous baseline, and a bounded reader that is not actually bounded.
The rest are observability fields that were deliberately added to make the
system inspectable and are structurally unable to fire.

Each item lands with a named regression test. The negative-input tests are
the substance of this milestone: the previous suite is overwhelmingly
positive-path, which is why these properties were never violated *in
testing* even though the guards existed.

## Items

### Group 1 — Evidence integrity (the property itself)

**1.1 Metric truncation must be visible.**
`comparison.rs:2378,2393,2561` silently drop resolved metrics past
`MAX_RECEIPT_METRICS` (256).
Fix: emit a `ComparisonReceipt.warnings` entry naming the count omitted. The
`warnings` field already exists, so **no schema bump**. Fail-closed direction:
a receipt is still produced, but it now says it is incomplete.
Test: a bundle whose resolved plan resolves more than 256 metrics produces a
receipt whose `warnings` records the omission.

**1.2 Metric semantics must be checked per trial, not first-wins.**
`comparison.rs:3755-3777` returns on the first baseline trial carrying a
metric.
Fix: every baseline trial that carries the metric must agree on
value/unit/aggregation/direction/basis_points; the first disagreement rejects
the comparison. Fail-closed and narrower than a policy change, so existing
valid evidence is unaffected.
Test: a baseline where trial 2's `latency_p99` is an order of magnitude above
trial 1's is rejected.

**1.3 Bounded reads must bound before allocating.**
`evidence.rs:1279` calls `read_to_end` and only then compares against
`MAX_ARTIFACT_BYTES` at `:1281`. The same file already does it correctly for
the manifest at `:1169-1178` (metadata pre-check plus `.take(max + 1)`).
Fix: mirror `:1176-1178`. Reachable from `inspect` on a user-supplied path
(`commands/inspect.rs:175-179`); the manifest cannot force it because
`validate_inner` bounds declared sizes (`:573-577`), so the attacker must
control actual bytes on disk.
Test: extracted `read_bounded` helper unit-tested with a small `max`, so the
test does not need a 256 MiB file.

**1.4 The comparison re-read must not bypass the secure reader.**
`comparison.rs:4092` re-opens `manifest.json` with a plain
`std::fs::File::open` on a joined path under its own 8 MiB cap, bypassing
`BundleReader`'s no-follow `secure_open` and 4 MiB cap. It is the only
verifiable artifact, so the re-read is legitimate, but it must inherit the
same protections.
Fix: use the crate's bounded no-follow read path.
Test: assert the re-read rejects an oversized manifest with the same error as
`BundleReader::open`.

**1.5 A scalar and a histogram diverge on an unresolvable reference — NOT
CHANGED, and this is a deliberate reversal of the original plan item.**
`metrics.rs:990-998` makes a scalar naming an unresolvable artifact
`invalid(malformed_source_reference)`; `metrics.rs:1030-1032` drops a
histogram naming an unresolvable artifact.

The original item proposed recording a `MetricWarning`. Reading the code
before implementing showed the divergence is **stated intent**, at
`metrics.rs:1028-1029`: "Unknown references are dropped with no claim; the
scalar path marks unknown refs invalid explicitly. Histograms are references
only." A scalar *is* the measurement, so an unresolvable source reference
invalidates it. A histogram is a pointer to raw evidence the normalizer never
parses, so an unresolvable one is simply not a reference to anything.

Overriding a documented design decision would change what
`trials/NNN/metrics.json` means for existing evidence on the strength of an
audit opinion, so this is recorded in C001 §D and left as-is. If the project
prefers a warning for symmetry, that is a contract decision and belongs in
C003, not in a corrective pass.

### Group 2 — Observability that cannot fire

**2.1 `cancelled` / `timed_out` must be real.**
`command.rs:133,135` are only set by `command.rs:299-300`, reachable only
after a `(false,false,status)` destructure at `:241-247`, so a timeout can
never be observed. A race is possible: if the deadline passes while output
streams are still being drained, `wait` returns `Ok` with partial bytes and
`truncated == false`.
Fix: derive the flags from the actual deadline outcome rather than a
success-only construction site.
Test: an adapter whose tool never exits within its timeout yields
`timed_out == true` and a `truncated` stream.

**2.2 `cleanup_notes` must be computed before the early returns.**
`command.rs:281-290` runs *after* the cancel/timeout returns at `:258-271`, so
the Windows `direct_child_only` orphan-risk note is never emitted — on the
platform where orphan risk is highest.
Fix: compute notes before the early returns.
Test: a cancelled command on the non-Linux path carries the cleanup note.

**2.3 `join_pipes` must not convert a drain timeout into empty success.**
`command.rs:342-355` uses `unwrap_or_default`, so a slow-draining pipe
yields `(Vec::new(), 0)` on the success path — empty stdout with
`truncated == false` — and the parser then reports "malformed output" where
the truth is "we stopped draining".
Fix: return an explicit `Err` for a drain timeout.
Test: a tool that floods stdout past the bound during drain reports a drain
failure, not a parse failure.

**2.4 Honor the `limit` parameter.**
`CapturedStream::collect` binds `_limit` and ignores it (`command.rs:99-101`);
truncation instead falls out of `total - retained` in `from_parts`
(`:104-114`). All three in-tree callers pass a limit, masking it.
Fix: either honor the parameter or remove it from the signature. Prefer
removing it, since `from_parts` already owns the bound and the parameter has
no caller that depends on it.
Test: construct via `from_parts` and assert the `truncated` flag and retained
length.

**2.5 Silent registry overwrite must become a rejection.**
`CorrectnessRegistry::register` (`correctness.rs:130-135`) and
`DiagnosticRegistry::register` (`diagnostics.rs:227-232`) return `()` and let
`BTreeMap::insert` overwrite, while `ServiceAdapterRegistry`
(`service.rs:188-198`) and `TelemetryRegistry` (`telemetry.rs:175-182`)
return `Result` and reject. Nothing registers twice today, so this is latent:
a duplicate would silently discard a resolved executable path and its
preflight proof with nothing in the evidence recording the replacement.
Fix: return `Result<(), String>` from both, mirroring the established pattern,
and update the three `run.rs` call sites (`:201,229,253`) plus the two test
helpers (`tests/orchestration.rs:2070,2463`).
Test: `duplicate_correctness_source_registration_rejected` and
`duplicate_diagnostic_source_registration_rejected`, modelled on
`tests/telemetry.rs:175`.

**2.6 `FakeDiagnosticExecutor` must not report a diagnostic failure as a
workload failure.**
`diagnostics.rs:311-313` returns `FailureCategory::WorkloadFailed` although
`FailureCategory::DiagnosticFailed` exists for exactly this case
(`orchestration.rs:284-287`). The category is serialized verbatim into
`runner-phases.json` via `report_status` (`orchestration.rs:2271`) and
`primary_failure` (`:2291`). No test exercises the path today.
Fix: return `DiagnosticFailed`. Affects only fake-produced qualification
bundles, so their digests change; no production driver and no schema.
Test: a failing diagnostic yields `primary_failure ==
Some(DiagnosticFailed)` and `report_status == "executor_diagnosticfailed"`.

**2.7 `TelemetryError` must not be constructible past its bound.**
`MAX_TELEMETRY_DETAIL_LEN` truncation lives only in `TelemetryError::new`
(`telemetry.rs:76`) while `category` and `detail` are `pub` (`:63-68`), so a
struct literal bypasses the documented bound. Every in-tree construction goes
through `new`, so narrowing the fields breaks nothing.
Fix: make the fields private, keep `new` as the sole constructor.
Test: `new` truncates at 512; the private fields make the bypass
unrepresentable.

### Group 3 — Wrong gate, wrong count, wrong coverage

**3.1 The phase-event bound is one short.**
`orchestration.rs:806-817` reserves `warmups + 3*trials + 2 + diagnostics +
correctness`. The real maximum is `+3`: four fixed phases
(StartupReadiness, workload Drain, Teardown, Finalization) plus a second,
telemetry Drain at `:1518` when a collector is active. Consequences, in order
of impact: the artifact-size floor at `:3163-3169` is 256 bytes short, which
can fail `stage_phase_artifacts` *after* all measurement work; the
admission guard at `:818-822` admits one phase event too many; the
`Vec::with_capacity` hint at `:826` is harmless (`begin_phase` does not cap).
Fix: replace the magic `2` with a named constant for the fixed-phase count.
Over-counting is safe.
Test: a max-shape plan (warmups + 3 trials + reset + cooldown + an active
telemetry collector + 1 diagnostic + 1 correctness) stays within the
reserved bound.

**3.2 Remove the write-only `workload_entered` flag.**
Written at `orchestration.rs:1040,1066,1201,1310`, never read. Drain is
unconditional by design (`orchestration.rs:1452-1454`).
Fix: delete the field and its four assignments. `RunState`-local, never
serialized, so no evidence change. C001 corrects the two documents that
describe the superseded gated behavior.
Test: existing `tests/orchestration.rs:776` (`assert!(workload.drained)`)
is the guard and must keep passing.

**3.3 `is_external_workload` must cover every registered external driver.**
`preflight.rs:25-34` names 5 drivers while `external_binary_present`
(`:41-52`) and `executable_path_for` (`:61-73`) name 7. Using the 5-name
predicate to decide whether a driver needs an external binary produces the
wrong answer for 2 registered drivers.
Fix: derive all three from one list.
Test: the three predicates agree over the full registered set.

**3.4 The catalog needs accessors for its own categories.**
`DriverCategory` has **8** variants (`resolved.rs:27-47`), not the 7 the audit
stated. `catalog.rs:81-117` exposes 5 accessors and no name-only getter, yet
registers `eggress-route` as `Route` and `eggchaos-stream` as `Fault`.
`doctor.rs:333-350` already hand-rolls a linear category scan to work around
the missing accessor. The gap is latent, not currently unreachable.
Fix: add `route`, `fault`, `execution_provider`, and `by_name`; switch
`doctor.rs` to them. Purely additive.
Test: `route("eggress-route")` and `fault("eggchaos-stream")` resolve under
`#[cfg(feature = "eggstack-path")]`; `by_name` returns `None` for an unknown
name.

**3.5 Keep catalog registration and CLI dispatch provably in step.**
Refuted as a divergence: the two name sets are identical in all four feature
combinations. The risk is that they can *drift*, since
`workload_registry.rs:390` already claims a guarantee the code does not
enforce.
Fix: no behavior change; add the guard test that makes the claim true.
Test: for each feature combination, assert the dispatch match arms and the
catalog registration names are equal.

**3.6 Resolve the inert `external-command` feature.**
`crates/eggbench-drivers/Cargo.toml:10` declares a feature with zero
`#[cfg(feature = "external-command")]` referents anywhere in `src`.
Fix: remove the dead feature and correct the doc that implies it gates
anything (C001). Removing it cannot change a build, because nothing reads it.
Test: `cargo check` with and without all features is unchanged; assert no
`cfg(feature = "external-command")` remains in `src`.

**3.7 Remove the unreachable path-validation arm.**
`evidence.rs:129`'s `ends_with(':')` predicate is subsumed by the
`contains(':')` rejection at `:124`. All operands are pure predicates in one
`||` chain, so removal is behavior-preserving.
Fix: delete the line.

## Deliberately not in this milestone

Everything in C003, plus the dead-but-public API surface listed in C001 §D,
which is recorded rather than removed because deletion is an API change.

## Verification

- `cargo check --workspace --all-targets --locked`
- `cargo clippy --all-features -- -D warnings`
- `cargo test --workspace --all-features` — every item above has a named test
- Feature matrix: `cargo check --no-default-features`, `--features eggstack-http`,
  `--features eggstack-path`, `--all-features`
- `python3 scripts/verify-architecture-docs.py`
- For 2.1–2.3, note they are most meaningful on non-Linux; record which CI
  lanes cover them in the closure record rather than claiming full coverage
  from a Linux-only run.
