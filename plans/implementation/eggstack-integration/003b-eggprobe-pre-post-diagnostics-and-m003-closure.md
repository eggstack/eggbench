# Eggstack Integration M003b — Eggprobe Pre/Post Diagnostic Evidence and M003 Closure

Status: authored; blocked on M003a closure

Repository baseline: 5e67fd03e8331cabacb8ae6f4f4e1fcdd9e45f5a

Subsystem roadmap:

- plans/subsystems/eggstack-integration-roadmap.md

Predecessor:

- plans/implementation/eggstack-integration/003a-eggreplay-semantic-replay-workload.md

Controlling architecture:

- plans/000-long-term-specification.md
- plans/001-terminology-and-domain-model.md
- plans/002-long-term-roadmap.md
- plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md
- plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md
- plans/adrs/ADR-0004-eggstack-composition-and-independent-oracles.md

Primary class: Eggstack integration / diagnostic lifecycle and evidence.

## 1. Objective

Add first-class pre-workload and post-workload network diagnostics using Eggprobe's qualified machine contract, keeping those observations outside the benchmark measurement interval.

M003b also provides the combined Eggstack M003 closure proof:

~~~text
startup + readiness
  -> Eggprobe pre-workload diagnostics
  -> warmups
  -> measured EggReplay semantic workload trials
  -> workload drain
  -> Eggprobe post-workload diagnostics
  -> service teardown
  -> evidence finalization
~~~

Eggprobe remains authoritative for:

- DNS/TCP/TLS/HTTP diagnostic execution;
- route-safe machine report semantics;
- probe result/error/timing meaning;
- assertion finding meaning;
- its JSON schema.

Eggbench owns:

- when diagnostics execute;
- target binding resolution;
- whether a request is required or optional;
- binary/schema provenance;
- bounded raw evidence retention;
- execution-status effect;
- comparison-critical diagnostic configuration.

Probe timings are diagnostic evidence only in M003b. They are NOT benchmark trial metrics.

## 2. Sibling seam audit — 2026-09-25

Eggprobe current main:

- workspace version 0.1.1;
- Rust 1.89;
- current source schema 0.4;
- contains unreleased Phase 8 native diagnostic work;
- reserves ICMP/UDP/trace/PMTU families whose implementation status varies;
- current main is not the qualified immutable release identity.

Eggprobe's own documentation identifies the qualified release of record as:

- tag v0.1.1;
- commit 53ea53d;
- active machine contract schema 0.3.

The v0.1.1 machine CLI provides:

- eggprobe run <plan>;
- eggprobe run - reading a plan from stdin;
- JSON ProbeReport output for a single plan;
- stable process codes:
  - 0 success;
  - 1 negative probe/assertion outcome;
  - 2 invalid plan/invocation;
  - 3 internal failure;
  - 130 interrupted;
- bounded plan input;
- report-safe route summary with no raw credentials.

Qualified v0.1.1 ProbeReport contains:

- schema_version;
- tool provenance;
- execution_id;
- target summary;
- redacted route summary;
- report status;
- ordered probe results;
- findings;
- warnings.

The current unreleased main still reports package version 0.1.1 while its machine schema is 0.4. Therefore M003b MUST negotiate/enforce schema compatibility and MUST NOT trust SemVer alone.

Production seam: external eggprobe binary using schema 0.3 JSON.

Do not depend on eggprobe-core in Eggbench: that would import Eggprobe's network engine and sibling dependency graph, violating the intended diagnostic ownership boundary.

## 3. Scope

M003b delivers:

1. ExperimentPlan schema v5 diagnostic requests.
2. A canonical Diagnostic driver category and eggprobe diagnostic descriptor.
3. A runner-owned one-shot diagnostic execution seam separate from workload and telemetry.
4. Eggprobe binary resolution/version/SHA and schema-0.3 compatibility handshake.
5. Direct DNS/TCP/TLS/HTTP diagnostic plan generation.
6. Pre-workload and post-workload execution outside measured intervals.
7. Required/optional diagnostic policy.
8. Versioned bounded raw diagnostic evidence.
9. Comparison-critical diagnostic request/provenance identity.
10. CLI validate/doctor/run/inspect integration.
11. A combined EggReplay + Eggprobe M003 end-to-end qualification.
12. Four-lane hosted qualification and M003 closure reconciliation.

