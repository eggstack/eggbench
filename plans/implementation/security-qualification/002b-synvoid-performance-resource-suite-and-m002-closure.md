# Security Qualification M002b — SynVoid Performance/Resource Suite and M002 Closure

Status: conditionally closed by plans/closure/security-qualification/002b-status.md
(implementation `b74f861`; umbrella M002 closure; shares M002a's live/upstream
conditions)

Repository baseline: `5b5104f734f2e2c252ebb605a1153da10ea96dea`

Source roadmap:

- `plans/subsystems/security-qualification-roadmap.md` — M002

Hard prerequisite:

- Security Qualification M002a closed with a live SynVoid correctness profile
  and frozen SynVoid qualification-asset contract.

Primary class: concrete security/performance qualification suite and milestone closure.

## 1. Objective

Extend the M002a SynVoid correctness profile into a reproducible security +
performance qualification suite using only already-qualified Eggbench workload,
comparison, telemetry, and external-oracle seams.

M002b must prove that a SynVoid candidate cannot be accepted merely because it
is faster when its required WAF behavior regresses.

The suite combines:

- M002a fixed-corpus correctness;
- native Eggfetch proxy workloads;
- independent oha/h2load HTTP oracle workloads where their current capability
  model can truthfully express the same request class;
- direct-origin control scenarios;
- optional/full-profile Gregg host telemetry;
- explicit baseline-relative performance receipts.

## 2. Scope discipline

M002b does NOT add a new load generator or telemetry backend.

Current capability limits are part of the contract:

- Eggbench's oha/h2load adapters do not expose arbitrary headers, bodies,
  multi-URI mixes, connection-churn switches, or scripting;
- M001b fixed-corpus execution is deterministic correctness traffic, not a
  load/stress workload;
- Eggbench has Gregg host telemetry but no generic Prometheus collector.

Therefore M002 v1 explicitly defers:

- mixed malicious/benign traffic under load;
- request-body attack performance campaigns requiring arbitrary POST/body
  driver controls;
- explicit connection-churn/keepalive-disabled performance;
- SynVoid Prometheus/event-loop/queue metric ingestion;
- challenge/stall/tarpit/drop performance semantics;
- HTTP/2/HTTP/3/TLS security-performance variants;
- network-path/fault injection.

Those are candidates for Security Qualification M003 or a focused later
extension. Do not fake them with unrelated metrics.

## 3. Suite variants

Provide two checked-in profile families.

### 3.1 CI/smoke profile

Purpose: deterministic correctness and runtime health on ordinary hosted CI.

Includes:

- M002a WAF correctness scenario;
- small benign SynVoid proxy workload with native Eggfetch;
- large-response proxy workload with native Eggfetch;
- one independent oha or h2load smoke scenario where the tool is available;
- direct-origin control;
- absolute error-rate/status sanity gates.

It must remain bounded and suitable for Linux live qualification.

### 3.2 Baseline-relative qualification profile

Purpose: actual performance-regression qualification.

Includes explicit baseline references for the same scenario IDs and workload
drivers.

No automatic baseline discovery.

The operator materializes/records baseline bundles from the accepted SynVoid
revision first, then runs the candidate profile against those immutable
bundles.

## 4. Scenario inventory v1

Recommended required scenarios:

~~~text
synvoid-waf-correctness
synvoid-benign-small-native-c1
synvoid-benign-small-native-c8
synvoid-benign-small-native-c32
synvoid-large-response-native-c1
synvoid-large-response-native-c8
synvoid-large-response-native-c32
synvoid-benign-small-oha-c8
synvoid-benign-small-h2load-c8
origin-benign-small-native-control
origin-large-response-native-control
~~~

The exact set may be reduced only when a named driver capability or hosted
availability makes a scenario impossible; any reduction must be documented in
the M002 closure.

Concurrency 128 is a manual/full-profile extension, not required for routine CI.
It may be included in the baseline-relative profile if implementation
qualification shows stable host capacity.

## 5. Controlled origin paths

Reuse the M002a EggServe origin with deterministic GET paths.

At minimum:

~~~text
/bench/small
/bench/large
~~~

Recommended responses:

- small: bounded fixed body around 1 KiB;
- large: bounded fixed body around 64 KiB.

The exact bytes and headers are deterministic and identical whether reached
through SynVoid or directly.

These scenarios exercise proxy/request path and response streaming without
pretending to measure request-body WAF scanning.

## 6. Native Eggfetch workload

Use the existing `eggfetch-http` driver.

For each performance scenario:

