# Post-M003 Hosted Qualification Corrective C001 — Current-Tip CI Repair and Closure Reconciliation

Status: ready for handoff

Repository baseline: 29e70e78cf29d05a755a79450c222b5aad8653c2

Source corrective:

- plans/subsystems/post-m003-hosted-qualification-corrective-addendum.md

Failed hosted evidence:

- CI run 36014465034
- implementation/closure baseline HEAD 5ffe87ba9bb352822f85b7780cd15745097dc230

Primary class: closure qualification / portability corrective.

## 1. Objective

Repair the narrow current-tip portability/configuration defects exposed by the first combined hosted qualification after Measurement M003, then produce one authoritative four-lane green run that qualifies the accumulated Measurement M002/M003, External Oracles M001/M002, and Eggstack M001a/M001b implementation state.

Expected production semantic change: none.

Expected source changes are limited to:

1. replace the manual pair-id ceiling division with the standard integer operation;
2. update the Windows-only ResolvedPlan test fixture for schema v2;
3. make feature-dependent expected-count tests warning-free in both default and all-feature builds;
4. add regression/static guards where they prevent recurrence;
5. reconcile closure/registry status only after green hosted evidence exists.

## 2. Exact observed failures

### Linux stable

CI failed at:

~~~text
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
~~~

Finding:

~~~text
crates/eggbench-runner/src/orchestration.rs:1840
(arm, (number + 1) / 2)
clippy::manual_div_ceil
~~~

### macOS stable

Same Clippy finding and location as Linux stable.

### Windows stable

CI failed at:

~~~text
cargo check --workspace --all-targets --locked
~~~

Finding:

~~~text
crates/eggbench-runner/tests/platform.rs:25
error[E0063]: missing field paired in initializer of eggbench_core::ResolvedPlan
~~~

### Default/no-feature configuration warning

Linux cargo check also emitted:

~~~text
crates/eggbench-cli/src/workload_registry.rs:665
variable does not need to be mutable

crates/eggbench-cli/src/workload_registry.rs:666
variable does not need to be mutable
~~~

These are configuration-dependent expected-count variables. They did not cause the first Clippy failure only because runner Clippy stopped earlier.

## 3. Corrective invariants

1. Pair assignment remains:
   - odd measured trial -> Baseline;
   - even measured trial -> Candidate;
   - trials 1/2 -> pair 1;
   - trials 3/4 -> pair 2;
   - and so on.
2. No comparison receipt golden changes.
3. No plan/resolved/trial/manifest schema version changes.
4. Windows platform fixture remains unpaired.
5. Feature-off and all-feature builds remain first-class configurations.
6. No lint suppression is added for the known defects.
7. Rust 1.89 remains the MSRV.
8. Historical closure documents remain historical evidence and are not rewritten to imply earlier hosted qualification.
9. The new corrective closure becomes the additive hosted qualification record.
10. No dependency is introduced.

## 4. Frozen contracts / non-goals

Do not change:

- paired design semantics or alternating schedule;
- pair numbering or arm-specific seeds;
- comparison policy v1/v2 math;
- comparison receipt v1/v2 schemas or goldens;
- CLI comparison exit codes 6/7/8;
- oha/h2load/iperf3 capabilities or parsers;
- EggServe/Eggfetch/Gregg behavior;
- local-runner timing/cleanup;
- driver catalog ownership;
- evidence layout;
- ExperimentPlan, ResolvedPlan, TrialExecutionResult, or manifest schema versions.

Do not weaken -D warnings, pin/downgrade stable Rust to avoid the lint, raise MSRV, add Eggstack M002 work, or mark hosted qualification green from local tests alone.

## 5. Work package A — Preserve pair assignment with standard ceiling division

Change the pair-id calculation from:

~~~rust
(arm, (number + 1) / 2)
~~~

to:

~~~rust
(arm, number.div_ceil(2))
~~~

Rust 1.89 must remain green.

Required regression evidence must lock at least:

| trial | arm | pair |
|---:|---|---:|
| 1 | baseline | 1 |
| 2 | candidate | 1 |
| 3 | baseline | 2 |
| 4 | candidate | 2 |
| 5 | baseline | 3 |
| 6 | candidate | 3 |

Add a direct paired_assignment unit test if current coverage is only indirect. Also test the highest accepted trial ordinal or another boundary case to prove the old number+1 arithmetic was not relied upon for overflow behavior.