## 4. Non-goals

M003b does not add:

- benchmark metrics derived from Eggprobe timings;
- Eggprobe compare statistics;
- Eggprobe route-table/native schema-0.4 features;
- ICMP echo;
- UDP service diagnostics;
- traceroute;
- PMTU discovery;
- routed Eggprobe diagnostics through the M002 network_path;
- automatic reuse of proxy credentials;
- arbitrary Eggprobe plan passthrough;
- Eggprobe assertions as a second Eggbench gate language;
- telemetry polling;
- per-trial probes;
- diagnostic retries;
- remote execution;
- security scanning.

These are future work if demanded.

## 5. Schema v5

Add:

- EXPERIMENT_PLAN_SCHEMA_VERSION_5;
- resolved-plan schema increment if required.

v1-v4 remain supported.

Add:

~~~text
diagnostics: Vec<DiagnosticRequest>
~~~

Default empty for v5.

Schemas v1-v4 must reject an explicit diagnostics field rather than silently accept future semantics.

Conceptual model:

~~~text
DiagnosticRequest {
  id: Name,
  source: Name,
  phase: DiagnosticPhase,
  target: Name,
  probes: Vec<DiagnosticProbe>,
  required: bool,
  timeout_ms: DurationMs
}

DiagnosticPhase
  PreWorkload
  PostWorkload
  Both

DiagnosticProbe
  Dns
  Tcp
  Tls
  Http
~~~

No opaque arbitrary Eggprobe JSON is accepted.

## 6. Diagnostic request validation

Validate before resolution:

- unique diagnostic IDs;
- source initially must be eggprobe;
- non-empty probe set;
- maximum request count, e.g. 32;
- maximum probes/request, e.g. 16;
- target references a declared service or otherwise explicitly supported target;
- timeout nonzero and bounded;
- duplicate probe entries either rejected or deterministically de-duplicated by schema policy; prefer rejection;
- TLS requires target binding with HTTPS semantics at runtime;
- HTTP requires an HTTP URL runtime binding;
- no M003b user-supplied route expression.

The core schema should express intent, not Eggprobe implementation detail.

## 7. Driver category and descriptor

Add DriverCategory::Diagnostic.

Canonical descriptor:

~~~text
eggprobe
~~~

Properties:

- category Diagnostic;
- external_process = true;
- upstream_name = eggprobe;
- machine_output_schema identifies the accepted contract where the existing descriptor type can represent it;
- exact executable version/digest retained in resolved/runtime evidence;
- capability set advertises only M003b supported families:
  - DNS;
  - TCP;
  - TLS;
  - HTTP;
- no ICMP/UDP/trace/PMTU claim;
- no diagnostic timing-as-metric claim.

Do not use Telemetry category for Eggprobe. Telemetry is repeated trial observation; diagnostics are bounded one-shot lifecycle checks.

## 8. External binary substrate

Reuse the trusted external-command resolver/process machinery.

Resolve/probe:

~~~text
eggprobe --version
~~~

Record canonical executable path, SHA-256, and version.

Because SemVer does not distinguish qualified schema 0.3 from unreleased main schema 0.4, immediately perform a machine-contract handshake before managed startup.

## 9. Schema compatibility handshake

Use stdin plan execution with a no-network compatibility request.

Construct a minimal schema-0.3 Eggprobe plan that:

- uses loopback as target;
- route direct;
- has an empty probe list if accepted by the qualified schema;
- uses one bounded execution;
- has no assertions.

Invoke:

~~~text
eggprobe run -
~~~

with the JSON on stdin.

Require:

