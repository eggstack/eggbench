# Security Qualification M001c — Qualification Suite Execution, Receipt, and M001 Closure

Status: ready

Repository baseline: `afd9322`

Source roadmap:

- plans/subsystems/security-qualification-roadmap.md — M001 / §2A

Controlling architecture:

- plans/000-long-term-specification.md
- plans/001-terminology-and-domain-model.md
- plans/002-long-term-roadmap.md — Phase 7
- plans/003-planning-process.md
- plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md
- plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md
- plans/adrs/ADR-0003-trial-level-comparison-and-regression-verdicts.md
- plans/adrs/ADR-0004-eggstack-composition-and-independent-oracles.md

Hard prerequisite:

- Security Qualification M001a and M001b closed; M001b closure evidence is at `plans/closure/security-qualification/001b-status.md`.

Primary class: capability — bounded multi-scenario qualification orchestration and evidence.

## 1. Objective

Complete Security Qualification M001 by adding a bounded suite execution surface over the explicit profile expansion defined by M001a and the correctness/comparison substrate defined by M001b.

The suite layer answers one question:

Did every required scenario satisfy its independently defined correctness and performance gates under the declared profile inputs?

It must not become a second experiment engine.

Each scenario is executed and compared through normal Eggbench run/compare machinery. Each ordinary bundle and comparison receipt remains authoritative evidence.

M001c adds only:

- profile-level orchestration;
- scenario-result indexing;
- immutable qualification receipt;
- conservative suite aggregation;
- profile CLI ergonomics;
- M001 closure evidence.

## 2. Invariants

- One scenario is one normal Eggbench experiment contract.
- Trial-level measurement/statistics remain owned by ordinary comparison.
- Suite aggregation never recomputes metric estimates.
- Suite aggregation never reinterprets security cases.
- Correctness and performance remain separately visible per scenario.
- A failed scenario cannot be outweighed by a passing or faster scenario.
- Invalid evidence dominates the suite.
- Inconclusive remains visible and is not converted to Pass.
- Scenario input identity is frozen by the M001a expansion manifest.
- Scenario order is deterministic.
- Profile execution is bounded and local-first.
- No hidden parallel execution may alter testbed contention semantics.
- Source bundles and comparison receipts remain immutable.

## 3. Non-goals

Do not add:

- weighted security/performance scoring;
- averaging of scenario verdicts;
- cross-scenario bootstrap/statistical inference;
- dynamic scenario generation;
- arbitrary matrix expansion;
- remote scheduler/execution;
- internet-facing scan orchestration;
- automatic baseline selection from historical results;
- target-specific SynVoid logic;
- scanner semantics;
- dashboards/databases;
- background qualification.

## 4. Qualification command surface

Add a profile-oriented CLI rather than overloading ordinary single-plan run.

Recommended:

~~~text
eggbench qualify validate <profile>
eggbench qualify expand <profile>
eggbench qualify run <profile> --output <path>
eggbench qualify inspect <qualification-receipt>
~~~

Exact spelling may follow existing CLI conventions.

qualify run must:

1. parse and validate the profile;
2. produce and freeze the deterministic M001a expansion manifest;
3. execute each declared scenario through ordinary run machinery;
4. produce each scenario comparison receipt through ordinary compare machinery;
5. collect immutable evidence identities only;
6. write one qualification receipt last.

Do not special-case scenario internals in the suite executor.

## 5. Baseline/reference model

Each scenario must declare its normal comparison mode through an explicit suite/profile reference contract.

Initial supported forms:

- absolute-only comparison;
- explicit baseline bundle path/reference resolved before candidate execution.

Do not add automatic latest-successful-baseline lookup in M001c.

Baseline references are inputs and must be frozen before the scenario starts.

If a scenario requires a baseline and it is missing or incompatible, that scenario is Invalid. Never fall back silently to absolute-only.

## 6. Execution ordering

