# SynVoid qualification profile v1 (M002a, synthetic routine scope)

Status: routine scope implemented and locally verified. Live SynVoid
qualification remains a named condition (upstream SynVoid asset contract
open; see closure `plans/closure/security-qualification/002a-status.md`).

## Ownership

SynVoid owns qualification config materialization, WAF fixture selection,
Detect/Pass semantics, the observable mapping policy, and
source/exclusion provenance. Eggbench owns profile/scenario plans, managed
lifecycle, the controlled origin, corpus/config hashing, HTTP correctness
execution, and the qualification receipt. No SynVoid Rust crate is
imported. The corpus in this directory is an Eggbench-authored synthetic
fixture (`owner` says so); it must not be mistaken for the SynVoid-owned
export.

## Layout

```text
qualification/synvoid/v1/
  profile.json                 M001 profile schema v1 (correctness scenario)
  corpus.json                  synthetic WAF corpus (schema 1, 5 cases)
  target-config.json           synthetic target-config identity
  upstream-manifest.md         expected SynVoid import manifest + checks
  materialized/provenance.json synthetic manifest fixture for negative tests
  scenarios/waf-correctness.json  plan schema 8 (origin + fake subject)
  stubs/fake_synvoid.py        routine-test-only WAF stand-in (fixed port)
```

## Source-fixture mapping (planning audit: SynVoid 1.1.0)

| Synthetic case | Source fixture / expectation | Adaptation |
|---|---|---|
| `benign-search-coffee` | `benign_query_strings` / pass | verbatim path+query |
| `benign-search-encoded` | `benign_url_encoding` / pass | repathed `/api/data` -> `/search` (single-route origin) |
| `benign-search-unicode` | `benign_percent_encoded_unicode` / pass | relocated to query (single-route origin) |
| `detect-path-traversal` | `path_traversal_literal` / detect | verbatim; client dot-segment normalization is a live risk |
| `detect-xss-path` | `xss_percent_encoded` / detect | verbatim |

All other 22 source fixtures are excluded with reasons in
`materialized/provenance.json` (M001-forbidden headers/bodies/URLs or
deferred semantics). The upstream export replaces this fixture; Eggbench
does not translate Detect/Pass itself.

## Running (routine, synthetic)

All commands run with this directory as the working directory (plan
`corpus_ref`, stub `argv`, and service working directories resolve
against the process working directory):

```sh
cd qualification/synvoid/v1
eggbench qualify validate profile.json --json
eggbench qualify expand profile.json --json
eggbench qualify run profile.json --output /tmp/synvoid-qual
eggbench qualify inspect /tmp/synvoid-qual/qualification-receipt.json --json
```

The checked-in scenario uses fixed loopback port `18080`. Tests and the
live harness copy this workspace and patch free ports before running.

## Recorded deviations from the M002a plan

- **D1 (static-config port discovery).** The runner has no binding
  interpolation: a managed command subject cannot discover the named
  `eggserve-origin` adapter's ephemeral port. The checked-in scenario
  therefore runs the real adapter as the controlled origin (lifecycle +
  binding evidence) while the synthetic subject emulates the adapter's
  deterministic contract byte-for-byte (200 + 1024 x `0x42` on benign
  routes). The live harness wires the real SynVoid to a fixed-port
  deterministic origin. Generic binding interpolation is a Security
  Qualification M003 candidate.
- **D2 (upstream export).** No SynVoid-owned export exists yet; the
  corpus/provenance here are Eggbench-authored synthetic fixtures with an
  explicit non-SynVoid owner. Live Pass/Detect proof against the real
  export is the remaining closure condition.
- **D3 (block status).** Detect expectations assume HTTP 403 (SynVoid
  `WafDecision::Block(403)`). The site-level `action = "block"` mapping
  was not conclusively located in SynVoid source; live verification must
  confirm the wire status before trusting detector-action coverage.

## M002b suite (v1)

Two checked-in profile families share this workspace:

- `smoke.profile.json` (`synvoid-smoke-v1`): correctness + small/large
  native c1 proxy workloads + direct-origin controls, absolute
  error-rate gates only. Bounded; suitable for hosted CI smoke.
- `perf.profile.json` (`synvoid-perf-v1`): correctness (absolute) +
  native c1/c8/c32 small/large proxy workloads + controls, each
  performance scenario bound to an explicit baseline bundle with frozen
  v1 guardrails (throughput regression 15%, p95 regression 20%,
  error_rate absolute 0). Baselines are materialized locally first (see
  `baselines/README.md`); the profile refuses to expand without them.

Scenario inventory: `scenarios/waf-correctness.json`,
`perf-{small,large}-c{1,8,32}.json`, `smoke-{small,large}.json`,
`control-{small,large}.json`, plus `oracle-{oha,h2load}-c8.json` for the
documented external-oracle `run`/`compare` procedure below.

Sample policy (recorded §8 evidence): 7 measured trials (1 warmup),
200/800/2000 requests per trial at c1/c8/c32, `min_trials` 5. An earlier
5-trial/400-request policy produced a same-source Fail on loopback
scheduling noise; the bumped policy yields Pass/Inconclusive with zero
Fail across repeated same-source pairs on the reference host. Thresholds
were NOT widened. The live host must repeat same-build repeatability
qualification; if it violates, M003 revises the policy with evidence.

## Further deviations (M002b)

- **D4 (per-scenario workload drivers).** `qualify run` pins no
  per-scenario workload driver (it resolves the default,
  `eggfetch-http`), so the oracle scenarios cannot execute inside a
  mixed qualify profile. They run through the documented procedure:
  `eggbench run --workload-driver oha|h2load <plan> <bundle>` followed
  by `eggbench compare` against the same-driver baseline bundle, with
  receipts retained alongside the suite. Per-scenario driver selection
  is a Security Qualification M003 candidate.
- **D5 (Gregg telemetry).** Checked-in profiles declare no Gregg
  collector: no qualified daemon can be provisioned for routine runs,
  and an unreachable collector must not invalidate the suite. The live
  harness probes for a daemon and records host telemetry where
  available; metrics stay labeled host/testbed, never SynVoid-process.
- **D6 (deferred load shapes).** Mixed malicious/benign traffic under
  load, request-body attack campaigns, explicit connection churn, and
  SynVoid Prometheus/event-loop/queue ingestion are NOT approximated;
  they are M003 candidates (see `docs/synvoid-qualification.md`).
