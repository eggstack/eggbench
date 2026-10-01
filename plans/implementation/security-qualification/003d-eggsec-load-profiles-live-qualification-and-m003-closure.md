# Security Qualification M003d — Eggsec Load Profiles, Live Qualification, and M003 Closure

Status: **CONDITIONALLY CLOSED — M003d scope complete and live-verified** (2026-10-01; closure record `plans/closure/security-qualification/003d-status.md`). Eggsec load adapter slice at `739f382`; CI/portability corrective closed at `4703333` (hosted runs `36640125422` four-lane and `36640125453` live); the M003 profile family, explicit baselines, single-owner-instance live workspace, and negative demonstrations are closed. Executing against the real pinned Eggsec exposed and repaired four adapter-contract defects (`--headers` spelling, the load-only execution policy Eggsec requires, `--quiet` suppressing the machine report, and preflight document extraction). Milestone record: `plans/closure/security-qualification/003-status.md`. The milestone inherits M003c's single upstream gate; no Eggbench work remains.

Research baselines:

- original Eggbench planning baseline:
  `eggstack/eggbench@1eb1bfed4edbc6c4af4f193fa71dc82235ad927e`;
- current corrective/reconciliation baseline:
  `eggstack/eggbench@a96407017716b8ca7d18da8237a135b1a49af8cc`;
- Eggsec baseline used for the adapter design:
  `eggstack/eggsec@7ebddbf1669ca1af18957eca27d09e90ed5c718e`
  (current Eggsec main was rechecked during reconciliation; no new upstream
  requirement was identified for the CI repair);
- closed SynVoid M002 asset-contract implementation:
  `dbowm91/synvoid@ae045481752b8f750d6e6079b185c526a09c91d5`;
- closed SynVoid M003 telemetry-v2 corrective implementation:
  `dbowm91/synvoid@739e7ba6f02c5e3f83fe9ff5321b09213182b193`;
- SynVoid current telemetry closeout head observed:
  `0dc1f7fb21a5df60e72fc7f2cd60b7cb73bc9f35`.

## 1. Objective

Prove the M003 substrate with reusable security-service profiles and close the milestone.

The terminal M003 profile must demonstrate:

- body-bearing security-performance load;
- exact benign/malicious mixed load;
- pooled and fresh-connection methods;
- target telemetry plus host telemetry where provisioned;
- correctness/performance independence;
- an Eggsec-owned load/defense-validation execution path;
- real SynVoid live qualification using owner-authored assets.

Eggbench must remain a laboratory/orchestrator, not a scanner.

## 1A. Post-slice corrective gate

Hosted evidence after the adapter slice is stronger than the initial local
verification record.

On exact-head normal CI run `36607614262`:

- Linux MSRV passed;
- Linux stable and macOS stable failed the stale production runtime
  workload/descriptor count assertion after `eggsec-load` was added;
- Windows stable failed strict Clippy because Unix-only permission mutation
  leaves `DirBuilder`/`OpenOptions` unnecessarily mutable on Windows.

The adapter's strict-scope/load semantics are not being redesigned. These
integration/portability defects were repaired under
`003d-corrective-eggsec-load-ci-portability-and-unblock.md`, which closed at
`4703333` with green exact-head four-lane (`36640125422`) and live
(`36640125453`) evidence. Terminal M003d profile work may continue.

That corrective is also what released M002 C002's remaining four-lane closure
criterion; C002 is now closed.

## 2. M002 gate

M003 terminal closure is not allowed until the post-M002 corrective C002 closes.
C002 is closed: the SynVoid owner asset contract that was absent when C002 was
first attempted is closed upstream, and C002's real `live-synvoid-linux`
execution is green on hosted run `36607614200`; its exact-head four-lane
blocker was the M003d adapter defect set assigned to the corrective above, and
C002 revalidated on the repairing source `4703333` with four-lane run
`36640125422` and live run `36640125453` before performing terminal
roadmap/registry reconciliation. This M002 gate is satisfied.