- bounded execution;
- exit 0;
- valid ProbeReport JSON;
- report.schema_version == "0.3";
- tool name is eggprobe;
- tool version is non-empty;
- safe direct route summary.

If the qualified schema requires at least one probe, instead use another no-external-network contract probe proven by v0.1.1 fixtures; do not connect to an arbitrary public target merely to detect schema.

A binary emitting schema 0.4 is incompatible with the M003b adapter even if its version string is 0.1.1.

Fail before managed startup with diagnostic_contract_unsupported.

Do not automatically reinterpret 0.4 data as 0.3.

## 10. Runner diagnostic seam

Add a runner-level object-safe contract separate from WorkloadExecutor and TelemetryCollector.

Conceptual API:

~~~text
DiagnosticExecutor
  source() -> &str
  preflight(...)
  execute(DiagnosticContext) -> DiagnosticOutput

DiagnosticRegistry
  register(...)
  lookup(...)
~~~

DiagnosticContext includes:

- run_id;
- diagnostic ID;
- phase;
- target runtime bindings;
- cancellation token;
- timeout.

DiagnosticOutput contains:

- bounded raw report artifact;
- typed execution disposition;
- producer/schema provenance;
- optional bounded warnings.

The runner/core contract must remain sibling-neutral. No Eggprobe type crosses into eggbench-runner or eggbench-core.

## 11. Lifecycle placement

Required lifecycle:

~~~text
prepare
 -> startup
 -> readiness
 -> diagnostics(pre_workload)
 -> warmups
 -> measured trials
 -> workload drain
 -> diagnostics(post_workload)
 -> service teardown
 -> final evidence
~~~

Diagnostics are OUTSIDE every WorkloadExecutor measured interval.

Do not use MeasurementSignal to pretend diagnostics are part of a workload invocation.

Do not run diagnostics once per trial.

Pre diagnostics run after all target service readiness and runtime bindings exist.

Post diagnostics run after workload drain while target services are still alive.

## 12. Failure and cleanup precedence

Diagnostics must never bypass cleanup.

### Pre-workload

If a required pre diagnostic returns a negative/unsupported result or operational failure:

- no warmup/measured workload begins;
- execution becomes Invalid;
- services still tear down through the common cleanup tail.

If optional:

- record result/warning;
- continue workload.

### Post-workload

Post diagnostics should run after completed or workload-failed execution when services remain available because they are useful failure context.

If user cancellation is already requested:

- do not prolong shutdown to run nonessential post diagnostics;
- record skipped_due_to_cancellation;
- preserve Cancelled as primary status.

If a required post diagnostic fails after an otherwise Completed workload:

- execution becomes Invalid.

If workload already Failed/Cancelled/Invalid:

- diagnostic outcome is secondary evidence;
- do not replace the primary failure category.

In every case, service teardown remains mandatory.

## 13. Diagnostic failure category

Add a distinct redaction-safe category such as:

~~~text
DiagnosticFailed
~~~

only if the runner's existing failure taxonomy requires a phase category.

Do not map diagnostic negatives to WorkloadFailed or TelemetryFailed.

The persisted diagnostic artifact carries richer source-specific status.

## 14. Target binding lowering

M003b initially uses runtime target bindings rather than arbitrary addresses.

For each target:

### HTTP

- consume http_url;
- method GET;
- target host/port from the bound URL.

### TCP

- derive host/port from http_url;
- direct TCP check.

### DNS

- use the hostname from http_url;
- if the runtime binding is a literal IP, either emit a deterministic skipped/not_applicable diagnostic or reject DNS for that target at preflight; choose one documented behavior and test it.

### TLS

- requires https_url or another explicitly TLS-capable binding;
- do not infer TLS merely from port number;
- if unavailable, required request fails preflight; optional request records unavailable.

Do not fabricate a public DNS name for an ephemeral loopback service.

## 15. Generated Eggprobe plan

M003b owns deterministic lowering into schema 0.3.

Generated plan:

