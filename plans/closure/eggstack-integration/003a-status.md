# Eggstack Integration M003a — EggReplay Semantic Replay Workload — Closure

Disposition: **closed**

Closed: 2026-09-25

Implementation commit: `adc3c15` (`feat(eggstack): implement M003a EggReplay semantic replay workload`), on top of planning baseline `9d8b32b9b23a1c30888c0f63ebf353b04abed761`.

Hosted qualification: four-lane CI is triggered by the combined M003a+M003b push; local verification below is complete and hosted lanes are recorded in the M003b/M003 umbrella closure (`plans/closure/eggstack-integration/003b-status.md`). No live `eggreplay` binary was present on the implementation host, so closure distinguishes hosted absence-safe/parser/fixture-contract evidence from live-binary evidence (none claimed).

## 1. Requirement-to-evidence matrix

| Plan requirement | Evidence | Outcome |
|---|---|---|
| Schema v4 `SemanticReplay` with v1-v3 compatibility | `crates/eggbench-core/src/plan.rs`, `lib.rs`; v4 round-trip, v1-v3 compat, fixture/gate/paired tests | Pass |
| External workload, not copied library implementation | `crates/eggbench-drivers/src/external/eggreplay.rs`; JSON CLI only (`validate` + `replay --output json`), no `eggreplay-*` Rust dep | Pass |
| No EggReplay Rust production dependency | `crates/eggbench-drivers/Cargo.toml` unchanged; `cargo tree --locked` contains no `eggreplay-*` | Pass |
| Trusted binary resolution/version/SHA recorded | `BinaryResolver`, `VersionProbe --version`, `check_min_version 0.1.0`; version/digest in preflight, method metadata, and `semantic-replay.json` | Pass |
| Workspace-confined immutable fixture | `compute_fixture_identity` (canonicalize, prefix check, symlink-escape rejection, directory, special-file rejection, bounded traversal) | Pass |
| Digest-based fixture identity, not path-based | Sorted relative paths + lengths + SHA-256 contents + aggregate SHA-256; path-independence test with two workspaces | Pass |
| `eggreplay validate` succeeds before managed startup | CLI `run` preflight `preflight_semantic_replay` before session/bundle; `missing`/`invalid_fixture`/`external_tool` fail before startup | Pass |
| Envelope schema 1 enforced | `parse_validate_envelope`, `parse_replay_envelope` require `schema_version == 1`, `command` match, bounded warnings | Pass |
| `RegressionReport` schema 2 enforced | Replay parser requires every report `schema_version == 2`, bounded reports/findings, `finding_count` sum consistency | Pass |
| One fixture replay equals one trial | `EggReplayWorkload::execute` runs one `replay` process per warmup/measured invocation; no per-flow splitting | Pass |
| Mismatch remains successful observation | Nonzero findings with exit 0 return `WorkloadOutput` with `semantic_findings > 0`; only nonzero exit/timeout/cancel become `WorkloadFailed`/`TimedOut`/`Cancelled` | Pass |
| Absolute gate supported | `semantic_findings` (`count`, lower-is-better, `Direct`) normalizes; absolute gate passes through existing gate machinery | Pass |
| Relative/statistical gates fail preflight | `plan.rs` rejects `RelativeRegression`/`StatisticalRelative` for `semantic_findings` with `unsupported_gate` | Pass |
| No process timing mislabeled as latency | No latency metrics emitted; `measurement_elapsed` left `None`; only `semantic_findings`/`semantic_flows` (`Direct`, `count`) | Pass |
| `network_path` composition fails closed | `plan.rs` rejects `SemanticReplay + network_path` with `workload_path_incompatible`; other drivers reject `SemanticReplay` with `unsupported_option` | Pass |
| Cancellation/timeout uses existing ownership | `run_command` with invocation timeout + cancellation token; process-group TERM/KILL on Unix, direct-child on Windows | Pass |
| Bounded raw evidence | `stdout.raw`/`stderr.raw`/`command-metadata.json` (4 MiB/256 KiB caps) + `eggreplay-summary.json`; run-level `semantic-replay.json` ≤128 KiB | Pass |
| Comparison-critical identity | `workload_summary` is kind+target (path-free); `semantic-replay.json` digest/session/envelope/report/tool version/digest checked by `compare_semantic_replay`; semantic vs non-semantic incomparable | Pass |
| CLI validate/doctor/run/inspect | `validate` accepts v4 example; `doctor` reports `eggreplay-semantic` descriptor/capability/binary presence; `run` preflights before startup; `inspect` surfaces run digest + per-trial `semantic_findings` | Pass |
| Feature/dependency isolation | Unconditional external descriptor (no new feature); default/all-feature/MSRV checks pass; no sibling Rust crate | Pass |

## 2. Sibling seam audit — implementation time

