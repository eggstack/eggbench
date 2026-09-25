# Eggstack Integration M004a — Eggsec Strict WAF Correctness Adapter

Status: ready for handoff

Repository baseline: `4f714678f7e8791fde52868c97f0e09cb145ab33`

Subsystem roadmaps:

- `plans/subsystems/eggstack-integration-roadmap.md`
- `plans/subsystems/security-qualification-roadmap.md`

Controlling architecture:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md`
- `plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md`
- `plans/adrs/ADR-0004-eggstack-composition-and-independent-oracles.md`

Prerequisites:

- Measurement/comparison M001-M003 closed/qualified.
- Eggstack M001-M003 closed/qualified.
- Post-M003 live external-tool corrective C002 closed:
  `plans/closure/post-m003-live-tool-qualification-corrective/002-status.md`.

Primary class: Eggstack integration / security-correctness execution and evidence.

## 1. Objective

Add a first-class, bounded Eggsec correctness check that executes outside
performance trial timing and reports Eggsec-owned WAF bypass semantics without
turning Eggbench into a scanner.

Initial composition:

~~~text
managed/external target becomes ready
        |
        v
Eggbench SecurityCheckExecutor
        |
        +--> eggsec strict-scope preflight (no network execution)
        |
        +--> eggsec waf --json against declared local/private target
        |
        v
sanitize + validate Eggsec ScanResults
        |
        v
security-checks.json + per-check sanitized evidence
        |
        v
performance workload remains unchanged
~~~

Eggsec remains authoritative for:

- payload generation;
- WAF detection;
- bypass technique execution;
- the meaning of `bypass_successful`;
- severity/category/finding semantics;
- scope/policy enforcement.

Eggbench owns:

- declarative selection of the bounded supported operation;
- target binding and local/private confinement;
- exact executable provenance;
- strict-scope preflight;
- lifecycle placement;
- expectation threshold declared before execution;
- bounded/sanitized evidence;
- conversion of Eggsec's already-semantic bypass result into a typed
  correctness observation.

M004a does not change comparison verdicts. M004b consumes this evidence as a
separate correctness gate family.

## 2. Eggsec seam audit — 2026-09-25

Audited Eggsec default-branch HEAD:

`0509ac668adfd78e9899cd3428a807d0b3c9f27b`

Observed workspace:

- version 0.1.0;
- Rust MSRV 1.89;
- no immutable Git tag/release surfaced by the repository audit;
- broad main crate with many optional offensive/defense-lab capabilities;
- narrow `eggsec-report-model` data-only crate exists but is not the machine
  format emitted by the selected WAF CLI operation.

Relevant public surfaces:

### Strict policy/scope

- global `--scope <file>`;
- global `--strict-scope`;
- `preflight <operation> --target <target> --profile guarded --json`;
- command dispatch calls the same enforcement path before `waf` execution;
- strict/automated surfaces do not honor manual scope/high-risk overrides.

### WAF operation

`eggsec waf <url> --json` emits Eggsec `ScanResults`:

- target;
- timestamp;
- duration_ms;
- optional WAF detection;
- findings;
- summary.

Each WAF finding includes:

- severity;
- category/OWASP semantics;
- `bypass_successful`;
- technique;
- payload;
- response status.

The initial M004a correctness contract consumes only Eggsec's explicit
`bypass_successful` semantics plus structural consistency. Eggbench does not
interpret payload content or severity as proof of correctness.

### Rejected candidate seams

M004a does NOT use:

- the full Eggsec Rust crate graph;
- `eggsec-report-model` as a production dependency;
- generic `eggsec scan --json` / `PipelineReport` as correctness authority
  because stage success primarily means execution success and the pipeline WAF
  stage does not preserve a WAF regression report in the pipeline report;
- `eggsec ci` because it is a passive gate over pre-existing
  `AgentFinding[]`, not an assessment executor;
- `WafRegressionReport` because the audited type exists internally but is not
  an established selected machine CLI seam;
- NSE, packet, stress, remote, C2, db-pentest, web-proxy, Python, daemon, MCP,
  REST, or agent surfaces.

Production seam: external Eggsec CLI process with strict scope and JSON output.

## 3. Scope

M004a delivers:

1. ExperimentPlan schema v6 `security_checks`.
2. A distinct `DriverCategory::Correctness`.
3. A sibling-neutral runner `CorrectnessExecutor` lifecycle seam.
4. Canonical external descriptor `eggsec-waf`.
5. Trusted Eggsec binary resolution/version/SHA provenance.
6. Generated strict loopback/private Eggsec scope manifest.
7. No-network Eggsec preflight before managed startup.
8. One bounded WAF correctness execution per requested check.
9. Sanitized typed result/evidence with no payload bytes retained.
10. Comparison-critical security configuration identity.
11. CLI validate/doctor/run/inspect surfaces.
12. Real-tool qualification against the exact audited Eggsec source revision.
13. Default/all-feature/MSRV/four-lane hosted qualification.

## 4. Non-goals

M004a does not add:

- generic arbitrary Eggsec commands;
- generic scan profiles;
- arbitrary payload files/corpora;
- public/internet targets;
- stress/load/flood operations;
- packet/raw socket operations;
- smuggling/evasion/header-bypass expansion;
- credentials/authenticated scanning;
- custom proxies/routes;
- M002 network-path translation;
- NSE;
- database pentesting;
- web interception;
- C2/post-exploitation;
- remote/cluster execution;
- Eggsec daemon/REST/MCP/agent integration;
- security timing as a benchmark metric;
- severity heuristics in Eggbench;
- baseline-dependent security comparison;
- performance-verdict combination.

Those require later explicit planning.

## 5. Schema v6

Add:

- `EXPERIMENT_PLAN_SCHEMA_VERSION_6`;
- additive `security_checks` field.

Conceptual shape:

~~~text
security_checks: Vec<SecurityCheckRequest>

SecurityCheckRequest {
  id: Name,
  source: Name,                 // initially "eggsec-waf"
  target: Name,                 // declared service/runtime binding
  test_type: EggsecWafTestType,
  max_successful_bypasses: u32,
  concurrency: PositiveCount,
  timeout_ms: DurationMs
}

EggsecWafTestType
  Sqli
  Xss
  Ssrf
  Cmd
  Traversal
~~~

Do not expose `all` initially: one typed family per check keeps expectations
and evidence attributable and bounded.

v1-v5 remain readable. Presence of `security_checks` on v1-v5 fails closed
with `unsupported_option`, even when the field is empty.

Bounds:

- maximum 16 security checks/run;
- unique IDs;
- concurrency 1..=32;
- timeout 1s..=120s;
- successful-bypass threshold bounded by the maximum accepted finding count;
- no duplicate `id`.

## 6. Correctness is not a metric

Do NOT create metrics such as:

- `eggsec_bypasses`;
- `security_pass`;
- `waf_correctness_ratio`.

M004a's typed security result is separate from `TrialMetrics`.

Reason:

- payload cases are not Eggbench performance trials;
- the security result is categorical correctness evidence;
- bootstrap/ratio inference is inappropriate;
- the security roadmap requires correctness and performance gate families to
  remain independent.

M004b will integrate the correctness result into comparison receipts without
routing it through `MetricRequest`.

## 7. Driver category and capability

Add:

~~~text
DriverCategory::Correctness
~~~

Canonical descriptor:

~~~text
eggsec-waf
~~~

Capabilities should be explicit, e.g.:

~~~text
Capability::SecurityCheck {
  family: "waf_bypass"
}
~~~

Descriptor facts:

- external_process = true;
- upstream_name = `eggsec`;
- adapter version = Eggbench package version;
- upstream version from `eggsec --version`;
- executable SHA-256 retained;
- no default correctness driver;
- compatible service types require a runtime HTTP URL binding.

Do not overload `Diagnostic`: diagnostics establish environment/target
health; correctness checks determine whether security behavior meets a
predeclared expectation.

Do not overload `Workload`: the performance workload remains the statistical
trial workload.

## 8. External-process substrate

Reuse the existing trusted external-command machinery:

- canonical executable resolution;
- explicit argv;
- no shell;
- bounded stdout/stderr;
- timeout/cancellation;
- process-group ownership;
- environment clearing;
- SHA-256 executable provenance.

Version probe:

~~~text
eggsec --version
~~~

The adapter MUST NOT trust workspace SemVer alone because current Eggsec is an
evolving 0.1 tree without an immutable release selected for this contract.

The parser fails closed on incompatible JSON.

## 9. Qualification pin and removal gate

Initial live qualification uses exact Eggsec source:

`0509ac668adfd78e9899cd3428a807d0b3c9f27b`

Build only for qualification:

~~~text
cargo build --locked --release -p eggsec-cli --no-default-features
~~~

Record:

- source SHA;
- Cargo.lock SHA-256;
- binary SHA-256;
- binary size;
- `eggsec --version`;
- Rust/Cargo versions.

This is NOT a production Git dependency.

Removal gate: once Eggsec publishes an immutable release carrying the selected
strict-preflight + WAF JSON contract, future qualification should pin that
release/tag instead of a source revision.

## 10. Target confinement

Initial M004a supports only HTTP targets obtained from declared runtime
bindings.

Accepted:

- `127.0.0.0/8`;
- `::1`;
- RFC1918 IPv4;
- IPv6 ULA/link-local only where the runner can establish the binding
  unambiguously;
- `localhost` / explicitly local names.

Reject public targets before spawning Eggsec.

Prefer loopback in all checked-in/hosted qualification.

No arbitrary URL string is accepted from `security_checks`.

## 11. Generated scope manifest

For each run, generate a temporary Eggsec scope file from the resolved runtime
target.

Requirements:

- `require_explicit_scope = true`;
- allow only the exact resolved target host/IP required by the check;
- no wildcard broader than the declared local target;
- no credentials;
- no public network rule;
- file created inside runner-controlled temporary state;
- restrictive file permissions where supported;
- deterministic canonical content;
- SHA-256 recorded as security configuration identity;
- removed during cleanup.

Do not accept a caller-supplied arbitrary Eggsec scope file in M004a.

## 12. Pre-start policy handshake

Before any managed subject/service starts, execute a no-network policy preview
against the selected binary and generated scope:

~~~text
eggsec   --scope <generated-scope>   --strict-scope   --json   preflight waf   --target <resolved-host-or-url>   --profile guarded
~~~

The exact ordering/argv must match current CLI parsing during implementation.

Require:

- process success;
- bounded valid JSON;
- operation identity resolves to `waf`;
- decision is allowed;
- no required manual override;
- target/scope facts correspond to the generated local scope.

Fail before startup on:

- missing binary;
- malformed preflight JSON;
- denied operation;
- unsupported feature/capability;
- scope mismatch;
- binary contract mismatch.

Stable categories should include:

- `security_driver_missing`;
- `security_contract_unsupported`;
- `security_scope_denied`;
- `security_target_incompatible`.

Do not bypass a denial with `--yes` or any `--allow-*` flag.

## 13. Runner lifecycle

Add a sibling-neutral executor contract, conceptually:

~~~text
CorrectnessExecutor {
  source() -> &str
  preflight(...)
  execute(CorrectnessContext) -> CorrectnessOutput
}

CorrectnessRegistry
~~~

Execution placement:

~~~text
prepare
 -> service startup
 -> readiness
 -> diagnostics(pre), when requested
 -> correctness checks
 -> warmups
 -> measured performance trials
 -> drain
 -> diagnostics(post)
 -> teardown
 -> final evidence
~~~

Rationale:

- security behavior is established on the ready target before performance
  trials;
- it is outside every measured interval;
- a correctness failure must not prevent performance evidence from being
  collected, because M004b must be able to report "security failed and
  performance passed/regressed" independently.

Therefore:

- an Eggsec *observed successful bypass* does NOT make execution
  `ExecutionStatus::Failed` or `Invalid`;
- an Eggsec operational/process/policy/schema failure makes the run invalid or
  failed according to the existing phase-failure taxonomy;
- the runner continues into performance trials after a valid correctness
  observation, whether that observation passes or fails its expectation.

This distinction is mandatory.

## 14. Actual Eggsec invocation

For each check, invoke only the bounded selected operation:

~~~text
eggsec   --scope <generated-scope>   --strict-scope   --json   waf <runtime-http-url>   --bypass   --test-type <family>   --concurrency <N>   --timeout <seconds>
~~~

Do NOT pass:

- `--header-bypass`;
- `--smuggling`;
- `--evasion`;
- credentials;
- proxies;
- manual override flags.

No shell.

One SecurityCheckRequest produces one process invocation.

## 15. Machine-output validation

Parse the real `ScanResults` shape narrowly.

Require:

- target present and compatible with the requested binding;
- findings array bounded;
- summary present;
- every finding has:
  - severity string bounded;
  - technique bounded;
  - boolean `bypass_successful`;
  - response_status;
- summary.total_findings == findings.len();
- summary bypass-success percentage is finite and consistent within a small
  serialization tolerance with counted booleans;
- optional WAF detection request_error is absent for a valid observation.

Do not derive correctness from:

- severity;
- title text;
- payload text;
- HTTP status alone;
- WAF product name.

Correctness observation:

~~~text
successful_bypasses = count(finding.bypass_successful == true)

Pass  iff successful_bypasses <= max_successful_bypasses
Fail  otherwise
~~~

The user-declared maximum exists before candidate execution. Eggsec owns the
meaning of `bypass_successful`; Eggbench only applies the declared threshold.

Require at least one evaluated finding for a valid check. A zero-case output
is Invalid, never a vacuous Pass.

## 16. Security-check result schema

Add versioned Eggbench-owned result:

~~~text
SecurityCheckResultV1 {
  schema_version: 1
  id
  source
  target
  test_type
  disposition: pass | fail | invalid
  evaluated_cases
  successful_bypasses
  allowed_successful_bypasses
  producer_version
  producer_sha256
  scope_sha256
  sanitized_cases[]
}

SanitizedCase {
  technique
  severity_label
  response_status
  bypass_successful
}
~~~

Do not persist payload bytes or arbitrary descriptions.

If needed for diagnosis, preserve a SHA-256 of the payload string, not the
payload itself.

## 17. Evidence

Run-level index:

~~~text
security-checks.json
~~~

Schema v1 records:

- adapter identity/version;
- Eggsec version/SHA;
- exact audited operation;
- generated scope digest;
- ordered request/result summaries;
- artifact paths/digests;
- lifecycle placement version.

Per check:

~~~text
security/<id>.json
~~~

contains only the sanitized typed result.

Child stderr may be retained under existing bounded/redacted rules if it
contains no raw payloads/secrets.

Raw Eggsec stdout with payload strings MUST NOT be placed in a public bundle
artifact. Parse it in memory and persist only the sanitized projection.

If the shared external-command substrate automatically stages raw stdout for
this path, M004a must add an explicit sensitive-output suppression/sanitizing
boundary rather than leaking payloads.

## 18. Evidence sensitivity

- plan/resolved plan: existing redaction policy;
- generated scope contents: do not persist raw path; persist canonical safe
  identity/digest and target class;
- sanitized security result: `Sensitivity::Redacted` initially;
- raw Eggsec stdout: not retained;
- raw stderr: redacted/bounded.

Add sentinel tests with secret-like strings in unrelated environment/config
ensuring no security evidence leaks them.

## 19. Comparison identity

M004a extends driver/testbed comparability identity but does not produce final
correctness verdict combination.

Comparison-critical security configuration:

- ordered check IDs;
- source/driver;
- target service identity;
- test type;
- max successful bypasses;
- concurrency;
- timeout;
- Eggsec executable version/SHA;
- generated scope digest;
- correctness adapter semantic version.

Result values (pass/fail/count) are not configuration identity.

A bundle with security checks must not compare strictly against a bundle with
different security-check configuration without an explicit mismatch.

## 20. CLI

### validate

Validate schema v6 security-check intent and bounds.

### doctor

Report:

- `eggsec-waf` descriptor;
- resolved binary/version/SHA;
- supported family: `waf_bypass`;
- supported test types;
- explicit unsupported Eggsec operations;
- strict-scope/preflight compatibility.

Doctor must not send security traffic.

### run

Perform Eggsec preflight before managed startup; execute correctness checks
after readiness and outside performance timing.

### inspect

Show:

- check ID;
- source;
- test type;
- disposition;
- evaluated case count;
- successful bypass count/allowed count;
- producer version/SHA;
- evidence artifact.

Do not print payload strings.

## 21. Interaction with existing features

### Diagnostics

Allowed. Ordering is pre-diagnostics -> correctness -> workload -> post-diagnostics.

### Semantic replay

Allowed as the performance/correctness-independent workload. Security
correctness remains a separate phase.

### network_path

Rejected in M004a for Eggsec security checks. Do not translate the Eggbench
Eggress path into Eggsec proxy arguments.

### paired

Reject `security_checks + paired` in M004a. A single run-level security check
cannot truthfully represent two simultaneously live arms without an explicit
per-arm correctness schedule.

A later plan may define paired security checks.

## 22. Failure semantics

Differentiate:

### Policy/config/preflight failure

Before startup -> Invalid/preflight failure.

### Process timeout/cancellation/invalid JSON

Operational correctness-phase failure -> existing run failure/invalid rules;
cleanup remains mandatory.

### Valid Eggsec result with successful bypasses above threshold

Execution continues and may complete.

Security result disposition = Fail.

This is NOT `WorkloadFailed`, `DiagnosticFailed`, or execution Invalid.

## 23. Tests

Core/schema:

- v1-v5 compatibility;
- v6 security-check roundtrip;
- old-schema explicit-field rejection;
- unique IDs;
- concurrency/timeout bounds;
- unsupported source/test family;
- public target rejection;
- paired/network_path rejection.

Driver/parser:

- exact WAF argv;
- no forbidden override/stress/evasion flags;
- preflight JSON allowed/denied;
- ScanResults real-shape parser;
- summary consistency;
- zero-case Invalid;
- successful-bypass threshold Pass/Fail;
- malformed/nonfinite/oversized JSON rejection;
- payload omission from persisted result;
- binary version/SHA provenance;
- timeout/cancel cleanup.

Lifecycle:

- correctness after readiness;
- correctness outside measured interval;
- security Fail still permits warmups/trials;
- operational correctness error uses cleanup tail;
- diagnostics ordering remains correct.

Evidence:

- security-checks.json schema;
- per-check artifact;
- payload string absent;
- secret sentinel absent;
- comparison-critical identity changes on config/tool digest changes.

## 24. Real Eggsec qualification

Build exact audited Eggsec source revision:

`0509ac668adfd78e9899cd3428a807d0b3c9f27b`

Run against a deterministic loopback WAF fixture designed only for authorized
local qualification.

Prove:

1. `eggsec --version`;
2. strict guarded preflight allowed for the in-scope local target;
3. the selected WAF JSON shape parses;
4. a fixture that blocks the selected test family yields zero successful
   bypasses and Pass;
5. a deliberately permissive fixture yields at least one Eggsec-declared
   successful bypass and Fail;
6. both are valid execution observations;
7. no raw payload appears in the finalized Eggbench bundle.

The local fixture may be qualification-only; do not copy Eggsec scanner logic
into it.

## 25. Hosted qualification

Add/extend a Linux-only live sibling qualification job:

- check out exact Eggsec source revision;
- build the headless CLI with `cargo build --locked --release -p eggsec-cli --no-default-features`;
- run local strict-scope positive/fail fixtures;
- verify sanitized bundle evidence;
- record bounded provenance.

Normal Eggbench CI must also be green on the same candidate:

- Linux stable;
- Linux Rust 1.89;
- macOS stable;
- Windows stable.

The live Eggsec job may be Linux-only because it qualifies the external
machine seam, not Eggsec's platform portability.

## 26. Verification matrix

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

No Eggsec production Rust dependency is expected.

## 27. Closure

Create:

`plans/closure/eggstack-integration/004a-status.md`

Record:

- implementation SHA;
- Eggsec exact source SHA;
- Cargo.lock SHA;
- binary SHA/version;
- scope/preflight evidence;
- safe/failed WAF correctness fixtures;
- sanitized result examples;
- payload-nonretention proof;
- lifecycle/timing proof;
- config comparability proof;
- default/all-feature/MSRV results;
- live-tool hosted run;
- normal four-lane run;
- unresolved findings.

M004a closure unblocks M004b.

## 28. Acceptance criteria

M004a closes only when:

1. schema v6 security checks is additive;
2. Correctness is a distinct category from Workload/Diagnostic/Telemetry;
3. Eggsec is consumed as an external strict-scope process;
4. no Eggsec Rust production dependency is added;
5. only local/private runtime targets are accepted;
6. generated scope is exact and digest-addressed;
7. strict preflight succeeds before managed startup;
8. no manual override flag is used;
9. only bounded direct WAF bypass checks are supported;
10. at least one case is required for a valid observation;
11. `bypass_successful` is the only initial semantic correctness signal;
12. threshold Pass/Fail is declared before execution;
13. security Fail does not fail execution or suppress performance trials;
14. operational/security-tool failure remains distinguishable;
15. raw payload bytes are not retained in bundles;
16. security checks run outside measured intervals;
17. security configuration participates in comparability;
18. paired/network_path combinations fail closed;
19. real pinned Eggsec safe/fail cases are qualified;
20. Rust 1.89 remains green;
21. normal four-lane CI is green;
22. live Eggsec qualification is green;
23. closure record is committed.

## 29. Stop conditions

Stop and re-plan if:

- real Eggsec strict-scope `waf --json` does not provide a stable parsable
  `bypass_successful` result;
- strict policy cannot authorize the intended local check without a manual
  override;
- raw payloads cannot be prevented from entering persistent evidence;
- supporting the check requires the full Eggsec Rust crate graph;
- supporting the check requires a public target;
- the only workable path is stress/flood/raw packet behavior;
- Eggsec output forces Eggbench to infer maliciousness from payload text;
- the runner cannot keep correctness execution outside measured timing;
- correctness Fail cannot remain independent of execution status.

If the needed machine seam is missing in Eggsec, author/register an upstream
Eggsec plan and block M004a rather than duplicating scanner semantics in
Eggbench.

## 30. Handoff order

~~~text
schema v6 + Correctness category
 -> sibling-neutral runner seam
 -> external eggsec descriptor/preflight
 -> generated strict scope
 -> WAF JSON adapter + sanitization
 -> lifecycle/evidence
 -> real Eggsec qualification
 -> four-lane qualification
 -> M004a closure
 -> M004b
~~~
