# Security Qualification M003d Corrective — Eggsec Load CI Portability and Qualification Unblock

Status: **closed**; implementation and hosted four-lane + live qualification
complete.

Implementation commit:
`470333370e14c6a2ac31047f3f6836ce756f9a2a`
(`fix(security): repair Eggsec-load registration accounting and CI
portability`).

Closure record:
`plans/closure/security-qualification/003d-corrective-status.md`.

Hosted evidence on the exact implementation head: normal CI run `36640125422`
(four lanes green) and live qualification run `36640125453` (all four live jobs
green, including `live-synvoid-linux` with real SynVoid stages executed).

Source milestone:
`plans/implementation/security-qualification/003d-eggsec-load-profiles-live-qualification-and-m003-closure.md`.

Trigger implementation:
`739f38285776c1ceb13df4fbfb9a04eb27a83077`
(`feat(security): add Eggsec strict-scope load workload`).

Corrective planning baseline:
`a96407017716b8ca7d18da8237a135b1a49af8cc`.

Related status records:

- `plans/closure/security-qualification/003d-status.md`;
- `plans/closure/security-qualification-m002-corrective/002-status.md`;
- `plans/closure/security-qualification/003c-status.md`.

This is a narrow corrective under `plans/003-planning-process.md` §8. It
repairs defects introduced by the first M003d Eggsec-load slice and restores
the repository-wide qualification gate. It does not implement the remaining
M003d profile/live milestone.

## 1. Why this corrective exists

The initial Eggsec-load slice is architecturally usable, but exact-head hosted
qualification exposed two repository regressions that local verification did
not catch.

On Eggbench implementation SHA
`9d9d8b99d12127e959dab6ca895f278aa090b742`:

- live qualification run `36607614200` was green and executed the real
  SynVoid M002 stages;
- four-lane normal CI run `36607614262` was red;
- `linux-msrv` passed;
- `linux-stable` and `macos-stable` failed
  `workload_registry::tests::production_runtime_reports_no_driver`;
- `windows-stable` failed strict Clippy on
  `crates/eggbench-drivers/src/external/eggsec_load.rs`.

The failing source was introduced by M003d commit `739f382`; the intervening
M002 C002 implementation contains no `crates/*/src` changes. C002 therefore
correctly remains `CLOSING` and assigns its remaining four-lane criterion to
this repair.

## 2. Current-state findings

### 2.1 Production workload/descriptors counts are stale

`WorkloadRegistry::production()` now registers five unconditional workloads:

1. `eggsec-load`;
2. `eggreplay-semantic`;
3. `h2load`;
4. `iperf3`;
5. `oha`.

`eggfetch-http` remains the feature-gated native/default workload.

The direct registry test was updated to expect `eggsec-load`, but
`production_runtime_reports_no_driver` still calculates:

~~~text
expected_workload = 4 + eggstack_http
expected_descriptors = 6 + ...
~~~

That is internally inconsistent with the same module and with
`DriverCatalog`, which also gained the `eggsec-load` descriptor.

The expected base counts are now:

~~~text
expected_workload = 5 + eggstack_http
expected_descriptors = 7 + existing feature-dependent additions
~~~

Implementation should avoid another magic-count drift if a small,
non-invasive derivation from the registered inventory/catalog is practical.
Do not weaken the test into a vacuous self-comparison: it must still prove
that production runtime registration and descriptor exposure contain the
intended fixed driver set.

### 2.2 Unix-only permission hardening creates Windows `unused_mut`

`create_scope_file()` currently declares both
`std::fs::DirBuilder` and `std::fs::OpenOptions` as mutable on all
platforms, while their mutating `.mode(...)` calls are inside
`#[cfg(unix)]` blocks.

On Windows stable with `-D warnings`, those bindings are unnecessarily
mutable and fail Clippy/rustc linting.

Preserve the Unix security properties:

- scope directory mode `0700`;
- scope file mode `0600`;
- `create_new(true)`;
- private temporary directory;
- cleanup on `ScopeFile::drop`.

Preferred correction is a cfg-specific builder/helper shape that is genuinely
non-mutable on non-Unix targets. Do not solve this by globally suppressing
`unused_mut` or removing Unix permission hardening.

### 2.3 Previous M003d verification text is no longer sufficient

`plans/closure/security-qualification/003d-status.md` says strict Clippy and
CLI/driver targets were green after the adapter addition, but hosted
cross-platform evidence subsequently disproved that as terminal portability
evidence.

The local results remain historical evidence. They must not be deleted, but
the status record must identify run `36607614262` as the stronger
cross-platform finding and point to this corrective.

## 3. Scope

Required production changes are limited to:

- correcting production workload/descriptor registration expectations;
- correcting the cfg-specific scope-file builder construction so all supported
  targets satisfy strict warnings;
- adding or strengthening focused regression tests only as needed to prevent
  recurrence.

Required evidence/planning changes are limited to:

- exact-head four-lane normal CI;
- exact-head C002 revalidation after the production corrective;
- M003d status reconciliation.

No schema, workload semantics, scope rules, result parsing, metric names,
thresholds, qualification policy, or security expectation may change.

## 4. Invariants

The corrective must preserve:

- `eggsec-load` remains explicit and non-default;
- no fallback to native Eggfetch when Eggsec is unavailable;
- one reviewed digest-pinned `http_corpus` case per Eggsec-load trial;
- local/private strict-scope enforcement;
- guarded no-network preflight before load traffic;
- bounded requests, concurrency, headers, body, stdout, stderr, and timeout;
- expected HTTP outcome mismatch remains distinct from transport error rate;
- Eggsec/Eggfetch transport is recorded as non-independent corroboration;
- no public/wildcard target scope;
- no production SynVoid or Eggsec dependency enters Eggbench.

## 5. Work package A — registration accounting correction

Update `crates/eggbench-cli/src/workload_registry.rs` so the production
runtime test reflects the actual registry/catalog.

Minimum assertions:

- exact unconditional workload names include `eggsec-load`;
- expected workload count is correct with and without `eggstack-http`;
- expected descriptor count includes the additional Eggsec-load descriptor;
- default workload remains `eggfetch-http` only when that feature is enabled;
- `fake-load` remains absent from production;
- qualification runtime remains isolated to its fake driver.

If a helper is introduced for expected names/counts, keep it test-only or
otherwise non-semantic.

## 6. Work package B — portable secure temporary scope creation

Refactor the directory/file builder setup in
`crates/eggbench-drivers/src/external/eggsec_load.rs`.

Acceptance:

- Unix builds still apply mode `0700` to the directory and `0600` to the
  scope file before creation;
- non-Unix builds contain no unnecessary mutable bindings;
- `create_new(true)` remains;
- collision retry semantics remain;
- cleanup semantics remain;
- no weakening of strict-scope manifest privacy.

A narrow cfg-specific helper is preferred over lint suppression.

## 7. Work package C — focused verification

Before push:

~~~text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test -p eggbench-cli --all-features --locked
cargo test -p eggbench-drivers --all-features --locked
cargo +1.89.0 check --workspace --all-targets --all-features --locked
git diff --check
~~~

Also exercise feature-isolation checks equivalent to the Linux stable workflow
for `eggbench-drivers` and `eggbench-cli`.

## 8. Work package D — hosted qualification and C002 unblock

The corrective is not closed on local tests alone.

Push the proof-bearing corrective SHA and require the normal CI workflow to be
green on all four supported lanes:

- `linux-stable`;
- `linux-msrv`;
- `macos-stable`;
- `windows-stable`.

Because M002 C002's terminal record explicitly requires revalidation on the
later exact HEAD that repairs this failure, also run the live qualification
workflow on the same corrective head (or a documentation-only descendant whose
source tree is identical) and verify the real `live-synvoid-linux` stages
still execute rather than becoming NOT-EXECUTED.