- target is either SynVoid static `http_url` or direct EggServe control;
- cleartext HTTP/1.1 initially;
- warmups remain outside measured trials;
- multiple measured trials are required;
- concurrency is declared by ordinary workload plan;
- throughput, error_rate, latency mean/p50/p95/p99 are requested where
  available.

Do not add SynVoid-specific client behavior.

## 7. External workload oracles

Use existing oha/h2load adapters only for request shapes they already support.

### oha

Mirror the benign small GET request at a representative concurrency, initially
c8 and optionally c32 in the full profile.

### h2load

Use cleartext H1 mode through the existing adapter for the same benign small
GET class at c8.

Do not numerically compare native Eggfetch and external-oracle results to each
other as if they were samples from the same driver.

Each driver is qualified against its own explicit baseline bundle.

The purpose of the external oracle is independence of load-generation/parser
implementation, not metric equality.

## 8. Performance gates

Every release/profile performance scenario must have an explicit baseline
bundle.

Use existing Measurement M003 statistical-relative comparison.

Initial v1 practical guardrails:

- throughput regression: fail if candidate practical regression exceeds 15%;
- p95 latency regression: fail if candidate practical regression exceeds 20%;
- error_rate: absolute ceiling must remain explicit and should be 0 for the
  controlled local fixture unless implementation evidence proves a small
  nonzero harness artifact is unavoidable.

These thresholds are v1 qualification policy, not universal statements about
all SynVoid deployments.

Before freezing the checked-in profile, run repeated same-revision baseline
qualification on the live host. If same-build noise routinely violates these
guardrails, stop and revise the profile policy with recorded evidence rather
than widening thresholds ad hoc during candidate interpretation.

Statistical uncertainty may yield Inconclusive; it must not be converted to
Pass.

## 9. Correctness/performance independence

The WAF correctness scenario remains separately visible.

Required milestone proof:

### Security regression, performance pass

- modify only the temporary owner-authored corpus expectation or use a
  qualification-only known mismatch fixture;
- performance scenarios remain Pass;
- WAF correctness is Fail;
- M001 suite aggregate is Fail.

### Performance regression, security pass

Inject a qualification-only controlled delay/throttle in the origin or subject
test harness without changing security semantics.

- correctness Pass;
- performance gate Fail;
- suite Fail.

Do not change SynVoid production WAF code to create these test conditions.

## 10. Direct-origin controls

Direct-origin scenarios are required diagnostic/control workloads.

They use the same Eggfetch workload shape against EggServe directly.

They must:

- record throughput/latency/error metrics;
- prove the controlled origin itself is healthy;
- carry their own gates/baselines if required by M001c;
- never be substituted as the SynVoid candidate baseline under strict
  comparability.

A direct origin and SynVoid proxy are different testbed subjects.

## 11. Gregg telemetry

The full qualification profile should support the existing Gregg collector.

Initial normalized resource observations:

- host_cpu_percent;
- host_memory_used_bytes;
- host_memory_percent;
- optionally network rx/tx rates.

These are host/testbed metrics, not SynVoid-process-only metrics.

Evidence must say so explicitly.

For a Gregg-enabled profile:

- Gregg remains an external service;
- Eggbench does not install/manage Gregg;
- endpoint remains loopback under existing policy;
- resource metrics may have explicit baseline-relative gates only after
  repeatability is demonstrated.

M002 closure SHOULD include at least one live Gregg-enabled SynVoid run if the
qualified Gregg daemon can be provisioned in the Linux live job without an
upstream change. If not, record the hosted limitation and rely on the already
qualified Gregg adapter plus a local live run; do not add a SynVoid-specific OS
scraper.

## 12. SynVoid Prometheus metrics

Current M002 scope does not add a Prometheus collector.

SynVoid event-loop, queue, and target-specific Prometheus metrics may be
retained by SynVoid's own operational surfaces, but Eggbench does not normalize
or gate them in M002.

Register this as an explicit M003 candidate rather than scraping Prometheus ad
hoc inside the suite harness.

## 13. Subject/config identity

Every SynVoid scenario must preserve:

- exact SynVoid source SHA;
- binary SHA/version;
- upstream qualification policy/materializer version;
- target config digest;
- corpus digest when correctness is involved;
- static binding;
- build feature profile (`--no-default-features`);
- controlled origin identity;
- workload driver and exact version/provenance;
- Gregg identity when used.

Baseline and candidate must use comparison-compatible profile/config identity
except for the intended subject revision/binary identity dimensions allowed by
the selected comparison policy.

## 14. Baseline materialization workflow

Document an explicit two-stage workflow.

### Stage A — accepted revision baseline

For the accepted SynVoid revision:

1. materialize upstream qualification assets;
2. build the minimal binary;
3. execute each baseline-required scenario;
4. retain the finalized `.eggb` bundles in the profile's expected baseline
   directory;
