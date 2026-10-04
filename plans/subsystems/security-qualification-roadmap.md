# Security Performance Qualification Roadmap

Status: active (M001 closed; M002 closed/hosted-qualified via the post-M002 corrective C002 (implementation `9d9d8b9`, exact-head hosted runs `36640125422` four-lane and `36640125453` live on the repairing source `4703333`); C001 remains STOPPED with verification at `b98973f` as historical evidence; SynVoid M002 asset contract closed; M003a-b closed; M003c generic collector implemented at `2742e0e` and CLOSING on real SynVoid v2 evidence; M003d Eggsec-load slice implemented at `739f382` with its CI portability corrective closed at `4703333`)

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

- `plans/implementation/security-qualification/001a-profile-corpus-config-identity-and-static-http-bindings.md` — closed at `plans/closure/security-qualification/001a-status.md`;
- `plans/implementation/security-qualification/001b-fixed-corpus-http-correctness-family.md` — closed at `plans/closure/security-qualification/001b-status.md`;
- `plans/implementation/security-qualification/001c-qualification-suite-execution-and-m001-closure.md` — closed at `plans/closure/security-qualification/001c-status.md`.

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
   Closed. Establishes the versioned qualification profile/corpus contracts, generalized bounded content-tree identity, deterministic expansion manifest, and generic command/external `http_url` binding seam.
2. **M001b — Fixed-corpus HTTP correctness family**  
   Closed and hosted-qualified. Executes immutable corpus cases through Eggfetch outside performance timing, adds sanitized observable-status evidence, correctness policy v2, and ComparisonReceipt v4 compatibility.
3. **M001c — Qualification suite execution/receipt and M001 closure**  
   Closed by `plans/closure/security-qualification/001c-status.md`. Runs explicit scenarios serially through ordinary run/compare machinery and produces an immutable qualification receipt without recomputing metrics/security semantics.

### M002 — SynVoid reproducible qualification suite

Status: closed and hosted-qualified. The routine scope is recorded
conditionally in `plans/closure/security-qualification/002a-status.md`
and umbrella `plans/closure/security-qualification/002b-status.md`
(implementation `b74f861`), and terminal qualification is recorded by
the post-M002 corrective C002 at
`plans/closure/security-qualification-m002-corrective/002-status.md`.
The previously missing SynVoid-owned qualification-asset contract is now
**closed upstream**:
`dbowm91/synvoid@30e68af8f6e79ce0fe07f0c1871f1d0caa6be6f2`,
implementation `ae045481752b8f750d6e6079b185c526a09c91d5`, with the
proof-bearing closeout at
`architecture/eggbench_security_qualification_asset_contract_closeout.md`.
The owner contract supplies the 15-case live-proxy corpus, deterministic
materializer, loopback minimal config, provenance, 15/15 live semantic proof,
and 2/2 performance-path proof.

The upstream asset dependency and the Eggbench terminal evidence are therefore
both satisfied. C002 consumed the owner export without reinterpreting SynVoid
Detect/Pass semantics, executed the real Stage B correctness/negative/teardown
proof and the real Stage C performance/oracle/same-source-repeatability path,
and closed on exact-head hosted runs `36640125422` (four lanes green) and
`36640125453` (`live-synvoid-linux` pass=25 stopped=0 notexec=5 with real
stages executed) against repairing source `4703333`.

Satisfied cross-repo prerequisite:

- `dbowm91/synvoid:plans/eggbench_security_qualification_asset_contract.md` — CLOSED 2026-09-28. SynVoid remains the authority for exported fixture selection, Detect/Pass-to-observable mapping, minimal loopback config materialization, and provenance.

Eggbench implementation sequence:

1. **M002a — SynVoid controlled correctness profile**  
   `plans/implementation/security-qualification/002a-synvoid-controlled-correctness-profile.md`  
   Conditionally closed (`plans/closure/security-qualification/002a-status.md`,
   implementation `b74f861`). Routine scope (synthetic stand-in) is green;
   real owner-exported live proof is now available upstream and must be consumed
   by C002 for terminal Eggbench qualification.
2. **M002b — SynVoid performance/resource suite and M002 closure**  
   `plans/implementation/security-qualification/002b-synvoid-performance-resource-suite-and-m002-closure.md`  
   Conditionally closed (umbrella `plans/closure/security-qualification/002b-status.md`,
   implementation `b74f861`). Smoke + baseline-relative profiles, independence
   proofs, and oracle procedures are green in routine scope; same shared conditions.

