# Security Qualification M002a — SynVoid Controlled Correctness Profile

Status: conditionally closed by plans/closure/security-qualification/002a-status.md
(implementation `b74f861`; routine scope green; live reverse-proxy proof and
the SynVoid upstream asset contract remain named conditions)

Repository baseline: `c77a755ba33f103526f44d574afb9dd2676228e3`

Source roadmap:

- `plans/subsystems/security-qualification-roadmap.md` — M002

Hard prerequisites:

- Security Qualification M001 closed.
- SynVoid upstream plan
  `dbowm91/synvoid:plans/eggbench_security_qualification_asset_contract.md`
  closed with a proof-bearing source SHA and generated asset contract.

Primary class: subject qualification / concrete reusable profile.

## 1. Objective

Instantiate the generic M001 security-qualification substrate for SynVoid
without adding SynVoid-specific runtime code to Eggbench.

M002a consumes SynVoid-owned exported qualification assets and builds the first
reproducible local profile that proves:

- SynVoid can run as a managed command subject under Eggbench;
- its exported WAF corpus executes through M001b fixed-corpus correctness;
- owner-authored Detect/Pass semantics survive the live reverse-proxy path as
  externally observable HTTP expectations;
- profile/corpus/config/source identities are preserved in immutable evidence;
- correctness failures remain independent from benchmark execution status.

## 2. Ownership boundary

SynVoid owns:

- qualification config materialization;
- source WAF fixture selection;
- source Detect/Pass semantics;
- mapping policy from source semantics to observable statuses;
- source/exclusion provenance.

Eggbench owns:

- profile/scenario plans;
- managed command lifecycle;
- controlled origin;
- corpus/config hashing;
- HTTP correctness execution;
- qualification receipt.

Do not import SynVoid Rust crates.

## 3. Upstream asset contract

M002a must consume the closed SynVoid asset contract, not reconstruct it.

Expected materialized inputs:

~~~text
synvoid-qualification/
  config/
    main.toml
    sites/...
  corpus.json
  provenance.json
~~~

The implementation must verify:

- upstream policy identifier;
- exact SynVoid source SHA;
- package version;
- source-fixture provenance;
- generated config digest;
- generated corpus digest;
- listen/origin ports;
- observable block/pass mapping.

If the actual upstream layout differs, adapt only at the M002a fixture-import
boundary. Do not reinterpret SynVoid source fixtures inside Eggbench.

## 4. Qualification source pin

At implementation handoff, re-audit current SynVoid `main` and pin the exact
closed upstream asset-contract implementation SHA used for live qualification.

Do not pin the pre-plan baseline `49b4624...` merely because it was used
during planning.

Record:

- SynVoid source SHA;
- package version;
- Cargo.lock digest;
- minimal binary SHA-256;
- materializer policy/version;
- exported corpus/config digests.

## 5. SynVoid build/runtime profile

Live qualification uses SynVoid's supported minimal profile:

~~~text
cargo build --locked --release --no-default-features
~~~

Run under Eggbench as a managed command:

~~~text
synvoid --foreground --config-path <materialized-config>
~~~

Requirements:

- controlled PATH or explicit resolved executable;
- no global install required;
- loopback-only data plane;
- static `http_url` from M001a matches generated listen port;
- no public bind;
- no admin/mesh/DNS requirement;
- child environment contains only the explicitly required qualification
  values;
- normal Eggbench cancellation/drain/teardown owns the SynVoid process tree.

## 6. Controlled origin

Use the existing EggServe controlled-origin adapter.

The origin must provide deterministic loopback responses required by the
SynVoid export policy, including at minimum:

- success response for Pass cases;
- stable small-response path for later performance reuse;
- stable larger-response path for later performance reuse.

The WAF correctness corpus targets SynVoid, not the origin directly.

Origin request counts may be retained as diagnostic proof that blocked requests
did not reach the origin where the harness can observe this safely.

## 7. M002a profile layout

Add checked-in profile/scenario assets under a clearly owned path, e.g.:

~~~text
qualification/synvoid/v1/
  profile.json
  scenarios/
    waf-correctness.json
  README.md
~~~

The generated SynVoid config/corpus themselves need not be committed if they
are materialized from the pinned upstream source during live qualification.

The checked-in profile must use M001 profile schema v1 and normal ExperimentPlan
schema contracts.

## 8. Correctness scenario

One required scenario uses:

- managed EggServe origin;
- managed SynVoid command subject;
- M001a static SynVoid `http_url`;
- M001b `eggbench-http-corpus/http_observable` correctness family;
- SynVoid-exported corpus/config identity.

The scenario is correctness-gating.

Expected semantics:

- every exported Pass case must satisfy its owner-authored observable status;
- every exported Detect case must satisfy its owner-authored block status;
- any valid mismatch -> correctness Fail;
- transport/config/evidence failure -> Invalid;
- a correctness Fail does not become `WorkloadFailed`.