- Planning HEAD observed: `29ce133f`, workspace 0.1.0, Rust 1.89, envelope schema 1, `RegressionReport` schema 2, stable exit classes 0/1/2/3/4/5, `validate` + `replay --output json` as stable seam. Long-lived `serve` not selected (human stderr readiness).
- Implementation host: no `eggreplay` binary found in trusted `PATH`; no local EggReplay checkout in `~/projects`. No EggReplay Rust crates added. Accepted fixture session schemas are `[1, 2]`; envelope 1 and report 2 are pinned and other versions fail closed.
- Live binary evidence: none claimed. Hosted lanes execute absence-safe/parser/fixture-contract tests. A fake-`eggreplay` script was not needed because parser/fixture unit tests plus CLI absence-safe tests prove the contract without inventing upstream semantics.

## 3. Verification record

- `cargo fmt --all -- --check` — pass (after `cargo fmt --all`).
- `cargo check --workspace --all-targets --locked` — pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — pass.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass.
- `cargo test --workspace --all-targets --locked` — pass: **371 tests across 20 suites** (`EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process`).
- `cargo test --workspace --all-targets --all-features --locked` — pass: **428 tests across 20 suites**.
- `cargo +1.89.0 check --workspace --all-targets --locked` — pass.
- `cargo +1.89.0 check --workspace --all-targets --all-features --locked` — pass.
- `cargo +1.89.0 test -p eggbench-core --all-features --locked` — pass (137 tests).
- `cargo +1.89.0 test -p eggbench-drivers --all-features --locked` — pass.
- `cargo tree --locked` — pass; no `eggreplay-*` crate in normal graph.
- `git diff --check` — pass.
- `cargo run -p eggbench-cli --locked -- validate examples/eggstack-replay.json` — pass (`eggbench: validate ok`).
- `cargo run -p eggbench-cli --features eggstack-http --locked -- doctor examples/eggstack-replay.json --workload-driver eggreplay-semantic --json` — truthful `missing_executable_path` (no binary installed), descriptor + `SemanticReplay` capability reported.

## 4. Schema, compatibility, and provenance

- Plan schema v4 is additive; v1-v3 remain readable with existing semantics. `SemanticReplay` in v1-v3 fails with `unsupported_option`; `network_path` in v4 fails with `unsupported_option` except the specific `SemanticReplay + network_path` case which fails with `workload_path_incompatible`.
- Resolved-plan schema remains v3 (no shape change); `SemanticReplay` workloads require the `SemanticReplay` capability at resolution. `ResovledPlan` v1/v2 remain readable.
- `semantic-replay.json` schema v1 records driver/adapter version, executable SHA-256/version, envelope/report schemas, fixture relative path (operator context), aggregate digest (comparison-critical), session schema, and flow count. Comparison loads and verifies it; missing evidence for a replay workload fails closed.
- Paired replay is supported through the existing target-rewrite contract (`with_target` preserves fixture); each trial launches a fresh process and no client pool survives across arms/trials.

## 5. Security and lifecycle evidence

- No shell, no fixture mutation, no recording, no credential-bearing route; `--route direct` is explicit.
- Bounded fixture traversal (1024 files, 8 MiB/file, 64 MiB aggregate, 512-byte rel path, 16 depth) and bounded JSON/stdout/stderr (4 MiB/256 KiB, warnings ≤32×512).
- Secrets never enter method/provenance evidence; sentinel test proves a secret-like value does not leak into artifacts beyond the safe JSON contract. EggReplay redaction markers are preserved (no independent reversal).
- Target URL must be loopback (`127.x`, `::1`, `localhost`); otherwise the invocation fails without spawning load.
- Diagnostics run outside measured intervals by construction (the workload has no measured latency; normalization runs after the interval).

## 6. Known limitations and follow-up boundaries

- No live `eggreplay` binary was available locally or on hosted runners (expected); live matching/mismatch qualification (EggServe matching vs changed response, zero-gate failure path, multi-trial unit proof) remains to be recorded when a qualified binary is installed. Parser/fixture/process-absence contracts are hosted and green.
- Recording, `serve` as managed origin, append/re-record, fixture mutation, Python bindings, interception/MITM, WebSocket/SSE controls, timeline scheduler, `network_path` composition, latency/throughput derivation, probe/diagnostic behavior, and security scanning are explicitly non-goals and remain future work.
- M003b (Eggprobe diagnostics + M003 closure) is unblocked by this closure.

## 7. Disposition

**Closed.** M003a delivers schema-v4 semantic replay, the external `eggreplay-semantic` workload, trusted provenance, confined digest identity, pre-start contract verification, one-replay-per-trial semantics with correctness-as-metric mapping, absolute gating, bounded evidence, comparison identity, CLI surfaces, feature isolation, documentation, and local qualification. Hosted four-lane qualification is covered by the combined M003 push and recorded in the M003b umbrella closure.
