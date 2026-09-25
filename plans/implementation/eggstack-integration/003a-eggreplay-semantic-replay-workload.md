# Eggstack Integration M003a — EggReplay Semantic Replay Workload

Status: ready for handoff

Repository baseline: 2437ddb406153fda8140b0e93838a7e9fe6004c9

Subsystem roadmap:

- plans/subsystems/eggstack-integration-roadmap.md

Controlling architecture:

- plans/000-long-term-specification.md
- plans/001-terminology-and-domain-model.md
- plans/002-long-term-roadmap.md
- plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md
- plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md
- plans/adrs/ADR-0004-eggstack-composition-and-independent-oracles.md

Prerequisites:

- Eggstack M001 closed and hosted-qualified.
- Eggstack M002 closed and hosted-qualified at plans/closure/eggstack-integration/002-status.md.
- Measurement M001-M003 closed/qualified.
- External-command driver substrate is closed/qualified.

Primary class: Eggstack integration / semantic workload.

## 1. Objective

Add EggReplay as a first-class semantic HTTP workload without importing EggReplay transport or fixture semantics into Eggbench.

One Eggbench measured trial executes one complete immutable EggReplay fixture against the trial target:

~~~text
ExperimentPlan
  -> EggReplay semantic workload intent
  -> trusted external eggreplay binary
  -> eggreplay replay --output json
  -> candidate target runtime binding
  -> EggReplay RegressionReport[]
  -> bounded raw evidence + semantic finding count
  -> Eggbench trial/evidence/comparison
~~~

EggReplay remains authoritative for:

- .eggr fixture format and validation;
- recorded request semantics;
- matching/replay order;
- candidate request execution;
- stream/SSE/WebSocket semantics when enabled upstream;
- semantic regression finding meaning.

Eggbench owns:

- fixture identity and confinement;
- driver selection/version/digest provenance;
- process lifecycle/cancellation;
- trial scheduling;
- mapping a complete replay into one trial;
- bounded evidence retention;
- the optional absolute semantic-correctness gate.

M003a MUST NOT reimplement EggReplay flow matching, fixture parsing, HTTP execution, or diff logic.

## 2. Sibling seam audit — 2026-09-25

Current EggReplay main is workspace version 0.1.0, Rust 1.89, and has continued substantial development after its original v0.1 qualification.

Relevant current state:

- v0.1 core record/replay/regression work is closed and hosted-qualified.
- M009 stateful replay, M010 streaming/SSE, M011 semantic WebSocket, and M012 Python work are closed.
- M013 interception is active planning and currently gated through M013B0.
- Current default branch HEAD observed during planning: 29ce133f9845aef2efb5080988796b7ae1dbc7f8.
- No immutable GitHub release artifact/API surface was found during this audit; the workspace still reports 0.1.0 while main continues to move.

Stable machine-facing CLI facts used by M003a:

- commands include record, serve, replay, test, diff, inspect, validate;
- --output json emits an envelope with schema_version = 1;
- replay JSON payload carries RegressionReport[] plus finding_count;
- EggReplay REPORT_SCHEMA_VERSION is 2;
- stable exit classes are documented:
  - 0 success;
  - 1 regression/assertion mismatch for asserting commands;
  - 2 configuration;
  - 3 fixture;
  - 4 runtime;
  - 5 internal;
- replay itself reports semantic findings without using exit 1 as an assertion result;
- validate emits fixture schema/flow-count metadata.

Long-lived serve is NOT selected for M003a because the current CLI publishes its selected bound address only as human stderr while running and emits the JSON result only after shutdown. Parsing that human readiness line would create a brittle lifecycle contract.

The Rust ReplayFixture service API is also NOT selected because doing so would require coupling Eggbench to EggReplay's rapidly evolving 0.1 crate graph and current sibling pins.

Therefore the M003a production seam is the EggReplay external JSON CLI, not a Rust library dependency.

## 3. Scope

