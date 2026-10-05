# C001 — Contract Truthfulness

Status: implementation
Corrective classification: post-closure corrective, documentation-only
Blocked by: nothing
Deliberately excludes: any change to a code comment's surrounding code, and
any C003 item

## Purpose

Make every document and code comment in the repository agree with the code,
so that a reader following the documentation builds a correct model. This
milestone exists because 15 of the 38 audit findings were documentation
defects, and the repository treats `docs/` as the canonical contract: a
contract that is wrong is worse than one that is absent.

No behavior changes. No schema changes. No source logic changes.

## Findings in scope

All line references were verified against `dedef4e`. Three findings were
**corrected during implementation** because verification contradicted the
deep dive that reported them, and the corrections are recorded here rather
than silently dropped:

- `docs/equivalence.md` does not exist and never has (no git history). The
  finding that motivated a row here was reported against a non-existent file,
  and its substance is also wrong: `ComparisonReceipt` **does** set
  `#[serde(deny_unknown_fields)]` (`comparison.rs:493`), and
  `ComparisonRequest` is not serialized at all. No edit is made.
- The catalog-versus-CLI driver-name divergence is **refuted**: the dispatch
  arms and the catalog registrations are identical in all four feature
  combinations. Only the comment claiming otherwise was wrong. C001 corrects
  the comment; C002 adds the guard test that makes the claim true.
- The histogram/scalar unresolvable-reference asymmetry (`metrics.rs:990-998`
  versus `metrics.rs:1030-1032`) is a **documented design decision**, not an
  oversight: the code states "Unknown references are dropped with no claim …
  Histograms are references only." C002 therefore records the asymmetry
  rather than changing it, and C001 states the reasoning where it matters.

### A. Version claims that outran the code

| Location | Claim | Fact |
|---|---|---|
| `architecture/drivers.md:11`, `docs/driver-capabilities.md:25` | `ResolvedPlan` "current v3", v1/v2 readable | `RESOLVED_PLAN_SCHEMA_VERSION = 6` (`resolved.rs:12`); evidence accepts v1–v5 plus current (`evidence.rs:1748-1758`) |
| `docs/comparison.md:61`, `docs/paired-experiments.md:97` | paired receipts are schema v2 | `COMPARISON_RECEIPT_SCHEMA_VERSION = 4` is emitted |
| `docs/trial-orchestration.md:49` | `TrialExecutionResult` is schema v1 | pinned to v2 (`orchestration.rs:44`) |
| `docs/gregg-telemetry.md:120-121` | provenance records response bytes | those fields are never written |
| `docs/gregg-telemetry.md:88-89` | retention truncation marks metrics invalid | the sample is dropped and a counter increments; no invalidation |

### B. Behavioral claims contradicted by code

| Location | Claim | Fact |
|---|---|---|
| `architecture/runner.md:109-110`, `docs/trial-orchestration.md:32` | drain runs only "if the executor was reached" | drain is unconditional (`orchestration.rs:1462`); `workload_entered` is never read |
| `docs/trial-orchestration.md:78-86` | exactly four timeout names | the allowlist also contains `telemetry`, which defaults to the measurement bound (`orchestration.rs:1900,1925`) |
| `docs/external-oracles.md:111-112` | partial stdout is staged on cancellation | cancellation returns `Err`; no outcome, no artifact |
| `docs/external-drivers.md:46-48` | secrets can never appear in `Debug` | `ExternalCommandSpec` derives `Debug` and holds argv/env verbatim |
| `docs/external-drivers.md:52` | no inherited working directory | `None` leaves the child inheriting the parent's cwd |
| `docs/external-drivers.md:27` | `external-command` gates the substrate | the feature has zero referents (C002 also resolves this) |
| `docs/metrics.md:21-22` | percentile basis points are part of the contract | `aggregation_matches` (`metrics.rs:431`) accepts any percentile against any expected percentile |
| `docs/metrics.md:46` | `UnsupportedByDriver` is a normalization outcome | never constructed |
| `architecture/evidence.md:9` | v1 tolerates a curated set of additive fields | neither manifest struct declares `deny_unknown_fields`, so tolerance is blanket top-level |
| `architecture/drivers.md:13` | a fault plan retains the seed in the resolved plan | `seed` is copied unconditionally; fault-conditional namespacing lives in path evidence (`path/evidence.rs:131`) |
| `architecture/drivers.md:39` | only `oha`/`h2load`/`iperf3` are unconditional | 7 external drivers register unconditionally |
| `architecture/drivers.md:19` | `eggstack-http` registers origin + fetch | also registers `eggbench-http-corpus`; the catalog narrative omits `prometheus-http` |
| `qualification/synvoid/v1/README.md:3` | the upstream asset contract is open | `upstream-manifest.md:5` records it CLOSED; `registry.md:121` discharges the condition |
| `README.md` | five commands | six: `qualify` is absent from the README (`main.rs:35-37,104-119`) |

