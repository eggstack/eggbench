# Post-M003 Live-Tool Corrective C002 — M003b Eggprobe Adapter Contract Correction

Status: closed (implementation `98f16e6`; closure `plans/closure/post-m003-live-tool-qualification-corrective/002-status.md`; hosted live-tool run `36177440371` green; four-lane run `36177440220` green)

Repository baseline: `5aa12cd57d8f47e09301391768827f055a0a9bbd` plus the
committed C001 stopped-evidence HEAD (harness + closure).

Corrective authority:

- `plans/subsystems/post-m003-live-tool-qualification-corrective-addendum.md`
- C001 stopped evidence:
  `plans/closure/post-m003-live-tool-qualification-corrective/001-status.md`
- C001 handoff (still controlling for EggReplay and method):
  `plans/implementation/post-m003-live-tool-qualification-corrective/001-eggreplay-eggprobe-live-contract-qualification.md`

Historical M003 remains closed at:

- `plans/closure/eggstack-integration/003a-status.md`
- `plans/closure/eggstack-integration/003b-status.md`

Primary class: corrective compatibility fix + deferred live qualification.

Expected production semantic change: narrow M003b adapter correction only
(plan shape + report vocabulary, same qualified schema-0.3 contract). No
M003a change. No metric/lifecycle/comparison semantic change.

## 1. Objective

Fix the exact adapter defect C001 proved (section 5 of the C001 closure),
then complete the live qualification C001 could not reach: standalone
contracts, schema-0.4 rejection isolation, combined positive/mismatch runs,
trial-unit/timing proofs, required/optional diagnostics, cancellation smoke,
hosted live-tool job, and four-lane CI — and close the live-tool corrective.

## 2. Current implementation evidence

- C001 proved the EggReplay (M003a) side matches the real binary exactly; no
  M003a production change is expected or wanted.
- C001 proved the M003b adapter emits a plan/report dialect the real
  `eggprobe v0.1.1` (`53ea53d`) never spoke. Failing code is confined to
  `crates/eggbench-drivers/src/external/eggprobe.rs`:
  `build_probe_plan`, `handshake_eggprobe` (+ `try_handshake_plan`),
  `parse_probe_report` (+ `ProbeReportWire`/`ProbeEntryWire`),
  `classify_probe_exit`, and the handshake `status == "pass"` gate.
- The committed harness
  (`scripts/qualification/m003-live-tools/run-live-qualification.sh` +
  `origin_raw.py`) automates pinned builds, provenance, fixture generation,
  standalone checks, and combined runs, and already encodes the C001 stop
  assertions. C002 extends it; it does not replace it.
- Local default/all-feature/MSRV matrix is green on the C001 stopped HEAD.

## 3. Invariants (must survive the fix)

- Sibling ownership: no Eggprobe Rust crate becomes a production dependency;
  the seam stays `eggprobe run -` over stdin/stdout.
- Qualified contract stays schema `0.3` against `v0.1.1`; schema `0.4` stays
  rejected. Same package version `0.1.1` never implies compatibility.
- Diagnostics stay outside measured intervals; probe timings never enter
  `TrialMetrics`, never satisfy a `MetricRequest`, never gate acceptance.
- Semantic findings stay successful workload observations, never
  `WorkloadFailed`; the absolute gate keeps comparison-fail exit 6.
- Required/optional diagnostic policy, loopback-only binding, credential-free
  plans, bounded evidence, and fail-closed handshake behavior are unchanged.

## 4. Non-goals

- No Eggprobe schema-0.4 adoption, no routed diagnostics, no compare
  statistics, no new probe families.
- No M003a/EggReplay adapter change unless C002 live evidence forces a
  dedicated finding (unlikely: C001 standalone EggReplay evidence is green).
- No remote/scheduler/credential machinery; no production use of the
  harness's recorder-readiness parsing.

## 5. Expected production changes

Confined to `crates/eggbench-drivers/src/external/eggprobe.rs` (+ its unit
tests):

1. Generate typed schema-0.3 plans: `probes` as `{"kind","port","url"}`
   objects derived from the existing lowered target (TCP gets the bound
   port; HTTP gets the bound URL; DNS stays `{"kind":"dns"}`; TLS gets the
   bound TLS port when an `https_url` binding exists); `execution` as
   `{"deadline":<micros>,"repetitions":1,"retries":0}` converted from the
   diagnostic timeout; `target` as `{host, port?}` only (drop `tls` and
   `http_url` from the wire plan; keep using them internally for lowering).
2. Handshake plans use the same corrected shape (empty-probe plan, then the
   bounded localhost DNS retry only if the qualified schema rejects empty
   probe lists with exit 2).
3. Parse real `ProbeResult` entries from `kind` with `ok` / `failed` /
   `unsupported` / `cancelled` vocabulary; map report `status` the same way;
   require `ok` for handshake pass and exit-0 positive disposition; keep
   exit 1 + parseable report as negative evidence; keep 2/3/130 semantics.
