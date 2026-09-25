# C002 Eggprobe Adapter Contract Correction — Closure Status

Status: closed (implementation `98f16e6`; local live qualification 20/20;
hosted four-lane + live-tool runs green on the corrective HEAD)

Plan:

- `plans/implementation/post-m003-live-tool-qualification-corrective/002-eggprobe-adapter-contract-correction.md`

Corrective authority:

- `plans/subsystems/post-m003-live-tool-qualification-corrective-addendum.md`
- C001 stopped evidence:
  `plans/closure/post-m003-live-tool-qualification-corrective/001-status.md`

Historical M003 remains closed at:

- `plans/closure/eggstack-integration/003a-status.md`
- `plans/closure/eggstack-integration/003b-status.md`

Primary class: corrective compatibility fix + deferred live qualification.

## 1. Disposition

**CLOSED.** Every C001 criterion 1–23 reads PASS/verified (section 4 matrix),
with criteria 6–17 now proven live against the corrected adapter. The adapter
diff is confined to the M003b seam plus two minimal live-proven corrections
recorded as material deviations D1/D2 (section 5). Real-shape regression
tests exist. The hosted live-tool job and four-lane CI are green on the
corrective HEAD (section 7). No M003a adapter change. No
metric/lifecycle/comparison semantic change.

## 2. Implementation under test

- Eggbench corrective HEAD: `98f16e6`
  (`feat(live-tools): implement C002 eggprobe adapter correction and
  deferred live qualification`)
- Corrective/harness SHA: same HEAD (harness +
  `scripts/qualification/m003-live-tools/origin_raw.py` +
  `.github/workflows/live-tools.yml` in the same commit).
- Eggbench release binary built with `--locked --all-features`
  (`eggbench 0.1.0`).

Production diff summary (6 files, +626/−65):

- `crates/eggbench-drivers/src/external/eggprobe.rs` — the C002 adapter
  correction (section 5, items P1–P6 resolved) + real-shape regression tests.
- `crates/eggbench-drivers/src/eggstack/origin.rs` — deviation D1: runtime
  `Date` suppression for the controlled origin (no timestamp header).
- `crates/eggbench-core/src/comparison.rs` — deviation D2: compare readers
  now match the production evidence writers (semantic-replay identity fields;
  diagnostics index selected by path).
- `scripts/qualification/m003-live-tools/` — harness extended to the full
  C002 proof suite; `origin_raw.py` header set aligned with the controlled
  origin (Content-Length only).
- `.github/workflows/live-tools.yml` — new Linux-only hosted live-tool job.

## 3. Provenance record

Fresh local qualification run kept at `/tmp/m003-live-qual.tU4uHX`
(harness exit 0, `RESULT: PASS`, 20/20). Sibling checkouts were clean
clones; builds used `cargo build --locked --release -p <cli>` with no
global install (binaries in isolated qualification directories under a
controlled PATH; `command -v` + `sha256sum` + `--version` verified before
every Eggbench execution).

| Input | Value |
|---|---|
| EggReplay source SHA | `d39f4b794620a2d0647688a914e0e7a6be42e184` |
| EggReplay `Cargo.lock` SHA-256 | `5e4c85311c5acb3008d6650ed06e0c4b29da95ec04af376e2488a4d0e0742fb3` |
| EggReplay binary SHA-256 | `36f61506dac15aa24676f104123e10b79fe0a6ce420cc590e3088454b3caa06b` |
| EggReplay binary size | 8457976 bytes (bit-identical to the C001 value) |
| EggReplay `--version` | `eggreplay 0.1.0`, exit 0 |
| Eggprobe positive tag / SHA | `v0.1.1` / `53ea53d14560c150d0ebc10de83eca10de37202d` |
| Eggprobe positive `Cargo.lock` SHA-256 | `029870c4165413f94ec2861943b23e90624f9f961cec5175a526ca55caeadc2f` |
| Eggprobe positive binary SHA-256 | `c70b842493501ea9a1a8be45427945fd18fed43a89161f636ac03075d1170688` |
| Eggprobe positive binary size | 13011272 bytes |
| Eggprobe positive `--version` | `eggprobe 0.1.1`, exit 0 |
| Eggprobe negative SHA | `0ce9597aa2acad9a61c45a70c3ffaf56333cf3d5` |
| Eggprobe negative `Cargo.lock` SHA-256 | `b7dfb5e3e603bfce46505ebf5337f2a50101eb31c5bce5919f21eca2f3aa7338` |
| Eggprobe negative binary SHA-256 | `096e555453133a6766a2eaba54fb67c5846cbe6c4e2e49da6be1817e11639e77` |
| Eggprobe negative binary size | 16141248 bytes |
| Eggprobe negative `--version` | `eggprobe 0.1.1`, exit 0 |
| Sibling build toolchain | `rustc 1.98.1 (48a229cea 2026-09-01)`, `cargo 1.98.1` |