### C. Wrong or misleading code comments (comments only, no logic change)

- `comparison.rs:102-103` — an orphan doc comment about a "normalization method
  label" attached to `MAX_RECEIPT_METRICS`; the constant actually bounds
  resolved metrics.
- `comparison.rs:2065` — "Accepts schema v1, v2, and v3"; the body accepts v4.
- `comparison.rs:2302` — "Emits a schema-v3 receipt"; emits v4.
- `comparison.rs:514` — comment says receipt schema v2; the emitted schema is v4.
- `comparison.rs:2266-2283` — `compare`'s doc comment, `#[must_use]`, and
  `# Panics` sit above `correctness_section`, which has its own doc comment.
  Move them back onto `compare`.
- `comparison.rs:3711` — the `StrictSameTestbed` arm of `descriptive_reason`
  is unreachable; that policy is handled earlier at `:3643`.
- `evidence.rs:382` — `execution_status` "Missing only in the normalized view of
  ambiguous v1 evidence", but `into_current_view` always assigns `Some` and
  `validate_inner` rejects `None`. (C003-c decides whether to drop the
  `Option`; C001 only corrects the comment.)
- `evidence.rs:129` — unreachable `ends_with(':')` arm (C002 removes the line;
  C001 records it here so the removal is traceable).
- `run.rs:3-6` — module doc orders `prepare BundleWriter` before
  `LocalSession::prepare`; the code does the reverse (`run.rs:398` then
  `:408`). It also names `execute_run`, which the CLI never calls — only
  `execute_run_with_diagnostics` (`run.rs:447`). The swap is harmless:
  `LocalSession::prepare` starts no process.
- `catalog.rs:3-12` — module doc under-reports the registration list.
- `workload_registry.rs:390` — "Only catalog-registered drivers resolve".
  **Refuted as a divergence**: the dispatch set and the catalog set are
  identical in all four feature combinations. The comment is misleading, not
  the code. C001 corrects the comment; C002 adds the guard test that keeps
  them equal.
- `correctness.rs:18` and `diagnostics.rs:9` — "no Eggsec/Eggprobe types cross
  here". Literally true (neither module imports a sibling type), but the
  modules hardcode sibling *evidence vocabulary*: `diagnostics.rs:169,173`
  (`"eggprobe"`, `"0.3"`) and `correctness.rs:44,47,59,182-184` (WAF labels).
  Reword to state the real coupling.
- `resolved.rs` field-capability skip at `:615-621` — its comment says the CLI
  fails "at the readiness boundary", but construction failure occurs at
  collector construction (`run.rs:431-439`), so `doctor` reports green.
  C003-d decides the remedy; C001 corrects the comment.
- `ComparisonReceipt.created_by_version` — document that it is a build
  version, not a contract version, and that byte-identical receipts are
  reproducible only for a fixed build.

### D. Dead-but-public API surface: record, do not delete

These are public, exported, and unconstructed. They are unlinked by contract,
not by accident, so C001 records that status at each declaration and C002
leaves them in place. Deleting them is an API change, not a corrective.

- `MissingReason::UnsupportedByDriver` (`metrics.rs:94`)
- `HistogramReference.source_metric` (`metrics.rs:196-197`, set to `None` at
  `metrics.rs:1043`)
- `ErrorCategory::OutputTruncated` (`error.rs:36,60`)
- `ExternalParseError` (`parser.rs:25-36`, re-exported `mod.rs:62`)
- `ExternalCommandOutcome.cleanup_notes` — *not* dead; it was dead only on the
  cancel/timeout paths, which C002 fixes.
- The histogram/scalar unresolvable-reference asymmetry is a stated design
  decision (`metrics.rs:1028-1029`), not dead code. It is documented, not
  changed: a scalar IS the measurement, so an unresolvable source reference
  makes it invalid, while a histogram is a pointer to raw evidence that the
  normalizer never parses, so an unresolvable one is dropped.

## Explicitly out of scope

- Rewording prose for style where it is already accurate.
- Correcting `architecture/overview.md` or the 21 deep dives: they were
  written against this commit and are accurate. `overview.md` §7 indexes
  these findings, and its "Known drift" tables must be updated to point at
  this milestone once C001 lands.
- Any C003 item.

## Verification

- `python3 scripts/verify-architecture-docs.py` — zero errors. The corrected
  documents add `file:line` citations, so this is a real check.
- `cargo check --workspace --all-targets --locked` — comment-only edits must
  not break doc tests or intra-doc links.
- `cargo test --workspace --all-features --doc` — rustdoc link resolution.
- Grep the corrected claims to confirm each replacement cites real code.