4. Keep every bound, redaction, skip-label, and evidence-shape guarantee;
   update stale doc comments that describe the old dialect.

## 6. Schema/storage/protocol/compatibility effects

- Wire change only on the M003b external-process seam; no bundle/plan schema
  version bump is expected (diagnostic evidence artifacts keep their names
  and redaction classes; report bytes are opaque raw evidence).
- If any persisted diagnostic summary field must change meaning (e.g. a
  status label), record it explicitly in the C002 closure; prefer keeping
  existing labels where the runner already normalizes disposition.

## 7. Ordered work packages

1. **Adapter correction.** Implement section 5 against the pinned v0.1.1
   sources (read, do not copy). Keep the fix minimal and dialect-faithful.
2. **Real-shape regression tests.** Add unit tests using byte-faithful
   plan/report shapes from the real binary (typed probes, micros deadline,
   minimal target, `kind`/`ok` vocabulary), including: 0.4-plan rejection,
   exit-1-with-report negative, exit-1-without-report failure, and the
   handshake `ok` gate. Existing mock-dialect tests must be updated, never
   deleted silently — record the change.
3. **Local live re-qualification.** Extend and run the committed harness to
   green: standalone EggReplay match/mismatch, standalone Eggprobe
   accept/reject, schema-0.4 isolation (positive accepts, negative rejects,
   and the two are distinguishable), combined positive run (bundle
   finalizes, two trials map to two replay processes, pre/post diagnostics
   in slot, timings excluded from TrialMetrics), combined mismatch run
   (completed observation + absolute gate Fail/exit 6), required-pre
   negative (Invalid + teardown), optional-post negative (completed stays
   completed), cancellation smoke (no surviving children, teardown, existing
   cancelled status).
4. **Repository gates.** Full section-9 matrix green with no new production
   sibling dependency.
5. **Hosted qualification.** Add the Linux-only live-tool job (pinned
   revisions, `--locked --release` builds, harness run, bounded
   provenance/evidence upload, fail on mismatch) and obtain a green
   four-lane run plus a green live-tool run on the corrective HEAD.
6. **Closure and reconciliation.** Write the C002 closure record, mark the
   live-tool corrective closed, unblock M004 implementation subject to its
   own plan, and link the Eggstack roadmap addendum.

## 8. Failure/cancellation/restart semantics

Unchanged from M003b: required pre failure blocks workload (Invalid);
optional post failure is recorded, never overwrites workload outcome;
cancellation propagates `Cancelled`, kills child processes, tears down the
managed origin, and follows existing partial-evidence rules. C002 must prove
these live (work package 3), not merely re-assert them.

## 9. Verification

Focused: new/updated `eggbench-drivers` unit tests for the corrected dialect
plus the harness's standalone checks.

Broad (all required, same commands as C001 section 16):

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked

cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-targets --all-features --locked

cargo +1.89.0 check --workspace --all-targets --locked
cargo +1.89.0 check --workspace --all-targets --all-features --locked
cargo +1.89.0 test -p eggbench-core --all-features --locked
cargo +1.89.0 test -p eggbench-runner --all-features --locked
cargo +1.89.0 test -p eggbench-drivers --all-features --locked

cargo tree --locked
git diff --check
```

Plus: extended harness green locally; hosted live-tool job green; four-lane
hosted CI green on the corrective HEAD.

## 10. Static guards / documentation

- No new production dependency on any `eggprobe`/`eggreplay` crate
  (guarded by `cargo tree` in CI and in the closure).
- Update the M003b adapter docs to the real dialect; do not rewrite the
  historical M003a/M003b closures.

## 11. Acceptance criteria

C002 closes only when every C001 criterion 1–23 (same numbering as the C001
closure matrix) reads PASS/verified, with criteria 6–17 now proven live
against the corrected adapter, plus: the adapter diff is confined to the
M003b seam; real-shape regression tests exist; the hosted live-tool job and
four-lane CI are green on the corrective HEAD; closure/registry/roadmap
reconciliation is committed.

## 12. Stop conditions

Stop for planning review if: the real binary rejects the corrected typed
plan; report vocabulary differs from `ok`/`failed`/`unsupported`/`cancelled`
in a way that breaks disposition mapping; the fix requires schema 0.4,
Rust sibling dependencies, metric/lifecycle semantic changes, or non-loopback
networking; a further substantive sibling defect appears. Do not close by
weakening a criterion.

## 13. Closure evidence required

`plans/closure/post-m003-live-tool-qualification-corrective/002-status.md`
with: adapter diff summary; sibling provenance (same pins as C001 unless
re-pinned by planning); fixture digests via the M003a identity code;
standalone and combined bundle identities; per-trial finding counts;
comparison receipt/verdict/exit 6; diagnostic lifecycle/timing proofs;
required/optional/cancellation results; full local matrix; hosted live-tool
run; four-lane run; unresolved findings and final disposition.
