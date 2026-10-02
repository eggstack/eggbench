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
| Blocked requests never reach the origin | `m003d-3` (24,594 logged requests locally; 30,994 in hosted `36972886077`; no owner-blocked case present) | met |
| Second security-owner execution path | `m003d-6` real `eggsec-load` under strict scope plus a generated load-only execution policy | met |
| External transport corroboration | `m003d-4` `oha` body scenario against its own baseline | met |
| Owner telemetry contract consumed unmodified | `m003c-6`/`m003c-10` (mapping byte-identical, contract identity fields equal) | met |
| Subject telemetry in measured evidence | `m003c-13` (3 trials × 20 in-window samples, required fields observed); hosted `36897109527` likewise reports samples [20, 20, 20] | met |
| Fail closed on missing / renamed / TYPE drift | `m003c-16`, `m003c-17` | met |
| Optional absence never fabricated | `m003c-14` | met |
| No listener or process leak | `m003c-15`, teardown stage | met |
| **Required subject series carry live owner values** | `m003c-13b` — owner publishes the v2 inventory at zero | **blocked upstream** |
| Green hosted four-lane CI + live M003 job | normal CI `36972886787` green on all four lanes; live `36972886077` `live-m003-linux` pass=28 stopped=1 notexec=1, the only stop `m003c-13b` | met |

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
3. **Low, testbench (closed).** The same-source perf pair is sensitive to host
   speed and load: on a heavily contended shared host it can return `Inconclusive`
   instead of `Pass`, and on the two-core hosted runner the M002a live pair
   intermittently returned `Invalid` (insufficient evidence, which is never
   softened to `Inconclusive`) or `Fail` — observed failing in three of the six
   hosted live runs since the M003 head was pushed, and passing in the other
   three, on commits that do not touch its harness. The harness accepts 0 or 7 for
   the contended-host case and still requires the correctness family to pass;
   hosted repeatability is the authority, and the same class of failure reproduces
   on pristine `1b79f62` under load. This is recorded rather than repaired here:
   the M002a live harness belongs to a closed milestone, and relaxing its gate to
   tolerate a same-source `Fail` would weaken a terminal condition.

4. **Medium, routine-fixture portability (closed by hosted evidence).** Two
   defects made the M003 routine suite host-dependent, and only the macOS lane
   exposed them. The measured window of a finite HTTP-corpus campaign is bounded
   by workload volume, so a fast host served the telemetry scenario in ~82ms —
   shorter than the poll cadence — which collapsed the in-window sample count and
   moved the stand-in's fault injections outside the measured window entirely; the
   campaigns are now sized for the evidence they must carry. Separately, both
   stand-ins used the stdlib `ThreadingHTTPServer` backlog of 5, so a host fast
   enough to reach the declared fresh-per-request connection rate had connections
   refused, and the loss surfaced as subject transport errors against an absolute
   zero-error gate rather than as a fixture limitation.

5. **Medium, packaging (closed by hosted evidence).** `qualification/synvoid/v2/
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
   revision. Both now hold: CI `36972886787` is green on all four lanes, and the
   live job's only stop is `m003c-13b`. The job still exits non-zero, because
   the harness reports a stop as exit 10 by design.

Nothing further is required on the Eggbench side.