M003a delivers:

1. ExperimentPlan schema v4 semantic replay workload intent.
2. A canonical external workload driver named eggreplay-semantic.
3. Trusted eggreplay binary resolution/version/digest provenance.
4. Workspace-confined immutable .eggr fixture validation and identity.
5. Pre-start eggreplay validate machine-contract verification.
6. One replay process per warmup/measured invocation.
7. JSON envelope/report parsing with bounded raw evidence.
8. A normalized semantic finding count suitable for an absolute zero gate.
9. Comparison-critical fixture and producer identity.
10. CLI validate/doctor/run/inspect coverage.
11. Default/all-feature/MSRV and four-lane hosted qualification.

## 4. Non-goals

M003a does not add:

- EggReplay recording;
- EggReplay serve as an Eggbench managed origin;
- append-new/re-record modes;
- fixture mutation;
- EggReplay Python bindings;
- EggReplay interception/MITM;
- EggReplay WebSocket-specific controls;
- EggReplay SSE/stream comparison controls;
- timeline scheduler controls;
- an EggReplay Rust dependency;
- EggReplay's own routing grammar as an Eggbench network path;
- M002 network_path composition;
- new latency/throughput benchmark metrics derived from EggReplay process runtime;
- probe/diagnostic behavior;
- security scanning.

Those require separate later planning if needed.

## 5. Schema v4

Add:

- EXPERIMENT_PLAN_SCHEMA_VERSION_4;
- the corresponding resolved-plan schema version if serialized shape changes.

v1-v3 remain readable and retain their existing semantics and fixtures.

Add a Workload variant conceptually equivalent to:

~~~text
SemanticReplay {
  target: Name,
  fixture: String
}
~~~

The field is deliberately small.

Eggbench does not mirror EggReplay matching/scheduler/stream/WebSocket policy in the first integration.

Validation:

- fixture must be a relative workspace path;
- no absolute paths;
- no parent traversal;
- no NUL/control characters;
- bounded path length;
- target must resolve to a declared service or allowed existing target under the normal workload rules;
- SemanticReplay + network_path is rejected in M003a with workload_path_incompatible;
- the selected workload driver must advertise SemanticReplay capability.

Do not overload the existing closed-loop/open-loop variants to smuggle a fixture path through service config.

## 6. Paired experiments

M003a MAY support paired experiments because each invocation launches a fresh EggReplay process and the runner already rewrites the workload target per arm.

Requirements if enabled:

- SemanticReplay must implement the same target-rewrite contract used by paired workloads;
- both arms must publish the required HTTP runtime binding;
- the exact same fixture identity is used on both arms;
- each measured trial launches a new EggReplay process;
- no EggReplay process or client pool survives across arms/trials;
- paired schedule/seed/comparison semantics remain unchanged.

If implementation reveals that target rewriting cannot be made explicit and testable without changing M003 paired semantics, stop and reject paired SemanticReplay in v4 rather than weakening the existing contract.

## 7. Driver descriptor

Canonical driver:

~~~text
eggreplay-semantic
~~~

Descriptor:

- category Workload;
- external_process = true;
- upstream_name = eggreplay;
- adapter version = Eggbench crate version;
- upstream version = observed --version result;
- capability = SemanticReplay;
- no ClosedLoop/OpenLoop capability claim unless the core capability model requires a neutral workload discriminator;
- compatible initial service type: eggserve-origin and any future service explicitly publishing the same HTTP binding contract.

Do not mark this driver as the default workload driver. Eggfetch-http remains the native default where compiled.

## 8. External-command substrate reuse

Reuse the qualified External Oracles M001 substrate for:

- explicit/secure PATH resolution;
- canonical executable path;
- SHA-256 executable identity;
- argv-only execution;
- env_clear/hermetic child environment;
- null stdin except where explicitly needed;
- bounded concurrent stdout/stderr drain;
- timeout/cancellation;
- no shell/glob expansion.

