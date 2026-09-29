# Security Qualification M003a — Qualification Method Binding and Runtime Binding Resolution

Status: closed; implementation and four-lane hosted qualification complete.

Research baseline: `eggstack/eggbench@1eb1bfed4edbc6c4af4f193fa71dc82235ad927e`.

Source roadmap:

- `plans/subsystems/security-qualification-roadmap.md` — M003
- `plans/002-long-term-roadmap.md` — Phase 7

Related contracts:

- `plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md`
- `plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md`
- `plans/adrs/ADR-0004-eggstack-composition-and-independent-oracles.md`

## 1. Objective

Establish the generic methodology-binding substrate required by Security Qualification M003 before adding richer HTTP/security workloads.

This plan closes two current truthfulness gaps:

1. qualification profiles cannot freeze a workload-driver selection per scenario; `qualify run` currently invokes every scenario with `workload_driver: None`;
2. managed command services cannot consume runtime bindings published by already-ready dependencies, so an owner-supplied wrapper cannot safely materialize a subject configuration from an ephemeral controlled-origin port without hard-coded ports or out-of-band orchestration.

The implementation must remain subject-neutral. No SynVoid, Eggsec, WAF, attack-family, or security-product semantic belongs in the core/runner contracts added here.

## 2. Research findings

At the research baseline:

- `SecurityQualificationProfileV1` stores scenario ID, plan path, and optional baseline bundle only.
- `qualify_run()` resolves the profile expansion and then calls the ordinary run path with no explicit workload driver.
- standalone `eggbench run --workload-driver ...` already supports explicit workload-driver selection, so M003 does not need a second execution engine.
- runtime bindings are already a generic runner-owned map and are retained in runtime-topology evidence.
- named adapters may publish dynamic values such as EggServe's ephemeral `bound_port` and `http_url`.
- command/external services may publish a static `http_url`, but there is no general static binding map and no dependency-binding-to-command argument/environment resolution.
- command process specs are currently fully lowered before startup, which prevents a later dependency binding from influencing an argv/env value.

M003 must reuse these existing seams rather than introducing a security-specific launcher.

## 3. Qualification profile v2

Add a backward-compatible profile schema v2.

Each scenario must be able to declare an optional explicit workload driver:

~~~json
{
  "id": "mixed-corpus-c8",
  "plan": "scenarios/mixed-c8.json",
  "baseline_bundle": "baselines/mixed-c8.eggb",
  "workload_driver": "eggfetch-http"
}
~~~

Requirements:

- profile v1 remains readable and retains current behavior;
- profile v2 rejects unknown fields and malformed driver names;
- driver selection is frozen during expansion before candidate execution;
- `ExpandedScenario` records the selected driver;
- qualification receipts record the selected driver/method identity or bind it transitively to the candidate bundle identity;
- `qualify inspect` verifies that scenario evidence was produced by the frozen selection;
- absent driver selection preserves ordinary automatic/default resolution semantics only where that resolution is deterministic and evidence-identifiable;
- an unavailable requested driver is an explicit capability/invalid execution result, never fallback to another workload driver.

Do not add driver-specific options to the profile. Driver options belong in typed experiment/workload contracts.

## 4. General static runtime bindings

Generalize the existing one-off static `Service.http_url` mechanism into a bounded non-secret static binding map for the next plan schema while retaining `http_url` compatibility.

Recommended model:

~~~text
service.static_bindings:
    http_url    -> "http://127.0.0.1:18080/"
    metrics_url -> "http://127.0.0.1:19090/metrics"
~~~

Requirements:

- bounded key/value counts and byte lengths;
- binding keys use the existing runtime-binding key grammar;
- control characters and secret-bearing values remain rejected;
- `http_url` continues to parse/read exactly as before;
- when both legacy `http_url` and `static_bindings.http_url` are present they must agree or validation fails;
- bindings appear through the existing `RuntimeBindings` and runtime-topology evidence;
- schemas v1-v8 reject explicit use of the new field.

This change is generic and is needed later by target telemetry as well as security qualification.

## 5. Dependency runtime-binding references

Add an explicit typed way for a managed command to consume a binding from an already-ready dependency.