## 9. Readiness

Readiness must prove the SynVoid data plane is serving the intended
qualification config.

Prefer a bounded HTTP readiness request through the static binding to a benign
qualification path.

Do not scrape human logs for readiness if a network-level readiness probe is
available.

Readiness must occur after both controlled origin and SynVoid process startup.

## 10. Evidence identity

The resulting candidate bundle/profile receipt must bind:

- SynVoid executable SHA/version/source SHA;
- SynVoid qualification policy identifier;
- exported source-fixture provenance digest;
- corpus schema/ID/digest;
- target-config digest;
- SynVoid static binding;
- controlled-origin identity;
- scenario plan digest;
- M001 correctness policy v2;
- candidate bundle/comparison receipt identities.

Do not persist raw attack payload bytes in the bundle beyond the workspace
input corpus itself. Portable evidence remains the sanitized M001b case
projection.

## 11. Local/private safety

- all listeners loopback;
- no public target;
- no remote DNS dependency;
- no credentials;
- no arbitrary proxy route;
- no Eggress/network-path composition in M002a;
- no stress/flood behavior;
- no scanner generation;
- no admin API exposure requirement.

## 12. Unsupported upstream cases

M002a must fail closed if SynVoid's upstream export includes an input forbidden
by the M001 corpus contract.

Do not weaken Eggbench to accept:

- hop-by-hop/request-smuggling headers;
- credentials/cookies/auth headers;
- absolute target URLs;
- public targets;
- unsupported binary material.

The correction belongs in the SynVoid export policy/allowlist.

## 13. Routine tests

Without requiring a real SynVoid checkout:

- validate profile/scenario files;
- consume a checked-in synthetic materialized asset fixture matching the
  upstream manifest schema;
- prove digest/provenance mismatch rejection;
- prove static binding/config port mismatch rejection;
- prove source policy mismatch rejection;
- prove correctness Pass/Fail/Invalid mapping;
- prove cancellation cleanup with a fake command subject.

These tests must remain cross-platform where the generic runner supports them.

## 14. Live SynVoid qualification

Add a Linux-only live qualification job or extension to the existing live
workflow.

It must:

1. check out the exact pinned SynVoid source;
2. build `--release --no-default-features`;
3. invoke the upstream qualification materializer;
4. run its upstream check/configtest;
5. place the generated assets under an Eggbench qualification workspace;
6. run `eggbench qualify validate/expand/run/inspect`;
7. verify the correctness scenario is Pass;
8. retain bounded provenance summaries;
9. clean all child processes and temporary assets.

Do not count an upstream unit-corpus test as live reverse-proxy qualification.

## 15. Negative live proof

The live harness must also demonstrate one deterministic failing case without
changing SynVoid production semantics.

Preferred approach:

- copy the exported corpus into a temporary workspace;
- alter one expected observable status only;
- recompute the temporary corpus identity through normal Eggbench input
  handling;
- run the profile and require correctness Fail / qualification Fail.

This proves the Eggbench gate detects an expectation mismatch.

Do not modify the SynVoid source fixture to manufacture the failure.

## 16. Verification matrix

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

Hosted closure requires normal Linux stable, Linux 1.89, macOS, and Windows
Eggbench CI plus the Linux-only live SynVoid qualification.

## 17. Closure

Create:

`plans/closure/security-qualification/002a-status.md`

Record:

- Eggbench implementation SHA;
- SynVoid asset-contract closure SHA;
- SynVoid exact live qualification SHA/version/binary digest;
- materializer policy/version;
- corpus/config/source-fixture digests;
- positive/negative live correctness results;
- cleanup evidence;
- normal four-lane CI;
- Linux live job;
- unresolved findings.

## 18. Acceptance criteria

M002a closes only when:

1. SynVoid upstream asset contract is closed;
2. Eggbench does not parse SynVoid internal fixture semantics;
3. SynVoid minimal binary runs as a managed command subject;
4. loopback controlled origin and static binding are deterministic;
5. exported corpus executes through M001b;
6. live Pass/Detect mappings match owner-authored observable expectations;
7. negative expectation mutation yields qualification Fail;
8. raw payloads are not copied into portable result evidence;
9. executable/config/corpus/source provenance is immutable;
10. cancellation/teardown leave no SynVoid child behind;
11. Rust 1.89 and four-lane CI are green;
12. Linux live SynVoid qualification is green.

## 19. Stop conditions

Stop and re-plan if:

- SynVoid asset export is not deterministic;
- live SynVoid behavior disagrees with its owner-exported expectations;
- SynVoid requires a public bind;
- SynVoid process lifecycle cannot be owned by the generic command runner;
- the profile requires SynVoid-specific Rust integration;
- closing the scenario requires weakening M001 corpus safety rules.
