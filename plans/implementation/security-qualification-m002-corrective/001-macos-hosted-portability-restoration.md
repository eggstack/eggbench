# Security Qualification M002 Corrective C001 — macOS Hosted Portability Restoration

Status: ready

Corrective authority:

- `plans/subsystems/security-qualification-m002-post-conditional-closure-corrective-addendum.md`

Repository baseline:

- `660ab472ebf214e1378da777b1f06412decc008a`

Trigger:

- GitHub Actions run `36262615058`;
- job `macos-stable` / `108461189505`;
- failure occurred in
  `cargo test --workspace --all-targets --all-features --locked`;
- macOS fmt/check/Clippy had already passed;
- Linux stable, Linux Rust 1.89, and Windows stable passed.

Primary class: portability/qualification corrective.

## 1. Objective

Restore exact-head four-lane hosted qualification without changing the
semantic meaning of M002 correctness/performance evidence.

The plan is investigation-first because the available GitHub check metadata
does not expose the failing test assertion. Do not guess the root cause from
the job result alone.

## 2. Initial investigation order

On a macOS runner matching hosted CI as closely as practical:

1. reproduce the full command:
   `cargo test --workspace --all-targets --all-features --locked -- --nocapture`;
2. capture the exact failing package/test/assertion and stderr;
3. rerun the failing package/test in isolation at least twice;
4. rerun the new M002 suites explicitly:
   - `cargo test -p eggbench-cli --test synvoid_m002a --all-features --locked -- --nocapture`;
   - `cargo test -p eggbench-cli --test synvoid_m002b --all-features --locked -- --nocapture`;
5. if neither M002 suite fails, partition workspace tests by package until the
   exact regression is isolated;
6. record whether failure is deterministic, timing/resource-sensitive, or
   environment/tool-presence dependent.

Do not commit a fix before the failing assertion is identified.

## 3. Allowed correction classes

Allowed narrow corrections include:

- portable filesystem/path handling;
- portable process lifecycle/cleanup;
- deterministic port allocation/use;
- test-only timing synchronization replacing unsafe sleeps;
- test fixture assumptions about Python/tool discovery;
- qualification fixture path normalization;
- bounded tolerance changes supported by repeated same-build evidence;
- platform guard correction where a capability is genuinely unsupported and
  was already documented as such.

Any production semantic change must be justified by a real cross-platform bug,
not only by a flaky test.

## 4. Disallowed shortcuts

Do not:

- remove macOS from CI;
- mark the failing test ignored;
- globally relax assertions without evidence;
- turn a deterministic failure into a retry loop;
- skip all M002 tests on macOS simply because SynVoid itself is Linux-primary;
- weaken correctness/performance thresholds to make hosted CI green;
- hide failures behind tool-absence detection if the failing path does not
  actually require that tool.

Eggbench profile/schema/orchestration machinery remains cross-platform even
though real SynVoid runtime proof is Linux-only.

## 5. M002-specific portability checks

If the failure is in M002 code/tests, verify:

- temporary workspace copying is portable;
- `python3` discovery/skip behavior is truthful;
- loopback port reuse after teardown is race-free;
- sidecar temp-file naming is collision-safe;
- baseline materialization never reuses stale paths;
- qualification receipt paths remain confined and normalized;
- child cleanup completes before port rebind assertions;
- performance-only regression fixtures are not sensitive to macOS scheduling
  in a way that turns qualification logic into a host benchmark.

For same-source statistical scenarios, Pass or Inconclusive remains
acceptable where the plan already permits it; Fail is not.

## 6. Test determinism

For the isolated failing test:

- run at least 10 consecutive repetitions after the fix where practical;
- include one full workspace all-feature test run;
- preserve a targeted regression test that fails before the fix and passes
  after it.

If the root cause is nondeterministic test timing, fix synchronization or the
test oracle rather than merely increasing sleep durations.

## 7. Verification

Local/hosted candidate:

~~~text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
cargo +1.89.0 check --workspace --all-targets --all-features --locked
git diff --check
~~~

Hosted acceptance requires, on the exact candidate SHA:

- Linux stable green;
- Linux Rust 1.89 green;
- macOS stable green;
- Windows stable green.

The existing live-tool workflow should also remain green unless a documented
unrelated infrastructure outage prevents it.

## 8. Closure

Create:

`plans/closure/security-qualification-m002-corrective/001-status.md`

Record:

- exact failing test/assertion from run `36262615058`;
- root cause;
- implementation SHA;
- targeted regression evidence;
- repeated determinism evidence;
- exact-head four-lane hosted run;
- live workflow result;
- any remaining limitations.

C001 does not fully close M002; it only restores the normal hosted
qualification prerequisite.

## 9. Acceptance criteria

C001 closes only when:

1. the macOS failure is identified, not inferred;
2. a narrow root-cause correction is landed;
3. no M002 semantic threshold/correctness rule is weakened;
4. the targeted regression test is green repeatedly;
5. full macOS workspace all-feature tests are green;
6. exact-head four-lane CI is green;
7. Linux/MSRV/Windows remain green;
8. closure evidence is committed.

## 10. Stop conditions

Stop and author a successor corrective if:

- the failure exposes a broader runner/process portability defect;
- fixing it requires changing public plan/evidence schemas;
- multiple independent failures appear after the first correction;
- a production semantic change would exceed this portability scope.
