---
name: eggbench-verification-gates
description: |
  Run eggbench's pre-commit verification gates and read their results. Use when a
  change to any crate in this workspace is complete and needs proving, before
  committing or opening a PR, or when a gate fails and the failure needs
  interpreting — triggers on "verify", "run the tests", "is this ready to
  commit", "CI would pass", "clippy is failing", "feature isolation",
  "run the checks". Do not use for authoring or debugging the failing code
  itself, for one-off `cargo test <name>` exploration while still iterating, or
  for the live-binary harnesses under `scripts/qualification/` which have their
  own gating and are not part of this skill.
---

# Eggbench verification gates

## Inputs to collect

- Which crates the change touches. `--all-features` on the workspace builds
  every feature combination, so a drivers change is what makes the isolation
  matrix matter; a core-only change can start at the workspace commands.
- Whether the change is docs-only. Doc changes still need
  `verify-architecture-docs.py`, but a prose edit does not need the cargo
  matrix re-run if no Rust file changed.

## Procedure

1. Run the four workspace commands in the order below.
   The order is cheapest-first so a formatting slip costs a second rather than
   a full clippy pass.

   ```bash
   cargo fmt --all -- --check
   cargo check --workspace --all-targets --locked
   cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
   cargo test --workspace --all-targets --all-features --locked
   ```

   `--locked` is not optional: the lockfile is part of the reproducibility
   contract, and omitting it lets a resolution change slip into a diff that
   looks source-only.

2. If any `eggbench-drivers` or `eggbench-cli` file changed, run the feature
   isolation matrix. These are additive features, and the invariant under test
   is that the Eggstack path crates stay out of default and `eggstack-http`-only
   builds:

   ```bash
   cargo check -p eggbench-drivers --no-default-features --locked
   cargo check -p eggbench-drivers --no-default-features --features eggstack-http --locked
   cargo check -p eggbench-drivers --no-default-features --features gregg --locked
   cargo check -p eggbench-drivers --no-default-features --features eggstack-path --locked
   cargo check -p eggbench-drivers --no-default-features --features eggstack-path,gregg --locked
   cargo check -p eggbench-drivers --no-default-features --features prometheus-http --locked
   cargo check -p eggbench-cli --no-default-features --features eggstack-path --locked
   if cargo tree -p eggbench-drivers --no-default-features --edges normal | grep -E 'eggress|eggchaos'; then exit 1; fi
   if cargo tree -p eggbench-drivers --no-default-features --features eggstack-http --edges normal | grep -E 'eggress|eggchaos'; then exit 1; fi
   cargo run -p eggbench-cli --features eggstack-path --locked -- validate examples/eggstack-path.json
   ```

   The two `cargo tree | grep` lines are the real assertion; the greps exiting
   clean is the pass condition, so run them in a context where a non-empty
   match fails the step rather than printing to the terminal.

3. If any `architecture/*.md` file changed, run the doc verifier:

   ```bash
   python3 scripts/verify-architecture-docs.py
   ```

   It checks that relative links resolve, every `file:line` citation is in
   range, every deep dive carries the ten standard sections, and `overview.md`
   links every deep dive. It cannot tell whether a citation points at the
   *right* line, so a moving `file.rs:LINE` reference is still worth a manual
   look when the cited code changed in this diff.

4. If the change touched the CLI surface, also run the feature-off test target.
   `run` must fail closed with a stable category when no feature is enabled,
   and that path is only exercised here:

   ```bash
   cargo test -p eggbench-cli --no-default-features --test cli --locked
   ```

5. To reproduce a single platform-specific qualification locally, run the exact
   command CI runs rather than the whole suite. On macOS CI qualifies process
   group cleanup and symlink confinement; on Windows it runs the platform test
   target and the path suite:

   ```bash
   cargo test -p eggbench-runner --test lifecycle descendant_cleanup_reaches_process_group --locked
   cargo test -p eggbench-runner --test lifecycle workspace_and_cwd_are_resolved_through_the_filesystem --locked
   cargo test -p eggbench-runner --test platform --locked
   cargo test -p eggbench-drivers --all-features --test eggstack_path --locked
   ```

## Interpreting results

- Clippy is workspace-wide `pedantic = warn` with `-D warnings`, so new lints
  surface as errors. `assert_is_empty` is deliberately `allow` in
  `Cargo.toml` — a report about `assert!(x.is_empty())` is the known lint debt
  being intentionally deferred, not a regression, so leave those alone.
- The MSRV lane is 1.89. It only runs `cargo check` plus core and drivers
  tests, so an MSRV break can pass the full local matrix and still fail CI:
  check with `cargo +1.89.0 check --workspace --all-targets --locked` when a
  change touches language or standard-library usage.
- `run` exit codes carry meaning: `2` parse/validation, `3`
  capability/preflight/path, `4` finalized non-success, `5` bundle I/O. A `3`
  is a correct fail-closed result, not a crash — check the category before
  treating it as a defect.
- `--all-features` clippy can differ from a default build because of
  `#[cfg]`-gated code that only compiles with a feature on. If a lint fires
  only under `--all-features`, the offending code is behind a gate.

## Failure handling

- Report the failing command verbatim with its first error, not a paraphrase.
  The distinction between a compile error, a lint, and a failed assertion
  points at three different owners.
- A gate that cannot run in this environment (missing toolchain, missing pinned
  external binary) is reported as unverified. Do not describe a skipped gate as
  passing.
- Do not run anything under `scripts/qualification/` or
  `.github/workflows/live-tools.yml` to satisfy this skill. Those harnesses
  need real pinned sibling binaries (Eggsec, SynVoid, EggReplay, Eggprobe,
  `oha` on a specific arch) and produce evidence gated on exact-head hosted
  runs. Ordinary `cargo test` is the gate; the live harnesses are driven by
  `plans/registry.md` milestones.

## Examples

Input: "I added a new field to the plan schema — is this ready to commit?"
Output: workspace four commands, then the drivers isolation matrix (a schema
change is felt by driver resolution), then `verify-architecture-docs.py` if any
architecture citation moved.

## Windows (win32) platform notes

Managed `run` is unsupported on Windows, so verification there is limited to
`cargo fmt`/`check`/`clippy` and the test targets CI itself runs
(`eggbench-core`, `eggbench-runner --test platform`,
`eggbench-drivers --test eggstack_path`). In PowerShell the isolation greps
need the exit status checked explicitly, because `Select-String` does not
short-circuit:

```powershell
$hits = cargo tree -p eggbench-drivers --no-default-features --edges normal |
  Select-String -Pattern 'eggress|eggchaos'
if ($hits) { Write-Error "feature isolation violated"; exit 1 }
```

The verifier is invoked as `python scripts\verify-architecture-docs.py`; `python3`
is not on PATH by default.