M003a-c substrate may land independently, but this plan must not claim M003
completion while M003c terminal qualification is outstanding.

## 3. Eggsec load driver

Add a narrow external workload adapter, `eggsec-load` or equivalent, using the trusted external-command substrate.

Current Eggsec already supplies the needed upstream surface:

- `eggsec load <url>`;
- finite request count;
- concurrency;
- method;
- request body;
- headers;
- timeout;
- JSON output;
- rate limit via common HTTP args;
- strict scope enforcement through the normal command context;
- production load transport backed by Eggfetch.

No Eggsec repository change is required for initial M003.

The Eggbench adapter must:

1. resolve/probe exact Eggsec executable/version/digest;
2. generate a strict local/private scope manifest using the established Eggsec adapter pattern;
3. run Eggsec preflight before load traffic;
4. pass only reviewed method/body/header shapes;
5. parse bounded JSON;
6. retain bounded raw JSON where safe;
7. normalize throughput/latency;
8. distinguish transport errors from HTTP-status outcomes using Eggsec's categorized results/status distribution;
9. record that Eggsec's production load path currently uses Eggfetch, so it is not an independent transport oracle.

Never fall back to the native Eggfetch workload if Eggsec is unavailable.

## 4. Eggsec role

Use Eggsec as a security-owner execution path, not as an independent load-generator oracle.

Suitable profile cases:

- benign body-bearing request repeated under load;
- one owner-known blocked request repeated under load, with HTTP status treated as expected outcome rather than generic transport failure;
- defense-validation profile demonstrating strict scope and result provenance.

Independent transport corroboration remains oha/h2load where their adapters support the same request class.

## 5. SynVoid M003 profile v2

Create a new checked-in SynVoid M003 profile family rather than mutating M002 v1 evidence in place.

It must retain the owner policy/materializer provenance and include a bounded inventory such as:

~~~text
synvoid-waf-correctness-v1
synvoid-benign-json-body-native-c8
synvoid-mixed-80-20-native-c8
synvoid-mixed-80-20-native-c32
synvoid-benign-small-pooled-c8
synvoid-benign-small-fresh-c8
synvoid-body-native-c8
synvoid-body-oha-c8
synvoid-target-telemetry-pressure-c32
origin-body-control
eggsec-benign-body-c8
~~~

The exact inventory may be adjusted from implementation evidence, but the closure must contain:

- at least one body-bearing case;
- at least one exact mixed schedule;
- both pooled and fresh connection policies;
- at least one target telemetry scenario;
- one strict Eggsec load scenario;
- direct-origin controls where they isolate harness/origin effects.

Ordinary CI remains smaller than the full release/profile suite.

## 6. Correctness/performance independence

Retain explicit negative demonstrations.

### Security-only regression

Use a qualification-only mismatch in immutable expected outcome or owner-approved fixture variant:

- performance metrics remain acceptable;
- correctness fails;
- combined qualification fails.

### Performance-only regression

Use a qualification-only delay/throttle in controlled origin or subject wrapper:

- owner security outcomes remain correct;
- performance gate fails;
- combined qualification fails.

### Telemetry-only diagnostic anomaly

Inject/fixture a required target telemetry disappearance or counter reset:

- trial becomes Invalid/required-telemetry failure according to M003c policy;
- no silent Pass.

Do not modify production SynVoid WAF behavior to manufacture these tests.

## 7. Baselines and repeatability

For every primary performance scenario:

- materialize accepted-revision baseline bundles explicitly;
- no latest-baseline discovery;
- same scenario/driver/request schedule/connection policy/telemetry mapping must match;
- repeat same-revision qualification before setting any new target-resource gate;
- preserve the existing throughput/p95/error policy until evidence justifies a reviewed M003 policy version.

Mixed/body/churn scenarios may initially carry diagnostic performance metrics while repeatability is established.

## 8. Host vs target telemetry

Where Gregg is provisioned:

- retain `host_*` observations;
- collect SynVoid/subject metrics separately as `subject_*`;
- do not derive process CPU/RSS by subtracting host values;
- receipt/report output should make the distinction obvious.