Note: the Eggprobe binary SHAs differ from the C001 record while source
SHAs, lockfile hashes, versions, and the EggReplay binary SHA are
identical. The difference is build-path embedding (different absolute
checkout paths); source SHA + lockfile hash + `--version` remain the
identity, and the per-run binary SHA is recorded above. `0.1.0`/`0.1.1`
alone were never treated as identity.

## 4. Requirement-to-evidence matrix (C001 criteria 1–23)

| Plan criterion | Verdict | Evidence |
|---|---|---|
| 1. exact real binaries built from pinned revisions | PASS | Section 3 table; harness `criterion-1/2` |
| 2. binary SHA/provenance recorded | PASS | Section 3 table |
| 3. real EggReplay validates generated fixture | PASS | `validate --output json`: envelope 1, `flow_count = 1`, session schema 2, exit 0; fixture digest `50cbfdd91c10afc456969015f43d25541c6c5b7f4f6ff312d7b0f4427af1663e` via the M003a identity code |
| 4. matching replay yields zero findings | PASS | Standalone `replay --output json` vs identical origin: exit 0, envelope 1, all reports schema 2, `finding_count = 0` |
| 5. mismatch replay yields nonzero findings | PASS | Same fixture vs status-201 origin: `success: true`, exit 0, `finding_count = 1` (`response.status` 200 vs 201) |
| 6. real Eggprobe v0.1.1 accepts the exact schema-0.3 stdin plan | PASS | Corrected adapter plan accepted: exit 0, schema `0.3`, `tool.name == eggprobe`, report `status == ok`, direct route; standalone negative exits 1 with a parseable `failed` report (never a process failure) |
| 7. real schema-0.4 Eggprobe rejected before startup | PASS (isolated) | `eggbench run` with the negative binary fails before startup with `diagnostic_contract_unsupported` (handshake exit 1), no bundle, no service, no workload; direct binary checks prove isolation: the 0.4 binary emits schema `0.4` for 0.4 plans and rejects qualified 0.3 plans, while the positive binary accepts them |
| 8. combined positive run finalizes successfully | PASS | `eggbench run` exit 0, status `completed`, 2 measured trials, `eggbench inspect` verifies the bundle; run `b1c4887c-d7e2-47df-9c56-278b221857c8` |
| 9. two measured trials map to two replay processes | PASS | Exactly two measured trial records; each trial carries its own replay invocation (separate stdout raw + command metadata, real binary SHA `36f61506…`, exit 0); per-flow reports nested (`report_count 1`, `baseline_flow_ids`), never promoted to trials; one-shot processes per trial |
| 10. combined mismatch run finalizes successfully | PASS | Status-201 plan variant with the same fixture: run `6804d17b-1a33-42e5-934b-0104f638580f`, exit 0, status `completed`, 2 measured trials |
| 11. mismatch stays a completed workload observation | PASS | Per-trial `finding_count = 1` (`response.status`), replay `success: true`; execution is `completed`, never `Failed` |
| 12. absolute gate Fail/exit 6 | PASS | `eggbench compare --absolute-only mismatch.eggb`: aggregate verdict `Fail` (trials [1,2], estimate 1.0 vs gate 0.0), exit code 6; the positive bundle gates `Pass`/exit 0, proving discrimination; verdict is `Fail`, never `Invalid` |
| 13. pre/post diagnostics in lifecycle slots | PASS | Phases: `startup_readiness → diagnostics_pre → measured_trial ×2 (+cooldown) → drain → diagnostics_post → teardown → finalization`; pre/post dispositions `positive`, report `ok`, real SHA/version provenance in `diagnostics.json`; DNS deterministically withheld for the literal-IP origin with a recorded warning |
| 14. Eggprobe timings absent from TrialMetrics | PASS | Trial `metrics.json` carries only `semantic_findings`; probe `timing` fields exist only in diagnostic artifacts (microsecond `total` values) |
| 15. required pre negative prevents workload + tears down | PASS | Required TLS pre without an `https_url` binding: exit 4, status `Invalid`, 0 measured trials, no `measured_trial` phase, `teardown completed`; diagnostic provenance `executor_diagnosticfailed` staged. The failure originates in Eggbench-side lowering (fail-closed before binary invocation — the only deterministic through-Eggbench required negative, since managed bindings are healthy by construction); binary-side negative vocabulary is proven by the standalone exit-1-with-report check plus the exit-1→Negative unit mapping |
| 16. optional post negative does not invalidate workload | PASS | Optional TLS post without binding: exit 0, status `completed`, 2 trials; post artifact records `tls unavailable` (withheld warning + unavailable probe status) from a real empty-plan `ok` report; `teardown completed`; pre-existing workload outcome untouched |
| 17. cancellation leaves no sibling child | PASS | 25-trial run + SIGINT mid-run: exit 4, status `cancelled`, 7 measured trials, `drain → teardown completed → finalization` per partial-evidence rules; post-run scan finds no `eggreplay`/`eggprobe` processes; managed origin torn down |
| 18. no EggReplay/Eggprobe production Rust dependency | PASS | `cargo tree --locked` contains zero `eggreplay`/`eggprobe` edges; seam stays `eggprobe run -` over stdin/stdout |
| 19. default/all-feature Clippy/tests green | PASS | Section 6 |
| 20. Rust 1.89 green | PASS | Section 6 |
| 21. hosted live-tool qualification passes | PASS | Section 7: `live-tools.yml` run green on the corrective HEAD |
| 22. normal four-lane hosted CI on corrective HEAD | PASS | Section 7: `ci.yml` run green on the corrective HEAD |
| 23. closure/registry/roadmap reconciliation | PASS | This record + registry/addendum/roadmap updates + C002 plan marked closed, committed together |

