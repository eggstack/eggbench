---
name: eggbench-add-driver
description: |
  Add a driver to eggbench's production catalog and wire it through resolution,
  dispatch, and the CLI without breaking feature isolation. Use when a task adds
  a workload, service, telemetry, correctness, diagnostic, route, or fault
  driver, adds a new capability descriptor, or registers an external tool.
  Triggers on "add a driver", "new workload driver", "register a telemetry
  collector", "support <tool> as a driver", "new capability", "catalog
  registration", "feature-gated driver". Do not use for changing an existing
  driver's parsing or execution internals, for a fake/test-only driver (those
  are injected through the qualification seam and never enter the catalog), or
  for adding a plan field unrelated to a driver.
---

# Eggbench add-driver

## Inputs to collect

- The driver category. `DriverCategory` has eight variants — `Service`,
  `Workload`, `Telemetry`, `Fault`, `Diagnostic`, `Correctness`,
  `ExecutionProvider`, `Route` — and the category decides which registry and
  which evidence path the driver lands in.
- Whether the driver needs a cargo feature, and whether that feature implies
  another. `eggstack-path` implies `eggstack-http`; a new path-shaped driver
  belongs behind that same gate.
- Whether the driver is a spawned external process or an in-process adapter.
  External-process drivers are unconditional by design: a missing binary is a
  resolution/preflight failure, not a link-time one.

## Procedure

1. Add a `*_descriptor()` constructor in `eggbench-drivers` returning a
   `DriverDescriptor` with a stable name, exactly one category, a platform
   label, and typed capability values. Put the name in a `pub const
   <TOOL>_DRIVER_NAME` rather than a string literal, and re-export it from
   `lib.rs` — the CLI and the catalog then share one name definition, so the
   names cannot drift.

2. Register it in `DriverCatalog::production` in `catalog.rs`. Feature-gated
   registrations sit above the unconditional external block, each behind its
   own `#[cfg(feature = ...)]`. Keep the unconditional list last so the
   no-feature build is obviously unaffected.

3. Confirm the category is reachable through an accessor. All eight categories
   have one (`workload`, `service`, `telemetry`, `diagnostic`, `correctness`,
   `route`, `fault`, `execution_provider`), plus a category-independent
   `by_name`. A test asserts every registered category resolves through its own
   accessor, so a registration in a category with no accessor fails immediately.

4. Add the dispatch arm. Executor construction is a name match in
   `production_workload_executor` in the CLI's `workload_registry.rs`, not a
   catalog lookup, so a catalog entry with no arm is a runtime failure rather
   than a compile error. Resolve an external binary synchronously in the arm so
   a missing binary fails before managed startup; executors may self-probe their
   version on first execution.

5. Add the driver to the expected-name lists in the tests that enumerate the
   inventory — `production_catalog_matches_feature` in `catalog.rs` and
   `expected_production_driver_names` in `workload_registry.rs`. Both are
   hardcoded on purpose, because they are what proves the two name lists stay
   equal.

6. Run the two tests that catch an incomplete wiring on their own:

   ```bash
   cargo test -p eggbench-drivers --all-features --locked production_catalog_matches_feature
   cargo test -p eggbench-cli --all-features --locked every_catalog_workload_driver_has_a_dispatch_arm
   ```

   The second one iterates every catalog workload driver and fails if the
   factory returns its "no production executor" error, which is the exact
   symptom of a missing arm.

7. If the driver needs a new plan field, that is a plan schema change, not a
   driver change — route it through the versioning discipline so existing plans
   and bundles keep their meaning. Adding only a capability the plan selects
   through an existing field needs no bump.

8. Update the inventory prose that now has one more entry: the module doc
   comment at the top of `catalog.rs` (which enumerates the unconditional and
   feature-gated sets), `docs/driver-capabilities.md`, and
   `architecture/drivers-catalog-features.md`. The feature matrix in
   `architecture/overview.md` carries a driver count that changes with the
   registration.

9. Prove feature isolation still holds — a new feature-gated driver must not
   drag Eggress or Eggchaos into default or `eggstack-http`-only builds. Use
   the matrix from the `eggbench-verification-gates` skill, and check the
   dependency tree rather than trusting the `#[cfg]`.

## Failure handling

- A driver that compiles but is not in the catalog fails resolution, usually as
  `missing_driver`; one that is in the catalog but not in an executor fails at
  dispatch with "no production executor for workload driver". Those two messages
  identify which of the two lists was missed.
- Do not add a fake or test-only driver to the production catalog. Deterministic
  qualification injects through `with_qualification_fake` / the qualification
  runtime, and a test asserts the production registry contains no fake driver.
  There is no public flag that selects one.
- If a driver must be selectable from the command line, that is a plan or
  `--workload-driver` concern; do not add a new CLI flag for a single driver.
  Route, fault, network-path, and seed selection live in the plan by contract.

## Examples

Input: "add Eggsec-WAF as a correctness driver"
Output: descriptor with `Correctness` category behind the existing feature,
registered in `catalog`, name const re-exported, added to both expected-name
lists, and documented in the three inventory locations — with no schema bump,
because security correctness is already selected by an existing plan field.

## Windows (win32) platform notes

A `Workload` driver backed by a spawned process behaves differently on Windows:
the external substrate passes `direct_child_only` and rejects `.ps1` only on
Windows while `.bat`/`.cmd` are rejected everywhere, so a Windows lane needs its
own resolution test. `cargo test -p eggbench-drivers --all-features --test
eggstack_path --locked` is the path-specific test the Windows lane runs.