A Gregg-disabled hosted environment may still qualify M003 target telemetry if the target collector is present; record the missing host collector as an explicit environment/profile choice.

## 9. Live workflow

Extend the Linux live qualification workflow with bounded M003 stages after M002 C002 is green.

Required stages:

1. checkout exact pinned SynVoid;
2. materialize owner qualification assets and telemetry contract;
3. build real minimal SynVoid binary;
4. run configtest;
5. start controlled origin / wrapper topology;
6. run fixed-corpus correctness;
7. run native body/mixed/churn M003 scenarios;
8. run target telemetry scenario;
9. run Eggsec load scenario when Eggsec binary is provisioned;
10. run at least one external oracle body/churn scenario where available;
11. validate/inspect every bundle and qualification receipt;
12. retain bounded provenance/evidence artifacts.

A missing optional external binary may skip only an explicitly optional oracle scenario. Required native/SynVoid stages may not pass as NOT-EXECUTED.

## 10. Documentation

Update:

- `docs/security-qualification.md`;
- `docs/synvoid-qualification.md`;
- driver capability documentation;
- metric documentation for `subject_*` versus `host_*`;
- profile/baseline materialization instructions.

Document deferred areas explicitly:

- malformed/raw HTTP and request smuggling;
- multipart/binary upload performance unless owner contracts expand;
- challenge/stall/tarpit/drop timing semantics;
- broad TLS/H2/H3 security matrices;
- network impairment/netem combinations;
- unbounded stress/flood workloads.

## 11. Closure evidence

Create:

`plans/closure/security-qualification/003-status.md`

Record:

- M003a-d implementation SHAs;
- M002 C002 terminal closure reference;
- SynVoid source/binary/materializer/telemetry-contract provenance;
- Eggsec executable/version/digest;
- profile version and scenario inventory;
- request/corpus/schedule/connection-policy identities;
- baseline identities;
- target and host telemetry method;
- same-source repeatability evidence;
- correctness-only/performance-only/telemetry-failure demonstrations;
- external oracle evidence;
- normal four-lane CI;
- live Linux SynVoid M003 run;
- unresolved findings/deferred shapes.

Then reconcile the security roadmap and active registry.

## 12. Acceptance criteria

Security Qualification M003 closes only when:

1. M002 C002 is fully closed;
2. M003a-c are closed;
3. real SynVoid M003 live stages execute;
4. body-bearing load is qualified;
5. exact mixed load is qualified;
6. pooled/fresh connection methods are explicit;
7. target telemetry is collected through the owner contract;
8. Eggsec strict-scope load executes as a separate security-owner path;
9. correctness-only regression defeats acceptable performance;
10. performance-only regression defeats correct security behavior;
11. required telemetry disappearance fails closed;
12. evidence distinguishes host and target resources;
13. four-lane CI and bounded Linux live qualification are green;
14. closure/registry/roadmap are reconciled.

## 13. Stop conditions

Stop and re-plan if:

- closing M003 requires weakening M002 correctness expectations;
- Eggsec needs public-target/wildcard scope;
- SynVoid metrics require authenticated admin scraping or internal IPC parsing by Eggbench;
- mixed workloads cannot preserve exact schedule identity;
- fresh-connection semantics cannot be proven;
- CI requires unbounded/security-stress traffic.

## 14. Handoff

Immediate handoff order:

1. close M003c on the available SynVoid v2 owner contract and real
   Eggbench current-head telemetry trials;
2. resume this plan's profile/baseline/live/negative-demonstration work;
3. write the terminal M003 closure only after all acceptance criteria remain
   satisfied on green hosted evidence.

The CI/portability corrective and the M002 C002 closure it enabled are
complete (`003d-corrective-eggsec-load-ci-portability-and-unblock.md` closed at
`4703333`; C002 closed on runs `36640125422` / `36640125453`).

M003a-b are already closed. SynVoid telemetry v2 is already closed and is no
longer an upstream blocker.

No Eggsec upstream implementation plan is required for this initial milestone.
