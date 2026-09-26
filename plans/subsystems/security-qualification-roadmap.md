# Security Performance Qualification Roadmap

Status: active (M001a closed; M001b ready; M001c dependency-blocked)

Long-term references:

- plans/000-long-term-specification.md — security-performance experiments
- plans/002-long-term-roadmap.md — Phase 7

Related ADR:

- plans/adrs/ADR-0004-eggstack-composition-and-independent-oracles.md

## 1. Purpose and ownership boundary

This subsystem defines how Eggbench combines security correctness evidence with performance and resource evidence.

Eggbench does not decide whether a payload is malicious or whether a WAF/scanner finding is semantically correct. Those meanings remain with Eggsec, SynVoid, or another explicit security workload owner.

## 2. Invariants

- Correctness and performance are independent gate families.
- Performance cannot override failed correctness.
- Corpus, profile, and target configuration digests are evidence.
- Expected security outcomes are fixed before candidate interpretation.
- Local/private lab targets are the safe default.
- No unbounded stress or flood profile is introduced as ordinary CI.
- Security payload/log redaction requirements are explicit.

## 2A. M001 handoff research — 2026-09-26

M001 was re-audited after Eggstack M004 closure. The main conclusion is that
M004 already owns the generic one-run correctness execution/evidence and
combined-verdict substrate. M001 should not become "more M004" or broaden the
Eggsec scanner adapter. Its missing layer is a reusable qualification
profile/corpus contract that can expand deterministically into ordinary
Eggbench experiments while carrying immutable security-test identity.

### Existing Eggbench substrate

M004 provides:

- schema-v6 `security_checks`;
- the sibling-neutral `CorrectnessExecutor` / `CorrectnessRegistry` seam;
- an initial `waf_bypass` correctness family backed by strict-scope Eggsec;
- sanitized per-check evidence;
- comparison-critical correctness identity;
- ComparisonReceipt v3 with separate performance and correctness verdicts;
- `eggbench.security-correctness.v1` and conservative combined precedence.

That v1 correctness policy is explicitly tied to Eggsec WAF bypass counts.
`CorrectnessObserved` currently has only `WafBypass` and
`CorrectnessExpectationRecord` only `MaxSuccessfulBypasses`. M001 must not
silently reinterpret or extend that policy identifier.

M003a also contains a useful bounded deterministic directory/file identity
implementation for EggReplay fixtures. M001 should extract/generalize that
workspace-confined hashing primitive rather than create a second ad hoc corpus
or configuration digest algorithm.

### Current subject/runtime-binding gap

The runner currently accumulates runtime bindings from registered named
in-process service adapters. Managed command services and observed external
services do not themselves publish an `http_url` binding.

Security Qualification must keep SynVoid a benchmark subject/process, not add
a SynVoid Rust dependency or SynVoid-specific in-process service adapter.
Therefore M001 planning needs a small generic non-secret static/runtime HTTP
binding contract for command/external services (or an equivalent runner-owned
binding source) so a local process subject can be targeted by correctness and
workload drivers without subject-specific code.

### SynVoid audit

Current audited SynVoid source:

`dbowm91/synvoid@174fdbcd6f133b35099ed4492f5ed8d3fcaa7d4c`

SynVoid already owns a useful security corpus under
`crates/synvoid-waf/tests/fixtures/waf`. Request fixtures carry stable case
IDs, request method/path/headers/query/body inputs, an owner-defined
`detect|pass` expectation, and attack-family labels. The corpus includes
benign controls plus SQLi, XSS, path traversal, SSRF, request-smuggling and
other cases.

SynVoid also owns a canonical enforcement vocabulary
(`Allow/Observe/Challenge/Stall/Tarpit/Block/Drop`) and WAF configuration
whose attack-detection action may be `stall`, `block`, or `log`.

Eggbench must not import those internal types or decide what constitutes an
attack. For the first reusable qualification contract, the profile owner
should supply an observable HTTP expectation. A SynVoid M002 profile can
configure attack detection to `block`, then map its own source expectations
to deterministic external behavior (for example blocked status versus
controlled-origin success) before Eggbench executes the corpus. Challenge,
stall, tarpit, drop, and internal enforcement-reason semantics remain later
profile work.

SynVoid's existing performance campaign already covers the useful future M002
axes: benign and suspicious WAF traffic, representative body sizes,
concurrency 1/8/32/128, end-to-end throughput/tail latency, event-loop lag and
RSS. M001 should provide the reusable contract for these scenarios, not copy
SynVoid's benchmark implementation.

### Eggsec re-audit

Current Eggsec main observed during this research is
`d412e204e1866a8ea09d1d9d6e8be66f4a9097e2`; the closed M004 qualification
remains pinned to its audited `0509ac66...` source contract.

