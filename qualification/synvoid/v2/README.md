# SynVoid M003 qualification workspace (subject telemetry + load profiles)

This workspace is the routine (hosted-CI, synthetic-stand-in) counterpart of
`scripts/qualification/synvoid-m003/run-live-qualification.sh`, which runs the
same qualification stages against the real pinned SynVoid and Eggsec binaries.
The two scopes must agree: a change that passes here and fails live is a real
defect in this workspace, and the live harness is the terminal evidence.

## Ownership split

SynVoid owns the telemetry contract: `telemetry/telemetry-mapping.json` and
`telemetry/telemetry-contract.json` are byte-for-byte copies of the owner
export produced by `cargo xtask eggbench-qualification export` at
`dbowm91/synvoid@739e7ba6f02c5e3f83fe9ff5321b09213182b193`. The mapping's raw
SHA-256 is `622f6a13c4353cc7465cce39a57ed86fa0db2fe4114258e6f06226c1748d2d99`;
its Eggbench content-tree identity (what plans pin) is
`dd7d58dd204a691ab0b83b82a83d49f3438b67e3ea6d0e14c7c1fba3832584b0`. The
`v1` contract is withdrawn upstream; Eggbench never reconstructs the mapping
and never invents subject series.

Eggbench owns the plans, profiles, corpora, controlled origin, and the
synthetic stand-in used for routine tests. The corpora here are
Eggbench-authored synthetic fixtures and say so in their `owner` field; they
are not the SynVoid-owned export. The live harness translates the owner
corpus instead of reusing these files.

## Layout

```text
qualification/synvoid/v2/
  telemetry/telemetry-mapping.json   owner mapping, byte-for-byte
  telemetry/telemetry-contract.json  owner contract, byte-for-byte
  corpus.json                        synthetic load corpus (schema 1, 8 cases)
  correctness-corpus.json            synthetic correctness corpus, separate
  routes.json                        controlled-origin route table
  target-config.json                 synthetic target-config identity
  smoke.profile.json                 M003c smoke: correctness, load, telemetry
  perf.profile.json                  M003d performance: baselines + gates
  scenarios/*.json                   plan schema 9 (corpus checks) / 10
  stubs/fake_synvoid_m003.py         routine-only synthetic subject
  stubs/controlled_origin_m003.py    routine-only controlled origin
  baselines/                         empty: no baseline is auto-discovered
```

## Telemetry endpoint shape

Plan schema 10 (`http_corpus_checks` plans) rejects `static_bindings`, and the
subject's metrics listener is assigned at runtime, so telemetry is declared as
an external named service:

```json
{
  "name": "subject-metrics",
  "kind": { "kind": "named", "service_type": "prometheus-http" },
  "lifecycle": "external",
  "config": {
    "target_service": "subject-metrics",
    "binding_key": "http_url",
    "mapping_ref": "telemetry/telemetry-mapping.json",
    "mapping_sha256": "<content identity>",
    "poll_interval_ms": "200"
  }
}
```

`poll_interval_ms` is a string because service config values are strings. The
`subject-metrics` endpoint must be the *same process* the plan drives; a
per-scenario managed subject plus a harness-owned metrics port would attribute
another process's series to the subject under test.

The telemetry preflight runs after managed-service readiness
(`TelemetryPreflightTiming::AfterReadiness`) and retries on the declared
cadence until the required fields are observable, because an owner publishes
its worker series on its own refresh cadence (the pinned contract declares
`source_refresh_cadence_secs = 5`). A required field that never appears, drifts
type, resets mid-trial, or disappears mid-trial fails the run closed; an
optional field that is absent is warned and recorded as
`prometheus_missing_samples`, never zero.

## Verdict roles

`smoke.profile.json` proves the mechanics; `perf.profile.json` is the frozen
M003d performance contract. Only `body-gated-c8` carries policy gates
(statistical-relative throughput and p95 latency against
`baselines/body-gated-c8.eggb`). Every other scenario's performance
observations are diagnostic: faster execution never overrides a security
correctness failure.

## Running (routine, synthetic)

```sh
cd qualification/synvoid/v2
eggbench qualify validate smoke.profile.json --json
eggbench qualify run smoke.profile.json --output /tmp/synvoid-m003-smoke
eggbench qualify inspect /tmp/synvoid-m003-smoke/qualification-receipt.json --json
```

External-oracle and Eggsec scenarios are run outside `qualify` (deviation D4):

```sh
eggbench run scenarios/body-oha-c8.json /tmp/oha.eggb --workload-driver oha --json
eggbench run scenarios/eggsec-benign-body-c8.json /tmp/eggsec.eggb \
  --workload-driver eggsec-load --json
```

`eggsec-load` authorizes `load-test` through a generated execution-policy
manifest that permits load testing only; every other elevated-risk capability
stays denied, explicit scope stays mandatory, and both the scope identity and
the policy identity are recorded in `eggsec-load-method.json`.

## Repinning identities

Plan `corpus_sha256` and telemetry `mapping_sha256` values are Eggbench
content-tree identities, not raw file digests. After any corpus or mapping
edit:

```sh
python3 scripts/qualification/synvoid-m003/resolve-content-digests.py \
  --workspace qualification/synvoid/v2 --repin
```
