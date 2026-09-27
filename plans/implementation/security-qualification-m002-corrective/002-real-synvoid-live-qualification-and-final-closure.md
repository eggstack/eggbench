# Security Qualification M002 Corrective C002 — Real SynVoid Qualification and Final M002 Closure

Status: authored; blocked on C001 closure and SynVoid asset-contract closure

Corrective authority:

- `plans/subsystems/security-qualification-m002-post-conditional-closure-corrective-addendum.md`

Hard prerequisites:

1. C001 closed:
   `plans/closure/security-qualification-m002-corrective/001-status.md`;
2. SynVoid owner-side plan closed:
   `dbowm91/synvoid:plans/eggbench_security_qualification_asset_contract.md`;
3. SynVoid closeout records a proof-bearing implementation SHA and live
   reverse-proxy semantic proof.

Primary class: external sibling interoperability qualification and final milestone closure.

## 1. Objective

Replace the remaining synthetic-only M002 qualification evidence with real
SynVoid owner-authored assets and real SynVoid reverse-proxy execution, then
convert M002 from conditional closure to full closure.

C002 is evidence-first. The expected production semantic change is none.

## 2. Re-audit at handoff

At implementation start, record:

- current Eggbench HEAD;
- current closed C001 SHA;
- exact SynVoid asset-contract closure SHA;
- exact SynVoid package version;
- exact SynVoid Cargo.lock SHA-256;
- materializer/export policy identifier;
- exported corpus/config/provenance schema versions;
- minimal SynVoid binary SHA-256.

Do not use the earlier planning baseline as live proof.

## 3. Asset import boundary

Consume only the owner-authored SynVoid export.

Required inputs are whatever the closed SynVoid contract names as canonical,
but must include equivalent facts to:

- normalized Eggbench-compatible corpus;
- loopback-only SynVoid config tree;
- owner policy identifier;
- exact source SHA/version;
- selected fixture IDs;
- explicit exclusions;
- source fixture digests;
- corpus/config digests;
- block/pass wire-status mapping;
- materializer version.

Eggbench must not:

- parse SynVoid internal WAF source fixture format;
- derive Detect/Pass mapping itself;
- silently add excluded fixtures;
- normalize unsupported hop-by-hop/smuggling/binary cases into a weaker form.

## 4. Harness Stage B — real correctness proof

Replace the NOT-EXECUTED owner-contract gate with actual live execution.

Required topology:

~~~text
Eggbench controlled EggServe origin
          ^
          |
real SynVoid minimal proxy/WAF
          ^
          |
M001b fixed-corpus correctness
~~~

Required positive proof:

- owner export verifies successfully;
- SynVoid minimal binary configtest succeeds;
- SynVoid starts under the generic command-service lifecycle;
- all exported Pass cases satisfy owner-authored success status;
- all exported Detect cases satisfy owner-authored block status;
- correctness aggregate Pass;
- qualification aggregate Pass;
- no blocked request reaches the controlled origin where the harness can prove
  this without adding product-specific instrumentation;
- teardown leaves no child process or bound port behind.

Required negative proof:

- mutate one temporary expected observable status only;
- recompute normal Eggbench corpus identity;
- rerun;
- require correctness Fail and qualification Fail;
- execution must not be reclassified as workload/process failure merely
  because the security expectation mismatched.

## 5. Harness Stage C — real performance path

Run the existing M002b benign proxy/profile procedures against the real SynVoid
binary and owner-generated config.

At minimum:

- native Eggfetch small-response proxy scenario;
- native Eggfetch large-response proxy scenario;
- direct-origin controls;
- at least one independent external oracle (oha preferred; h2load if its
  current absolute-gate scoping remains valid);
- explicit baseline/candidate workflow;
- same-source repeatability proof with no false Fail under the frozen v1 policy.

The purpose is to prove the real subject path, not to claim a performance
improvement from same-source pairs.

## 6. Baseline and candidate identity

Real runs must bind:

- SynVoid source SHA;
- SynVoid binary SHA/version;
- build feature profile;
- owner qualification policy/materializer version;
- corpus digest;
- config digest;
- static binding;
- controlled-origin identity;
- workload driver/version;
- baseline bundle identity;
- comparison receipt identity.

If owner config/corpus identity differs between baseline and candidate outside
an explicitly intended reviewed change, comparison must fail closed.

## 7. Synthetic fixtures