M002 v1 remains bounded/local. Current driver truth means the following earlier roadmap ideas are explicitly deferred rather than approximated incorrectly: mixed malicious traffic under load, arbitrary request-body attack load, explicit connection-churn controls, and SynVoid Prometheus/event-loop/queue metric ingestion. These become M003 candidates unless an existing qualified seam evolves before M002 implementation.

### M002 post-conditional-closure corrective

The routine M002 implementation at `b74f861` remains recorded as
conditionally closed; terminal qualification is owned by the corrective and is
now closed. Two proof gaps were registered under:

`plans/subsystems/security-qualification-m002-post-conditional-closure-corrective-addendum.md`

Trigger evidence:

- exact conditional-closure HEAD `660ab472...`;
- hosted CI run `36262615058` passed Linux stable, Linux Rust 1.89 and
  Windows stable but failed macOS workspace all-feature tests;
- live workflow `36262614991` was green, but the real SynVoid Stage B
  remained NOT-EXECUTED because the owner asset contract was still open at
  that historical execution point. The owner contract has since closed at
  SynVoid `30e68af8`; the trigger record remains historical evidence.

Corrective sequence:

1. **C001 — macOS hosted portability restoration**  
   `plans/implementation/security-qualification-m002-corrective/001-macos-hosted-portability-restoration.md`
   → closure `plans/closure/security-qualification-m002-corrective/001-status.md`
   — **STOPPED with verification** at implementation `b98973f`
   (`fix(qualification): restore macOS hosted portability via tcp-loopback
   readiness probe`). Replaced the runner's `delay`-based readiness check
   for managed command subjects with a `tcp-loopback` probe that retries
   the connect against the declared budget so the cold-cache Python
   `ThreadingHTTPServer` bind time on Apple Silicon can no longer declare
   the service ready before the listen socket exists. Hosted CI run
   `36451730637` records `synvoid_m002a` 7/7 green including the new
   routine regression. C001 STOP-with-verification is recorded on plan §10
   because the first correction surfaced an independent failure on
   `synvoid_m002b::perf_same_source_pair_never_fails` (Apple Silicon
   live-host repeatability), which the test source itself documents as
   "M002 closure condition" and which belongs to C002.
2. **C002 — real SynVoid qualification and final M002 closure**  
   `plans/implementation/security-qualification-m002-corrective/002-real-synvoid-live-qualification-and-final-closure.md`
   → closure `plans/closure/security-qualification-m002-corrective/002-status.md`
   — **closed**: the closed owner export is consumed (policy
   `synvoid.eggbench-qualification.v1`, implementation `ae045481`);
   Stage B real proof is green (15/15 positive, negative Fail,
   origin-log and teardown proofs); Stage C-real is green (smoke Pass,
   same-source Pass/Inconclusive never Fail, oha/h2load green); the full
   live harness runs 29/0/1 locally. Terminal hosted evidence was
   re-gathered on the repairing source `4703333` after the M003d CI
   portability corrective: four-lane run `36640125422` (all lanes green,
   including the `macos-stable` same-source lane) and live run
   `36640125453` (`live-synvoid-linux` pass=25 stopped=0 notexec=5 with
   real owner-exported stages executed).

M002 is closed and hosted-qualified. `live-synvoid-linux` executed real
owner-exported SynVoid stages rather than passing with those stages
NOT-EXECUTED, and the C001-surfaced live-host same-source condition is
discharged both on the real subject and on the green `macos-stable` lane.

M003c terminal telemetry qualification and M003d profile work no longer carry
an M002 dependency. Terminal M003 closure still must satisfy the original
M003a-d acceptance gates.

### M003 — Reusable security performance patterns

Extend to Eggsec defense-validation/load profiles and other authorized local security services without turning Eggbench into a scanner.

#### M003 handoff research — 2026-09-28

Research baseline: `eggstack/eggbench@1eb1bfed4edbc6c4af4f193fa71dc82235ad927e`.

Re-audited sibling baselines:

- SynVoid current owner qualification baseline: `30e68af8f6e79ce0fe07f0c1871f1d0caa6be6f2`;
- Eggsec current main observed during planning: `7ebddbf1669ca1af18957eca27d09e90ed5c718e`.

Findings:

1. **Qualification methodology is not fully frozen.** Profile v1 cannot bind a workload driver per scenario, and `qualify run` currently dispatches with `workload_driver: None`. External oha/h2load scenarios therefore sit outside the profile.
2. **Dynamic dependency bindings stop at topology publication.** Named adapters can publish ephemeral bindings (for example EggServe `bound_port`), but command argv/env cannot consume a dependency binding at launch time. Reusable owner wrappers therefore need a generic typed binding-resolution seam rather than fixed ports or subject-specific runner code.
3. **Native HTTP workload shape is too narrow.** `eggfetch-http` is GET-only, uses the target's complete `http_url`, and intentionally keeps one client/pool for the invocation. M003 body/mixed/churn work needs method/path/header/body requests plus explicit connection policy.
4. **The existing HTTP security corpus should be reused.** It already owns bounded method/path/query/header/body inputs, expected observable status, workspace confinement, and immutable content identity. M003 should schedule those owner-authored cases under load rather than create another payload schema.
5. **Mixed traffic must be exact, not probabilistic.** Qualification needs a deterministic finite multiset/permutation derived from immutable inputs and the experiment seed so baseline/candidate receive identical mixtures.
6. **Expected security blocks are not transport failures.** A correct 403 cannot be fed into a generic HTTP-error gate. M003 must separate transport failure, HTTP response, and owner-expected outcome match.
7. **Connection churn is a method variable.** Current Eggfetch pooling is explicit method provenance. Fresh-per-request must be a comparison-critical connection policy, not a `Connection: close` header approximation.
8. **Target telemetry has a clean runner seam but no stable SynVoid export yet.** `TelemetryCollector` already has correct trial start/stop ownership. Gregg remains host/testbed telemetry. SynVoid worker payloads now contain event-loop lag, queue percentiles, active connections, worker resource data, body-buffering counters, and CPU-offload counters, but M002 disables metrics and those fields are not yet a frozen live Prometheus contract.
9. **SynVoid upstream work is required only for telemetry publication.** A new independent handoff is registered at `dbowm91/synvoid:plans/eggbench_security_qualification_m003_telemetry_contract.md`.
10. **Eggsec needs no upstream change for initial M003.** Its current strict-scope `load` CLI already supports finite requests, concurrency, method, body, headers, timeout, JSON output and rate control; Eggbench can consume it through the existing trusted external-command/scope pattern. Eggsec's load path currently uses Eggfetch, so it is a security-owner execution path, not an independent transport oracle.
11. **External oha is already capable of more than the Eggbench adapter exposes.** Current upstream supports method/headers, inline/file bodies, URL files, JSON output, explicit HTTP versions, and `--disable-keepalive`; M003 can extend the adapter without an oha upstream change.

Ordered implementation decomposition:

1. **M003a — qualification method binding and runtime binding resolution**  
   `plans/implementation/security-qualification/003a-qualification-method-binding-and-runtime-binding-resolution.md`  
   **CLOSED** at `plans/closure/security-qualification/003a-status.md`. Adds profile-v2 per-scenario driver binding, generalized non-secret static runtime bindings, and typed dependency binding resolution into command argv/env with auditable evidence.
2. **M003b — HTTP security load shapes, mixed schedules, and connection policy**  
   `plans/implementation/security-qualification/003b-http-security-load-shapes-mixed-schedules-and-connection-policy.md`  
   **CLOSED** at `plans/closure/security-qualification/003b-status.md`. Reuses immutable HTTP corpora for deterministic body/mixed load, owner-expected outcomes, pooled/fresh connection methods, and the independent oha single-case body/churn subset.
3. **M003c — subject telemetry and SynVoid metrics ingestion**  
   `plans/implementation/security-qualification/003c-subject-telemetry-and-synvoid-metrics-ingestion.md`  
   Closure: `plans/closure/security-qualification/003c-status.md` — **CLOSED** (corrected owner `1338ce7b`). Generic bounded Prometheus collector (`2742e0e`) plus the `TelemetryPreflightTiming` probe-point seam, bounded polling drain, cadence-aware preflight retry, and fail-closed negatives. Live evidence against the real pinned minimal SynVoid: `m003c-13` collected 3 trials × 20 in-window samples of the owner mapping with required fields observed; renamed-metric and TYPE-drift runs fail closed; optional owner samples stay absent and warned. The formerly blocked gate `m003c-13b` now passes (`subject_active_connections = 8.0`, `subject_event_loop_lag_ms = 1.0`); the harness was never weakened.