Initial M001c executes scenarios serially in declared profile order.

Reasons:

- avoid hidden host contention;
- preserve reproducibility;
- avoid turning suite orchestration into a scheduler;
- keep cleanup/resource ownership clear.

Parallel scenario execution is deferred until an explicit later plan defines resource isolation and comparability.

Each scenario must fully teardown and finalize before the next starts.

## 7. Scenario state machine

Conceptual:

~~~text
Pending
 -> Validating
 -> Running
 -> Comparing
 -> Completed

or

 -> Invalid
 -> Cancelled
~~~

A correctness Fail or performance Fail is still a successfully completed scenario execution with a failing comparison verdict.

Operational inability to create trustworthy bundle/receipt evidence yields Invalid.

## 8. Failure and continuation policy

Default initial policy: continue after a completed Fail so the final receipt can show all scenario failures, but stop starting new scenarios when infrastructure invalidity or unsafe cleanup makes later evidence untrustworthy.

Required semantics:

- Pass: continue;
- Fail: continue;
- Inconclusive: continue;
- comparison/config Invalid: continue only when scenario isolation proves later evidence trustworthy;
- runner/session corruption, unsafe cleanup failure, or cancellation: stop later starts.

The receipt records whether execution completed.

No unexecuted required scenario may count as Pass.

An unexecuted required scenario makes the suite aggregate Invalid.

## 9. Cancellation and cleanup

On cancellation:

- cancel the current ordinary scenario through existing cancellation machinery;
- complete mandatory drain/teardown;
- do not start later scenarios;
- publish a qualification receipt only if current artifact policy permits a truthful incomplete/cancelled receipt.

Never leave a subject/service from one scenario running into the next.

## 10. Qualification receipt v1

Add an immutable profile-level receipt.

Conceptual:

~~~text
SecurityQualificationReceiptV1 {
  schema_version: 1
  policy_id: "eggbench.security-qualification.v1"
  created_by_version
  profile_id
  profile_sha256
  expansion_sha256
  corpus_identity
  target_config_identity
  scenarios: Vec<QualificationScenarioRecordV1>
  aggregate_verdict
  execution_complete: bool
  warnings
}

QualificationScenarioRecordV1 {
  id
  source_plan_sha256
  candidate_bundle_identity
  baseline_bundle_identity: Option
  comparison_receipt_sha256
  performance_verdict
  correctness_verdict
  combined_verdict
  status
  reason: Option
}
~~~

Reuse existing bundle/receipt identity types where appropriate.

The qualification receipt references, rather than duplicates, scenario metric and security evidence.

## 11. Qualification policy v1

Immutable identifier:

~~~text
eggbench.security-qualification.v1
~~~

Aggregate precedence:

~~~text
Invalid > Fail > Inconclusive > Pass
~~~

Rules:

- any required scenario Invalid => suite Invalid;
- else any required scenario Fail => suite Fail;
- else any required scenario Inconclusive => suite Inconclusive;
- else all required scenarios Pass => suite Pass.

V1 should keep every scenario required/gating. Optional descriptive scenarios are deferred unless a concrete need appears.

Do not count verdicts or derive percentages.

## 12. Correctness/performance visibility

Each scenario record preserves:

- performance verdict;
- correctness verdict;
- combined scenario verdict.

The suite receipt must distinguish:

- performance Pass + correctness Fail;
- performance Fail + correctness Pass;
- performance Inconclusive + correctness Pass;
- correctness Invalid + performance Pass.

The top-level aggregate alone is insufficient evidence.

## 13. Input/evidence identity

Bind at minimum:

- profile schema/ID/digest;
- expansion policy/version/digest;
- ordered scenario IDs;
- source-plan digest per scenario;
- corpus schema/ID/digest;
- target-config digest;
- correctness policy/version;
- candidate bundle manifest identity;
- baseline bundle identity where used;
- comparison receipt digest;
- subject/environment identities already contained in scenario bundles.

