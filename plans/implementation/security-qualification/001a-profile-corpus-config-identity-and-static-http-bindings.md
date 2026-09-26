# Security Qualification M001a — Profile, Corpus, Configuration Identity, and Static HTTP Bindings

Status: closing

Repository baseline: `9a6c51781f47794b8c96b4f77387bb027df79303`

Source roadmap:

- `plans/subsystems/security-qualification-roadmap.md` — M001 / §2A

Controlling architecture:

- `plans/000-long-term-specification.md` — security-performance experiments
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md` — Phase 7
- `plans/003-planning-process.md`
- `plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md`
- `plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md`
- `plans/adrs/ADR-0004-eggstack-composition-and-independent-oracles.md`

Hard prerequisites:

- Eggstack M004a/M004b closed.
- Measurement/comparison M001-M003 closed and hosted-qualified.
- Security Qualification M001 research grounded in the source roadmap.

Primary class: capability/infrastructure — reusable security-qualification input contract.

## 1. Objective

Establish the immutable input and expansion contract required by Security Qualification M001 without adding a scanner, target-specific adapter, or new security semantics.

M001a delivers four related boundaries:

1. a versioned `SecurityQualificationProfile` separate from `ExperimentPlan`;
2. a bounded normalized HTTP security-corpus format with owner-authored observable expectations;
3. one generalized workspace-confined content-tree identity primitive for corpus/config/fixture inputs;
4. a generic non-secret static HTTP binding seam for managed command and external services.

M001a does not execute security corpus cases. M001b is the first consumer.

## 2. Current implementation evidence

At the baseline:

- M004 exposes `CorrectnessExecutor`, `CorrectnessRegistry`, sanitized correctness evidence, comparison-critical security identity, and ComparisonReceipt v3.
- M004's `eggbench.security-correctness.v1` is explicitly the Eggsec `waf_bypass` policy and must remain unchanged.
- M003a implements bounded deterministic EggReplay fixture traversal/digesting inside the EggReplay driver.
- `RuntimeBindings` are currently populated by named in-process service adapters.
- managed command services and external services are represented by the runner but do not publish an `http_url` binding.
- SynVoid is a benchmark subject, not an Eggbench library dependency.

This plan extracts the generic substrate required by M001 rather than duplicating M003a or introducing a SynVoid adapter.

## 3. Invariants

- A qualification profile is an input contract, not a result-derived template.
- Expected security outcomes are immutable before candidate execution.
- Local filesystem paths are operator context; content digests are durable identity.
- Content-tree identity is deterministic across equivalent workspaces.
- Symlink/path escape is rejected.
- Qualification expansion is deterministic and bounded.
- No arbitrary template language, JSON Patch, shell interpolation, or code execution is added.
- Static HTTP bindings are non-secret and explicit.
- SynVoid/Eggsec types do not enter Eggbench core.
- M001a introduces no measured interval and no security-performance verdict.
- Old experiment plans, receipts, bundles, and M004 behavior remain compatible.

## 4. Non-goals

Do not add:

- corpus execution;
- scanner payload generation;
- security classification;
- baseline-derived expectations;
- SynVoid-specific Rust code;
- Eggsec profile imports;
- public-target authorization;
- challenge/stall/tarpit/drop interpretation;
- a generic parameter matrix engine;
- JSON/YAML templating;
- remote execution;
- credentials in corpus headers or runtime bindings;
- timing expectations;
- changes to `eggbench.security-correctness.v1`.

## 5. SecurityQualificationProfile v1

Add a new versioned library contract, separate from `ExperimentPlan`.

Conceptual shape:

~~~text
SecurityQualificationProfileV1 {
  schema_version: 1
  id: Name
  owner: String
  scenarios: Vec<QualificationScenarioRef>
  corpus: ContentInputRef
  target_config: ContentInputRef
}

QualificationScenarioRef {
  id: Name
  plan: relative workspace path
}
~~~

The exact Rust organization may vary, but the serialized contract must remain narrow.

V1 rules:

- 1..=32 scenarios;
- unique scenario IDs;
- scenario plans are explicit existing Eggbench plan files;
- all referenced paths are relative to an explicit workspace root;
- no glob expansion;
- no inheritance;
- no field substitution;
- no dynamic patching;
- no environment-variable interpolation;
- no arbitrary command execution.

Profile parsing belongs in a dependency-light library surface. CLI-specific presentation remains outside core contracts.

## 6. Deterministic expansion manifest

Profile resolution must produce a versioned deterministic expansion artifact before execution.

Conceptual:

~~~text
QualificationExpansionV1 {
  schema_version: 1
  expansion_policy: "eggbench.security-profile-expansion.v1"
  profile_id
  profile_sha256
  corpus_identity
  target_config_identity
  scenarios: [
    {
      id
      source_plan_sha256
      source_plan_schema
      source_plan_path_context
    }
  ]
}
~~~

Rules:

- preserve declared scenario order;
- hash exact source-plan bytes;
- validate every referenced plan through normal `ExperimentPlan` parsing/validation;
- reject duplicate IDs or duplicate plan paths that would make evidence ambiguous;
- do not rewrite scenario plans;
- do not infer security expectations from baseline artifacts;
- do not include timestamps, absolute paths, process IDs, or host-local inode metadata in identity.

The source path may be retained as bounded presentation context, but digest and schema are the durable identity.

## 7. Normalized HTTP security corpus v1

Introduce an Eggbench-owned transport-level corpus envelope.

Conceptual:

~~~text
HttpSecurityCorpusV1 {
  schema_version: 1
  owner: String
  corpus_id: Name
  cases: Vec<HttpSecurityCaseV1>
}

HttpSecurityCaseV1 {
  id: Name
  category: Option<String>
  request: HttpCaseRequestV1
  expectation: HttpObservableExpectationV1
}

HttpCaseRequestV1 {
  method
  path_and_query
  headers
  body: none | inline_utf8 | file
}

HttpObservableExpectationV1 {
  status_exact: u16
    OR
  status_any_of: bounded Vec<u16>
}
~~~

Initial limits:

- 1..=1,024 cases;
- unique IDs;
- bounded method/path/header count/header bytes/body bytes;
- only valid HTTP methods accepted;
- relative request targets only;
- no absolute URL per case;
- no authority override;
- reject `Authorization`, `Proxy-Authorization`, `Cookie`, and other configured credential-bearing headers in v1;
- body file paths are workspace-confined and participate in the corpus digest;
- response timing is never an expectation;
- category is opaque owner metadata and has no Eggbench semantics.

Do not encode "malicious", "blocked", "detected", severity, CWE, or WAF product semantics as Eggbench-owned truth. The owner converts those semantics into observable expectations before the corpus is accepted.

## 8. Generalized content-tree identity

Extract/generalize the deterministic bounded hashing logic currently embedded in the EggReplay integration.

Create one reusable owner-neutral primitive, conceptually:

~~~text
ContentTreeIdentity {
  aggregate_sha256
  file_count
  total_bytes
  files: optional bounded canonical records
}
~~~

Canonical record input must include at least:

- normalized relative path bytes;
- file length;
- SHA-256 of file bytes.

Canonical ordering is lexical over normalized relative paths.

Required safety:

- explicit workspace root;
- reject absolute requested paths;
- canonicalize every path;
- reject any resolved path outside workspace root;
- reject symlinks as tree members, or otherwise fail closed before following them;
- reject special files;
- bound depth;
- bound relative-path length;
- bound files;
- bound per-file bytes;
- bound aggregate bytes;
- bounded read buffers.

M003a must be migrated to this shared primitive with byte-for-byte equivalent fixture identity for existing fixtures. If that cannot be preserved, stop and re-plan rather than silently changing M003a comparison identity.

Recommended initial general bounds may preserve M003a's current 1,024-file / 8 MiB-per-file / 64 MiB aggregate ceiling unless a stricter per-use policy is passed by the caller.

## 9. Target configuration identity

The profile must bind target configuration independently from the subject executable revision.

V1 accepts a workspace-confined file or directory reference and records its `ContentTreeIdentity`.

This is evidence identity only:

- Eggbench does not parse SynVoid semantics;
- Eggbench does not decide whether a config enables a WAF;
- Eggbench does not mutate target config;
- the profile owner is responsible for selecting the correct config.

A changed target configuration digest is a comparison-critical profile mismatch unless a future policy explicitly permits it.

## 10. Generic static HTTP binding seam

Add a generic way for managed command and external services to publish a non-secret `http_url` without a subject-specific adapter.

Preferred shape: a typed or narrowly validated service binding declaration on the service model rather than magic parsing of arbitrary `config` keys.

Conceptual:

~~~text
Service {
  ...
  bindings: {
    http_url: "http://127.0.0.1:8080"
  }
}
~~~

Exact schema placement may differ if an existing compatible extension point is cleaner, but the semantics must be explicit.

Requirements:

- available to `RuntimeBindings` before workload/correctness execution;
- command process binding is static declaration, not scraped from stdout;
- external service binding is static declaration;
- named adapter runtime binding continues to win only where conflict semantics are explicitly defined;
- duplicate/conflicting values fail closed rather than silently override;
- v1 binding key is `http_url`;
- URL must be HTTP/HTTPS;
- credentials/userinfo rejected;
- fragment rejected;
- host must satisfy the consumer's local/private policy when security execution uses it;
- binding value is non-secret evidence;
- runtime-topology evidence records the effective binding.

Do not add SynVoid-specific service type or process-output parsing.

## 11. Schema compatibility

If the service model requires an ExperimentPlan schema bump:

- introduce a new additive schema version;
- continue reading all currently supported historical versions;
- explicit use of the new binding field on old schemas fails closed;
- resolved-plan schema must advance if effective bindings become part of resolved plan identity;
- old bundle/receipt readers remain unchanged.

The profile/corpus schemas are independent version domains and must not overload `ExperimentPlan.schema_version`.

## 12. Comparison identity

M001a itself does not create a new comparison receipt, but it must define comparison-critical identity that M001b/M001c can consume:

- profile schema/version;
- profile digest;
- expansion-policy ID;
- ordered scenario IDs + source plan digests;
- corpus schema/ID/digest;
- target-config digest;
- expectation-policy version;
- static HTTP binding identity where relevant.

Do not include:

- absolute filesystem paths;
- timestamps;
- observed response codes;
- trial measurements;
- local process IDs.

## 13. CLI/library surface

Add library APIs for:

- parse/validate profile;
- parse/validate corpus;
- resolve profile against workspace root;
- compute content identity;
- emit deterministic expansion JSON.

Add a thin CLI surface, preferably one of:

~~~text
eggbench qualify validate <profile>
eggbench qualify expand <profile>
~~~

or an equivalent subcommand hierarchy consistent with current CLI conventions.

M001a must not run the scenarios yet.

Machine JSON output is required and must contain no ANSI/progress noise.

## 14. Failure semantics

Profile/corpus/config failures are pre-execution input failures.

Stable failure categories should distinguish:

- invalid profile schema;
- invalid corpus schema;
- unsafe content path;
- content bound exceeded;
- plan reference invalid;
- duplicate scenario/case;
- invalid static binding;
- unsupported expectation;
- forbidden credential-bearing header.

No managed subject/service starts when M001a validation/expansion fails.

## 15. Work packages

### A — shared content identity

- extract M003a hashing/confinement;
- preserve EggReplay identity compatibility;
- add reusable tests for traversal, symlink escape, bounds, order determinism.

### B — static service bindings

- extend core plan/resolved model as needed;
- lower static bindings into `RuntimeBindings`;
- record effective bindings in runtime topology;
- validate conflict/URL rules.

### C — profile/corpus schemas

- add versioned typed contracts;
- add bounded validation;
- add deterministic serialization/golden fixtures.

### D — expansion

- validate referenced scenario plans;
- compute all input identities;
- emit deterministic expansion manifest;
- add CLI validate/expand.

### E — docs and guards

- update schema/docs/examples;
- document security ownership boundary;
- add static dependency guard proving no SynVoid/Eggsec production dependency was introduced.

## 16. Verification

Focused:

- content-tree digest stable under filesystem enumeration order;
- content changes change digest;
- path name changes change digest;
- symlink/path escape rejected;
- M003a EggReplay digest compatibility golden;
- corpus bounds and duplicate IDs;
- credential-bearing header rejection;
- exact/set status expectation roundtrip;
- profile deterministic expansion;
- static command/external binding available through `RuntimeBindings`;
- binding conflict fails closed;
- old plan schemas remain readable.

Broad:

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

Also rerun the existing M003 EggReplay qualification tests because content identity moved.

Hosted closure requires Linux stable, Linux Rust 1.89, macOS stable, and Windows stable green on the exact implementation candidate.

## 17. Closure evidence

Create:

`plans/closure/security-qualification/001a-status.md`

Record:

- implementation commit(s);
- profile/corpus schema versions;
- expansion-policy identifier;
- content-tree canonicalization algorithm and bounds;
- M003a digest compatibility proof;
- static HTTP binding schema/precedence;
- backward compatibility evidence;
- dependency-tree proof that SynVoid/Eggsec were not added;
- local/MSRV/all-feature results;
- hosted run IDs;
- unresolved findings and severity.

## 18. Acceptance criteria

M001a closes only when:

1. profile v1 is separate from ordinary ExperimentPlan;
2. explicit bounded scenario references replace generic templating;
3. corpus v1 carries only transport data + owner-authored observable expectations;
4. expected outcomes are immutable inputs;
5. corpus/config identity is content-based and workspace-confined;
6. one shared content-tree implementation is used;
7. EggReplay fixture identity compatibility is preserved;
8. command/external services can publish a generic static `http_url`;
9. runtime topology preserves effective binding evidence;
10. no SynVoid/Eggsec production dependency is added;
11. all old supported schema versions remain readable;
12. M001b has all required stable input/runtime seams;
13. Rust 1.89 and four-lane hosted CI are green.

## 19. Stop conditions

Stop and re-plan if:

- preserving M003a digest identity requires a breaking change;
- static bindings require a SynVoid-specific adapter;
- corpus validation requires interpreting attack semantics;
- profile expansion requires a general template/patch language;
- raw secrets/credentials must enter the corpus to close the milestone;
- a public target must be supported;
- profile input identity cannot be made deterministic and bounded.