4. **M003d — Eggsec load profiles, live qualification, and M003 closure**  
   `plans/implementation/security-qualification/003d-eggsec-load-profiles-live-qualification-and-m003-closure.md`  
   Closure: `plans/closure/security-qualification/003d-status.md` — **closed and hosted-qualified (corrected owner `1338ce7b`; four-lane CI `37143714313` + live `37143714261` green on `30a3825`)**. Adapter slice `739f382`, CI portability corrective `4703333`, then the M003 profile family, explicit baselines, single-owner-instance live workspace, and negative demonstrations. Executing against the real pinned Eggsec found three further adapter contract defects (`--header` vs `--headers`, the load-only execution policy Eggsec requires before it will authorize `load-test` under any profile, and `--quiet` suppressing the machine report) plus a preflight document-extraction defect; all are repaired with tests. Live `m003d-7`/`m003d-8` prove correctness-only and performance-only regressions each fail the suite. M003 milestone record: `plans/closure/security-qualification/003-status.md`.

Explicitly deferred from M003 v1:

- malformed/raw HTTP and request smuggling;
- multipart/binary upload performance unless an owner contract expands;
- challenge/stall/tarpit/drop timing semantics;
- broad TLS/H2/H3 security matrices;
- network impairment/netem combinations;
- unbounded stress/flood traffic.

## 6. Verification strategy

Use fixed local corpora with known expected dispositions, intentional false-positive/false-negative fixture failures, target configuration hash changes, performance-only regressions, correctness-only regressions, and combined failures.

## 7. Completion definition

The roadmap closes when a security optimization cannot be reported as successful solely because it is faster while required security behavior regressed.

## 8. Milestone status

Measurement/comparison prerequisites are closed and hosted-qualified. Eggstack M004 is now closed and hosted-qualified (`plans/closure/eggstack-integration/004a-status.md`, umbrella `plans/closure/eggstack-integration/004b-status.md`); the integration substrate gate is satisfied:

- M004a (`plans/implementation/eggstack-integration/004a-eggsec-strict-waf-correctness-adapter.md`) is closed and establishes the first strict Eggsec correctness executor/evidence contract.
- M004b (`plans/implementation/eggstack-integration/004b-security-correctness-gate-and-m004-closure.md`) is closed and establishes the generic independent correctness gate family plus combined verdict precedence.

Security Qualification M001 is closed by `plans/closure/security-qualification/001c-status.md`.

- M001a closed at `plans/closure/security-qualification/001a-status.md` (implementation `1622054`, hosted CI run `36214130746`, live-tools run `36214130734`).
- M001b is closed and hosted-qualified.
- M001c is closed and provides the reusable suite/receipt substrate.

M002 is closed and hosted-qualified. Routine scope landed at `b74f861` and is recorded conditionally in `plans/closure/security-qualification/002a-status.md` and `plans/closure/security-qualification/002b-status.md`; terminal qualification is the post-M002 corrective: C001 restored the runner-side macOS readiness defect and remains STOPPED with verification at `b98973f`; C002 discharged live-host repeatability, exact-head/four-lane verification, real owner-exported SynVoid live stages, and terminal reconciliation. The SynVoid owner-side asset contract prerequisite is closed upstream.

Implementation-time SynVoid re-audit at
`7f1b79452a683e758e0b4ea1e70f6c0f2463f0d1` (package 1.1.0) confirmed the
supported `--no-default-features` minimal WAF/proxy runtime,
`--foreground --config-path` lifecycle, a 27-case authoritative WAF fixture
corpus with Detect/Pass semantics (5-case GET path/query allowlist identified;
the rest use M001-forbidden headers/bodies/URLs or deferred semantics), and no
existing Eggbench export contract. The upstream plan therefore still owns
fixture selection/mapping/config materialization; Eggbench does not duplicate
those semantics.

SynVoid owner-side asset contract is closed at implementation `ae045481` / closeout head `30e68af8`, so it is no longer an active C002 blocker. Eggbench C001 is **STOPPED with verification** at `b98973f` (closure `plans/closure/security-qualification-m002-corrective/001-status.md`, disposition `STOPPED with verification`) — hosted CI run `36451730637` recorded `synvoid_m002a` 7/7 green and the live-host repeatability finding is owned by C002.