Absolute filesystem paths are presentation context, not comparison identity.

## 14. Storage/layout

Use an explicit output directory or receipt path. Do not mutate scenario bundles.

Suggested layout:

~~~text
qualification/
  expansion.json
  scenarios/
    <id>.eggb
    <id>.comparison.json
  qualification-receipt.json
~~~

Final receipt is written only after referenced scenario identities are finalized and verified.

Partial/staging state must be distinguishable from final evidence.

## 15. Atomicity

The qualification receipt follows manifest-last principles:

- stage output;
- compute referenced bundle/receipt digests after finalization;
- validate all references;
- serialize final receipt;
- publish atomically where supported;
- never silently overwrite finalized evidence.

A crash may leave staging artifacts but must not leave a final receipt claiming a complete suite.

## 16. Scenario comparability

Delegate baseline/candidate comparability to ordinary compare.

Suite validation additionally requires:

- scenario ID matches expansion;
- source-plan digest matches expansion;
- profile/corpus/config identity matches the frozen expansion;
- comparison receipt points to the exact candidate bundle;
- baseline identity matches the frozen reference where applicable.

Do not create a weaker suite comparability policy.

## 17. Rendering and exit behavior

Human output should preserve the three verdict layers:

~~~text
Scenario                 Performance   Correctness   Combined
benign-small             pass          pass          pass
malicious-corpus         pass          fail          fail
large-body               pass          pass          pass

Qualification: fail
~~~

Machine JSON exposes the complete typed receipt.

Reuse existing verdict/exit semantics where possible. The typed verdict in JSON remains authoritative.

## 18. Resume/restart

M001c does not add transparent resume.

If execution stops mid-suite:

- finalized scenario bundles remain valid standalone evidence;
- the suite is not final;
- rerun begins from the profile contract again unless a later plan defines safe resume.

Do not silently reuse stale scenario output.

## 19. Interaction with M002

M001c remains subject-neutral.

Security Qualification M002 may later create a SynVoid profile with explicit scenarios such as:

- benign small requests;
- malicious fixed corpus;
- mixed benign/malicious performance workload;
- larger body paths;
- bounded concurrency ladder;
- routed/direct-origin controls;
- CPU/RSS/event-loop telemetry.

M001c itself defines none of those SynVoid semantics.

## 20. Work packages

### A — suite model

- typed qualification request/state;
- explicit baseline-reference model;
- deterministic scenario order;
- bounds.

### B — scenario orchestrator

- invoke ordinary run;
- guarantee teardown before next scenario;
- invoke ordinary compare;
- collect immutable identities;
- enforce cancellation/stop policy.

### C — receipt

- v1 schema;
- policy ID;
- digest/reference validation;
- conservative aggregation;
- atomic publication.

### D — CLI

- qualify run/inspect;
- machine JSON;
- stable exit behavior;
- partial-state diagnostics.

### E — end-to-end fixtures

- multiple scenarios;
- correctness/performance combinations;
- config/corpus mismatch;
- cancellation/cleanup.

### F — M001 closure

- create umbrella closure;
- mark M001 complete;
- unblock M002;
- preserve M001a/M001b closure history.

## 21. Tests

Suite model:

- deterministic order;
- max scenario bound;
- duplicate ID rejection;
- missing baseline rejection;
- expansion/profile digest mismatch.

Aggregation:

- all Pass => Pass;
- any Fail => Fail unless any Invalid;
- any Inconclusive with no Fail/Invalid => Inconclusive;
- any Invalid => Invalid;
- unexecuted required scenario => Invalid.

Execution:

- serial order;
- teardown before next startup;
- Fail continues;
- cancellation stops later starts;
- stale output is not reused;
- partial suite never yields complete final receipt.

Reference integrity:

- wrong bundle manifest digest;
- wrong comparison receipt digest;
- wrong candidate reference;
- baseline differs from frozen reference;
- source-plan digest differs from expansion.