## 5. Adapter diff and recorded material deviations

C001 proved defects P1–P6 (closure section 5). The C002 correction in
`crates/eggbench-drivers/src/external/eggprobe.rs`:

- P1 — probes are now typed `ProbeSpec` objects: `{"kind":"dns"}`,
  `{"kind":"tcp","port":N}`, `{"kind":"tls","port":N}`,
  `{"kind":"http","url":"..."}` (TCP gets the bound port, HTTP the bound
  URL, TLS the bound TLS port).
- P2 — `execution` is now `{"deadline":<micros>,"repetitions":1,"retries":0}`
  converted from the diagnostic timeout.
- P3 — `target` is now `{host, port}` only; `tls`/`http_url` no longer cross
  the seam (kept internally for lowering). When an HTTP probe is present
  the authority is read back from the bound HTTP URL so the sibling's
  host/port cross-check passes trivially.
- P4 — handshake plans (empty-probe, then bounded localhost-DNS retry gated
  on exit 2) use the same corrected shape.
- P5 — report entries are read from `kind`.
- P6 — `ok`/`failed`/`unsupported`/`cancelled` vocabulary; handshake and
  exit-0 positive disposition require `ok`; exit 1 + parseable report stays
  negative evidence; 2/3/130 semantics kept; exit-0-with-non-`ok` fails
  closed (the real binary only exits 0 with `ok`).

Regression tests added (mock-dialect tests updated, never silently
deleted): byte-faithful typed-plan shape test, legacy `family`/`pass`
dialect rejection test, real-shape negative report + exit mapping test
(including exit-0-with-`failed` fails closed), TLS-port tests
(TLS-only and TLS+HTTP authority handling), all existing bounds tests
kept green.

Material deviations from the C002 handoff (recorded, not hidden):

