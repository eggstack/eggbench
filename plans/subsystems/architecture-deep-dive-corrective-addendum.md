# Architecture Deep-Dive Audit Corrective Addendum

Status: **active**
Milestones: `C001` contract truthfulness, `C002` evidence integrity and
observability, `C003` decision-gated follow-ups
Owning subsystems: none — this corrective is deliberately cross-subsystem (see
"Scope and ownership" below)
Corrective classification: **post-closure corrective pass** (a new
implementation plan referencing already-closed milestones, not a new subsystem)

## Why this corrective exists

The 21 component deep dives added in `13eb443` were the first read of this
workspace that traversed every crate at contract depth and compared the code
against its own prose. They surfaced 38 findings, each recorded in the
relevant deep dive's *Review focus* section. Every finding was then
independently re-verified against source by three read-only research passes
before entering this plan, because a deep dive is a lead, not a verdict.

Of the 38 findings: **24 verified**, **3 verified with the finding's own
framing needing correction**, **1 refuted** (the catalog/CLI driver-name
divergence does not exist — only the comment claiming it is wrong), and the
remainder are documented code-level observations with no defect.

Three findings are significant enough to name here, because they bear on the
property this system exists to provide:

1. **Metric truncation in comparison was silent.** A bundle carrying more
   than 256 resolved metrics was truncated with no warning and no receipt
   field, so the receipt looked complete. Reachable from a crafted or
   malformed bundle, because the comparison path bounds only the resolved
   plan, not the resolved metric set.
2. **Per-trial metric semantics were checked against the first baseline
   trial only.** A heterogeneous baseline — one where trial 2's `latency_p99`
   is 10x trial 1's — passed the semantic check.
3. **`BundleReader::trial_metrics` read an artifact to completion before
   applying its 256 MiB cap.** The manifest cannot force this (declared
   sizes are validated), but actual bytes on disk in a shared or imported
   bundle can, yielding a memory-amplification vector on untrusted evidence.

## Why previous verification missed these

This section is required by `plans/003-planning-process.md` §8 and is the
part most worth reading.

**The evidence-producing path was verified; the evidence-consuming path was
not exercised with inputs the producer cannot emit.** Findings 1–3 all live
in `comparison.rs` and `evidence.rs` on the *read* side. Every prior closure
verified the runner: that a run produces correctly-shaped evidence, and that
comparison of two such bundles yields the documented verdict. But a
runner-produced bundle *cannot* exceed 256 metrics (the plan validates
`trials[*].metrics` against the vocabulary) and *cannot* carry per-trial
semantic disagreement (one producer normalizes consistently). The
properties were therefore never violated in testing, and the guards that
should have caught them were never on the path. The missing test is a
negative-input test, and the repository's test suite is overwhelmingly
positive-path.

**Later schema bumps outran the prose.** `ResolvedPlan` advanced from v3 to
v6 and `TrialExecutionResult` from v1 to v2, `ComparisonReceipt` from v2 to
v4. Each bump was verified against the code, but the *document* that states
the version was not re-audited, so `architecture/drivers.md`,
`docs/driver-capabilities.md`, `docs/comparison.md`, and
`docs/paired-experiments.md` now understate the current versions and the
readable ranges. This is the single largest class of finding (15 of 38) and
it is documentation-only.

**A contract was changed without its invariant record being updated.** In the
M002 corrective, workload drain was made unconditional — deliberately, to
preserve the existing M002 contract, with an explanatory comment at
`orchestration.rs:1452`. The `workload_entered` field that used to gate it
was left in place, now written four times and never read, and
`architecture/runner.md` plus `docs/trial-orchestration.md` were left stating
the superseded gated behavior. The change was correct; the record was not
maintained. A test that predates the change (`tests/orchestration.rs:776`
asserts `workload.drained`) now locks in the *new* behavior, so nothing in
CI would have caught the stale documentation.

**Observability fields were added to a success-only construction site.**
`ExternalCommandOutcome.cancelled` and `.timed_out`
(`command.rs:133,135`) are only ever set by a code path reachable after the
command has already completed, so they are structurally always `false` — a
timeout can never be observed. Similarly `cleanup_notes` is computed at
`command.rs:281`, *after* the cancel and timeout early returns at 258–271,
so the Windows `direct_child_only` orphan-risk note is never emitted — on
exactly the platform where it matters. The fields were added deliberately to
make the substrate observable; the observation never became reachable.