Keep the synthetic stand-in tests as routine contract tests.

Do not remove them when real live proof lands; they remain useful for:

- deterministic cross-platform CI;
- negative contract testing;
- provenance mismatch testing;
- cancellation/teardown testing.

But documentation must clearly distinguish:

- synthetic routine tests;
- real SynVoid live qualification.

## 8. Real-SynVoid CI job semantics

The `live-synvoid-linux` job must become truthfully proof-bearing.

It must fail when:

- SynVoid asset materializer is absent;
- export check fails;
- minimal SynVoid build fails;
- configtest fails;
- Stage B is NOT-EXECUTED;
- Stage B positive correctness fails;
- Stage B negative proof does not Fail as expected;
- Stage C required real-subject scenarios fail;
- cleanup fails.

A green job with a required real stage marked NOT-EXECUTED is not acceptable
for C002 closure.

## 9. Exact-head closure qualification

On the final proof-bearing Eggbench SHA require:

- Linux stable pass;
- Linux Rust 1.89 pass;
- macOS stable pass;
- Windows stable pass;
- live-tools-linux pass;
- live-eggsec-linux pass;
- live-m004b-linux pass;
- live-synvoid-linux pass with real SynVoid stages executed.

The same SHA must be referenced by the final closure record.

## 10. Production change policy

Expected production semantic change: none.

Allowed C002 code changes:

- adapt the qualification harness/import layer to the exact closed SynVoid
  export layout;
- add bounded provenance parsing/validation for the owner manifest;
- add test fixtures for the real manifest schema;
- fix harness-only path/port wiring.

If real SynVoid execution exposes an Eggbench production defect such as:

- command lifecycle cannot own real SynVoid;
- corpus executor produces wrong requests;
- static binding semantics are insufficient;
- cleanup leaves children behind;
- comparison/evidence identity is wrong;

stop C002 and author C003 before changing production semantics.

If the defect is in SynVoid's export or live behavior, correct it in SynVoid
and update its closeout; do not compensate in Eggbench.

## 11. Documentation reconciliation

After successful proof:

- update `plans/closure/security-qualification/002a-status.md` only through a
  clearly labeled addendum or successor reference; do not rewrite its
  historical conditional evidence;
- create the corrective closure record below;
- update Security Qualification roadmap M002 from conditionally closed to
  closed/hosted-qualified;
- update registry;
- state that M003 research/planning remains next and that M003 implementation
  may now rely on fully qualified M002.

## 12. Closure

Create:

`plans/closure/security-qualification-m002-corrective/002-status.md`

This is the terminal proof-bearing M002 closure.

Record:

- C001 closure SHA;
- SynVoid asset-contract closeout SHA;
- exact SynVoid source SHA/version/Cargo.lock digest;
- SynVoid binary SHA;
- owner policy/materializer version;
- selected/excluded fixture provenance;
- config/corpus digests;
- real Stage B positive result;
- real Stage B negative result;
- real Stage C performance/oracle results;
- baseline repeatability evidence;
- cleanup evidence;
- final four-lane CI run;
- final live workflow run;
- unresolved deferrals to M003.

## 13. Acceptance criteria

C002/M002 closes only when:

1. C001 is closed;
2. SynVoid owner-side asset contract is closed;
3. real owner export is consumed without SynVoid semantic reinterpretation;
4. real SynVoid minimal binary is built and config-tested;
5. real Pass/Detect correctness is green;
6. deterministic negative expectation mutation yields Fail;
7. real benign native performance path executes;
8. at least one real independent-oracle path executes;
9. same-source repeatability produces no false performance Fail;
10. synthetic cross-platform contract tests remain green;
11. exact-head four-lane normal CI is green;
12. exact-head live workflow is green;
13. `live-synvoid-linux` executed real stages rather than NOT-EXECUTED;
14. closure/roadmap/registry reconciliation is committed.

## 14. Stop conditions

Stop and create C003 or an upstream SynVoid corrective if:

- owner-exported wire expectations disagree with real SynVoid behavior;
- the closed SynVoid export violates M001 corpus safety;
- real SynVoid cannot run under the generic command lifecycle;
- a production Eggbench semantic change is required;
- same-source performance policy yields deterministic false Fail on the real
  subject;
- full proof requires mixed malicious load, Prometheus ingestion, or another
  explicitly deferred M003 capability.