- schema_version 0.3;
- target from runtime binding;
- route Direct;
- requested probe families;
- execution repetitions = 1;
- retries = 0;
- deadline derived from DiagnosticRequest timeout;
- assertions = [].

Eggbench does not use Eggprobe assertions in M003b.

Invoke:

~~~text
eggprobe run -
~~~

Pass generated JSON via stdin.

No temporary plan file is required.

## 16. Exit-code semantics

Interpret:

### 0

Valid report; inspect report status/probes.

### 1

This is a valid negative diagnostic outcome, NOT a process execution failure.

Parse and retain the report.

Apply required/optional policy based on the typed report.

### 2

Invalid generated plan or invocation. This indicates an adapter/contract error and fails the diagnostic.

### 3

Eggprobe internal failure; diagnostic operational failure.

### 130

Cancellation.

Any other exit code is unsupported/malformed execution.

Never discard a valid JSON report merely because exit code is 1.

## 17. Report parser

Accept only schema 0.3 for M003b.

Validate bounds:

- producer name/version;
- execution_id length;
- target summary;
- route must be direct;
- probe count <= requested/repetition bound;
- findings count bounded;
- warnings count/string lengths bounded;
- status enum;
- every probe corresponds to a requested family.

Retain raw ProbeReport JSON as authority.

Do not duplicate Eggprobe's probe timing/error calculations.

## 18. Evidence

### Per diagnostic artifact

Store:

~~~text
diagnostics/pre/<id>.json
diagnostics/post/<id>.json
~~~

or equivalent manifest-safe deterministic paths.

Artifact is Eggprobe's bounded raw ProbeReport JSON.

### Run-level index

Add:

~~~text
diagnostics.json
~~~

Schema v1.

For each execution record:

- diagnostic request ID;
- phase;
- required;
- target service;
- requested probe kinds;
- execution disposition;
- report status;
- artifact path/digest;
- Eggprobe binary version/SHA;
- Eggprobe machine schema 0.3;
- warnings/skipped reason if applicable.

Never copy raw route credentials. M003b uses Direct only.

## 19. Diagnostic timing policy

Probe timing fields remain raw diagnostic evidence.

They MUST NOT:

- enter TrialMetrics;
- become latency/throughput measurements;
- satisfy MetricRequest;
- become primary/diagnostic Eggbench metric gates;
- affect bootstrap comparisons.

Documentation and inspect output must label them diagnostic timings.

A later explicit plan may define a diagnostic workload if benchmarking probe latency itself is desired.

## 20. Comparison identity

Diagnostic configuration is comparison-critical for runs that claim comparable diagnostic context.

Compare:

- presence/order of requests;
- IDs;
- phase;
- required flag;
- target identity;
- requested probe kinds;
- timeout policy;
- Eggprobe producer version/digest;
- machine schema version.

Observed probe statuses/timings are result evidence, not configuration identity.

Under StrictSameTestbed, configuration/provenance mismatch invalidates relative comparison according to the existing comparability framework.

Do not invent a new statistical policy.

## 21. M003a interaction

M003b must not modify EggReplay semantics.

Combined M003 scenario:

- managed EggServe controlled origin;
- M003a eggreplay-semantic workload;
- pre-workload Eggprobe direct diagnostics against origin;
- measured semantic replay trials;
- post-workload Eggprobe direct diagnostics;
- all diagnostic timing absent from TrialMetrics;
- semantic_findings remains the workload correctness metric.

This combined run is the primary M003 closure proof.

## 22. M002 network-path interaction

M003b diagnostics do NOT automatically inherit network_path.

For M003b:

- generated Eggprobe route is Direct;
- diagnostic route/probe evidence describes the target service itself;
- network_path + Eggprobe diagnostics may coexist only if documentation clearly states that diagnostics bypass the benchmark network path.

If that would be misleading for a given plan, validation may reject diagnostics + network_path in M003b. Prefer explicit rejection over silently presenting direct diagnostics as evidence about the routed/faulted path.

