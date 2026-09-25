# Eggstack Integration M004b — Security Correctness Gate Family and M004 Closure

Status: authored; blocked on M004a closure

Repository baseline: `aeed8f709bb84e840ffd8257b11a832a24211575`

Predecessor:

- `plans/implementation/eggstack-integration/004a-eggsec-strict-waf-correctness-adapter.md`

Subsystem roadmaps:

- `plans/subsystems/eggstack-integration-roadmap.md`
- `plans/subsystems/security-qualification-roadmap.md`

Controlling architecture:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md`
- `plans/adrs/ADR-0003-trial-level-comparison-and-regression-verdicts.md`
- `plans/adrs/ADR-0004-eggstack-composition-and-independent-oracles.md`

Primary class: correctness comparison/gating / Eggstack M004 closure.

## 1. Objective

Make M004a's security-correctness evidence a first-class gate source without
encoding security outcomes as performance metrics.

M004b introduces a distinct correctness section in the comparison receipt and
combines it conservatively with the existing performance verdict:

~~~text
candidate bundle
  |
  +--> performance metric gates --------> performance verdict
  |
  +--> security-check evidence ---------> correctness verdict
                                            |
                                            v
                           conservative combined verdict
~~~

Core rule:

**A performance pass can never override a security-correctness failure.**

Likewise, an invalid correctness contract cannot be converted into a security
pass because performance evidence looks good.

## 2. Ownership boundary

Eggsec owns:

- WAF payload/corpus semantics;
- bypass classification;
- finding semantics.

M004a owns:

- safe execution;
- sanitized security evidence;
- the predeclared maximum-successful-bypass expectation.

M004b owns:

- validating security evidence from immutable bundles;
- producing a typed correctness comparison section;
- combining correctness with performance verdicts;
- receipt compatibility/versioning.

The later Security Qualification subsystem owns:

- broader reusable security profiles;
- multi-domain/corpus contracts;
- richer expected-outcome models;
- SynVoid-specific security suites;
- expansion beyond the initial WAF-bypass correctness family.

M004b must not preempt that roadmap by importing generic scanner semantics.

## 3. Scope

M004b delivers:

1. A versioned security-correctness comparison policy.
2. `ComparisonReceipt` schema v3.
3. A typed `CorrectnessComparisonSection`.
4. Candidate security-check evidence loading and validation.
5. Conservative correctness aggregation.
6. Conservative performance + correctness verdict combination.
7. Candidate-only correctness gating through existing compare flow.
8. Baseline comparison compatibility rules for security-enabled bundles.
9. Stable CLI output/exit semantics.
10. Receipt golden/backward-compatibility tests.
11. Combined real Eggsec + performance qualification.
12. Eggstack M004 closure and Security Qualification M001 unblocking.

## 4. Non-goals

M004b does not add:

- new Eggsec operations;
- new security payload/corpus semantics;
- baseline-derived WAF expected behavior;
- statistical inference over security cases;
- confidence intervals for bypass counts;
- severity weighting;
- risk scores;
- security timing metrics;
- automated exploit interpretation;
- public-target scanning;
- security-performance tradeoff scoring;
- weighted composite scores.

There is no "security score" that can compensate for a correctness failure.

## 5. Why correctness is a separate gate family

Existing metric comparison assumes trial-level scalar observations and supports
absolute/relative/statistical performance budgets.

Security correctness differs:

- the evaluated unit is an Eggsec-defined security case;
- case count does not create independent Eggbench trials;
- the expectation is categorical/bounded correctness;
- one successful bypass may be release-blocking even when latency improves;
- bootstrap inference would answer the wrong question.

Therefore M004b MUST NOT synthesize a `MetricRequest` from
`successful_bypasses`.

## 6. Correctness policy identifier

Introduce immutable identifier:

~~~text
eggbench.security-correctness.v1
~~~

This policy means:

- checks are evaluated against expectations declared in the candidate plan;
- M004 initial family is `waf_bypass`;
- each valid check is Pass or Fail;
- malformed/missing/incompatible evidence is Invalid;
- no statistical method is applied;
- configuration identity participates in comparability;
- aggregate precedence is defined in section 10.

Any semantic change requires a new policy identifier.

## 7. ComparisonReceipt schema v3

Bump:

~~~text
COMPARISON_RECEIPT_SCHEMA_VERSION = 3
~~~

Continue reading v1 and v2.

Add conceptually:

~~~text
ComparisonReceiptV3 {
  ...
  metrics: Vec<MetricComparison>
  performance_verdict: Option<AggregateVerdict>
  correctness: Option<CorrectnessComparisonSection>
  aggregate_verdict: Option<AggregateVerdict>
  ...
}
~~~

For schema v3:

- `performance_verdict` is the conservative aggregate of metric gates only;
- `correctness` is independent security evidence;
- `aggregate_verdict` is the final combined verdict.

For legacy v1/v2:

- their existing `aggregate_verdict` retains its historical metric-only
  meaning;
- readers may project it as performance-only for display;
- do not mutate historical receipt semantics.

Unknown-field/unsupported-version handling remains fail-closed according to
existing receipt policy.

## 8. Correctness receipt model

Add:

~~~text
CorrectnessComparisonSection {
  policy_id: String
  checks: Vec<CorrectnessCheckRecord>
  aggregate_verdict: AggregateVerdict
}

CorrectnessCheckRecord {
  id: Name
  source: Name
  family: Name
  target: Name
  disposition: CorrectnessDisposition
  observed: CorrectnessObserved
  expectation: CorrectnessExpectationRecord
  evidence_path: ArtifactPath
  evidence_sha256: String
  producer_version: String
  producer_sha256: String
  reason: Option<String>
}

CorrectnessDisposition
  Pass
  Fail
  Invalid

CorrectnessObserved::WafBypass {
  evaluated_cases: u32
  successful_bypasses: u32
}

CorrectnessExpectationRecord::MaxSuccessfulBypasses {
  value: u32
}
~~~

Do not add `Inconclusive` to per-check M004 v1 unless a real semantic need
appears. Missing/insufficient evidence is Invalid, not inconclusive.

## 9. Evidence loading

For a candidate bundle declaring `security_checks`:

1. verify bundle integrity;
2. load `security-checks.json`;
3. require index schema/version;
4. load every referenced `security/<id>.json`;
5. verify path/digest/role/sensitivity contract;
6. verify result config matches the resolved candidate plan;
7. verify producer version/SHA and scope identity;
8. recompute each check disposition from typed observed count + predeclared
   expectation;
9. reject any stored disposition that disagrees with recomputation.

Do not trust the persisted Pass/Fail string without recomputing the simple
Eggbench-owned threshold relation.

Do not reopen or parse raw Eggsec payload data.

## 10. Correctness aggregation

Per-check:

- any Invalid evidence -> correctness aggregate Invalid;
- else any Fail -> correctness aggregate Fail;
- else at least one Pass -> correctness aggregate Pass.

An empty correctness section is invalid when the plan declared checks.

A plan without security checks has `correctness = None`.

## 11. Combined verdict precedence

For schema-v3 receipts:

### Evidence invalidity dominates

If either:

- performance verdict = Invalid; or
- correctness aggregate = Invalid;

then final aggregate = Invalid.

### Known failure dominates uncertainty/pass

Else if either:

- performance verdict = Fail; or
- correctness aggregate = Fail;

then final aggregate = Fail.

### Performance uncertainty remains visible

Else if performance verdict = Inconclusive:

final aggregate = Inconclusive.

### Pass

Else if:

- correctness = Pass and performance = Pass; or
- correctness = Pass and no performance gate exists; or
- no correctness section and performance = Pass;

final aggregate = Pass.

### No gates

If neither correctness nor performance provides a gate:

`aggregate_verdict = None`.

This preserves existing conservative ordering:

~~~text
Invalid > Fail > Inconclusive > Pass
~~~

where ">" means stronger precedence, not desirability.

## 12. Security failure never changes execution status

A completed run whose Eggsec observation exceeds its allowed bypass count is:

- `ExecutionStatus::Completed`;
- correctness verdict Fail;
- final comparison verdict Fail.

Do not rewrite execution status to Failed or Invalid.

Operational inability to produce trustworthy security evidence remains an
execution/validity problem under M004a.

This distinction must be covered end-to-end.

## 13. Candidate-only comparison

Extend existing candidate-only absolute comparison so a security-enabled
bundle can be evaluated without a baseline.

Example conceptual flow:

~~~text
eggbench compare --absolute-only candidate.eggb
~~~

It evaluates:

- candidate absolute performance gates that are supported by existing policy;
- candidate security correctness checks.

If the plan has only security checks and no performance metrics, a valid
receipt is still produced.

Exit semantics remain existing comparison semantics:

- Pass -> success;
- Fail -> comparison-fail exit 6;
- Inconclusive/Invalid -> existing mapped exit behavior.

Do not add a special "security failure" process exit code.

## 14. Baseline-dependent comparison

When a baseline participates:

- performance comparison continues under the selected performance policy;
- correctness still evaluates the candidate against the candidate's
  predeclared expectation;
- baseline security outcomes do NOT redefine the candidate expectation.

Under `StrictSameTestbed`, security-check configuration identity must match
between baseline and candidate when both plans declare security checks.

A candidate security-enabled run compared against a baseline without the same
security configuration is a comparison-critical mismatch for M004 v1.

Reason: the test contract changed.

Security Qualification may later define an explicitly baseline-derived
security policy under a new policy/version.

## 15. Comparability dimensions

Extend driver/test configuration comparison to include:

- security-check presence/order;
- IDs;
- source/family;
- target;
- test type;
- allowed successful bypass count;
- concurrency/timeout;
- Eggsec producer compatibility identity;
- scope identity;
- correctness adapter semantic version.

Ephemeral timestamps, process IDs, durations, and actual observed bypass counts
are result evidence, not configuration identity.

## 16. Performance independence

Security execution duration, per-case response time, and Eggsec
`duration_ms`:

- remain diagnostic/raw security evidence only;
- never enter `TrialMetrics`;
- never affect performance estimates;
- never affect bootstrap samples;
- never alter performance gate thresholds.

Add a regression test that injects extreme Eggsec duration values and proves
the performance receipt is byte-identical except for correctness/evidence
identity where appropriate.

## 17. Receipt rendering

Human/JSON CLI output should present separate sections:

~~~text
Performance
  aggregate: pass
  ...

Security correctness
  aggregate: fail
  eggsec-waf/sqli: fail (1 successful bypass; allowed 0)

Combined
  fail
~~~

Do not collapse the explanation to a single generic "metric failed".

Machine JSON uses the typed v3 fields.

## 18. Inspect

`eggbench inspect` continues to show M004a security evidence.

If inspecting a comparison receipt through an existing surface, show:

- correctness policy;
- per-check disposition;
- observed/allowed count;
- final combined verdict.

Never print payload strings.

## 19. Manifest/comparison evidence

The source `.eggb` bundle remains immutable.

Comparison receipt remains standalone content-addressable output under the
existing compare command.

Do not modify source bundle manifests to retroactively add a verdict.

If the existing optional manifest comparison-verdict publishing path is used
elsewhere, only publish the final combined v3 verdict when the corresponding
v3 receipt is staged in the same transaction.

## 20. Security/correctness artifact integrity

Security evidence is release-gating evidence.

Require:

- exact artifact digest verification;
- bounded counts;
- no duplicate check IDs;
- no missing declared check;
- no undeclared extra check;
- producer identity match;
- expectation match;
- observed count <= evaluated cases;
- at least one evaluated case;
- no unknown family in policy v1.

Any inconsistency -> correctness Invalid.

## 21. Backward compatibility

Tests must prove:

- v1 receipt still parses with historical semantics;
- v2 paired receipt still parses with historical semantics;
- v3 performance-only receipt matches current performance verdict behavior;
- v3 correctness-only receipt works;
- v3 combined receipt works;
- v1/v2 serialization goldens remain unchanged;
- a v3 reader never projects correctness into a legacy receipt.

No migration rewrite of historical receipts.

## 22. Paired experiments

M004a rejects paired + security checks, so M004b does not need paired
correctness inference.

A v3 receipt for an ordinary paired run without security checks remains
supported and behaves exactly like current paired policy.

If security checks somehow appear in a paired bundle despite schema
validation, fail closed.

## 23. M002/network-path interaction

M004a rejects security-check/path composition. M004b merely validates this
contract.

Do not infer that Eggsec security traffic traversed Eggress/Eggchaos from the
presence of a network path.

## 24. Real combined qualification

After M004a closes, construct deterministic local candidate bundles proving:

### Case A — security Pass + performance Pass

Expected final: Pass.

### Case B — security Fail + performance Pass

Use the permissive local WAF fixture.

Expected:

- execution Completed;
- performance verdict Pass;
- correctness Fail;
- final Fail;
- comparison exit 6.

### Case C — security Pass + performance Fail

Expected final: Fail.

### Case D — invalid security evidence + performance Pass

Tamper/missing security artifact in a test fixture, not a production run.

Expected final: Invalid.

### Case E — security Pass + performance Inconclusive

Expected final: Inconclusive.

### Case F — correctness-only candidate

No performance gate.

Expected final: Pass or Fail entirely from correctness.

All real WAF observations must come from the M004a pinned Eggsec binary;
comparison behavior itself must also have deterministic unit/golden coverage
without invoking Eggsec.

## 25. Security Qualification boundary

After M004b closes, update:

`plans/subsystems/security-qualification-roadmap.md`

M004b provides the substrate:

- correctness execution category;
- evidence contract;
- independent gate family;
- combined verdict precedence.

Security Qualification M001 then owns broader reusable profile semantics such
as:

- named correctness profiles;
- expected blocked/allowed behavior matrices;
- multiple Eggsec domains;
- corpus versioning/digests;
- reusable target configuration contracts.

Do not duplicate those profile semantics in M004b.

## 26. Tests

### Pure aggregation

Table-test all combinations of:

- performance None/Pass/Fail/Inconclusive/Invalid;
- correctness None/Pass/Fail/Invalid.

Pin exact final verdict.

### Evidence

- missing index;
- missing result;
- bad digest;
- duplicate ID;
- config mismatch;
- producer mismatch;
- zero cases;
- successful > evaluated;
- stored/recomputed disposition disagreement.

### Receipt schemas

- v1 fixture;
- v2 fixture;
- v3 performance-only;
- v3 correctness-only;
- v3 combined;
- unknown version;
- unknown fields according to receipt compatibility policy.

### CLI

- candidate-only security Pass;
- candidate-only security Fail -> exit 6;
- combined security Fail + performance Pass -> exit 6;
- JSON rendering separates sections.

### No-contamination

- correctness data never appears in TrialMetrics;
- security duration changes do not change performance estimates;
- successful-bypass count never becomes a metric sample.

## 27. Verification matrix

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
cargo +1.89.0 test -p eggbench-drivers --all-features --locked

cargo tree --locked
git diff --check
~~~

Rerun M004a live Eggsec qualification plus the post-M003 live external-tool
qualification to prove no regression in sibling-process machinery.

## 28. Hosted qualification

Require on exact M004b candidate:

- Linux stable;
- Linux Rust 1.89;
- macOS stable;
- Windows stable;
- Linux live Eggsec correctness job.

All green before closure.

## 29. Closure

Create:

`plans/closure/eggstack-integration/004b-status.md`

This is the umbrella Eggstack M004 closure.

Record:

- M004a closure/implementation SHA;
- M004b implementation SHA;
- ComparisonReceipt v3 schema evidence;
- correctness policy ID;
- aggregation truth-table evidence;
- real Case A-F outcomes;
- comparison exit behavior;
- backward-compatible v1/v2 fixtures;
- no-contamination proof;
- Eggsec live-tool qualification;
- normal local/MSRV matrix;
- hosted runs;
- unresolved findings.

Then reconcile:

- Eggstack M004 closed/hosted-qualified;
- Security Qualification M001 becomes dependency-ready for its own planning/
  implementation sequence;
- no historical M003/M004a evidence rewritten.

## 30. Acceptance criteria

M004b/M004 closes only when:

1. security correctness remains separate from metrics;
2. ComparisonReceipt v3 is additive/read-compatible with v1/v2;
3. performance-only verdict is retained explicitly in v3;
4. correctness section has an immutable policy identifier;
5. correctness evidence is recomputed/validated from bundle artifacts;
6. security Pass/Fail/Invalid aggregation is deterministic;
7. final precedence is Invalid > Fail > Inconclusive > Pass;
8. performance Pass cannot override security Fail;
9. security Pass cannot override performance Fail;
10. security invalidity cannot be hidden by performance Pass;
11. security Fail leaves execution Completed when tool execution was valid;
12. candidate-only security comparison works;
13. security Fail returns existing comparison-fail exit 6;
14. baseline comparison never derives expectations from observed baseline
    security outcomes;
15. security configuration is comparison-critical;
16. security timing never enters TrialMetrics/bootstrap;
17. paired/path invalid combinations fail closed;
18. legacy v1/v2 receipts remain unchanged and readable;
19. real Eggsec Pass/Fail bundles exercise combined comparison;
20. Rust 1.89 is green;
21. live Eggsec hosted qualification is green;
22. normal four-lane hosted CI is green;
23. umbrella M004 closure/reconciliation is committed.

## 31. Stop conditions

Stop and re-plan if:

- implementing correctness requires encoding security results as metrics;
- legacy comparison receipt semantics would need to change in place;
- a security failure cannot be represented independently of execution status;
- baseline observations must be used to invent candidate security expectations;
- combined verdict precedence would permit a performance result to hide a
  correctness failure;
- M004a evidence cannot be validated without raw payload interpretation;
- paired/path support appears necessary to close the milestone;
- Security Qualification profile semantics begin leaking into this structural
  integration milestone.

## 32. Handoff order

~~~text
M004a closure
 -> ComparisonReceipt v3 schema
 -> correctness evidence loader
 -> deterministic correctness aggregation
 -> combined verdict precedence
 -> CLI/receipt rendering
 -> pure/golden tests
 -> real Eggsec + performance qualification
 -> hosted qualification
 -> M004b/M004 closure
 -> Security Qualification M001
~~~