- D1 — Controlled-origin `Date` suppression
  (`crates/eggbench-drivers/src/eggstack/origin.rs`): live combined runs
  proved the managed EggServe origin emits a volatile `Date` header while
  the qualification fixture origin does not, so a zero-findings positive
  run was unachievable (`response.headers.date` finding plus
  `connection`/`content-type` asymmetry). The adapter now builds its
  `RuntimeConfig` with `DatePolicy::Suppress`, and the
  qualification-only `origin_raw.py` emits exactly the controlled origin's
  header set (Content-Length only). Rationale: this is testbed
  determinism configuration, not a measurement semantic — status, body,
  route discipline (501 off-route), loopback binding, lifecycle, metric
  mapping, and comparison gates are unchanged, and it makes the code match
  its own documented byte-identical invariant. No M003a adapter change.
- D2 — Compare-reader/writer drift (`crates/eggbench-core/src/comparison.rs`):
  live gating proved `compare` could not load any production
  semantic-replay bundle (`StoredSemanticReplayEvidence` lacked the
  writer's `adapter_version`/`fixture_relative`/`flow_count` under
  `deny_unknown_fields`) and rejected every bundle with diagnostic
  executions (the run-level index was selected by the shared `diagnostics`
  role label, which per-diagnostic artifacts also carry). The readers now
  match the writers: the three identity fields are accepted and validated
  with the writer's bounds, and the index is selected by path
  (`diagnostics.json`) with its role verified. No verdict, threshold, or
  comparability semantic changed — previously-unloadable production
  bundles now load, and the absolute gate behaves as M003 specified
  (criterion 12).

No sibling defect was found; both siblings behaved per their own sources.
No schema-0.4 adoption; no Rust sibling dependencies; no routed
diagnostics; no remote/scheduler machinery.

## 6. Local qualification matrix (corrective HEAD `98f16e6`)

All green:

- `cargo fmt --all -- --check` — clean
- `cargo check --workspace --all-targets --locked` — clean
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — no issues
- `cargo test --workspace --all-targets --locked` — all suites ok (sentinel set)
- `cargo check/clippy --workspace --all-targets --all-features --locked` — clean
- `EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-targets --all-features --locked` — 458 passed, 0 failed
- `cargo +1.89.0 check --workspace --all-targets --locked` and `--all-features` — clean
- `cargo +1.89.0 test -p eggbench-core/runner/drivers --all-features --locked` — ok
- `cargo tree --locked` — zero `eggreplay`/`eggprobe` production edges
- `git diff --check` — clean
- Extended harness `scripts/qualification/m003-live-tools/run-live-qualification.sh` — `RESULT: PASS`, 20/20 (exit 0)

## 7. Hosted qualification

Both runs executed on the corrective HEAD `98f16e6`:

- Live-tool job (`.github/workflows/live-tools.yml`, Linux-only, pinned
  revisions, `--locked --release` builds, harness run, bounded
  provenance/evidence log upload, fail on mismatch):
  run `36177440371` — **success** (`live-tools-linux: success`).
- Normal four-lane CI (`.github/workflows/ci.yml`): run `36177440220` —
  **success** (`linux-stable`, `linux-msrv`, `macos-stable`,
  `windows-stable` all success).

## 8. Why previous verification missed the C002-live findings

Same root cause as C001 section 7, one layer deeper: hand-written
fixture/report JSON plus mock-driven comparison tests never crossed the
real process seam (C001: eggprobe dialect) or the real production-evidence
seam (C002-live: origin `Date` volatility; production `semantic-replay.json`
shape; bundles with real diagnostic executions). The live harness is the
first check that executes pinned sibling binaries end-to-end and gates a
production-written bundle. The committed harness plus the hosted
live-tool job are the regression guards against recurrence.

## 9. Unresolved findings

None blocking. Two notes:

1. (Informational) Eggprobe release binaries are not bit-reproducible
   across absolute build paths (source SHA + lockfile hash identical,
   binary SHA differs); provenance records the per-run binary SHA, and
   source/lock/version remain the identity.
2. (Informational) The required-pre live negative exercises Eggbench-side
   lowering fail-closure (criterion 15 table); binary-side negative
   vocabulary is covered standalone + by unit test. A through-Eggbench
   binary-negative would require an unhealthy managed binding, which the
   controlled origin never provides by construction.

## 10. Final disposition

C002 is **closed**. The live external-tool contract is now additively
qualified against the real pinned binaries; M003's historical closures
stand unchanged. M004 implementation is unblocked subject to its own
implementation plan.
