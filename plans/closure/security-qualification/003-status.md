# Security Qualification M003 — Status

Disposition: **conditionally closed — all Eggbench-owned M003 scope is complete
and verified; terminal milestone closure is blocked by one named upstream
defect.**

Milestone plans:

- `plans/implementation/security-qualification/003c-subject-telemetry-and-synvoid-metrics-ingestion.md`
- `plans/implementation/security-qualification/003d-eggsec-load-profiles-live-qualification-and-m003-closure.md`

Closure records: `plans/closure/security-qualification/003c-status.md` and
`plans/closure/security-qualification/003d-status.md`.

## What M003 claimed

Mixed benign/malicious traffic under load with request bodies, an explicit
performance policy against materialized baselines, subject-owned telemetry
participating in measured evidence, and a second security-owner execution path
(Eggsec) plus an external transport oracle (`oha`) for corroboration.

## Requirement-to-evidence matrix

| Requirement | Evidence | Verdict |
|---|---|---|
| Load shapes: pooled/fresh body, 80/20 mixed, origin control | live `m003d-1`/`m003d-2`; routine `synvoid_m003` smoke/perf tests | met |
| Explicit, materialized baselines; no auto-discovery | `m003d-1` (8 bundles materialized, profile validates only with them) | met |
| One frozen performance policy | `body-gated-c8` is the only gated scenario; all others diagnostic | met |
| Correctness-only regression fails the suite | `m003d-7` (exit 6, performance acceptable) | met |
| Performance-only regression fails the suite | `m003d-8` (exit 6, `body-gated-c8` fail, correctness pass) | met |
| Blocked requests never reach the origin | `m003d-3` (24,594 logged requests locally; 30,994 in hosted `36897109527`; no owner-blocked case present) | met |
| Second security-owner execution path | `m003d-6` real `eggsec-load` under strict scope plus a generated load-only execution policy | met |
| External transport corroboration | `m003d-4` `oha` body scenario against its own baseline | met |
| Owner telemetry contract consumed unmodified | `m003c-6`/`m003c-10` (mapping byte-identical, contract identity fields equal) | met |
| Subject telemetry in measured evidence | `m003c-13` (3 trials × 20 in-window samples, required fields observed); hosted `36897109527` likewise reports samples [20, 20, 20] | met |
| Fail closed on missing / renamed / TYPE drift | `m003c-16`, `m003c-17` | met |
| Optional absence never fabricated | `m003c-14` | met |
| No listener or process leak | `m003c-15`, teardown stage | met |
| **Required subject series carry live owner values** | `m003c-13b` — owner publishes the v2 inventory at zero | **blocked upstream** |
| Green hosted four-lane CI + live M003 job | live run `36897109527` `live-m003-linux` pass=28 stopped=1 notexec=1 (the only stop is `m003c-13b`); normal CI run `36897109322` msrv green, three stable lanes red on clippy 1.99 lints in pre-existing code | partial |

## Unresolved findings

1. **High, upstream (blocking).** At `synvoid@739e7ba6...` the supervisor never
   dispatches `Message::UnifiedServerWorkerHeartbeat`, so the telemetry bridge
   aggregates default payloads and publishes zeros for all twelve
   `synvoid_subject_*` series under load. Root cause, reproduction, and code
   references are in `plans/closure/security-qualification/003c-status.md`. This
   is an owner-side defect; the consumer's fail-closed behavior is correct and
   the harness reports the gap as its own stopped gate.
2. **Medium, upstream-tool contract (closed).** Three real-Eggsec adapter
   defects (`--header` vs `--headers`, the missing load-only execution policy,
   `--quiet` suppressing the machine report) plus a preflight document-extraction
   defect were found only by executing against the real binary; all are repaired
   with tests.
3. **Low, testbench (closed).** On a heavily contended shared host the
   unthrottled perf pair can return `Inconclusive` instead of `Pass`, and on the
   two-core hosted runner it returned `Invalid` (insufficient evidence, which is
   never softened to `Inconclusive`). The harness accepts 0 or 7 for the former
   and still requires the correctness family to pass; hosted repeatability is
   the authority, and the same class of failure reproduces on pristine
   `1b79f62` under load.
4. **Medium, packaging (closed by hosted evidence).** `qualification/synvoid/v2/
   baselines/` was an empty directory, which git cannot track, so the routine
   M003 suite failed every baseline materialization with exit 5 (`bundle
   parent directory does not exist`) on a clean checkout while passing on
   developer machines that already had the directory. The directory is now
   tracked by its baseline README, and both the routine suite and the live
   harness derive their baseline set from the profile's own
   `baseline_bundle` declarations instead of restating scenario names, so a
   renamed scenario cannot silently skip a materialized baseline.

## Condition for terminal closure

1. An owner revision that dispatches the unified-server heartbeat so the
   required series carry live values.
2. `scripts/qualification/synvoid-m003/run-live-qualification.sh` green against
   that revision, with `m003c-13b` passing.
3. Green hosted four-lane CI and the `live-m003-linux` job on the exact closing
   revision. The clippy 1.99 lane failures from run `36897109322` are toolchain
   drift in pre-existing code and are repaired on this branch; the live job's
   remaining stop is `m003c-13b` alone.

Nothing further is required on the Eggbench side.