**Two registries were written at different times.** `ServiceAdapterRegistry`
and `TelemetryRegistry` return `Result` and reject duplicate registration.
`CorrectnessRegistry` and `DiagnosticRegistry`, added later, return `()` and
silently overwrite. Nothing registers twice today, so the asymmetry is
latent — but a duplicate would silently discard a resolved executable path
and its preflight proof, with nothing in the evidence recording the
replacement.

## Scope and ownership

`plans/003-planning-process.md` §13 requires decomposition changes to be
explicit. This corrective spans subsystems and does **not** re-home any of
them. The findings map onto already-closed milestones:

| Finding area | Closed milestone of record |
|---|---|
| `evidence.rs` manifest, bounded reads, publication | Foundation experiment/evidence; its post-closure corrective |
| `comparison.rs` policies, receipts, truncation | Measurement/comparison |
| `orchestration.rs` phases, metrics normalization | Local runner M002; its post-closure corrective |
| `session.rs` / `service.rs` / `telemetry.rs` / `correctness.rs` / `diagnostics.rs` seams | Local runner/lifecycle; local runner M002 |
| `external/*` substrate and tool adapters | External oracles |
| `catalog.rs` and CLI driver dispatch | Eggstack integration |
| Security/qualification vocabulary | Security qualification M002 |

Each milestone below is cross-referenced to these rather than reopening them.
The `architecture/*.md` crate-level documents are corrected **only** where
they state something the code contradicts; no crate boundary, no dependency
direction, and no ADR is touched.

## Milestones

### C001 — Contract truthfulness

**Classification:** corrective, documentation-only, zero behavior change.

Fix every place where a document or a code comment states something the code
contradicts, in `docs/`, the four crate-level `architecture/` documents, the
`README.md`, and the affected source comments. Includes recording the
deliberate "unlinked by contract" status of dead-but-public API surface
rather than silently deleting it (see C002 for that disposition).

Full enumeration lives in
`plans/implementation/architecture-deep-dive-corrective/001-contract-truthfulness.md`.

### C002 — Evidence integrity and observability

**Classification:** corrective, fail-closed, **no serialized-schema change**.
Every fix either reuses an existing schema field (for example
`ComparisonReceipt.warnings`) or changes only internal behavior, error
taxonomy, or public Rust signatures that no serialized contract depends on.

The unifying property: **no input may cause the system to emit a
self-description that is more confident than the evidence supports.** The
three headline findings are all instances of that, as are the dead
observability fields and the silently-overwriting registries.

Full enumeration lives in
`plans/implementation/architecture-deep-dive-corrective/002-evidence-integrity-and-observability.md`.

### C003 — Decision-gated follow-ups

**Classification:** corrective, **blocked on an explicit maintainer
decision**, deliberately NOT implemented in this pass.

These findings each require a call on contract semantics, public API shape,
or evidence bytes. Per §8 the decision is recorded here rather than made
silently, and each row states what changes if the call goes either way. They
are not blockers for C001/C002.