## 6. Work package B — Repair Windows ResolvedPlan v2 fixture

Update crates/eggbench-runner/tests/platform.rs to initialize:

~~~rust
paired: None,
~~~

in its unpaired ResolvedPlan fixture.

Do not downgrade the resolved-plan schema, cfg-hide the field, or create a synthetic paired design for this platform-capability test.

Search the entire workspace for direct ResolvedPlan struct initializers. Every direct initializer must explicitly initialize paired or use a shared fixture builder that does so.

If many ad-hoc test constructors exist, a narrow test-support constructor may be introduced, but production resolution must not be refactored merely for this corrective.

## 7. Work package C — Fix feature-dependent CLI test expectations

The production catalog tests currently use mutable expected counts because optional features add adapters.

Refactor the expectations so they are computed without configuration-specific unused mutability. A cfg-aware additive expression/helper is preferred.

Requirements:

- default/no-feature build has no unused_mut warning;
- all-features assertions remain identical in meaning;
- no allow(unused_mut);
- no fake driver enters production;
- production descriptor/workload counts remain truthful.

## 8. Work package D — Default and all-feature configuration matrix

Run locally:

### Default/minimal

~~~text
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
~~~

### All features

~~~text
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-targets --all-features --locked
~~~

The closure record must include both test counts.

## 9. Work package E — MSRV guard

Run:

~~~text
cargo +1.89.0 check --workspace --all-targets --locked
cargo +1.89.0 check --workspace --all-targets --all-features --locked
cargo +1.89.0 test -p eggbench-core --all-features --locked
~~~

If the preferred div_ceil form unexpectedly does not compile on Rust 1.89, stop and select an MSRV-safe warning-free equivalent. Do not raise MSRV in this corrective.

## 10. Work package F — Preserve Measurement M003 semantics

Before hosted handoff, rerun focused paired qualification coverage proving:

- plan schema v1 remains unchanged;
- plan schema v2 paired design validates;
- alternating trial schedule is unchanged;
- pair IDs are unchanged;
- arm-specific deterministic seeds are unchanged;
- v1 comparison goldens are byte-identical;
- v2 paired comparison goldens are byte-identical;
- unpaired comparison of paired evidence remains Invalid;
- compare --paired keeps exit semantics 6/7/8.

This corrective is complete only if the CI repairs are semantic no-ops.

## 11. Work package G — Preserve driver/integration state

Run existing focused smoke/e2e tests for:

- production driver catalog;
- External Oracles oha/h2load/iperf3 parser fixtures;
- EggServe/Eggfetch loopback integration;
- Gregg telemetry fixture;
- evidence bundle verification.

Third-party executables do not need to exist on every hosted runner unless the existing CI already installs them. Fixture/parser coverage remains authoritative for absence-safe hosted qualification.

No adapter semantic code should change.

## 12. Work package H — Fresh hosted four-lane qualification

Push the corrective implementation and require a new GitHub Actions run on that exact corrective HEAD.

Required jobs:

### Linux stable

- fmt;
- workspace check;
- all-feature Clippy with -D warnings;
- workspace all-feature tests.

### Linux Rust 1.89

- workspace all-target check;
- existing core/MSRV tests.

### macOS stable

- workspace check;
- all-feature Clippy with -D warnings;
- workspace tests;
- existing qualified process-group/filesystem tests.

### Windows stable

- workspace all-target check including eggbench-runner/tests/platform.rs;
- all-feature Clippy;
- existing Windows supported-subset tests.

All four jobs must pass on the same commit. Rerunning the old red commit is not sufficient.

## 13. Work package I — Corrective closure record

After all four hosted jobs are green, create:

plans/closure/post-m003-hosted-qualification-corrective/001-status.md

It must record:

- corrective implementation commit(s);
- failed baseline run 36014465034;
- exact known defect classes and fixes;
- local default/all-feature verification;
- Rust 1.89 evidence;
- fresh hosted run ID and four job conclusions;
- confirmation M003 schemas/math/goldens are unchanged;
- confirmation driver/Eggstack semantic code was unchanged, if true;
- any additional CI defect exposed on rerun;
- unresolved findings by severity;
- final disposition.

## 14. Work package J — Closure and registry reconciliation

Only after hosted CI is green:

### Measurement/comparison

- record M002 as hosted-qualified by C001;
- promote M003 from conditionally closed to fully hosted-qualified;
- preserve and link original closure records.