Do not translate M002 Eggress route strings into Eggprobe route expressions in this milestone.

## 23. CLI

### validate

Validate schema v5 diagnostic requests and source/probe/phase bounds.

### doctor

Report:

- eggprobe descriptor;
- binary resolution/version/SHA;
- schema compatibility handshake outcome;
- supported M003b probe families;
- explicit unsupported families;
- note that timings are diagnostic-only.

### run

Build DiagnosticRegistry from resolved requests before managed startup.

Required configuration/contract errors fail before startup.

Runtime negative required pre/post results follow section 12.

### inspect

Show:

- diagnostic request identity;
- phase;
- required/optional;
- report status;
- probe kinds/statuses;
- artifact path;
- producer/schema.

Timing may be shown under an explicitly diagnostic label.

## 24. Security and privacy

- no shell;
- no arbitrary caller Eggprobe plan;
- no routed credential input;
- Direct route only;
- bounded stdin/stdout/stderr;
- bounded report fields;
- target comes from declared runtime binding;
- no automatic public target;
- report-safe route summary only;
- cancellation bounded;
- no diagnostics after cancellation that delay cleanup;
- no payload/body capture beyond what Eggprobe's qualified report schema already contains.

Add sentinel tests proving route secret-like values cannot enter generated plans/evidence because route input is not part of M003b.

## 25. Tests

### Schema

- v1-v4 compatibility;
- v5 empty diagnostics;
- valid pre/post/both requests;
- duplicate IDs;
- unsupported source;
- unsupported probe family;
- invalid bounds.

### Resolver/handshake

- trusted binary resolution;
- version capture;
- schema-0.3 handshake pass;
- schema-0.4 response rejection even with same tool SemVer;
- malformed JSON;
- exit 2/3 handling.

### Lifecycle

- pre diagnostics occur after readiness but before warmups;
- post diagnostics occur after workload drain and before teardown;
- diagnostic duration absent from measured elapsed;
- required pre negative prevents workload but still tears down;
- optional pre negative continues;
- required post negative changes Completed to Invalid;
- post diagnostic does not replace existing WorkloadFailed;
- cancellation skips/bounds post diagnostics and tears down.

### Exit 1

Fixture process returns exit 1 plus valid negative report:

- report parsed;
- evidence retained;
- no subprocess-failed misclassification.

### Target lowering

- HTTP;
- TCP;
- DNS hostname behavior;
- literal-IP DNS behavior;
- TLS requires HTTPS-capable binding.

### Evidence

- raw artifact digest;
- diagnostics index;
- producer/schema provenance;
- bounded warnings;
- no timings normalized to TrialMetrics.

### Comparison

Vary diagnostic config/version/schema and verify comparability mismatch handling.

### Combined M003

Prove:

~~~text
EggServe origin
 -> Eggprobe pre diagnostics
 -> EggReplay semantic workload
 -> Eggprobe post diagnostics
 -> teardown
~~~

with:

- semantic replay matching case;
- semantic mismatch case;
- required diagnostic failure case;
- measured interval excluding diagnostic phases.

## 26. Feature/dependency isolation

M003b should not add eggprobe-core or other Eggprobe Rust dependencies.

The eggprobe descriptor/parser can live with external drivers and use no new network crate.

Verify Cargo.lock/tree contain no eggprobe-* production crate.

If an Eggprobe Rust dependency seems required, stop: the selected process/JSON seam should be sufficient.

## 27. Verification matrix

Run:

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

Also rerun M003a targeted tests and M002 path qualification to prove no regression in network-path lifecycle/evidence.

## 28. Hosted qualification

Require one fresh four-lane run on the exact M003b/combined-closure candidate:

- Linux stable;
- Linux Rust 1.89;
- macOS stable;
- Windows stable.

Hosted lanes must execute:

- schema v5 tests;
- diagnostic lifecycle tests;
- exit-1 parsing fixture;
- schema-compatibility fake binary tests;
- combined M003 deterministic tests that do not require an installed external binary.