Do not create a second binary resolver/process runner.

Version probe:

~~~text
eggreplay --version
~~~

Record exact stdout-derived tool version and executable digest.

Reject malformed/unbounded version output.

## 9. Fixture preflight and confinement

Before managed service startup:

1. resolve the fixture path under RunnerOptions.workspace_root;
2. canonicalize the path and every relevant parent;
3. reject escape outside workspace root;
4. reject symlink escape;
5. require a directory;
6. reject special files where traversed;
7. enforce bounded traversal;
8. compute a deterministic fixture identity;
9. run EggReplay validate through the same selected executable.

The fixture identity must NOT be merely the local path.

Preferred deterministic identity:

- sorted relative file paths;
- file type;
- file byte length;
- SHA-256 file content;
- aggregate SHA-256 over the canonical ordered records.

Bounds must prevent an untrusted fixture tree from causing unbounded traversal or hashing.

At minimum enforce:

- maximum file count;
- maximum individual file size;
- maximum aggregate bytes hashed;
- maximum relative-path length/depth.

Use existing Eggbench ArtifactBounds where semantically appropriate, otherwise define a small explicit driver preflight bound.

## 10. Machine-contract preflight

Run before any managed startup:

~~~text
eggreplay validate --fixture <path> --output json
~~~

Require:

- process exit 0;
- envelope schema_version == 1;
- command == "validate";
- success == true;
- bounded warnings;
- payload flow_count is finite/bounded;
- payload fixture schema_version is one of the explicitly accepted EggReplay session schemas.

Record:

- EggReplay executable version/digest;
- envelope schema version;
- fixture session schema;
- flow count;
- fixture aggregate digest.

Malformed JSON, unknown envelope version, impossible counts, or fixture validation failure must fail capability/preflight before the subject/topology starts.

## 11. Invocation contract

For each warmup/measured invocation execute:

~~~text
eggreplay replay
  --fixture <fixture>
  --target <runtime target URL>
  --route direct
  --output json
~~~

No shell.

Initial target binding requirement:

- target service must publish non-secret http_url;
- URL must be loopback/private according to existing experiment configuration, not synthesized from arbitrary text;
- do not pass M002 network_path into EggReplay;
- --route direct is explicit to prevent a sibling default from silently changing.

The process timeout is bounded by the runner invocation timeout.

Cancellation terminates the child using the existing external-command process ownership semantics.

## 12. Trial semantics

One complete EggReplay fixture replay is one workload invocation and therefore one Eggbench trial observation.

Do not treat individual recorded HTTP flows as independent Eggbench trials.

Warmups execute the complete fixture but their metrics remain warmup-only under existing runner behavior.

A measured trial records the aggregate semantic result for that one complete fixture execution.

## 13. Parsing and correctness mapping

Require the replay JSON envelope:

- schema_version == 1;
- command == "replay";
- success field present;
- payload contains bounded RegressionReport array;
- every RegressionReport.schema_version == 2 for this M003a contract;
- finding_count matches the sum of report findings;
- report count/flow identifiers remain bounded.

Do not reproduce EggReplay's finding evaluator.

Retain EggReplay finding kinds/fields as raw diagnostic evidence.

Normalized metrics:

### semantic_findings

- unit: count;
- direction: lower-is-better;
- value: total finding_count;
- aggregation: one scalar per Eggbench trial;
- source: eggreplay-semantic.

### semantic_flows

Optional diagnostic-only count of replayed baseline reports/flows if the machine output supplies an unambiguous stable count.

Do not map EggReplay wall-clock process elapsed time to Eggbench latency.

Do not map per-flow timing findings to benchmark latency metrics.

## 14. Gate policy

The primary correctness use case is an absolute zero-findings gate.

If the plan requests a gate for semantic_findings:

- Absolute { value: 0 } is supported;
- a nonzero absolute threshold may be allowed but must remain explicit;
- RelativeRegression and StatisticalRelative for semantic_findings should fail preflight in M003a.

