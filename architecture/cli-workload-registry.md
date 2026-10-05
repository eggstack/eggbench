# CLI workload registry

Deep dive into `crates/eggbench-cli/src/workload_registry.rs` (1,005 lines): the seam that turns
a resolved driver name into a live runtime adapter, and the production/qualification split that
keeps test doubles out of the production binary.

## Purpose

This module is the CLI's **factory layer**, not only a workload registry. It holds four things:

1. a display-oriented inventory view over the production driver catalog
   ([`WorkloadDescriptor`](../crates/eggbench-cli/src/workload_registry.rs#L30),
   [`WorkloadRegistry`](../crates/eggbench-cli/src/workload_registry.rs#L106),
   [`DriverInventoryEntry`](../crates/eggbench-cli/src/workload_registry.rs#L92));
2. the workload **dispatch table** that maps a resolved driver name to a constructed
   `Box<dyn WorkloadExecutor>` ([`production_workload_executor`](../crates/eggbench-cli/src/workload_registry.rs#L398));
3. the service and telemetry factories the same commands need
   ([`production_service_adapters`](../crates/eggbench-cli/src/workload_registry.rs#L380),
   [`production_telemetry_registry`](../crates/eggbench-cli/src/workload_registry.rs#L521),
   [`gregg_endpoint_config_error`](../crates/eggbench-cli/src/workload_registry.rs#L775));
4. the qualification injection seam that admits a deterministic fake
   ([`QualificationRuntime`](../crates/eggbench-cli/src/workload_registry.rs#L176),
   [`BuiltinWorkloadExecutor`](../crates/eggbench-cli/src/workload_registry.rs#L338)).

The module header claims production inventory is owned by `eggbench-drivers` and that this module
is "a thin CLI-facing compatibility view over that catalog"
([workload_registry.rs:1-4](../crates/eggbench-cli/src/workload_registry.rs#L1)). That claim holds for
**registration** and only partly for **dispatch**: see [Review focus](#review-focus).

Three call sites consume it: the `run` production path
([commands/run.rs:85-86](../crates/eggbench-cli/src/commands/run.rs#L85),
[run.rs:119-145](../crates/eggbench-cli/src/commands/run.rs#L119)), the `run` qualification path
([run.rs:293-299](../crates/eggbench-cli/src/commands/run.rs#L293)), and `doctor`
([commands/doctor.rs:45-51](../crates/eggbench-cli/src/commands/doctor.rs#L45)). Diagnostic and
correctness factories are **not** here even though telemetry and service factories are: those
registries are built inline in
[run.rs:193-263](../crates/eggbench-cli/src/commands/run.rs#L193). Command behavior is covered in
[cli-commands.md](cli-commands.md).

## Source layout

| Region | Lines | Contents |
|---|---|---|
| Module docs | 1-14 | Ownership claim, qualification claim (partly stale — see Review focus) |
| Imports | 16-26 | `eggbench_runner::test_support::FakeWorkload` is imported unconditionally |
| Inventory types | 28-95 | `WorkloadDescriptor`, `DriverInventoryEntry` |
| Refusal type | 97-102 | `NoProductionAdapter` |
| Registry + runtime traits | 104-231 | `WorkloadRegistry`, `WorkloadRuntime`, `ProductionRuntime`, `QualificationRuntime` |
| Registry impl | 233-330 | `production`, `with_builtin`, `with_qualification_fake`, accessors |
| Fake executor | 332-372 | `BuiltinWorkloadExecutor` |
| Service factory | 374-385 | `production_service_adapters` |
| Workload factory | 387-506 | `production_workload_executor`, `build_eggfetch_executor` |
| Telemetry factory | 508-765 | telemetry registry, per-source collectors, pinned-mapping loader |
| Doctor-only validation | 767-804 | `gregg_endpoint_config_error` |
| Tests | 806-1005 | 8 unit tests plus 2 hand-maintained expectation lists |

The module is `mod workload_registry;` (private) with a partial re-export at
[lib.rs:22](../crates/eggbench-cli/src/lib.rs#L22) and [lib.rs:28-31](../crates/eggbench-cli/src/lib.rs#L28).
`ProductionRuntime` and the four `production_*` factories are **not** re-exported, so an external
test can only obtain a production runtime through `WorkloadRegistry::production()`
([lib.rs:28-31](../crates/eggbench-cli/src/lib.rs#L28), [tests/cli.rs:254](../crates/eggbench-cli/tests/cli.rs#L254)).

## Key types

| Type | Purpose | Notes |
|---|---|---|
| `WorkloadDescriptor` | Flat, display-oriented view of one workload driver | `capabilities` are pre-rendered strings via `format!("{capability:?}")` then sorted — display-only, not typed ([:50-55](../crates/eggbench-cli/src/workload_registry.rs#L50)) |
| `WorkloadRegistry` | `BTreeMap<String, WorkloadDescriptor>` | Ordering is by name, which is what makes `inventory()` and `default_workload()` deterministic ([:106-108](../crates/eggbench-cli/src/workload_registry.rs#L106), [:298](../crates/eggbench-cli/src/workload_registry.rs#L298)) |
| `DriverInventoryEntry` | One-entry wrapper so inventory rows can grow fields | Single field today ([:92-95](../crates/eggbench-cli/src/workload_registry.rs#L92)) |
| `WorkloadRuntime` | The injectable seam: `inventory`, `driver_descriptors`, `has_workload_driver` | Three methods, all inventory-shaped; **no method constructs an executor** ([:116-123](../crates/eggbench-cli/src/workload_registry.rs#L116)) |
| `ProductionRuntime` | Catalog-derived runtime; holds the full descriptor set for resolution | `driver_descriptors` returns the **catalog** descriptors, not the re-projection ([:162-164](../crates/eggbench-cli/src/workload_registry.rs#L162)) |
| `QualificationRuntime` | Fake descriptor plus fake-executor constructor | `service_descriptor()` is a Service-category claim with **no** capabilities, because external-lifecycle services spawn nothing ([:196-210](../crates/eggbench-cli/src/workload_registry.rs#L196)) |
| `BuiltinWorkloadExecutor` | `WorkloadExecutor` over `Arc<Mutex<FakeWorkload>>` | The mutex serializes the fake's mutable state across `execute`/`drain` ([:338-340](../crates/eggbench-cli/src/workload_registry.rs#L338), [:352-372](../crates/eggbench-cli/src/workload_registry.rs#L352)) |
| `NoProductionAdapter` | Declares one variant, `Empty` | Never constructed anywhere in the workspace; see [Failure model](#failure-model) |

Composition: `WorkloadRuntime` is implemented twice over the same `WorkloadRegistry` type. The two
implementations differ in exactly one way — `ProductionRuntime` keeps a separate copy of the
**complete** catalog descriptor vector for resolution ([:133-136](../crates/eggbench-cli/src/workload_registry.rs#L133)),
while `QualificationRuntime` derives descriptors through the lossy re-projection
([:224-226](../crates/eggbench-cli/src/workload_registry.rs#L224)). Production and qualification
therefore differ in *descriptor fidelity*, not in shape.

`WorkloadDescriptor::to_descriptor` ([:68-87](../crates/eggbench-cli/src/workload_registry.rs#L68)) is
the re-projection and it is lossy by construction: `upstream_version` is forced to `None`
([:78](../crates/eggbench-cli/src/workload_registry.rs#L78)), `capabilities` is replaced by a single
fabricated `LoadMode { mode: ClosedLoop }` ([:70-73](../crates/eggbench-cli/src/workload_registry.rs#L70)),
and `supported_platforms`, `machine_output_schema`, `compatible_service_types` are empty with
`external_process: false` hardcoded ([:81-85](../crates/eggbench-cli/src/workload_registry.rs#L81)).

## How it works

### Registration is derived, twice

Both production constructors run the same filter-and-register loop over the catalog:
`ProductionRuntime::new` ([:141-154](../crates/eggbench-cli/src/workload_registry.rs#L141)) and
`WorkloadRegistry::production` ([:242-253](../crates/eggbench-cli/src/workload_registry.rs#L242)). The
loop keeps descriptors where `category == DriverCategory::Workload`. This is a genuine derivation —
the CLI does not name a single production workload driver in these paths — but the loop is written
twice rather than shared, so the two entry points must be kept in step by hand.

`WorkloadRegistry::with_builtin` ([:255-262](../crates/eggbench-cli/src/workload_registry.rs#L255)) is a
legacy alias that now delegates to `production()`.

### Dispatch is a name match, not a lookup

`production_workload_executor(driver, resolved)` ([:398-401](../crates/eggbench-cli/src/workload_registry.rs#L398))
is a chain of `if driver.as_str() == eggbench_drivers::<NAME>_DRIVER_NAME` tests. It receives the
**resolved** driver name from `run` ([run.rs:119-123](../crates/eggbench-cli/src/commands/run.rs#L119))
and never consults `production_catalog()` or `DriverCatalog::workload`
([catalog.rs:81-85](../crates/eggbench-drivers/src/catalog.rs#L81)).

Adapter construction, keyed by driver name and required cargo feature:

| Driver name | Name constant (line) | Category | Required CLI feature | Construction performed | Binary resolution |
|---|---|---|---|---|---|
| `oha` | `OHA_DRIVER_NAME` ([:402](../crates/eggbench-cli/src/workload_registry.rs#L402)) | Workload | none | `OhaWorkload::resolve()` → `from_resolved(executable)` ([:403-407](../crates/eggbench-cli/src/workload_registry.rs#L403)) | synchronous, in the factory |
| `h2load` | `H2LOAD_DRIVER_NAME` ([:409](../crates/eggbench-cli/src/workload_registry.rs#L409)) | Workload | none | `H2loadWorkload::resolve()` → `from_resolved` ([:410-414](../crates/eggbench-cli/src/workload_registry.rs#L410)) | synchronous |
| `iperf3` | `IPERF3_DRIVER_NAME` ([:416](../crates/eggbench-cli/src/workload_registry.rs#L416)) | Workload | none | `Iperf3Workload::resolve()` → `from_resolved` ([:417-421](../crates/eggbench-cli/src/workload_registry.rs#L417)) | synchronous |
| `eggsec-load` | `EGGSEC_LOAD_DRIVER_NAME` ([:423](../crates/eggbench-cli/src/workload_registry.rs#L423)) | Workload | none | `EggsecLoadWorkload::resolve()` → `from_resolved` ([:424-428](../crates/eggbench-cli/src/workload_registry.rs#L424)) | synchronous |
| `eggreplay-semantic` | `EGGREPLAY_DRIVER_NAME` ([:430](../crates/eggbench-cli/src/workload_registry.rs#L430)) | Workload | none | requires a resolved plan ([:431-433](../crates/eggbench-cli/src/workload_registry.rs#L431)); requires `Workload::SemanticReplay` ([:434-438](../crates/eggbench-cli/src/workload_registry.rs#L434)); `current_dir()` as workspace root ([:439-440](../crates/eggbench-cli/src/workload_registry.rs#L439)); then `from_resolved(executable, workspace_root, fixture.clone())` ([:441-449](../crates/eggbench-cli/src/workload_registry.rs#L441)) | synchronous; version probe and `eggreplay validate` deferred to [run.rs:167-191](../crates/eggbench-cli/src/commands/run.rs#L167) |
| `eggfetch-http` | `EGGFETCH_HTTP_DRIVER_NAME` ([:453](../crates/eggbench-cli/src/workload_registry.rs#L453)) | Workload | `eggstack-http` | `build_eggfetch_executor` ([:454](../crates/eggbench-cli/src/workload_registry.rs#L454)) | none — in-process client |
| any other name | — | — | — | `Err("no production executor for workload driver {name}")` ([:456-459](../crates/eggbench-cli/src/workload_registry.rs#L456) with the feature, [:464-467](../crates/eggbench-cli/src/workload_registry.rs#L464) without) | — |

`build_eggfetch_executor` ([:474-506](../crates/eggbench-cli/src/workload_registry.rs#L474)) is the only
adapter whose construction depends on plan content rather than just a name. With `eggstack-path` it
allocates a shared diagnostics slot, lowers the resolved `network_path` into a dialer with a 30 s
budget, and wraps the client with it ([:478-497](../crates/eggbench-cli/src/workload_registry.rs#L478);
the client is built around the dialer at
[eggstack/fetch.rs:461-464](../crates/eggbench-drivers/src/eggstack/fetch.rs#L461)). With
`eggstack-http` but not `eggstack-path` it returns a plain `eggfetch_workload()` — unless the plan
resolved a `network_path`, in which case it refuses
([:500-501](../crates/eggbench-cli/src/workload_registry.rs#L500)). An unresolved plan and a plan
without `network_path` both take the plain-client path
([:480-485](../crates/eggbench-cli/src/workload_registry.rs#L480)).

### The production/qualification split

`NoProductionAdapter` documents the intent — the production category is empty, not faked
([:97-102](../crates/eggbench-cli/src/workload_registry.rs#L97)) — and `QualificationRuntime` is the
only way a fake enters: `WorkloadRegistry::with_qualification_fake` registers exactly one
descriptor named `fake-load` with `default: true` ([:271-282](../crates/eggbench-cli/src/workload_registry.rs#L271)),
and `QualificationRuntime::workload_executor` is the only constructor for the matching executor
([:214-216](../crates/eggbench-cli/src/workload_registry.rs#L214)).

The property that matters is negative: **no public production flag selects the fake path.** The fake
reaches a run only when a caller passes a `FakeWorkload` into
`run_with_qualification` ([run.rs:280-317](../crates/eggbench-cli/src/commands/run.rs#L280)), which
takes the fake as a function argument rather than reading argv, an env var, or a plan field. The
production entry points — `run` ([run.rs:74-84](../crates/eggbench-cli/src/commands/run.rs#L74)) and
`doctor` ([doctor.rs:35-56](../crates/eggbench-cli/src/commands/doctor.rs#L35)) — build their runtimes
from `ProductionRuntime::new` or `WorkloadRegistry::production`, and neither function references
`with_qualification_fake`. `main.rs` contains no reference to `QualificationRuntime`,
`with_qualification_fake`, or `run_with_qualification`; its `qualify` command family is an unrelated
security-qualification profile and receipt surface. A third construction site exists at
[run.rs:709-713](../crates/eggbench-cli/src/commands/run.rs#L709) (`builtin_executor_for_tests`, private,
`#[allow(dead_code)]`, not `#[cfg(test)]`-gated), which also takes the fake as an argument.

### Telemetry and service factories

`production_service_adapters` is a `#[cfg]` pair: the EggServe adapter with `eggstack-http`,
`ServiceAdapterRegistry::new()` otherwise ([:380-385](../crates/eggbench-cli/src/workload_registry.rs#L380)).
`production_telemetry_registry` walks the resolved plan's telemetry requests and builds one
collector per request, classifying failures as required-vs-optional
([:521-551](../crates/eggbench-cli/src/workload_registry.rs#L521)). Per-source construction is
cfg-gated inside `build_telemetry_collector` ([:556-579](../crates/eggbench-cli/src/workload_registry.rs#L556)):
the `prometheus-http` collector binds a target service and a runtime binding key
([:611-627](../crates/eggbench-cli/src/workload_registry.rs#L611)) and loads a workspace-pinned mapping,
and the `gregg` collector requires exactly one external named service and validates the endpoint
against loopback policy without dialing ([:710-765](../crates/eggbench-cli/src/workload_registry.rs#L710)).
`gregg_endpoint_config_error` is the same loopback check for `doctor`, which must never open a socket
([:767-804](../crates/eggbench-cli/src/workload_registry.rs#L767)).

## Invariants

1. **Production inventory is derived, never declared.** Both production constructors filter
   `DriverCategory::Workload` out of `eggbench_drivers::production_catalog()`
   ([:141-154](../crates/eggbench-cli/src/workload_registry.rs#L141), [:242-253](../crates/eggbench-cli/src/workload_registry.rs#L242)).
2. **Resolution is the gate; the factory is the second gate.** `run` resolves against the full
   catalog descriptor set ([run.rs:86](../crates/eggbench-cli/src/commands/run.rs#L86),
   [run.rs:114](../crates/eggbench-cli/src/commands/run.rs#L114)) and passes the name the resolver chose
   to the factory ([run.rs:119-123](../crates/eggbench-cli/src/commands/run.rs#L119)). The module header
   calls the second gate "defense in depth"
   ([run.rs:117-118](../crates/eggbench-cli/src/commands/run.rs#L117)).
3. **Nothing starts before construction.** Executor construction, external-tool probing, semantic
   replay preflight, and the diagnostic/correctness preflights all precede `run_impl`
   ([run.rs:126-277](../crates/eggbench-cli/src/commands/run.rs#L126)). See
   [drivers-external-substrate.md](drivers-external-substrate.md).
4. **Binary resolution is synchronous here; version probing is not.** The factory performs the
   `resolve()` call; version probes are `async` and live in `run`
   ([run.rs:147-165](../crates/eggbench-cli/src/commands/run.rs#L147)), which is why the executors
   self-probe again on first execution
   ([:391-393](../crates/eggbench-cli/src/workload_registry.rs#L391)).
5. **Only external workloads get the version probe, and `eggreplay-semantic` is carved out**
   ([run.rs:150-153](../crates/eggbench-cli/src/commands/run.rs#L150)) because it has its own preflight
   ([run.rs:167-191](../crates/eggbench-cli/src/commands/run.rs#L167)).
6. **The registry normalizes nothing.** The module contains no reference to `TrialMetrics` and no
   metrics writing; the only occurrence of "normalize" is prose in a warning message
   ([:537](../crates/eggbench-cli/src/workload_registry.rs#L537)). The driver's obligation is
   protocol-neutral raw observations (`WorkloadOutput` carries `RawMetricObservation` and
   `RawHistogramInput`, [orchestration.rs:243-262](../crates/eggbench-runner/src/orchestration.rs#L243));
   conversion to `metrics.json` happens later, in [core-metrics.md](core-metrics.md).
7. **Inventory order is stable.** `inventory()` sorts by name ([:298](../crates/eggbench-cli/src/workload_registry.rs#L298)),
   and the catalog sorts its own output the same way
   ([catalog.rs:73-77](../crates/eggbench-drivers/src/catalog.rs#L73)).
8. **No I/O beyond resolution, `current_dir`, and the pinned mapping read**
   ([:439](../crates/eggbench-cli/src/workload_registry.rs#L439), [:635](../crates/eggbench-cli/src/workload_registry.rs#L635),
   [:700](../crates/eggbench-cli/src/workload_registry.rs#L700)). No process is spawned from this module.

## Failure model

`production_workload_executor` returns `Result<Box<dyn WorkloadExecutor>, String>`; `run` maps any
`Err` to a failure with `ExitCode::CapabilityPreflight`
([run.rs:126-145](../crates/eggbench-cli/src/commands/run.rs#L126)). Error shapes:

| Condition | Message site | Presented category |
|---|---|---|
| External binary unresolved in the factory | [:404](../crates/eggbench-cli/src/workload_registry.rs#L404), [:411](../crates/eggbench-cli/src/workload_registry.rs#L411), [:418](../crates/eggbench-cli/src/workload_registry.rs#L418), [:425](../crates/eggbench-cli/src/workload_registry.rs#L425), [:442](../crates/eggbench-cli/src/workload_registry.rs#L442) | `unsupported_workload` when the plan has no `network_path` ([run.rs:139-141](../crates/eggbench-cli/src/commands/run.rs#L139)) |
| EggReplay without a resolved plan / wrong workload variant | [:431-438](../crates/eggbench-cli/src/workload_registry.rs#L431) | `unsupported_workload` |
| EggReplay workspace root unreadable | [:439-440](../crates/eggbench-cli/src/workload_registry.rs#L439) | `unsupported_workload` |
| `network_path` resolved without `eggstack-path` | [:500-501](../crates/eggbench-cli/src/workload_registry.rs#L500) | `unsupported_workload` |
| Path lowering refused | [:493](../crates/eggbench-cli/src/workload_registry.rs#L493) | route category, chosen by substring on the message ([run.rs:129-141](../crates/eggbench-cli/src/commands/run.rs#L129)) |
| No arm for the name | [:456-459](../crates/eggbench-cli/src/workload_registry.rs#L456), [:464-467](../crates/eggbench-cli/src/workload_registry.rs#L464) | `unsupported_workload` |
| Required telemetry unconstructible | [:572-577](../crates/eggbench-cli/src/workload_registry.rs#L572), [:543-544](../crates/eggbench-cli/src/workload_registry.rs#L543) | hard failure before startup |
| Optional telemetry unconstructible | [:546](../crates/eggbench-cli/src/workload_registry.rs#L546) | `telemetry_disabled` warning; metrics normalize as missing |
| Prometheus mapping digest/size/symlink violation | [:691-699](../crates/eggbench-cli/src/workload_registry.rs#L691) | mapping rejected before the collector exists |

Two observations a reviewer should hold onto:

- **`NoProductionAdapter` is not the refusal mechanism.** It is declared at
  [:97-102](../crates/eggbench-cli/src/workload_registry.rs#L97) and re-exported at
  [lib.rs:29](../crates/eggbench-cli/src/lib.rs#L29), but no code constructs it anywhere in the
  workspace. The production path refuses by returning a `String` error instead. Reading the type
  list will send you looking for a refusal path that does not exist.
- **A missing binary is labeled `unsupported_workload`, not `external_tool`,** because the factory
  fails first and `run` maps it there; the `external_tool` category belongs to the later probe stage
  ([run.rs:158-161](../crates/eggbench-cli/src/commands/run.rs#L158)), which only sees a tool that
  disappeared between construction and probing.

**Naming a feature-gated driver that is not compiled in** is a clean, explicit failure, not a panic
and not a silent no-op. `eggfetch-http` is only registered in the catalog under `eggstack-http`
([catalog.rs:51-52](../crates/eggbench-drivers/src/catalog.rs#L51)), so a feature-off build rejects the
name at resolution — `CliError::Resolution` in `run`
([run.rs:114-115](../crates/eggbench-cli/src/commands/run.rs#L114)), and the `missing_driver` category
documented for an unknown-but-well-formed `--workload-driver` in `doctor`
([doctor.rs:58-61](../crates/eggbench-cli/src/commands/doctor.rs#L58)). The factory's feature-off arm
([:461-468](../crates/eggbench-cli/src/workload_registry.rs#L461)) is never reached for that name; it
exists for names that resolved some other way.

## Boundaries

- **Delegates to `eggbench-drivers`:** descriptor registration, the catalog, the per-adapter types,
  the dialer lowering, the external binary substrate. Catalog ownership and its feature matrix are
  documented in [drivers.md](drivers.md) and [drivers-catalog-features.md](drivers-catalog-features.md);
  see also [drivers-eggstack-http.md](drivers-eggstack-http.md) and
  [drivers-network-path.md](drivers-network-path.md).
- **Delegates to `eggbench-runner`:** the `WorkloadExecutor` contract
  ([orchestration.rs:296-307](../crates/eggbench-runner/src/orchestration.rs#L296)), orchestration, and
  measurement boundaries ([runner-adapter-seams.md](runner-adapter-seams.md),
  [runner-orchestration.md](runner-orchestration.md)).
- **Delegates to `eggbench-core`:** descriptor and capability types, `resolve_plan`, the mapping
  digest primitive ([core-driver-resolution.md](core-driver-resolution.md),
  [core-metrics.md](core-metrics.md)).
- **Owns:** the name→adapter mapping, telemetry/service construction, doctor-time config
  validation, and the fake-injection seam.
- **Does not own:** normalization, the trial schedule, binary resolution policy, version probing,
  diagnostic and correctness factories.

Cargo feature surface (all forwarded to `eggbench-drivers`, `eggstack-path` implies
`eggstack-http` — [crates/eggbench-cli/Cargo.toml:15-20](../crates/eggbench-cli/Cargo.toml#L15)):

| Feature | Effect in this module |
|---|---|
| `eggstack-http` | EggServe service adapter ([:382](../crates/eggbench-cli/src/workload_registry.rs#L382)); `eggfetch-http` dispatch arm ([:451-460](../crates/eggbench-cli/src/workload_registry.rs#L451)); plain Eggfetch client ([:503](../crates/eggbench-cli/src/workload_registry.rs#L503)) |
| `eggstack-path` | Dialer composition in `build_eggfetch_executor` ([:478-497](../crates/eggbench-cli/src/workload_registry.rs#L478)) and the explicit refusal when a path resolved without it ([:500-501](../crates/eggbench-cli/src/workload_registry.rs#L500)) |
| `gregg` | Gregg collector ([:560-565](../crates/eggbench-cli/src/workload_registry.rs#L560), [:709-765](../crates/eggbench-cli/src/workload_registry.rs#L709)) and `gregg_endpoint_config_error` ([:775-798](../crates/eggbench-cli/src/workload_registry.rs#L775)) |
| `prometheus-http` | Prometheus collector and pinned-mapping loader ([:566-571](../crates/eggbench-cli/src/workload_registry.rs#L566), [:583-705](../crates/eggbench-cli/src/workload_registry.rs#L583)) |

## Tests and qualification

Eight unit tests plus one conditional, all in-module
([:806-1005](../crates/eggbench-cli/src/workload_registry.rs#L806)):

| Test | Line | Pins |
|---|---|---|
| `prometheus_mapping_requires_pinned_digest_and_rejects_symlink_escape` | [:868-908](../crates/eggbench-cli/src/workload_registry.rs#L868) | digest binding, `MAX_MAPPING_BYTES`, `content_tree_identity` rejecting a symlinked alias |
| `production_registry_contains_no_fake_driver` | [:910-924](../crates/eggbench-cli/src/workload_registry.rs#L910) | exact workload name set, no `fake-load`, `has_workload_driver()` unconditionally ([:922](../crates/eggbench-cli/src/workload_registry.rs#L922)) |
| `legacy_builtin_constructor_is_production_empty` | [:926-940](../crates/eggbench-cli/src/workload_registry.rs#L926) | `with_builtin` equals production and excludes `fake-load` |
| `qualification_registry_injects_fake_load` | [:942-948](../crates/eggbench-cli/src/workload_registry.rs#L942) | fake inventory is exactly one entry and is the default |
| `production_runtime_reports_no_driver` | [:950-974](../crates/eggbench-cli/src/workload_registry.rs#L950) | full descriptor set across all categories, with counts derived from the name list |
| `qualification_runtime_reports_fake_driver` | [:976-982](../crates/eggbench-cli/src/workload_registry.rs#L976) | qualification runtime is fake-only |
| `empty_registry_reports_no_driver` | [:984-989](../crates/eggbench-cli/src/workload_registry.rs#L984) | empty default state |
| `descriptor_serializes_into_driver_descriptor` | [:991-1004](../crates/eggbench-cli/src/workload_registry.rs#L991) | only name and category — not a round-trip test |

The two expectation lists ([:810-859](../crates/eggbench-cli/src/workload_registry.rs#L810)) are
deliberately hand-maintained rather than derived from the catalog, and the comment at
[:826-831](../crates/eggbench-cli/src/workload_registry.rs#L826) says why: a derived list could not
detect a registration change. Counts are taken from list lengths so totals cannot drift from names.
This is the only coupling between the catalog and the dispatch table.

**Not covered anywhere in the CLI crate:** `production_workload_executor` itself. No unit test and
no integration test calls it; `crates/eggbench-cli/tests/cli.rs` only exercises
`with_qualification_fake` and `WorkloadRegistry::production()`
([tests/cli.rs:241-254](../crates/eggbench-cli/tests/cli.rs#L241)). The dispatch table is therefore
verified only indirectly, by resolution + execution through `run`.

## Review focus

1. **Registration and dispatch are two hand-maintained lists that can drift.** The factory's doc
   comment states "Only catalog-registered drivers resolve"
   ([:387-390](../crates/eggbench-cli/src/workload_registry.rs#L387)). In code that is not what happens:
   `production_workload_executor` receives a `&Name` and compares it against five `*_DRIVER_NAME`
   constants plus one `#[cfg]`-gated arm
   ([:398-460](../crates/eggbench-cli/src/workload_registry.rs#L398)). It never calls
   `production_catalog()` or `DriverCatalog::workload`
   ([catalog.rs:81-85](../crates/eggbench-drivers/src/catalog.rs#L81)). Two independent lists define
   the reachable set: the catalog's registrations
   ([catalog.rs:49-67](../crates/eggbench-drivers/src/catalog.rs#L49)) and the arms at
   [:402-460](../crates/eggbench-cli/src/workload_registry.rs#L402). *Spelling* cannot drift — both sides
   use the same constants, e.g. [external/oha.rs:39](../crates/eggbench-drivers/src/external/oha.rs#L39),
   [external/eggsec_load.rs:34](../crates/eggbench-drivers/src/external/eggsec_load.rs#L34),
   [eggstack/mod.rs:42](../crates/eggbench-drivers/src/eggstack/mod.rs#L42), matching the catalog test's
   expected names ([catalog.rs:145-153](../crates/eggbench-drivers/src/catalog.rs#L145)). *Membership*
   can, in four directions:
   - **Catalogued but unreachable.** A new Workload-category descriptor appears in
     `ProductionRuntime::driver_descriptors`, so `doctor` reports it as available, and if it is the
     only `default: true` Workload driver the resolver can select it implicitly. `run` then fails at
     [:126-145](../crates/eggbench-cli/src/workload_registry.rs#L126) with `unsupported_workload` after
     resolution already succeeded — a `doctor` report and a blamed category that both misdescribe
     the actual cause (no adapter, not an unsupported workload).
   - **Dispatchable but unresolvable.** An arm for a name the catalog does not register as Workload
     is unreachable in practice, because `run` passes the *resolved* name
     ([run.rs:119-123](../crates/eggbench-cli/src/commands/run.rs#L119)) and resolution only emits names
     from the descriptor set it was handed. It fails safe, but it is untested dead code whose
     `resolve()` would run if ever reached.
   - **Category drift.** `eggprobe` (Diagnostic) and `eggsec-waf` (Correctness) are catalogued but
     are not workload-selectable ([catalog.rs:188-201](../crates/eggbench-drivers/src/catalog.rs#L188));
     the resolver looks only in `DriverCategory::Workload`
     ([run.rs:121](../crates/eggbench-cli/src/commands/run.rs#L121)). An arm added for either name is
     unreachable.
   - **A third list governs preflight.** `is_external_workload`
     ([external/preflight.rs:25-34](../crates/eggbench-drivers/src/external/preflight.rs#L25)) decides
     which names get the version-probe preflight ([run.rs:150-153](../crates/eggbench-cli/src/commands/run.rs#L150)).
     A new external workload present in the catalog and the dispatch table but absent here silently
     skips its probe, pushing a version failure from preflight into the trial. The
     `eggreplay-semantic` exclusion at [run.rs:152](../crates/eggbench-cli/src/commands/run.rs#L152) is
     already a one-off carve-out of that list.

   Why no test catches this: the dispatch table is a chain of `if`s with no introspection point —
   there is no `supported_workload_names()` a test could assert against — and the `#[cfg]` arms are
   invisible to a test compiled without the feature. The cheapest structural fix is a single
   function listing dispatchable names per feature matrix, asserted equal to the catalog's
   Workload-category names.

2. **`to_descriptor` is a lossy re-projection, and production correctly avoids it.** It drops
   `upstream_version`, replaces capabilities with one fabricated `LoadMode { mode: ClosedLoop }`, and
   hardcodes `external_process: false` and empty platform/schema/service-type sets
   ([:68-87](../crates/eggbench-cli/src/workload_registry.rs#L68)). Production paths use the catalog
   descriptors directly — `ProductionRuntime::driver_descriptors`
   ([:162-164](../crates/eggbench-cli/src/workload_registry.rs#L162)) feeds `run`
   ([run.rs:85-86](../crates/eggbench-cli/src/commands/run.rs#L85)) and `doctor`
   ([doctor.rs:45-51](../crates/eggbench-cli/src/commands/doctor.rs#L45)) — so the loss does not reach
   production. The lossy path is confined to `QualificationRuntime::driver_descriptors`
   ([:224-226](../crates/eggbench-cli/src/workload_registry.rs#L224)),
   `WorkloadRegistry::descriptors`
   ([:319-324](../crates/eggbench-cli/src/workload_registry.rs#L319)), and `run_with_registry`
   ([doctor.rs:85-93](../crates/eggbench-cli/src/commands/doctor.rs#L85)). Note that
   `production_registry_for_docs` ([doctor.rs:711-712](../crates/eggbench-cli/src/commands/doctor.rs#L711))
   returns the real production `WorkloadRegistry`; feeding that inventory through
   `run_with_registry` presents external oracles to the resolver as in-process drivers with a
   single fabricated capability.

3. **Module documentation is stale in three places, and the tests contradict two of them.** The
   header says the production registry is "empty without the `eggstack-http` feature"
   ([:5-8](../crates/eggbench-cli/src/workload_registry.rs#L5)) and `with_builtin` "Returns the
   production (empty) registry" ([:256-257](../crates/eggbench-cli/src/workload_registry.rs#L256)). Both
   are false: the catalog registers five Workload descriptors unconditionally
   ([catalog.rs:59-67](../crates/eggbench-drivers/src/catalog.rs#L59)) and the module's own tests assert
   `has_workload_driver()` without any `#[cfg]` guard
   ([:922](../crates/eggbench-cli/src/workload_registry.rs#L922),
   [:931](../crates/eggbench-cli/src/workload_registry.rs#L931)). The header's closing sentence —
   "Production adapters belong to External Oracles / Eggstack Integrations milestones"
   ([:14](../crates/eggbench-cli/src/workload_registry.rs#L14)) — predates the six arms now present.
   The header's other claim, that the qualification path is never used by `main.rs`
   ([:12](../crates/eggbench-cli/src/workload_registry.rs#L12)), does hold.

4. **The one `expect` in the module sits on a caller-controlled string.**
   `to_descriptor` does `Name::new(self.name.clone()).expect("static descriptor name")`
   ([:69](../crates/eggbench-cli/src/workload_registry.rs#L69)). For catalog-derived descriptors the
   name is a static literal, so it cannot fail; but `to_descriptor` is public and reachable from
   caller-built inventories (`run_with_registry`), where a name failing `Name`'s validation panics
   rather than returning an error. The same pattern at
   [:198](../crates/eggbench-cli/src/workload_registry.rs#L198) is genuinely static and safe.

5. **`default_workload`'s fallback contradicts its own documentation.** The doc says that without a
   default workload driver an explicit `--workload-driver` selection is required
   ([:129-131](../crates/eggbench-cli/src/workload_registry.rs#L129)), but the implementation falls back
   to the first entry in `BTreeMap` name order
   ([:304-309](../crates/eggbench-cli/src/workload_registry.rs#L304)) — on a feature-off build that is
   `eggsec-load`, not a "selection required" signal. The function currently has no production
   caller; only the in-file tests use it
   ([:921](../crates/eggbench-cli/src/workload_registry.rs#L921),
   [:933](../crates/eggbench-cli/src/workload_registry.rs#L933),
   [:946](../crates/eggbench-cli/src/workload_registry.rs#L946),
   [:988](../crates/eggbench-cli/src/workload_registry.rs#L988)), which the project already records as
   test-only surface ([plans/closure/external-oracles/002-status.md:115](../plans/closure/external-oracles/002-status.md#L115)).
   The first future production caller will get the silent fallback.

6. **Scope drift inside the module.** Telemetry construction
   ([:508-765](../crates/eggbench-cli/src/workload_registry.rs#L508)) and doctor-time Gregg validation
   ([:767-804](../crates/eggbench-cli/src/workload_registry.rs#L767)) sit in a file named
   `workload_registry`, while diagnostic and correctness factories sit in `run.rs`. Reviewers
   reading the file end to end will meet ~300 lines unrelated to workload dispatch, and the
   feature-gated failure modes for telemetry are as load-bearing as the workload ones.

## Related

- [overview.md](overview.md) — system shape; [cli-commands.md](cli-commands.md) — `run`/`doctor`
  flows that call into this module; [cli-surface.md](cli-surface.md) — argv, envelope, exit codes.
- [drivers.md](drivers.md) — crate-level ownership; its "Production catalog ownership and feature
  isolation" section describes the catalog this module consumes, with the per-feature registration
  matrix in [drivers-catalog-features.md](drivers-catalog-features.md).
- [drivers-external-substrate.md](drivers-external-substrate.md) — trusted binary resolution and the
  synchronous-resolve-before-startup ordering.
- [drivers-eggstack-http.md](drivers-eggstack-http.md), [drivers-network-path.md](drivers-network-path.md) —
  the Eggfetch/EggServe adapters and dialer lowering this factory composes.
- [runner-adapter-seams.md](runner-adapter-seams.md), [runner-orchestration.md](runner-orchestration.md) —
  the `WorkloadExecutor` contract and where constructed executors are consumed;
  [core-driver-resolution.md](core-driver-resolution.md) — the resolver that selects the name
  dispatched on here; [core-metrics.md](core-metrics.md) — the normalization drivers skip.
- [driver-capabilities.md](../docs/driver-capabilities.md) — per-driver and per-feature capability
  matrices; [trial-orchestration.md](../docs/trial-orchestration.md) — phase order and the
  measurement boundary around the constructed executor;
  [external-oracles.md](../docs/external-oracles.md) and
  [external-drivers.md](../docs/external-drivers.md) — the tool adapters behind the five
  unconditional arms.