Eggsec contains internal WAF behavior/regression and provider-profile types,
but those are not newly promoted M001 machine seams. M001 should not depend on
them or replace M004's external strict-scope adapter. No Eggsec upstream
change is required for the profile/corpus layer identified here.

### Required M001 contract

The M001 architecture is now captured in three ordered implementation plans:

- `plans/implementation/security-qualification/001a-profile-corpus-config-identity-and-static-http-bindings.md` — ready;
- `plans/implementation/security-qualification/001b-fixed-corpus-http-correctness-family.md` — authored, hard-blocked on M001a closure;
- `plans/implementation/security-qualification/001c-qualification-suite-execution-and-m001-closure.md` — authored, hard-blocked on M001b closure.

**M001a — qualification profile, corpus, and configuration identity**

Introduce a versioned `SecurityQualificationProfile` separate from
`ExperimentPlan`. V1 should reference explicit bounded scenario plan files
rather than invent an arbitrary templating/JSON-patch language. Resolution
produces a deterministic expansion manifest containing profile/scenario order,
normal ExperimentPlan identities, corpus identity, target-configuration
identity, and expansion-policy version.

Introduce an Eggbench-owned normalized HTTP security corpus envelope whose
cases contain only transport/request data plus an owner-authored observable
expectation. Attack-family/category labels are opaque provenance, not semantics
interpreted by Eggbench. Initial expectations should be deliberately narrow:
HTTP status exact/set matching. Binary or non-UTF8 request material may be
referenced through workspace-confined body files.

Corpus and target-configuration inputs must use the same generalized bounded
content-tree identity: canonical relative paths, lengths and SHA-256 values,
with symlink escape and aggregate-size bounds. The source path is operator
context; the digest is the comparison identity.

Expected outcomes are frozen before candidate execution. A baseline result
must never redefine the expected security behavior.

**M001b — fixed-corpus HTTP correctness family**

Add a new correctness source/family that sends the already-declared corpus
requests through the existing Eggfetch HTTP stack to a local/private runtime
binding and compares only the observable result with the predeclared
expectation. It is a deterministic case executor, not a scanner: it does not
generate payloads, classify attacks, infer severity, or interpret SynVoid
internals.

Persist sanitized case evidence (case ID, request/case digest, expected
observable outcome, observed outcome, pass/fail) without retaining attack
payload bytes in the bundle.

Because `eggbench.security-correctness.v1` is explicitly the M004
`waf_bypass` contract, M001 should use a new correctness policy identifier.
A ComparisonReceipt schema v4 is the preferred compatibility boundary if the
typed correctness observation/expectation enums gain new corpus variants;
legacy v1-v3 receipts and the M004 v1 security policy remain unchanged.

The initial corpus family stays local/private and direct. It should reject
public targets, credentials/secrets in corpus headers, arbitrary proxy/routing,
paired-security execution, and any expectation that depends on timing.

**M001c — qualification execution/receipt and M001 closure**

Add a bounded profile/suite execution surface that runs the explicit expanded
scenarios using normal Eggbench run/compare machinery. A versioned
qualification receipt references immutable scenario bundle and comparison
receipt identities and aggregates scenario verdicts conservatively
(`Invalid > Fail > Inconclusive > Pass`). It must not recompute metric
statistics, reinterpret security semantics, or create a weighted
security/performance score.

A profile-level result therefore answers "did every required scenario satisfy
its already-defined correctness and performance gates?" while preserving each
ordinary run/receipt as the evidence authority.

CLI work should stay profile-oriented (validate/expand/qualify or equivalent)
rather than overloading the ordinary single-plan `run` contract. A generic
matrix/template language is deferred; explicit scenarios are sufficient for
M001 and keep expansion deterministic.

### M001 evidence identity

At minimum the profile/suite evidence must bind:

- profile schema, ID and content digest;
- expansion-policy version;
- ordered scenario IDs and generated/resolved plan identities;
- corpus schema, owner/source provenance and aggregate digest;
- target configuration input digest;
- owner-authored expectation policy/version;
- correctness adapter policy/version;
- workload/performance plan identity already carried by each scenario;
- subject revision/digest and normal testbed identity;
- scenario bundle and comparison-receipt digests in the final suite receipt.

Observed security outcomes, timings, timestamps and local source paths are
results/presentation, not configuration identity.

### Safety and ownership boundaries

- local/private targets remain the default and initial M001 requirement;
- corpus inputs are immutable, workspace-confined and bounded;
- raw attack payloads are not copied into portable evidence;
- authorization/scope remains owned by the producing security tool/profile;
- no SynVoid or Eggsec Rust dependency enters Eggbench core;
- no scanner payload-generation logic is implemented in Eggbench;
- resource/performance metrics continue to use existing workload/telemetry
  drivers and M004 combined-verdict semantics.