Reason: a count of semantic mismatches is a correctness quantity, not a smooth performance quantity for ratio/bootstrap inference.

This does not create a second verdict system. Eggbench's existing metric/gate machinery remains authoritative.

## 15. Failure mapping

Differentiate:

- invalid fixture/config/machine contract -> preflight/capability failure before startup;
- replay process runtime failure -> WorkloadFailed;
- runner deadline -> TimedOut;
- cancellation -> Cancelled;
- semantic findings with a successful replay -> successful workload execution with semantic_findings > 0, not WorkloadFailed.

Do not turn a semantic mismatch into an execution failure. The metric gate decides acceptance.

## 16. Evidence

### Run-level evidence

Add a versioned artifact, e.g.:

~~~text
semantic-replay.json
~~~

Schema v1 records:

- driver name/adapter version;
- selected executable canonical path representation already permitted by Eggbench evidence policy;
- executable SHA-256;
- EggReplay version;
- CLI envelope schema version;
- accepted RegressionReport schema version;
- fixture relative path for operator context;
- fixture aggregate digest;
- fixture session schema;
- flow count;
- invocation command policy:
  - replay;
  - route direct;
  - default sequential scheduler;
  - no M003a stream/SSE/WebSocket special flags.

The local fixture path is not the stable identity; digest/schema/policy are.

### Per-invocation raw evidence

Retain bounded:

- replay stdout JSON;
- stderr;
- parsed summary/method artifact if useful.

Never retain environment secrets or unbounded sibling errors.

## 17. Comparison identity

Semantic replay configuration is comparison-critical.

Compare:

- workload kind;
- fixture aggregate digest;
- fixture session schema;
- EggReplay executable version/digest;
- CLI envelope schema;
- RegressionReport schema;
- replay policy/scheduler identity.

Do not make the workstation-local fixture path comparison-critical.

A semantic replay bundle compared against a non-semantic workload bundle is incomparable under existing policy.

No new statistical comparison policy identifier is required if the existing math is unchanged; only comparability identity is extended.

## 18. CLI

### validate

Schema-v4 semantic workload validation.

### doctor

Show:

- eggreplay-semantic descriptor;
- binary presence/path identity;
- version probe;
- SemanticReplay capability;
- supported EggReplay envelope/report schema versions;
- explicit note that M003a does not expose recording/serve/network_path composition.

Doctor need not validate a specific fixture unless a plan is supplied through an existing plan-aware path.

### run

Preflight fixture and machine contract before managed startup.

### inspect

Surface semantic-replay run identity and per-trial finding count from retained evidence.

Do not parse arbitrary EggReplay human text.

## 19. Security

- workspace-confined fixture;
- no shell execution;
- no fixture mutation;
- no EggReplay recording;
- no credential-bearing route;
- --route direct explicitly;
- bounded input traversal;
- bounded JSON/stdout/stderr;
- reject secrets in debug evidence;
- preserve EggReplay redaction markers; do not attempt to reverse/redact semantic payload summaries independently.

Add sentinel tests proving a secret-like fixture value does not leak into Eggbench method/provenance evidence beyond whatever already-safe EggReplay JSON contract explicitly emits.

## 20. Tests

Core:

- v1-v3 compatibility;
- v4 SemanticReplay round-trip;
- missing/escaping fixture rejection;
- network_path incompatibility;
- gate restriction;
- paired behavior if supported.

Driver:

- trusted resolver/version probe;
- fixture confinement including symlink escape;
- deterministic fixture digest independent of directory absolute path;
- validate envelope schema;
- replay envelope/report parser;
- finding_count consistency;
- bounded malformed JSON rejection;
- cancellation/timeout;
- semantic findings do not become WorkloadFailed.

Integration:

