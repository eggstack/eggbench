# Security Qualification M001 — Umbrella Closure

Disposition: closed

Implementation commits:

- M001a: `1622054` (content/identity and static bindings), `d288e57` (fixture follow-up), closure `8683e96`.
- M001b: `a0d49bd` (HTTP correctness family), `afd9322` (compatibility follow-up), closure `33aa90d`.
- M001c: `ed20cb6` (serial suite and receipt), `a51c33a` (A–G typed aggregation matrix), `071786a` (ordinary-run/compare suite integration), `aa76cff` (confined receipt references).

## Contract and evidence

| Requirement | Evidence | Result |
|---|---|---|
| Profile v1 and corpus v1 retain deterministic bounded identity; plan v8 carries fixed HTTP correctness; static bindings remain generic | M001a closure and M001b closure; current `qualification.rs` freezes explicit baseline bundle identity during expansion | Pass |
| Explicit baseline bundle is resolved before candidate run; omission selects absolute-only comparison | `QualificationExpansionV1` stores the verified baseline identity and path context; the suite checks the ordinary comparison receipt against it | Pass |
| Scenarios run serially in profile order through normal run and compare | `qualification_e2e_tests::fixed_corpus_suite_runs_serially_and_continues_after_correctness_fail`: controlled EggServe origin; three ordinary scenarios record Pass, correctness Fail, Pass in declared order | Pass |
| Completed Fail does not prevent later scenarios; invalid run and cancellation stop later starts | E2E Pass/Fail/Pass test; ordinary run non-success handling records Invalid/Cancelled and marks the remaining required scenarios NotRun; runner cancellation/drain/teardown tests remain green | Pass |
| Receipt is written after evidence, binds profile/expansion/corpus/config/scenario identities, and cannot overwrite an existing output | Staging directory, referenced bundle/receipt digests, atomic directory publication, existing-output refusal, receipt digest and ordered-source verification in `qualify inspect` | Pass |
| Receipt references remain inside the suite output and point to exact candidate and comparison evidence | Inspect rejects nonconforming scenario paths and canonicalized paths outside the suite root; E2E negative mutation of a bundle reference returns Invalid | Pass |
| Aggregate uses only typed comparison verdicts with Invalid > Fail > Inconclusive > Pass; correctness and performance stay separately visible | Core A–G truth-table test; ordinary receipt fields are copied and checked on inspect; no trial observations or raw security payloads are read by suite aggregation | Pass |
| No stale output or partial execution is represented as a complete suite | New output directory required; incomplete/cancelled/not-run required scenarios force aggregate Invalid and `execution_complete=false` | Pass |
| CLI validates, expands, runs, and inspects profiles; machine output carries the typed receipt | `eggbench qualify validate/expand/run/inspect`; example profile validate/expand; E2E run and inspect test; documentation updated | Pass |

## Aggregation matrix

| Suite case | Performance | Correctness | Combined / suite |
|---|---|---|---|
| A | Pass | Pass | Pass |
| B | Pass | Fail | Fail |
| C | Fail | Pass | Fail |
| D | Inconclusive | Pass | Inconclusive |
| E | Pass | Invalid | Invalid |
| F | Not run after invalid required scenario | — | Invalid |
| G | Cancelled; later scenarios not run | — | Invalid |

The deterministic A–G policy matrix is a core test. The composed local suite E2E exercises A/B/A ordering and receipt inspection. Performance-specific combinations continue to come from ordinary ComparisonReceipt v4 tests and the suite does not recompute them. Existing runner cancellation tests prove drain and teardown; the suite maps a cancelled run to a cancelled record and marks later scenarios NotRun.

## Schemas and policy versions

- Qualification profile: schema 1.
- HTTP corpus: schema 1.
- Experiment plan: schema 8 for fixed-corpus correctness.
- Expansion: `eggbench.security-profile-expansion.v1`.
- Correctness: `eggbench.security-correctness.v2` for HTTP corpus; WAF v1 remains unchanged.
- ComparisonReceipt: schema 4.
- Qualification receipt: schema 1, policy `eggbench.security-qualification.v1`.
- Aggregate precedence: Invalid > Fail > Inconclusive > Pass; every declared scenario is required.

## Verification

Local verification on the final implementation source:

- `cargo fmt --all -- --check` — pass.
- `cargo check --workspace --all-targets --locked` and all-feature check — pass.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass.
- `EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-targets --locked` — pass.
- `EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-targets --all-features --locked` — pass, including suite E2E.
- Rust 1.89.0 workspace checks, core/runner/CLI all-feature tests — pass (the runner/CLI test invocations used the required parent sentinel).
- Example profile validate/expand — pass.
- Real local qualify run and inspect using EggServe/Eggfetch controlled origin — Pass; generated evidence removed after verification.
- `git diff --check` — pass.

Hosted qualification for final implementation commit `aa76cff`:

- CI run `36249703582` — passed Linux stable, Linux Rust 1.89, macOS stable, and Windows stable.
- Live run `36249703586` — passed M003 live tools, M004a Eggsec, and M004b combined-verdict qualification.

## Findings and handoff

No unresolved code defect is known. No scanner semantics, target-specific behavior, metric recomputation, or automatic baseline discovery was added. Profile target configuration remains an owner-declared content identity as defined by M001a. The full SynVoid scenario contract remains outside M001 and belongs to M002.

M001a, M001b, and M001c are closed. Security Qualification M002 is dependency-ready for implementation planning; re-audit the current SynVoid and Eggsec interfaces at handoff. No authored M002 implementation plan exists yet, so M002 is listed as ready for planning rather than as an executable plan.