5. record their manifest digests.

### Stage B — candidate qualification

For candidate revision:

1. materialize candidate assets under the same qualification policy;
2. require corpus/config compatibility or fail comparison;
3. run `eggbench qualify run`;
4. produce ordinary comparison receipts;
5. produce the M001 qualification receipt.

No "latest baseline" lookup.

## 15. Profile/config drift

If a candidate intentionally changes the WAF corpus/config contract, the
existing profile must become Invalid/incomparable rather than silently treating
the new behavior as a performance result.

Updating expected security behavior requires a new profile/config input and
reviewed baseline.

Do not derive new expectations from the candidate.

## 16. Live qualification

Add/extend a Linux live workflow capable of:

- pinned SynVoid checkout/build;
- upstream asset materialization;
- controlled EggServe origin;
- positive WAF correctness;
- native Eggfetch small/large proxy scenarios;
- at least one external-oracle proxy scenario;
- direct-origin controls;
- explicit baseline + candidate run;
- suite inspect/reference-integrity verification;
- negative correctness and negative performance demonstrations.

The live workflow may use a qualification-only local baseline/candidate pair
from the same SynVoid source to prove orchestration mechanics plus injected
fixture-only failure variants.

Do not interpret that same-source run as a claim about a real release's
performance improvement.

## 17. Cross-platform qualification

SynVoid is Linux-primary and M002 live runtime qualification is Linux-only.

Eggbench changes/assets must still pass the normal four-lane CI because the
profile/schema/suite machinery is cross-platform.

Do not claim SynVoid macOS/Windows runtime support from Eggbench CI.

## 18. Documentation

Add a SynVoid qualification guide covering:

- upstream asset contract;
- supported Linux/minimal build;
- baseline materialization;
- candidate qualification;
- correctness vs performance verdicts;
- external-oracle role;
- Gregg host telemetry meaning;
- deferred mixed malicious load/connection churn/Prometheus metrics;
- cleanup and evidence locations.

## 19. Closure

Create:

`plans/closure/security-qualification/002b-status.md`

This is the umbrella M002 closure.

Record:

- M002a closure SHA;
- M002b implementation SHA;
- exact SynVoid source/binary/materializer provenance;
- checked-in profile version;
- scenario inventory;
- performance gate policy;
- same-build repeatability evidence;
- correctness-only regression proof;
- performance-only regression proof;
- native/external oracle results;
- direct-origin control results;
- Gregg evidence or explicit hosted limitation;
- normal four-lane Eggbench CI;
- Linux live SynVoid run;
- unresolved findings and M003 deferrals.

Then reconcile the roadmap:

- M002 closed;
- M003 becomes ready for research/planning;
- deferred mixed-load/churn/Prometheus items are carried explicitly into M003
  rather than forgotten.

## 20. Verification matrix

~~~text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked

cargo +1.89.0 check --workspace --all-targets --all-features --locked
cargo +1.89.0 test -p eggbench-core --all-features --locked
cargo +1.89.0 test -p eggbench-runner --all-features --locked
cargo +1.89.0 test -p eggbench-cli --all-features --locked

git diff --check
~~~

Also rerun inherited M003/M004/M001 live qualification where current release
policy requires it.

## 21. Acceptance criteria

M002/M002b closes only when:

1. M002a correctness profile is closed and green;
2. the SynVoid-owned upstream asset contract remains authoritative;
3. native Eggfetch proxy scenarios execute reproducibly;
4. at least one independent external-oracle proxy scenario is live-qualified;
5. small and large response paths are covered;
6. direct-origin controls are retained as distinct subjects;
7. explicit baseline bundles drive relative performance comparison;
8. throughput/p95/error practical gates are frozen before candidate
   interpretation;
9. correctness-only regression yields suite Fail despite performance Pass;
10. performance-only regression yields suite Fail despite correctness Pass;
11. security/config drift fails closed;
12. no SynVoid-specific load generator or Prometheus scraper is added;
13. host telemetry is labeled truthfully;
14. four-lane Eggbench CI is green;
15. Linux live SynVoid qualification is green;
16. umbrella M002 closure/reconciliation is committed.

## 22. Stop conditions

Stop and re-plan if:

- current workload drivers cannot express even the narrowed benign GET
  performance scenarios;
- same-revision performance noise makes v1 practical gates unusable;
- SynVoid needs subject-specific runner code;
- direct-origin and proxy identities are being compared as if equivalent;
- external-oracle support requires broad driver expansion;
- closing M002 requires mixed malicious load, explicit connection churn, or
  Prometheus ingestion rather than deferring them honestly.
