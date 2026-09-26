# Security Qualification M001b — Fixed-Corpus HTTP Correctness Family

Status: closed

Repository baseline: `afd9322`

Source roadmap:

- `plans/subsystems/security-qualification-roadmap.md` — M001 / §2A

Controlling architecture:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md` — Phase 7
- `plans/003-planning-process.md`
- `plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md`
- `plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md`
- `plans/adrs/ADR-0003-trial-level-comparison-and-regression-verdicts.md`
- `plans/adrs/ADR-0004-eggstack-composition-and-independent-oracles.md`

Hard prerequisite:

- Security Qualification M001a closed at `plans/closure/security-qualification/001a-status.md` (implementation `1622054`, platform fixture follow-up `d288e57`).

Primary class: capability — deterministic security correctness execution.

## 1. Objective

Add a new correctness family that executes an already-declared, immutable HTTP security corpus against a local/private target and compares only observable HTTP outcomes with owner-authored expectations.

M001b is deliberately not a scanner.

It must:

- consume the M001a corpus contract;
- use the existing Eggfetch HTTP stack rather than implementing a new client;
- run outside every performance measurement interval;
- emit sanitized per-case correctness evidence;
- participate in comparison as a new correctness policy without mutating M004's `eggbench.security-correctness.v1`;
- preserve separate correctness/performance verdicts.

## 2. Ownership boundary

Corpus/profile owner owns:

- why a case exists;
- whether the request is benign, malicious, suspicious, regression, false-positive control, etc.;
- conversion from product-specific semantics to observable expectations;
- category/attack labels;
- target configuration selection.

Eggbench owns:

- corpus validation and immutable identity;
- local/private target confinement;
- HTTP execution mechanics through Eggfetch;
- exact request reproduction within the supported v1 envelope;
- observable response capture;
- comparison of observed response against the declared expectation;
- sanitized evidence;
- correctness aggregation and compatibility/versioning.

Eggbench does not:

- infer attacks;
- classify vulnerabilities;
- generate payloads;
- interpret SynVoid enforcement internals;
- use response timing as correctness;
- derive expectations from baseline observations.

## 3. Current substrate

M004 currently provides:

- `DriverCategory::Correctness`;
- `CorrectnessExecutor` / registry;
- `SecurityCheckResultV1`;
- `CorrectnessObserved::WafBypass`;
- `CorrectnessExpectationRecord::MaxSuccessfulBypasses`;
- `eggbench.security-correctness.v1`;
- ComparisonReceipt v3.

Those semantics remain frozen.

M001b adds a separate fixed-corpus source/family and a new correctness policy.

## 4. New correctness source/family

Canonical source:

~~~text
eggbench-http-corpus
~~~

Canonical family:

~~~text
http_observable
~~~

Descriptor/capability concept:

~~~text
Capability::SecurityCheck {
  family: "http_observable"
}
~~~

This is an Eggbench-native deterministic case executor using Eggfetch, not an Eggsec adapter.

No external scanner binary is involved.

## 5. Plan/schema integration

Extend security-check intent with a typed variant rather than overloading M004's WAF request fields.

Preferred conceptual model:

~~~text
SecurityCheckRequest
  EggsecWaf {
    ...
  }
  HttpCorpus {
    id
    target
    corpus_ref
    timeout_ms
  }
~~~

Alternative struct organization is acceptable if serialized semantics remain explicit and old v6 plans retain their existing meaning.

The request must bind:

- stable check ID;
- target service;
- immutable corpus identity/reference established by M001a;
- bounded per-case timeout;
- correctness source/family.

Do not place attack categories, expected status counts, or payload bytes directly into the experiment plan if they already belong to the corpus contract.

## 6. Experiment-plan compatibility

If M001a already bumped ExperimentPlan for static bindings, M001b may extend that new schema only if additive semantics remain clear. Otherwise bump once more.

Rules:

- all currently readable plan versions remain readable;
- old M004 `security_checks` deserialize identically;
- old explicit fields never acquire new meaning;
- new corpus-check fields on old schemas fail closed;
- resolved-plan schema advances if needed;
- no rewrite/migration of historical bundles.

## 7. Target confinement

Initial M001b is local/private only.

Accepted target URL comes exclusively from `RuntimeBindings.http_url`.

Reject:

- public-routable target host/IP;
- URL userinfo;
- unsupported scheme;
- target authority overridden by corpus cases;
- per-case absolute URLs;
- proxy configuration;
- M002 Eggress route composition;
- arbitrary DNS rebinding to public addresses where the runner can determine it safely.

Loopback is preferred for checked-in and hosted qualification.

The correctness executor does not accept a raw arbitrary URL parameter from the security-check request.

## 8. HTTP request reproduction

Use Eggfetch's supported HTTP transport seam.

Required initial behavior:

- HTTP/1.1 support;
- deterministic method/path/query;
- bounded headers;
- bounded request body;
- inline UTF-8 or M001a-confined body file;
- one corpus case yields one observable result;
- redirect following disabled unless explicitly part of the v1 contract;
- automatic credential injection disabled;
- cookies/session state disabled across cases unless later planned;
- no retry that can hide the first observable outcome unless a transport-level retry policy is explicitly frozen and evidenced.

The implementation should prefer one reusable Eggfetch client per correctness execution where this does not create cross-case state semantics. If pooling affects correctness observability, record the exact policy.

## 9. Lifecycle placement

Reuse the existing correctness phase:

~~~text
prepare
 -> startup
 -> readiness
 -> pre diagnostics
 -> correctness checks
      -> fixed corpus
 -> warmups
 -> measured performance trials
 -> drain
 -> post diagnostics
 -> teardown
 -> final evidence
~~~

No corpus request contributes to `TrialMetrics`.

A valid case-level security failure does not change execution status.

An operational inability to produce trustworthy corpus evidence is Invalid/failure under existing correctness execution rules.

## 10. Case disposition

Initial expectation forms come from M001a:

~~~text
status_exact
status_any_of
~~~

Per case:

~~~text
Pass    = observed HTTP status satisfies expectation
Fail    = valid HTTP response does not satisfy expectation
Invalid = no trustworthy observation can be produced
~~~

Examples of Invalid:

- transport error;
- timeout;
- malformed corpus input that escaped preflight;
- body file digest mismatch;
- target binding disappeared/changed;
- unsupported protocol behavior.

Do not classify a timeout as "blocked" or Pass.

Do not infer challenge/block/drop/tarpit semantics from status codes. The profile owner must encode only the observable status contract it intends to assert.

## 11. Sanitized evidence

Add a versioned result contract distinct from M004's `SecurityCheckResultV1`, or bump to a discriminated security-result schema with explicit backward-compatible variants.

Preferred conceptual case projection:

~~~text
HttpCorpusCaseResultV1 {
  id
  case_sha256
  expectation
  observed_status: Option<u16>
  disposition: pass | fail | invalid
  reason: Option<String>
}
~~~

Run/check result records:

- corpus ID/schema/digest;
- target identity;
- source/family;
- adapter semantic version;
- Eggfetch version/provenance if part of current driver evidence conventions;
- evaluated/pass/fail/invalid counts;
- ordered sanitized cases;
- evidence artifact digest.

Never persist:

- raw attack body bytes;
- arbitrary request body;
- secret-bearing headers;
- response body by default;
- arbitrary server error text.

If request diagnosis requires payload identity, persist only the case/request digest.

## 12. Correctness policy v2

M004's identifier remains immutable:

~~~text
eggbench.security-correctness.v1
~~~

Introduce a new policy identifier, recommended:

~~~text
eggbench.security-correctness.v2
~~~

V2 means:

- both legacy `waf_bypass` and new `http_observable` records can be represented;
- each check is evaluated only against predeclared expectations;
- per-check dispositions remain Pass/Fail/Invalid;
- no security-case statistical inference;
- config/corpus identity participates in comparability;
- aggregate correctness remains Invalid > Fail > Pass;
- final combined precedence remains Invalid > Fail > Inconclusive > Pass.

Do not change the meaning of v1.

## 13. ComparisonReceipt v4

Because the typed correctness observation/expectation model expands, use a clean receipt compatibility boundary.

Recommended:

~~~text
COMPARISON_RECEIPT_SCHEMA_VERSION = 4
~~~

Continue reading v1-v3.

Extend typed records conceptually:

~~~text
CorrectnessObserved
  WafBypass { ... }
  HttpCorpus {
    evaluated_cases
    passed_cases
    failed_cases
    invalid_cases
  }

CorrectnessExpectationRecord
  MaxSuccessfulBypasses { value }
  AllCasesMatch
~~~

Per-case evidence stays in bundle artifacts; the comparison receipt should not duplicate every request/result unless needed for bounded explanation.

Receipt v4 rules:

- legacy v1-v3 semantics remain historical;
- M004-only candidates may continue to produce semantically equivalent v4 receipts if the writer globally advances;
- a v4 correctness section uses the v2 correctness policy identifier when any new family is present;
- no v3 reader is expected to accept v4;
- v4 reader must continue to parse historical receipts.

## 14. Comparability

For `http_observable`, comparison-critical identity includes:

- source/family;
- check ID/order;
- target identity;
- corpus schema/ID/digest;
- expectation-policy version;
- target config digest inherited from profile context when applicable;
- adapter semantic version;
- static target binding identity as defined by M001a.

Observed statuses and Pass/Fail counts are result evidence, not identity.

Under strict comparison, baseline/candidate corpus/config mismatch is a comparability failure. Baseline outcomes never become candidate expectations.

## 15. Interaction with M004 WAF checks

A plan may contain:

- only Eggsec WAF checks;
- only HTTP corpus checks;
- both, if the target/lifecycle constraints are compatible.

Mixed correctness aggregation is conservative across all declared checks.

M001b does not replace the Eggsec adapter.

The profile owner may use one or both families later.

## 16. Paired/network-path behavior

Initial M001b rejects:

- paired + `http_observable`;
- network_path + `http_observable`.

Reason: M001b must establish one unambiguous direct local/private correctness path before per-arm/path semantics are defined.

Ordinary performance workload may use existing features only where current core validation allows the combination without implying corpus traffic shared that path.

Do not infer that correctness traffic traversed Eggress/Eggchaos.

## 17. Cancellation, timeout, cleanup

Each corpus check has:

- bounded whole-check timeout;
- bounded per-case timeout;
- cancellation propagation through Eggfetch;
- no detached requests;
- no retry loop after cancellation;
- evidence finalization only after outstanding case execution is drained/cancelled.

If cases are executed concurrently, concurrency must be bounded and deterministic enough that the corpus contract does not accidentally become a load/stress tool. Initial implementation SHOULD execute serially unless measured evidence shows unacceptable overhead for qualification.

Correctness duration remains untimed diagnostic information.

## 18. Failure semantics

### Preflight/input failure

Before startup where possible:

- corpus digest mismatch;
- unsupported corpus schema;
- forbidden header;
- unsafe body path;
- unsupported expectation;
- invalid target binding.

=> preflight invalid; no managed startup.

### Operational execution failure

After readiness:

- cancellation;
- connection failure;
- timeout;
- protocol failure.

=> affected case/check Invalid under the versioned contract; existing runner failure semantics apply where trustworthy check completion cannot be produced.

### Valid mismatch

Observed HTTP status does not match declared expectation.

=> check Fail; execution may continue to collect performance evidence.

## 19. CLI/inspect

`validate` / profile validation must show corpus-check contract errors.

`doctor` should report:

- fixed-corpus family availability;
- Eggfetch transport support;
- local/private-only restriction;
- unsupported paired/path behavior.

`inspect` should show:

- source/family;
- corpus ID/digest;
- counts;
- per-case ID/disposition/status where bounded;
- no raw request bodies.

Comparison rendering must separate:

- Performance;
- Security correctness;
- Combined.

## 20. Work packages

### A — typed correctness extension

- schema/resolved request variant;
- descriptor/capability;
- validation and compatibility tests.

### B — executor

- Eggfetch-backed local/private case execution;
- strict request reproduction;
- bounded serial execution initially;
- cancellation/timeout behavior.

### C — evidence

- sanitized case/check schemas;
- artifact staging and integrity validation;
- comparability identity.

### D — policy/receipt

- `eggbench.security-correctness.v2`;
- ComparisonReceipt v4;
- v1-v3 compatibility;
- mixed-family aggregation.

### E — CLI/docs

- validate/doctor/inspect/compare rendering;
- example fixed corpus and local echo/WAF fixture;
- ownership/safety documentation.

## 21. Tests

Core/schema:

- old plan/receipt compatibility;
- new corpus check roundtrip;
- mixed M004 + M001b checks;
- old-schema explicit-field rejection;
- paired/path rejection.

Corpus executor:

- exact status Pass;
- set status Pass;
- status mismatch Fail;
- transport failure Invalid;
- timeout Invalid;
- redirect policy pinned;
- credential-bearing header rejected;
- body-file digest mismatch rejected;
- public target rejected;
- request body not persisted.

Evidence:

- missing case;
- duplicate case result;
- bad artifact digest;
- config/corpus mismatch;
- stored/recomputed disposition mismatch;
- observed count consistency.

Receipt:

- v1/v2/v3 fixtures unchanged;
- v4 performance-only;
- v4 WAF-only;
- v4 corpus-only;
- v4 mixed-family;
- Invalid > Fail > Inconclusive > Pass truth table retained.

No contamination:

- corpus response times never enter TrialMetrics;
- number of corpus cases never becomes statistical sample count;
- changing corpus execution duration does not change performance estimates.

## 22. Qualification

Use deterministic local fixtures:

### Case A — all corpus cases satisfy expectations + performance Pass

Final Pass.

### Case B — one malicious/control expectation intentionally mismatches + performance Pass

Execution Completed; correctness Fail; final Fail.

### Case C — corpus Pass + performance Fail

Final Fail.

### Case D — one case operationally Invalid + performance Pass

Final Invalid.

### Case E — corpus Pass + performance Inconclusive

Final Inconclusive.

### Case F — mixed M004 WAF Pass + HTTP corpus Fail

Correctness Fail; final Fail.

No internet target is used.

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
cargo +1.89.0 test -p eggbench-drivers --all-features --locked

git diff --check
~~~

Rerun M004 live/fixture correctness qualification and M003 live-tool qualification to prove no regression in existing correctness/external-process behavior.

Hosted closure requires four normal lanes plus any existing live Eggsec job that M004 marks mandatory for correctness-policy compatibility.

## 24. Closure evidence

Create:

`plans/closure/security-qualification/001b-status.md`

Record:

- implementation commit(s);
- schema versions;
- correctness policy v2 identifier;
- ComparisonReceipt v4 compatibility matrix;
- local/private confinement evidence;
- Eggfetch request reproduction policy;
- sanitized evidence examples;
- Case A-F outcomes;
- no-contamination proof;
- M004 compatibility proof;
- Rust 1.89/all-feature results;
- hosted run IDs;
- unresolved findings.

## 25. Acceptance criteria

M001b closes only when:

1. fixed-corpus execution uses M001a immutable corpus identity;
2. Eggfetch owns HTTP mechanics;
3. Eggbench interprets only observable status expectations;
4. no scanner/payload-generation semantics are added;
5. public targets and credential-bearing corpus inputs are rejected;
6. corpus traffic remains outside performance timing;
7. raw attack payloads are absent from portable evidence;
8. valid expectation mismatch is correctness Fail, not execution failure;
9. operationally untrustworthy evidence is Invalid;
10. M004 correctness policy v1 semantics are unchanged;
11. a new correctness policy identifies the expanded semantics;
12. ComparisonReceipt v4 preserves v1-v3 reading;
13. mixed-family aggregation is conservative;
14. performance cannot hide corpus failure;
15. Rust 1.89 and hosted qualification are green;
16. M001c has a stable single-scenario correctness/comparison substrate.

## 26. Stop conditions

Stop and re-plan if:

- HTTP execution requires a new protocol stack instead of Eggfetch;
- the implementation needs to know whether a payload is actually malicious;
- observed baseline behavior is required to define expectations;
- M004 v1 receipt/policy semantics must change in place;
- raw payload retention is required for comparison correctness;
- public targets are required to close the milestone;
- corpus execution needs stress/load semantics rather than deterministic correctness cases.
