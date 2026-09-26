# Security Qualification M001b Closure

Disposition: closed

Implementation commits:

- `a0d49bd2ab502836a8a9d0824142f80dfbbd2ef6` — fixed-corpus HTTP correctness implementation.
- `afd9322cec6459a34926ca4cb7a129c64505bacd` — mixed-family aggregation coverage and receipt-v4 qualification assertion.

## Requirement-to-evidence matrix

| Requirement | Evidence | Outcome |
|---|---|---|
| Plan/resolved compatibility and explicit HTTP corpus intent | ExperimentPlan v8, ResolvedPlan v6, schema compatibility tests, paired/network-path rejection tests | Pass |
| M001a immutable corpus identity includes request body files | confined corpus loader verifies content-tree digest; body-file mutation test | Pass |
| Eggfetch executes one bounded local/private HTTP/1.1 request per case, serially | `HttpCorpusExecutor`; Eggstack loopback fixture test covers exact/set expectations, status mismatch, sanitized projection, and transport failure | Pass |
| No scanner semantics, redirects, credential injection, or shared cookies | Eggfetch HTTP/1.1 executor uses the bound target only, no redirect/retry/session policy; corpus validation rejects credential, Host, proxy, and hop-by-hop headers | Pass |
| Correctness runs outside performance timing and does not enter TrialMetrics | runner correctness phase placement and reservation; runner correctness/metrics isolation tests | Pass |
| Valid expected-status mismatch is Fail; unusable observation is Invalid | result-contract recomputation tests; executor mismatch and dropped-loopback-port tests; M004 combined-verdict live cases | Pass |
| Sanitized, integrity-checked evidence and corpus/config comparability | per-check `security/<id>.json` evidence; bundle artifact digest validation; HTTP corpus identity participates in comparison | Pass |
| Policy v1 remains unchanged; corpus family selects policy v2 | WAF-only v1 and corpus/mixed-family v2 comparison tests; mixed WAF Pass + HTTP Fail aggregates to Fail | Pass |
| ComparisonReceipt v4 writes while v1-v3 historical receipts remain readable | v1-v3 fixture/parser compatibility (including retained v3 fixture), v4 golden receipts and writer checks | Pass |
| Conservative A-F verdict composition remains intact | hosted M004b qualification: Cases A-F cover Pass, Fail, Invalid, and Inconclusive precedence; HTTP executor and mixed-family tests cover the new correctness family | Pass |
| No internet target is used; local/private restriction is enforced | existing confinement helper and public-target rejection tests; deterministic loopback executor test | Pass |
| CLI, docs, and reproducible example are available | `validate` example check; doctor reports family availability; inspect displays bounded per-case outcomes; `docs/security-qualification.md`, `docs/eggstack-http.md` | Pass |
| Stable and Rust 1.89 verification | hosted CI run [36247323818](https://github.com/eggstack/eggbench/actions/runs/36247323818), all four lanes green; hosted Rust 1.89 checks and core/driver tests green | Pass |
| M004 and live-tool compatibility | hosted live qualification run [36247323829](https://github.com/eggstack/eggbench/actions/runs/36247323829): `live-m004b-linux`, `live-eggsec-linux`, and `live-tools-linux` green | Pass |

## Verification

- `cargo fmt --all -- --check` — hosted stable Linux pass.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — local pass and all hosted stable lanes pass.
- `cargo test --workspace --all-targets --all-features --locked` — local pass and hosted Linux/macOS pass; Windows supported-subset pass.
- `cargo +1.89.0 check --workspace --all-targets --all-features --locked` — local and hosted Linux 1.89 pass.
- Rust 1.89 core, runner, and driver all-feature suites — local pass; hosted core and driver suites pass.
- `git diff --check` — pass.
- `cargo run -q -p eggbench-cli --features eggstack-http -- validate examples/security-http-corpus-plan.json --json` — pass.

## Compatibility and evidence policy

- ExperimentPlan v8 and ResolvedPlan v6 add fixed-corpus intent while preserving historical readers.
- `eggbench.security-correctness.v1` retains its WAF-bypass meaning. `eggbench.security-correctness.v2` represents mixed WAF and observable-HTTP correctness.
- ComparisonReceipt v4 is the current writer schema; v1-v3 receipts remain readable with historical semantics.
- HTTP evidence contains ordered case IDs, request digests, declared status expectations, observed statuses, dispositions, and stable reason codes. Request and response bodies are not persisted.
- The executor is direct and local/private only; corpus correctness requests run serially after readiness and before warmups.

## Limitations and findings

- M001b does not provide profile-level multi-scenario orchestration or a qualification receipt; those remain M001c scope.
- The hosted combined-verdict harness exercises A-F using the established M004 WAF family. HTTP-specific status, operational-invalid, policy-v2, and mixed-family behavior is covered by the new deterministic Eggfetch/core tests.
- No unresolved M001b findings.

M001b is closed. M001c is unblocked and ready at `plans/implementation/security-qualification/001c-qualification-suite-execution-and-m001-closure.md`.