No reinterpretation:

- suite never reads trial observations directly;
- suite never recomputes bootstrap;
- suite never reads raw security payloads;
- aggregate uses typed scenario verdicts only.

## 22. End-to-end M001 qualification matrix

Use deterministic local fixtures.

Suite A — all pass:
- correctness Pass;
- performance Pass;
- qualification Pass.

Suite B — correctness regression:
- performance Pass;
- correctness Fail;
- qualification Fail.

Suite C — performance regression:
- correctness Pass;
- performance Fail;
- qualification Fail.

Suite D — inconclusive:
- correctness Pass;
- performance Inconclusive;
- qualification Inconclusive.

Suite E — invalid security evidence:
- correctness Invalid;
- qualification Invalid.

Suite F — target config digest mismatch:
- profile/comparison invalid;
- qualification Invalid.

Suite G — cancellation:
- cancel during a scenario;
- subject/service cleanup completes;
- later scenarios do not start;
- no complete Pass/Fail receipt is fabricated.

## 23. Verification matrix

~~~text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked

cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-targets --all-features --locked

cargo +1.89.0 check --workspace --all-targets --locked
cargo +1.89.0 check --workspace --all-targets --all-features --locked
cargo +1.89.0 test -p eggbench-core --all-features --locked
cargo +1.89.0 test -p eggbench-runner --all-features --locked
cargo +1.89.0 test -p eggbench-cli --all-features --locked

git diff --check
~~~

Rerun M004 correctness compatibility, M003 live-tool qualification, M001a content/static-binding tests, and M001b corpus correctness qualification.

Hosted closure requires Linux stable, Linux Rust 1.89, macOS stable, and Windows stable green on the exact candidate, plus any inherited mandatory Linux live qualification.

## 24. Closure evidence

Create:

plans/closure/security-qualification/001c-status.md

This is the umbrella Security Qualification M001 closure.

Record:

- M001a closure and implementation SHA;
- M001b closure and implementation SHA;
- M001c implementation SHA;
- profile/corpus/config schema versions;
- content identity policy;
- correctness policy v2;
- ComparisonReceipt v4;
- qualification policy/receipt v1;
- aggregation truth table;
- Suite A-G results;
- cleanup/cancellation evidence;
- no-reinterpretation proof;
- Rust 1.89/all-feature verification;
- hosted runs;
- unresolved findings.

Then reconcile:

- Security Qualification M001 closed;
- M002 becomes next dependency-ready SynVoid suite milestone;
- historical M004 and M001a/M001b evidence remain unchanged.

## 25. Acceptance criteria

M001/M001c closes only when:

1. explicit profile expansion drives ordinary scenarios;
2. scenarios execute serially and independently;
3. ordinary bundles remain authoritative;
4. ordinary comparison receipts remain authoritative;
5. suite aggregation uses typed scenario verdicts only;
6. no metric/statistical recomputation occurs;
7. no security semantic reinterpretation occurs;
8. profile/corpus/config identity is frozen and verified;
9. baseline references are explicit;
10. partial execution cannot publish a complete receipt;
11. Invalid > Fail > Inconclusive > Pass is deterministic;
12. correctness/performance remain separately visible;
13. cancellation cannot leak managed processes into later scenarios;
14. local/private safety remains enforced;
15. M001a/M001b compatibility tests remain green;
16. Rust 1.89 and hosted qualification are green;
17. umbrella closure evidence is committed;
18. M002 has a stable reusable qualification surface.

## 26. Stop conditions

Stop and re-plan if:

- suite execution needs a new scheduler;
- scenario parallelism is required to close M001;
- profile execution must recompute statistics from raw trials;
- a weighted security/performance score appears necessary;
- automatic historical baseline discovery is required;
- safe cleanup cannot be guaranteed between scenarios;
- the qualification receipt cannot reference immutable ordinary evidence without copying or reinterpreting it.