| Ref | Finding | Decision required |
|---|---|---|
| C003-a | Distinguish a stale `.{name}.finalize-lock` from a genuinely existing destination (`evidence.rs:1431-1457`). `BundleError` is not `#[non_exhaustive]`, so a new variant is a minor breaking change for downstream exhaustive matches. Error text and CLI exit category both change. | Add a variant, or recover the lock file automatically? Self-healing is racy across concurrent processes. |
| C003-b | Add `#[serde(deny_unknown_fields)]` to `BundleManifest` and `LegacyManifestV1` (`evidence.rs:376,420`). This is currently inconsistent with `PairedRunRecord` in the same file and every CLI envelope type. | Is the v2 manifest a closed contract? Adding it removes deliberate forward tolerance and makes out-of-contract bundles `ManifestParse` failures. |
| C003-c | Drop the `Option` from `BundleManifest.execution_status` (`evidence.rs:382`). It has no `None` producer: `into_current_view` always assigns `Some` and `validate_inner` rejects `None`. Serialized bytes unchanged; public Rust API changes. | Accept the public-API narrowing? |
| C003-d | Make `doctor` and `run` agree on `prometheus-http` field viability. `resolved.rs:615-621` skips field-capability validation for this source, so `doctor` reports green on a plan `run` rejects at collector construction. Deleting the skip is **not** correct — the descriptor cannot enumerate owner-defined names. | Which command is authoritative for telemetry field viability? |
| C003-e | `build.rs:78-80` returns the first matching `[[package]]`; `hdrhistogram` is absent from `TARGETS`; `prometheus_http.rs:452` writes a third hand-maintained literal `"exposition_format": "prometheus-text-scalar-v1"`. The lockfile-change portion is safe, but adding `hdrhistogram` and deriving the label **change `prometheus-provenance.json` bytes and therefore bundle digests**. | Accept an evidence-byte change for correct provenance? |
| C003-f | Candidate-wins environment comparability class (`comparison.rs:3030-3032`): a baseline that reclassifies a `comparison_critical` field to `informational` is undetected. | Should disagreement be a warning, a rejection, or candidate-wins? |
| C003-g | Percentile basis points are unenforced: `aggregation_matches` (`metrics.rs:431`) accepts any percentile against any expected percentile. `metrics.rs:426-428` documents this as intentional, and `metrics.rs:1003` retains the raw aggregation. | Is the documented permissiveness the intended contract, or should drivers validate at parse time? |
| C003-h | Ratio domain `[0,1]` is enforced by a hardcoded name list (`metrics.rs:581-583`), so a custom `Ratio` metric gets no enforcement. | Extend enforcement to the declared type, or keep the name list? |
| C003-i | `ResolutionOptions` is built independently in `run.rs:551-620` and `doctor.rs:141-217`, and `workload_load_mode` is duplicated (`run.rs:652`, `doctor.rs:676`). Safe to extract, but it is a refactor with no defect behind it today. | Accept the refactor now or defer to a `doctor`/`run` parity milestone? |
| C003-j | Preflight steps in `run.rs` each build a private `CancellationToken` (`run.rs:154,173,198,226`) that the SIGINT forwarder cannot reach, since it only spawns at `run.rs:446`. Consequence: Ctrl-C during preflight cannot cancel in-flight preflight. | Worth restructuring cancellation plumbing? |
| C003-k | `executable_path_for` uses `to_string_lossy()` (`preflight.rs:72`), so a non-UTF-8 canonical path is mangled and fed back through `resolve`, failing as `NotExecutable`. | Accept the degradation or add a non-UTF-8 path channel? |
| C003-l | `.ps1` rejection is `cfg(windows)`-only (`resolver.rs:169`) while `.bat`/`.cmd` are rejected everywhere (`resolver.rs:180-188`). | Should script extensions be rejected on all platforms? |
| C003-n | A failing diagnostic executor can fail the whole run with an evidence error. `orchestration.rs` builds the failed record with empty `producer_version`/`executable_sha256`, and `DiagnosticsIndex::validate_contract` requires non-empty version plus a 64-character digest unless every record is `skipped`. A plan whose only diagnostic fails therefore returns `Evidence { InvalidManifest }` instead of the clean `Invalid`/`DiagnosticFailed` disposition. Found during C002 implementation; pre-existing, not caused by the corrective. | Which is authoritative: the diagnostic's own negative disposition, or the index provenance contract? |
| C003-o | `doctor` does not construct telemetry collectors, so it cannot see the `prometheus-http` field rejection that `run` enforces at collector construction (C003-d). This is the mechanism behind that finding and constrains any fix: a fix that only makes `doctor` stricter would require `doctor` to build collectors, which changes what `doctor` means from "validate and preflight" to "construct adapters". | Recorded for the C003-d decision; not separable from it. |
| C003-m | End-to-end `"unknown"` provenance path: `build.rs:33-36,49-51` falls back to a literal `"unknown"`, and `resolved.rs:993-996` only requires `upstream_version` to be non-empty, so a missing lock entry flows into route descriptor evidence while still passing contract validation. Not verified end-to-end. | Needs a stripped-lockfile reproduction before deciding. |

## Verification obligations

Per §8, this corrective must add regression evidence, not only assertions.

- Every C002 code fix lands with a named regression test in the crate's
  existing test location (`crates/*/tests/` or the module's `mod tests`).
  The negative-input tests for C002 are the substance of this milestone:
  the properties were previously untested because the producing path cannot
  violate them.
- C001 carries no test obligation beyond
  `scripts/verify-architecture-docs.py`, which must pass with zero errors
  after the edits — in particular the `file:line` citations the corrected
  documents and comments now make.
- `cargo clippy --all-features -- -D warnings` and
  `cargo test --workspace --all-features` must both pass before closure.
- A hosted CI run is **not** claimed by this pass. Where a C002 fix is
  platform-conditional, the closure record must name the lane that covers it
  and the lanes that do not.
