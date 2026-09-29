# Security Qualification M003b — Closure

Disposition: closed.

Implementation: `0ec3baa` (`feat(security): add deterministic HTTP corpus load workload`), with strict-lint fixes through final head `58da6f5` (`fix(security): satisfy cross-platform workload lints`).

## Contract evidence

| Requirement | Evidence | Result |
|---|---|---|
| Versioned HTTP corpus load workload with bounded exact schedules | ExperimentPlan schema v10 adds `http_corpus`, validates relative corpus identity, unique known-shape schedule IDs, at most 1,000,000 requests, concurrency at most 256, safe defaults, and requires an explicit seed | Pass |
| Inputs validated before service startup | Runner preflight verifies corpus digest and schedule case IDs before lifecycle startup; corpus body files are loaded and confined by the shared corpus loader | Pass |
| Exact deterministic mixed schedule shared across comparison arms | Seed derivation is stable by pair ordinal and arm-independent; native Eggfetch materializes the exact multiset and records planned and realized schedule digests plus per-case dispatch counts | Pass |
| Expected outcomes remain distinct from transport failures | Native Eggfetch consumes response bodies, reports transport-error and expected-outcome-mismatch metrics separately, and treats an expected 403 as a completed response; mismatch rate is a required absolute-zero primary gate | Pass |
| Pooled and fresh physical-connection policy is explicit and comparable | Both policies are implemented in native Eggfetch and included with schedule/corpus/default-header identity in workload comparison; method evidence records the physical connection contract | Pass |
| Independent external body/churn exercise | The oha adapter supports one repeated owner-authored case, method, safe/default headers, inline/file body, finite closed-loop load, and `--disable-keepalive` for fresh-per-request policy | Pass |
| No security payload copy in dynamic result artifacts | Method evidence retains hashes, counts, statuses, and policy metadata; request/response bodies are consumed but not copied into dynamic artifacts | Pass |
| Focused deterministic tests | Core schema/gate test; native pooled POST and expected-403 test; fresh-connection and mismatch test; stable/seed-sensitive schedule test; oha body/churn argument test | Pass |
| Formatting, workspace check, strict clippy, workspace tests, MSRV, and four hosted CI lanes | Exact-head CI [36503467354](https://github.com/eggstack/eggbench/actions/runs/36503467354), head `58da6f53fe664bf4f59afa168f11e488561e5e89` | Pass |
| Live external-tool qualification workflow | Exact-head live workflow [36503467471](https://github.com/eggstack/eggbench/actions/runs/36503467471), same head | Pass |

## Verification details

Focused tests passed:

- `cargo test -p eggbench-core --lib --locked http_corpus_workload_requires_schema_seed_and_zero_mismatch_gate`
- `cargo test -p eggbench-drivers --lib --all-features --locked http_corpus_load_tests`
- `cargo test -p eggbench-drivers --lib --all-features --locked corpus_argv_binds_method_body_headers_and_fresh_connection_policy`
- `cargo check --workspace --all-targets --locked`
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- `cargo test --workspace --locked` with `EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process` (required by an existing lifecycle test and configured in CI).

The first CI attempt exposed strict-lint findings in the new code; those were corrected before the final exact-head matrix. No test threshold or expected security outcome was relaxed.

## Handoff and dependencies

M003c is ready for its generic collector implementation. Its terminal SynVoid live proof remains blocked on the upstream telemetry-contract plan at `dbowm91/synvoid`, which was rechecked at upstream blob `e60ebfef4953542a581788341dba632cb5af927f` and remains `READY`, not closed. M003d remains blocked on M003c terminal closure and M002 C002. M002 C002 remains blocked on actual real-SynVoid live execution and frozen-policy live-host repeatability; this closure does not claim M002 terminal closure or M003 completion.