Do not use arbitrary string interpolation.

The contract should support, at minimum:

- replacing one complete argv element with one dependency binding value;
- setting one non-secret environment variable from one dependency binding value;
- source service name;
- source binding key;
- destination argv index or environment key.

Example intent:

~~~text
origin.bound_port
      |
      +--> subject-wrapper argv[3]
~~~

Requirements:

- the source must be a declared dependency of the consuming service;
- resolution occurs only after the source dependency is successfully started and ready;
- missing binding fails before the consumer process is spawned;
- a binding reference cannot read secrets or arbitrary environment state;
- no shell expansion is introduced;
- resolved values are bounded and control-character checked;
- destination argv indices are validated before startup;
- environment keys use the runner's normal environment-name validation;
- conflict/duplicate destinations fail closed;
- cancellation between dependency readiness and consumer spawn still runs ordinary cleanup.

The runner may need to defer final `ProcessSpec` materialization for command services until their turn in dependency order. Preserve the existing prepare-time executable/cwd validation where possible, but do not fabricate runtime values before their producers exist.

## 6. Evidence contract

Add a bounded runner-owned artifact describing runtime binding consumption without exposing secrets.

It must identify:

- consumer service;
- destination kind/key/index;
- source service;
- source binding key;
- resolved value digest;
- whether the value also appears in the public runtime-topology binding map.

The artifact may retain the clear value only when it is already a non-secret connection fact present in runtime topology. Prefer digest/reference form by default.

Comparison/evidence identity must distinguish runs whose selected workload driver or resolved connection topology differs.

## 7. CLI and library behavior

`qualify validate` and `qualify expand` must expose the frozen driver selection.

`qualify run` must pass the frozen selection to ordinary run machinery rather than adding a qualification-only driver path.

No new subject-specific CLI flag is allowed.

## 8. Compatibility

Required compatibility rules:

- existing plan schemas remain readable;
- existing profile v1 remains readable;
- current M001/M002 checked-in profiles retain byte/semantic behavior unless explicitly migrated;
- new writes use the new schema only when new fields are present;
- comparison readers continue to read existing bundles/receipts;
- no public Rust API removal is required.

If preserving old Rust constructors requires additive defaults/builders, use those rather than source-breaking replacement.

## 9. Tests

Add deterministic tests for:

1. profile v1 unchanged;
2. profile v2 driver round-trip and duplicate/invalid rejection;
3. requested workload driver reaches ordinary run selection;
4. requested unavailable driver fails without fallback;
5. static binding merge with legacy `http_url`;
6. static binding conflict;
7. dependency binding argv resolution;
8. dependency binding environment resolution;
9. missing source binding fails before consumer spawn;
10. non-dependency reference rejected;
11. out-of-range argv destination rejected;
12. teardown still occurs after binding-resolution failure;
13. runtime-topology/evidence identity records binding resolution;
14. cross-platform command tests use portable fixtures.

## 10. Verification

At minimum:

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

Run the normal four-lane hosted CI before closure.

## 11. Acceptance criteria

M003a closes only when:

1. qualification profile v2 can freeze a workload driver per scenario;
2. driver selection is part of immutable expansion/evidence truth;
3. `qualify run` uses the ordinary workload registry with that selection;
4. command/external services can publish general bounded static bindings;
5. managed commands can consume dependency runtime bindings through typed argv/env references;
6. missing/conflicting bindings fail before consumer network activity;
7. runtime binding consumption is auditable in evidence;
8. old schemas/profiles remain compatible;
9. no subject-specific launcher or security semantic is added;
10. four-lane hosted CI is green.

## 12. Stop conditions

Stop and re-plan if:

- implementing binding resolution requires arbitrary template/string evaluation;
- driver selection cannot be frozen before candidate execution;
- the change would make runtime topology mutable after measurement begins;
- command binding resolution requires exposing secret values;
- existing plan/profile readers would need an incompatible rewrite.

## 13. Handoff

This plan is dependency-ready now.

M003b may be implemented only after M003a closes because request-shape qualification must rely on frozen workload-driver identity and the generalized binding substrate.
