# Security Qualification M001a Closure

Disposition: closed

Implementation commits:

- `162205439ce39bf526944406008b2d982dedddfe` — profile/corpus/config identity, static bindings, CLI, and shared content-tree identity.
- `d288e57` — Windows platform test fixture updated for schema-v7 service bindings; no production behavior change.

## Requirement-to-evidence matrix

| Requirement | Evidence | Outcome |
|---|---|---|
| Profile v1 remains separate from `ExperimentPlan`; scenarios are explicit, bounded, and ordered | `eggbench-core::SecurityQualificationProfileV1`, validator, deterministic `expand_qualification_profile`, `examples/security-profile.json` | Pass |
| Corpus v1 contains bounded HTTP request data and owner-authored status expectations | `HttpSecurityCorpusV1` validation; credential/header/target rejection tests; checked-in corpus example | Pass |
| Corpus/config/fixture identities are deterministic, bounded, and workspace-confined | shared `content_tree_identity`; order/content/path tests; symlink and escape rejection | Pass |
| EggReplay fixture identity remains byte-for-byte stable | migrated EggReplay integration and golden digest `cbbccb211f6de5ad3850440e5347bd66ce9c637f73410de96446e82727dbf760` | Pass |
| Managed command, external, and adapter services can expose static `http_url`; conflicts fail closed | schema-v7 `Service.http_url`, resolved-plan v5, runtime topology binding propagation, lifecycle tests including adapter cleanup on conflict | Pass |
| Existing experiment plans, resolved plans, bundles, and M004 v1 correctness semantics remain readable/unchanged | old plan/resolved fixture and compatibility tests; no M004 comparison/correctness policy changes | Pass |
| CLI validates and expands a profile without running scenarios | `eggbench qualify expand examples/security-profile.json --json` produced a deterministic expansion | Pass |
| No production dependency on SynVoid or Eggsec was introduced | `cargo tree --workspace --locked -e normal` contains neither package | Pass |
| Stable and Rust 1.89 verification | hosted CI run [36214130746](https://github.com/eggstack/eggbench/actions/runs/36214130746): Linux stable, Linux 1.89, macOS stable, and Windows stable all green; includes all-feature Clippy and the complete all-target workspace test matrix | Pass |
| Existing EggReplay/external-tool qualification after shared identity migration | live qualification run [36214130734](https://github.com/eggstack/eggbench/actions/runs/36214130734): `live-tools-linux` passed | Pass |

## Verification

- `cargo fmt --all -- --check` — pass.
- `cargo check --workspace --all-targets --locked` — pass on Linux, macOS, and Windows hosted lanes.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass on Linux, macOS, and Windows hosted lanes.
- `cargo test --workspace --all-targets --all-features --locked` — pass on Linux and macOS hosted lanes; Windows supported-subset tests passed.
- Rust 1.89 all-target/all-feature checks and core, runner, and driver test suites — pass on the hosted Linux 1.89 lane.
- `git diff --check` — pass.

## Compatibility, security, and lifecycle evidence

- Experiment plans through schema v6 remain readable. Static service URLs require plan schema v7; resolved-plan writes use v5 while v1-v4 remain readable.
- `eggbench.security-correctness.v1` and ComparisonReceipt v1-v3 semantics were not changed.
- Content paths reject absolute paths, parent traversal, symlinks, special files, and bounded-resource overruns. Inputs use fixed-size read buffers and explicit byte/file/depth/path limits.
- Static bindings are non-secret declarations. Managed commands publish them after readiness; external bindings are available at session preparation; adapter conflicts stop the adapter and fail the start.
- Qualification validation and expansion start no managed process and perform no HTTP request.

## Documentation and examples

- Added `docs/security-qualification.md` and profile, corpus, and configuration examples under `examples/`.
- Updated ExperimentPlan and ResolvedPlan schema version constants and the resolved snapshot fixture.

## Limitations and findings

- M001a does not execute corpus cases or apply the destination local/private policy; M001b owns those behaviors.
- No unresolved M001a findings.

M001a is closed. M001b may proceed against these frozen profile, corpus, content-identity, and static-binding contracts.