C002 is **closed**. Its real/live work is no longer hypothetical: hosted live run `36607614200` was green on the C002 source, its exact-head four-lane run `36607614262` was red only on M003d-owned source regressions (a stale production workload/descriptor count and Windows cfg-only `unused_mut` in the Eggsec-load adapter), C002 performed no production repair itself, and the registered repair closed at `4703333`. C002 then re-gathered its gates on that repairing source: four-lane run `36640125422` (all lanes green) and live run `36640125453` (`live-synvoid-linux` real SynVoid stages executed, pass=25 stopped=0 notexec=5).

M003 dependency reconciliation (2026-09-29):

- M003a-b are closed.
- M003c's generic collector landed at `2742e0e`.
- SynVoid's corrected owner handoff is now **CLOSED QUALIFIED** under
  `synvoid.eggbench-telemetry.v2` at proof-bearing implementation
  `739e7ba6f02c5e3f83fe9ff5321b09213182b193`; current closeout head
  observed is `0dc1f7fb21a5df60e72fc7f2cd60b7cb73bc9f35`, with mapping
  SHA-256 `622f6a13c4353cc7465cce39a57ed86fa0db2fe4114258e6f06226c1748d2d99`.
- SynVoid's owner closeout used Eggbench's actual M003c parser/collector and
  a live minimal endpoint, so the cross-repo interface dependency is closed.
  Eggbench still owns current-head M003b pressure/body trial, drain,
  fail-closed, hosted, and closure evidence.
- M003c is therefore **CLOSING**, not blocked, and may run its terminal v2 live
   qualification now. Its green-hosted-source closure precondition is
   satisfied by run `36640125422` on `4703333`.
- M003d's Eggsec-load adapter slice landed at `739f382`; the CI portability
  corrective it required closed at `4703333`
  (`plans/closure/security-qualification/003d-corrective-status.md`).
- Full M003d profile/live/closure work is now gated only on terminal M003c.
  SynVoid telemetry and M002 C002 are no longer blockers.

M003 live-value reconciliation (2026-10-02):

- The upstream defect is repaired in SynVoid: `ccf92694` routes
  `Message::UnifiedServerWorkerHeartbeat` into
  `ProcessManager::handle_unified_server_worker_heartbeat` and restores the
  worker-ID classification; `1338ce7b` additionally unifies the worker's
  `WorkerMetrics` instance (a second fresh instance had kept every
  request-derived counter at zero) and seeds the heartbeat lag accumulator at
  the schedule it measures (it had been one cadence ahead, clamping
  `event_loop_lag_ms` to a structural zero). No contract, mapping, name, unit,
  cadence, WAF, or harness-assertion change.
- The pin advanced to `1338ce7b60f3793701091b4c329f80eb542f802d`; the same
  harness now reports `pass=30 stopped=0 notexec=0` with `m003c-13b` green
  (`subject_active_connections = 8.0`, `subject_event_loop_lag_ms = 1.0`, 20
  in-window samples per trial).
- M003c is **CLOSED**; M003d is re-verified with it. The M003 milestone waits
  only on the hosted four-lane plus `live-m003-linux` runs on the closing
  revision (see `003c-status.md`).

M003 execution status after the profile/qualification pass:

- M003c and M003d Eggbench-owned scope is complete and verified both locally
  (`pass=30 stopped=0 notexec=0`) and on the hosted real owner
  (`pass=29 stopped=0 notexec=1`, the one `NOT-EXECUTED` being `m003d-4` on a
  runner without `oha`).
- The M003 milestone is **closed and hosted-qualified** at
  `plans/closure/security-qualification/003-status.md`, with exact-head hosted
  evidence on `30a3825`: four-lane CI `37143714313` and live external-tool
  qualification `37143714261`, both green.
- The upstream telemetry-population defect was reported to SynVoid and fixed at
  owner `1338ce7b`; the milestone's disposition is terminal.
- Remaining optional work, not a closure condition: none outstanding. Two items
  previously listed here are now done: `m003d-4` executes on hosted runners via
  a pinned digest-verified `oha` provisioned by
  `scripts/qualification/provision-external-oracles.sh`, with its claim asserted
  from the run evidence rather than the exit status (`973451f`); and the M002b
  same-source performance proof runs on every host — the Linux gate is removed
  because the macOS failure was a short-window measurement defect, fixed by
  sizing all six proxy scenarios to 12000 requests per trial against the frozen
  allowances (`973451f`), with the M002 harness retaining stage diagnostics
  past `$WORK` (`a895fb8`). Hosted confirmation of the new revisions is pending
  the in-flight CI/live runs. Both are recorded in their closure records.