If real eggprobe/eggreplay binaries are not installed on hosted runners, record live local tool qualification separately and do not claim hosted live-binary coverage.

## 29. M003 closure

After M003b implementation and qualification create:

~~~text
plans/closure/eggstack-integration/003b-status.md
~~~

This is also the umbrella M003 closure evidence.

Record:

- M003a closure SHA/evidence;
- M003b implementation SHA;
- exact EggReplay live qualification binary version/digest;
- exact Eggprobe live qualification binary version/digest;
- EggReplay envelope/report/session schemas;
- Eggprobe accepted machine schema 0.3;
- lifecycle ordering evidence;
- diagnostic required/optional semantics;
- exit-1 behavior;
- combined EggServe/EggReplay/Eggprobe run;
- proof diagnostic timings never enter TrialMetrics;
- default/all-feature/MSRV results;
- cargo-tree proof of no sibling Rust dependency for EggReplay/Eggprobe;
- hosted run ID and four conclusions;
- unresolved findings.

Then reconcile roadmap/registry:

- M003 closed/hosted-qualified;
- M003a/M003b historical closures linked;
- M004 Eggsec becomes plan-authorable if no other blocker remains.

## 30. Acceptance criteria

M003b/M003 close only when:

1. schema v5 diagnostics is additive and v1-v4 remain supported;
2. Diagnostic is a distinct runner/driver category, not Telemetry or Workload;
3. Eggprobe is consumed externally through JSON, not as a Rust networking dependency;
4. qualified schema 0.3 is enforced;
5. same-SemVer schema 0.4 binary fails the compatibility handshake;
6. pre diagnostics run after readiness and before workload;
7. post diagnostics run after workload drain and before teardown;
8. no diagnostic time enters measured workload duration;
9. exit 1 is treated as valid negative evidence;
10. required pre negative prevents workload and still tears down;
11. optional diagnostic negatives warn/continue;
12. required post negative invalidates an otherwise completed run;
13. diagnostic result never masks an earlier workload/cancellation failure;
14. Direct route is explicit;
15. only DNS/TCP/TLS/HTTP families are claimed;
16. raw diagnostic evidence is bounded and versioned;
17. diagnostic configuration/provenance participates in comparability;
18. M003a semantic replay remains intact;
19. combined EggServe + EggReplay + Eggprobe path is tested;
20. no Eggprobe/EggReplay Rust production dependency is introduced;
21. Clippy all-feature -D warnings passes;
22. Rust 1.89 remains green;
23. four hosted lanes pass;
24. M003 closure record and planning reconciliation are committed.

## 31. Stop conditions

Stop and re-plan if:

- Eggprobe v0.1.1 schema-0.3 machine contract cannot be invoked from stdin without network activity for handshake;
- exit 1 cannot reliably preserve a parseable report;
- diagnostic hooks cannot be inserted before teardown without risking cleanup;
- adding diagnostics would change measured interval semantics;
- target bindings are insufficient and implementation starts inventing addresses;
- only schema 0.4 unreleased main can satisfy the needed contract;
- M003 requires route credentials or M002 path translation;
- an Eggprobe Rust dependency appears necessary;
- a schema change beyond the scoped v5 diagnostic contract is required;
- a sibling defect is found that should be fixed upstream.

If an upstream Eggprobe seam is necessary, write/register it in eggstack/eggprobe and block M003b on its closure rather than duplicating diagnostic behavior inside Eggbench.

## 32. Handoff order

~~~text
M003a EggReplay semantic workload
  -> M003a closure
  -> M003b schema/Diagnostic seam
  -> Eggprobe resolver + schema handshake
  -> lifecycle execution/evidence
  -> combined EggReplay + Eggprobe qualification
  -> four-lane hosted qualification
  -> M003b/M003 closure
  -> M004 planning
~~~

Do not implement M003b before M003a closes unless the implementation is strictly isolated to the generic Diagnostic runner/core contract and the branch is not presented as dependency-ready closure work.