Record both hosted run IDs in:

`plans/closure/security-qualification-m002-corrective/002-status.md`.

If the four-lane and live requirements are green and no new C002-owned defect
appears, C002 may perform its terminal roadmap/registry reconciliation. Do not
re-run or weaken its frozen thresholds merely because the repair is M003d-owned.

## 9. Relationship to M003c and SynVoid v2

The former upstream blocker is gone.

SynVoid has closed the corrected owner telemetry contract:

- current SynVoid head observed during this planning pass:
  `0dc1f7fb21a5df60e72fc7f2cd60b7cb73bc9f35`;
- proof-bearing corrective implementation:
  `739e7ba6f02c5e3f83fe9ff5321b09213182b193`;
- owner contract: `synvoid.eggbench-telemetry.v2`;
- terminal closeout:
  `architecture/eggbench_security_qualification_m003_telemetry_corrective_closeout.md`;
- mapping SHA-256:
  `622f6a13c4353cc7465cce39a57ed86fa0db2fe4114258e6f06226c1748d2d99`.

SynVoid's owner closeout used Eggbench's real
`PrometheusMappingV1`/`PrometheusHttpCollector` at the M003c line and
proved parser/validator/preflight/trial compatibility. That is valid upstream
interface evidence, but it does not replace Eggbench M003c's own current-head
live qualification, M003b pressure/body scenario evidence, or hosted closure.

M003c is therefore no longer dependency-blocked and may gather terminal
evidence in parallel with this corrective. Final M003c closure should be
recorded only on an Eggbench head whose normal CI is green.

## 10. Relationship to remaining M003d work

This corrective does **not** close M003d.

After this corrective and C002 close:

1. M003c terminal SynVoid v2 qualification must close;
2. M003d may then implement/check in the planned SynVoid M003 profile family;
3. run body, mixed, pooled/fresh, telemetry, Eggsec load, and negative
   correctness/performance/telemetry demonstrations;
4. run bounded Linux real-SynVoid M003 qualification plus four-lane CI;
5. write `plans/closure/security-qualification/003-status.md`.

The order prevents the adapter's portability defect from blocking unrelated
M002 closure while preserving M003's existing terminal dependency rules.

## 11. Regression evidence

At minimum add/retain evidence for:

1. production workload inventory contains `eggsec-load`;
2. production runtime workload count matches the intended inventory under
   default/all-features configurations;
3. driver descriptor count includes `eggsec-load` under the same feature
   matrix;
4. qualification runtime still exposes only `fake-load`;
5. Unix secure directory/file modes remain configured;
6. non-Unix build/clippy has no `unused_mut` regression;
7. strict Eggsec load argv/preflight/report tests remain green;
8. no driver default/fallback semantics changed.

## 12. Acceptance criteria

Close this corrective only when:

1. the stale runtime count assertion is corrected;
2. Windows strict Clippy no longer reports the scope-builder mutation warning;
3. Unix private-mode behavior is retained;
4. focused driver/CLI/MSRV verification is green;
5. four-lane hosted CI is green on the corrective source;
6. the exact-head live qualification revalidates C002's real SynVoid stages;
7. C002 status records the new hosted evidence;
8. M003d status records this corrective as the portability/CI repair;
9. no Eggsec-load behavioral or security contract was weakened.

## 13. Stop conditions

Stop and re-plan if:

- fixing the descriptor count reveals inconsistent production/catalog
  registration beyond the new driver;
- Windows requires weakening scope-file confidentiality semantics;
- Eggsec load behavior differs across platforms in a way that changes
  experiment identity;
- the four-lane rerun exposes a different semantic failure;
- C002 live proof regresses for reasons attributable to this source change.

Do not fold any such finding into this narrow corrective without updating its
scope and evidence requirements.