### External Oracles

- record M001/M002 as hosted-qualified by the current-tip C001 matrix;
- preserve original local closure records.

### Eggstack integrations

- record M001a/M001b as cross-platform build/test qualified by C001;
- do not claim real external Gregg daemon or third-party oracle execution on every OS unless CI actually performed it.

### Registry cleanup

Remove/rewrite stale forward-looking text, including:

- After External Oracles M001 closes...;
- External Oracles M002 will add...;
- unconditional closure wording while C001 is still active.

After C001 closes, restore closed/qualified state with the corrective closure link.

## 15. Dependency gate

Until C001 closes:

- this corrective is the only dependency-ready implementation handoff;
- Eggstack M002 may continue planning/research but implementation must wait;
- External Oracles M003 implementation must wait;
- security qualification implementation remains blocked.

This keeps new work from obscuring whether the accumulated current state can pass its own qualification matrix.

## 16. Why this one corrective qualifies the accumulated milestones

Each newer closure record explicitly states that hosted four-lane qualification remained outstanding.

The repository is qualified as one integrated build graph. Therefore one green current-tip matrix after correcting the narrow CI defects supplies additive hosted evidence for:

- comparison v1/v2 and paired runner code;
- eggbench-drivers and external oracle adapters;
- Eggstack optional features;
- CLI catalog integration;
- platform-specific supported subsets.

The closure must still distinguish hosted build/test coverage from local/live third-party tool tests performed during the original milestone closures.

## 17. Static guards

Required:

- git diff --check;
- no allow(clippy::manual_div_ceil);
- no allow(unused_mut) for these findings;
- no ignored/disabled Windows platform test;
- no broad cfg exclusion of platform.rs;
- no new dependency;
- no schema-version change.

If the ResolvedPlan initializer audit finds other stale fixtures, repair them in this same pass.

## 18. Broad verification

Required before push:

~~~text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-targets --all-features --locked
cargo test --workspace --all-targets --locked
cargo +1.89.0 check --workspace --all-targets --locked
cargo +1.89.0 check --workspace --all-targets --all-features --locked
cargo +1.89.0 test -p eggbench-core --all-features --locked
cargo tree --locked
git diff --check
~~~

Then require the fresh hosted matrix in work package H.

## 19. Acceptance criteria

C001 closes only when:

1. paired_assignment uses warning-free arithmetic;
2. pair arm/id semantics remain unchanged;
3. Windows ResolvedPlan fixtures compile with paired: None;
4. all direct ResolvedPlan test constructors are schema-v2 complete;
5. default build has no feature-dependent unused_mut warning;
6. default Clippy passes with -D warnings;
7. all-feature Clippy passes with -D warnings;
8. Rust 1.89 remains green;
9. M003 v1/v2 golden comparison evidence remains unchanged;
10. no adapter/integration behavior changes;
11. Linux stable CI passes;
12. Linux MSRV CI passes;
13. macOS stable CI passes;
14. Windows stable CI passes;
15. corrective closure records exact hosted evidence;
16. registry/roadmaps accurately distinguish historical local closure from current hosted qualification;
17. no unresolved correctness/security/portability finding remains.

## 20. Stop conditions

Stop for planning review if:

- the pair-id lint fix changes serialized pair identities;
- Rust 1.89 cannot support a warning-free pair-id expression without an MSRV change;
- Windows compilation reveals a production schema incompatibility instead of a stale test fixture;
- rerun CI exposes failures in comparison math, lifecycle cleanup, driver parsers, or Eggstack semantics;
- a schema change appears necessary;
- fixing warnings requires weakening CI or broad lint suppression.

A newly exposed narrow cfg/test-hygiene defect may be folded into C001 only if it changes no frozen semantic contract and is explicitly recorded in closure evidence.

## 21. Closure evidence required

Record:

- corrective commit SHA(s);
- narrow diff summary;
- direct ResolvedPlan-initializer audit;
- pair-assignment regression evidence;
- v1/v2 comparison golden result;
- default/all-feature local test counts;
- Rust 1.89 results;
- dependency-tree no-change result;
- failed run 36014465034;
- fresh successful hosted run ID;
- four hosted job conclusions;
- any additional rerun defect;
- qualification disposition for Measurement M002/M003, External M001/M002, Eggstack M001a/M001b;
- registry/roadmap reconciliation commit;
- unresolved findings/severity;
- final disposition.