### Upstream/blocker disposition

No Eggsec or SynVoid upstream implementation blocker was identified for M001.
The SynVoid corpus is sufficient as a future M002 source fixture and its
observable block configuration can be made deterministic without promoting
SynVoid internals into Eggbench.

The substantive Eggbench-owned prerequisites are the reusable input-tree
digest extraction, the generic process/external HTTP binding seam, the
profile/corpus schemas, and the new fixed-corpus correctness family/policy.

M001 implementation planning should therefore be authored in Eggbench as the
next handoff. M002 remains responsible for the concrete SynVoid reproducible
suite after M001 closes.

## 3. Initial target: SynVoid

SynVoid is an especially useful first subject because it exposes a reverse proxy/WAF data plane, modern HTTP transport, rate limiting and bot controls, CPU/offload-sensitive detection paths, event-loop/resource telemetry, and prior performance campaign methodology.

Initial qualification profiles should cover:

- benign small requests;
- suspicious/malicious requests with expected dispositions;
- mixed benign/malicious traffic;
- representative small and larger body paths;
- connection churn;
- bounded concurrency ladder such as 1/8/32/128 where appropriate;
- routed subject versus direct-origin control;
- RSS, CPU, event-loop, queue, and target-specific telemetry where exposed.

## 4. Dependency graph

~~~text
Measurement gates + Eggstack/external drivers
       |
M001 Qualification-profile and correctness-gate contract
       |
M002 SynVoid reproducible profile suite
       |
M003 Reusable Eggstack security qualification patterns
~~~

## 5. Milestones

### M001 — Profile and correctness contract

Define reusable profile expansion, corpus/config digesting, expected-outcome assertions, and security-result profile semantics on top of the generic correctness execution/evidence and combined-verdict substrate established by Eggstack M004a/M004b.

A correctness failure must remain distinguishable from a benchmark invalidity or performance regression.

Ordered implementation decomposition:

1. **M001a — Profile/corpus/config identity + static HTTP bindings**  
   Ready. Establish the versioned qualification profile/corpus contracts, generalized bounded content-tree identity, deterministic expansion manifest, and generic command/external `http_url` binding seam.
2. **M001b — Fixed-corpus HTTP correctness family**  
   Authored; hard-blocked on M001a. Execute immutable corpus cases through Eggfetch outside performance timing, add sanitized observable-status evidence, correctness policy v2, and ComparisonReceipt v4 compatibility.
3. **M001c — Qualification suite execution/receipt and M001 closure**  
   Authored; hard-blocked on M001b. Run explicit scenarios serially through ordinary run/compare machinery and produce the immutable qualification receipt without recomputing metrics/security semantics.

### M002 — SynVoid suite

Build local controlled-origin WAF/proxy profiles and compare native and external workload drivers. Preserve target configuration and security result evidence.

Start with bounded local profiles rather than internet-facing or defense-lab flood modes.

### M003 — Reusable security performance patterns

Extend to Eggsec defense-validation/load profiles and other authorized local security services without turning Eggbench into a scanner.

## 6. Verification strategy

Use fixed local corpora with known expected dispositions, intentional false-positive/false-negative fixture failures, target configuration hash changes, performance-only regressions, correctness-only regressions, and combined failures.

## 7. Completion definition

The roadmap closes when a security optimization cannot be reported as successful solely because it is faster while required security behavior regressed.

## 8. Milestone status

Measurement/comparison prerequisites are closed and hosted-qualified. Eggstack M004 is now closed and hosted-qualified (`plans/closure/eggstack-integration/004a-status.md`, umbrella `plans/closure/eggstack-integration/004b-status.md`); the integration substrate gate is satisfied:

- M004a (`plans/implementation/eggstack-integration/004a-eggsec-strict-waf-correctness-adapter.md`) is closed and establishes the first strict Eggsec correctness executor/evidence contract.
- M004b (`plans/implementation/eggstack-integration/004b-security-correctness-gate-and-m004-closure.md`) is closed and establishes the generic independent correctness gate family plus combined verdict precedence.

Security Qualification M001 is now implementation-planned.

- M001a closed at `plans/closure/security-qualification/001a-status.md` (implementation `1622054`, hosted CI run `36214130746`, live-tools run `36214130734`).
- M001b is now dependency-ready; M001c remains blocked on M001b closure.
- M001b is authored but hard-blocked on M001a closure.
- M001c is authored but hard-blocked on M001b closure.

No Eggsec or SynVoid upstream blocker is currently known. The next executable handoff is M001b.