- build a small deterministic .eggr fixture using checked-in test fixture data;
- EggServe controlled origin returns matching response -> semantic_findings 0;
- changed origin response -> semantic_findings > 0 while execution completes;
- absolute zero gate fails through normal comparison/gate path;
- multiple measured trials treat fixture replay as trial unit;
- if paired supported, both arms use same fixture with rewritten target.

Negative:

- missing eggreplay binary;
- wrong machine envelope version;
- unsupported report schema;
- corrupt fixture;
- fixture escape;
- network_path + SemanticReplay.

## 21. Feature/dependency isolation

Because the production seam is an external binary, M003a should not add EggReplay Rust crates to Cargo.toml.

No EggReplay feature flag is required merely to discover the external driver descriptor if the existing external-driver catalog pattern can carry it unconditionally.

Default builds may include descriptor/parser code but no EggReplay library dependency.

Verify cargo tree has no eggreplay-* crate.

## 22. Verification

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
cargo +1.89.0 test -p eggbench-drivers --all-features --locked
cargo tree --locked
git diff --check
~~~

Live EggReplay integration tests may require an explicitly resolved test binary. If the hosted environment does not install EggReplay, keep fixture/parser/process-absence tests hosted and record the live local binary version/digest separately. Do not silently skip a test that is claimed as hosted.

## 23. Hosted qualification

Fresh current-tip CI on the exact M003a candidate:

- Linux stable;
- Linux Rust 1.89;
- macOS stable;
- Windows stable.

All lanes must compile/test the new schema/parser/external-driver behavior.

If live EggReplay binary execution is not present on hosted runners, closure must explicitly distinguish:

- hosted absence-safe/parser/fixture-contract evidence;
- local live EggReplay integration evidence.

## 24. Closure

Create:

plans/closure/eggstack-integration/003a-status.md

Record:

- implementation SHA;
- exact EggReplay binary version/digest used for live qualification;
- audited EggReplay source baseline;
- accepted envelope/report/session schemas;
- fixture identity algorithm/bounds;
- live matching and mismatch test results;
- semantic metric/gate behavior;
- local/default/all-feature/MSRV results;
- cargo-tree proof of no EggReplay Rust dependency;
- hosted run/job results;
- unresolved findings.

M003a closes only after this evidence exists.

## 25. Acceptance criteria

M003a closes only when:

1. schema v4 SemanticReplay exists and v1-v3 stay compatible;
2. EggReplay is an external workload, not a copied library implementation;
3. no EggReplay Rust crate becomes a production dependency;
4. trusted binary resolution/version/SHA is recorded;
5. fixture is workspace-confined and immutable;
6. fixture content identity is digest-based, not path-based;
7. eggreplay validate succeeds before managed startup;
8. machine envelope schema 1 is enforced;
9. RegressionReport schema 2 is enforced;
10. one complete fixture replay equals one Eggbench trial;
11. semantic mismatch remains a successful workload observation;
12. semantic_findings can be absolutely gated;
13. relative/statistical gates for semantic_findings fail preflight;
14. no process/runtime timing is mislabeled request latency;
15. network_path composition fails closed in M003a;
16. cancellation/timeout cleanup uses existing process ownership;
17. raw evidence is bounded;
18. default/all-feature Clippy is clean;
19. Rust 1.89 remains green;
20. four-lane hosted CI is green;
21. closure record is committed.

## 26. Stop conditions

Stop and re-plan if:

- current EggReplay CLI no longer emits the audited stable JSON envelope;
- replay requires parsing human output;
- fixture validation requires importing unpublished mutable Rust crates;
- target URL cannot be supplied from existing runtime bindings without adding a hidden listener;
- semantic mismatch cannot be represented separately from process/runtime failure;
- fixture identity cannot be computed safely within bounded workspace rules;
- supporting paired replay requires changing existing paired scheduling semantics;
- a schema or dependency change outside this scoped workload contract appears necessary.

If an upstream EggReplay change is necessary, write/register it in eggstack/eggreplay rather than duplicating its semantics inside Eggbench.
