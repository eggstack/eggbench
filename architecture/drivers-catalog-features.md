# Driver catalog ownership and the cargo feature boundary

## Purpose

`eggbench-drivers` is the sole owner of production driver registration. This document covers the two
pieces that make that ownership enforceable rather than conventional:

1. **`DriverCatalog`** — the authoritative production driver inventory, assembled in
   `crates/eggbench-drivers/src/catalog.rs`, and
2. **the cargo feature boundary** — which optional upstream crates (`eggfetch-core`,
   `eggserve-*`, `eggress-*`, `eggchaos-core`, `gregg-protocol`) may be linked into a build, and
   which drivers consequently appear in the catalog.

The boundary pays for itself three ways: binary/link size, keeping heavy optional upstream
dependencies out of the default build, and a CI-enforced *proof* that the native path code is
genuinely unlinked rather than merely unreferenced — see
[Tests and qualification](#tests-and-qualification).

For the descriptor schema and resolution algorithm see
[core-driver-resolution.md](core-driver-resolution.md) and
[../docs/driver-capabilities.md](../docs/driver-capabilities.md). For the CLI-side projection of
the catalog and the production/qualification split see `cli-workload-registry.md` (written in
parallel; not duplicated here).

## Source layout

| Path | Role |
|---|---|
| `crates/eggbench-drivers/src/catalog.rs` | `DriverCatalog`, `production()`, the five accessors, `production_catalog()` |
| `crates/eggbench-drivers/src/lib.rs` | Crate surface; the `#[cfg]`-gated module tree and the re-export inventory |
| `crates/eggbench-drivers/build.rs` | Lockfile scraping; emits `EGGBENCH_*_VERSION` compile-time values |
| `crates/eggbench-drivers/Cargo.toml` | `[features]` table and the optional-dependency edges |
| `crates/eggbench-cli/Cargo.toml` | Pure feature forwarding to `eggbench-drivers` |
| `crates/eggbench-drivers/src/eggstack/mod.rs` | `eggserve-origin`, `eggfetch-http`, `eggbench-http-corpus` descriptors + origin registry |
| `crates/eggbench-drivers/src/eggstack/path/mod.rs` | `eggress-route`, `eggchaos-stream` descriptors, path version constants |
| `crates/eggbench-drivers/src/gregg/mod.rs`, `.../prometheus_http.rs` | the two Telemetry descriptors |
| `crates/eggbench-drivers/src/external/*.rs` | the seven unconditional external-process descriptors |
| `.github/workflows/ci.yml` | The "Check Eggstack feature isolation" step that enforces the boundary |

The module tree in `lib.rs` is itself the inventory: `catalog` is always compiled (`lib.rs:15`), and
every adapter module is behind a feature (`lib.rs:16-17` `eggstack`, `lib.rs:19-20` `gregg`,
`lib.rs:21-22` `prometheus_http`). `mod catalog` is private — only `DriverCatalog` and
`production_catalog` are re-exported (`lib.rs:24`) — while the `#[cfg]`-gated surfaces at
`lib.rs:25-34` and `lib.rs:35-42` keep a feature-off build self-describing.

## Key types

### `DriverCatalog`

`crates/eggbench-drivers/src/catalog.rs:25-27`

```rust
pub struct DriverCatalog { descriptors: Vec<DriverDescriptor> }
```

A private `Vec` of `eggbench_core::DriverDescriptor` values (`catalog.rs:16`). It holds **no live
objects**: no `Arc<dyn …>`, no process handles, no tool paths. It derives
`Debug + Default + Clone` (`catalog.rs:24`), so every call to `production()` re-allocates fresh
descriptor values. Two constructors produce the empty M001 state, `Default` via the derive and
`DriverCatalog::empty()` (`catalog.rs:32-36`).

`DriverDescriptor` lives in core, not here. The split is deliberate: **core owns the identity
record, drivers owns the implementation.** Core has no dependency on any adapter crate, so plan
resolution, evidence, and comparison can be reasoned about — and tested — with no Eggstack code
linked at all.

| Descriptor field | Contract that matters here |
|---|---|
| `name` | Canonical lookup key; a validated `Name`, never a free string |
| `category` | One `DriverCategory`; drives which accessor can find it |
| `adapter_version` | Eggbench's own `CARGO_PKG_VERSION` for native adapters (`eggstack/mod.rs:44`) |
| `upstream_name` / `upstream_version` | The concrete upstream crate/tool identity, `None` for external tools whose version is probed at runtime (`external/oha.rs:706-707`) |
| `capabilities` | Typed `BTreeSet<Capability>`; the only input to resolution |
| `external_process` | Whether a host binary is required |
| `default` | Unambiguous category-default claim used by deterministic selection |

### Feature states

| Cargo feature | Declared at | Optional deps it enables |
|---|---|---|
| *(default)* | `Cargo.toml:9` — `default = []` | none |
| `eggstack-http` | `Cargo.toml:10-15` | `eggfetch-core`, `eggserve-primitives`, `eggserve-server`, `hdrhistogram` |
| `gregg` | `Cargo.toml:16` | `eggfetch-core`, `gregg-protocol` |
| `prometheus-http` | `Cargo.toml:17` | `eggfetch-core` |
| `eggstack-path` | `Cargo.toml:18-24` | `eggstack-http` (feature), `eggress-outbound`, `eggress-uri`, `eggress-core`, `eggchaos-core` |

The CLI forwards each one verbatim (`crates/eggbench-cli/Cargo.toml:15-20`) and adds nothing.
`eggstack-path` forwards to the CLI's *own* `eggstack-http` feature as well
(`crates/eggbench-cli/Cargo.toml:20`), which is load-bearing: the CLI's own `#[cfg]` arms — for
instance `workload_registry.rs:451` vs `workload_registry.rs:461` — key off the CLI feature, not the
drivers feature.

## How it works

### Assembly

`DriverCatalog::production()` (`catalog.rs:49-69`) is a straight-line builder. There is no registry
insert, no duplicate check, and no I/O: feature-gated descriptor groups are `extend`ed, the
unconditional external group is `extend`ed, and the result is returned.

| Order | Source | Gated by |
|---|---|---|
| `catalog.rs:51-52` | `crate::eggstack::eggstack_descriptors()` | `eggstack-http` |
| `catalog.rs:53-54` | `crate::eggstack::path::path_descriptors()` | `eggstack-path` |
| `catalog.rs:55-56` | `crate::gregg::gregg_telemetry_descriptor()` | `gregg` |
| `catalog.rs:57-58` | `crate::prometheus_http::prometheus_http_descriptor()` | `prometheus-http` |
| `catalog.rs:59-67` | seven `crate::external::*_descriptor()` calls | *(unconditional)* |

`production_catalog()` (`catalog.rs:134-136`) is a free-function alias over `production()`; the CLI
calls the free function (`workload_registry.rs:142`, `workload_registry.rs:243`,
`commands/doctor.rs:499`).

### Unconditional group

These seven register in every build, features or not. A missing host binary is a runtime capability
error, never a build-configuration question (`catalog.rs:42-44`).

| Name | Category | `external_process` | `default` | Descriptor |
|---|---|---|---|---|
| `oha` | Workload | true | false | `external/oha.rs:686` |
| `h2load` | Workload | true | false | `external/h2load.rs:612` |
| `iperf3` | Workload | true | false | `external/iperf3.rs:472` |
| `eggreplay-semantic` | Workload | true | false | `external/eggreplay.rs:861` |
| `eggsec-load` | Workload | true | false | `external/eggsec_load.rs:826` |
| `eggprobe` | Diagnostic | true | false | `external/eggprobe.rs:889` |
| `eggsec-waf` | Correctness | true | false | `external/eggsec.rs:1004` |

The four `oha`/`h2load`/`iperf3`/`eggreplay-semantic` oracles are never `default`. That is the
mechanism that keeps selection deterministic once `eggstack-http` links a native Workload default:
the native driver owns the category default, the oracles are explicit opt-ins, and
`docs/driver-capabilities.md` never has to break an ambiguity tie.

### Feature-gated group

| Feature | Name | Category | `default` | `upstream` | Descriptor |
|---|---|---|---|---|---|
| `eggstack-http` | `eggfetch-http` | Workload | true | `eggfetch-core` @ lockfile | `eggstack/mod.rs:88` |
| `eggstack-http` | `eggserve-origin` | Service | true | `eggserve-server` @ lockfile | `eggstack/mod.rs:59` |
| `eggstack-http` | `eggbench-http-corpus` | Correctness | false | `eggfetch-core` @ lockfile | `eggstack/mod.rs:134` |
| `eggstack-path` | `eggress-route` | Route | true | `eggress-outbound` @ lockfile | `path/mod.rs:69` |
| `eggstack-path` | `eggchaos-stream` | Fault | true | `eggchaos-core` @ lockfile | `path/mod.rs:87` |
| `gregg` | `gregg` | Telemetry | true | `gregg-protocol` @ lockfile | `gregg/mod.rs:55` |
| `prometheus-http` | `prometheus-http` | Telemetry | `!gregg` | `prometheus-text-exposition` `bounded-v1` | `prometheus_http.rs:1066` |

Two details are easy to miss. `eggfetch-http` gains `Capability::NetworkPath` only when
`eggstack-path` is on (`eggstack/mod.rs:97-98`), so the capability set of a descriptor is itself
feature-dependent — the same feature state is encoded twice, in the feature and in the capability
set, which is what makes the matrix in `../docs/driver-capabilities.md` enforceable. And the two
telemetry descriptors resolve their single-default slot by feature interaction, not by ordering:
`default: !cfg!(feature = "gregg")` (`prometheus_http.rs:1077`) yields exactly one Telemetry default
in every build where both features are enabled.

### Exact registration per feature state

Counts derived from `catalog.rs:51-67`, and pinned by the in-crate test at `catalog.rs:143-202`.

| Build (`--no-default-features` plus) | `eggfetch-http` | `eggserve-origin` | `eggbench-http-corpus` | `eggress-route` | `eggchaos-stream` | `gregg` | `prometheus-http` | Unconditional | Total |
|---|---|---|---|---|---|---|---|---|---|
| *(none)* | – | – | – | – | – | – | – | 7 | **7** |
| `eggstack-http` | ✓ | ✓ | ✓ | – | – | – | – | 7 | **10** |
| `eggstack-path` | ✓ | ✓ | ✓ | ✓ | ✓ | – | – | 7 | **12** |
| `gregg` | – | – | – | – | – | ✓ | – | 7 | **8** |
| `prometheus-http` | – | – | – | – | – | – | ✓ | 7 | **8** |
| `eggstack-path,gregg` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | – | 7 | **13** |
| *all features* | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 7 | **14** |

Note that `gregg` pulls `dep:eggfetch-core` (`Cargo.toml:17`) without implying `eggstack-http`, so a
`gregg`-only build links Eggfetch yet registers no Eggfetch *driver*. `eggstack-path` implies
`eggstack-http` (`Cargo.toml:20`) because the path dialer implements `eggfetch_core::Dialer`
(`path/mod.rs:5-6`) and cannot exist outside an Eggfetch client.

### Identity and dispatch

Lookup is a linear scan over the descriptor vector filtered by category *and* name:

```rust
self.descriptors.iter().find(|d| d.category == DriverCategory::Workload && d.name == *name)
```

(`catalog.rs:81-85`; the other four accessors are the same shape at `catalog.rs:89-93`,
`catalog.rs:97-101`, `catalog.rs:105-109`, `catalog.rs:113-117`.) `descriptors()`
(`catalog.rs:73-77`) clones and sorts by name, giving presentation a stable order without imposing
one on the stored vector.

A descriptor is **not** a handle to an implementation. It is a capability claim and an identity
record. Obtaining the concrete object is a separate, name-keyed dispatch step owned by the consumer:

| Category | Where the implementation is constructed | Mechanism |
|---|---|---|
| Workload | `workload_registry.rs:398-469` | `production_workload_executor(&Name, Option<&ResolvedPlan>) -> Result<Box<dyn WorkloadExecutor>, String>`; a chain of `driver.as_str() == …_DRIVER_NAME` comparisons returning `Box::new(Concrete::from_resolved(executable))` |
| Service | `eggstack/mod.rs:162-168` | `eggstack_service_adapters() -> ServiceAdapterRegistry` holding `Arc::new(EggServeOriginAdapter::new())` |
| Telemetry | `workload_registry.rs:521-533` | Per-request `build_telemetry_collector` over `resolved.telemetry`, registered into `TelemetryRegistry` |
| Diagnostic / Correctness | *(preflight-side)* | invoked through `preflight_eggsec`, `handshake_eggprobe`, `parse_waf_stdout` rather than a trait object |

`production_workload_executor` never consults the catalog. Its doc comment states "Only
catalog-registered drivers resolve" (`workload_registry.rs:390`), but the body is a pure name match,
so "registered in `production()`" and "has a constructor" are two hand-maintained lists that agree
by convention. The failure is a late one: an unmatched name yields
`"no production executor for workload driver {name}"` (`workload_registry.rs:456-459`) — after plan
resolution has already accepted the descriptor.

The deliberate asymmetry follows from this: a missing binary and a missing registration are
different failures with different timing. Resolution is I/O-free and sees only the catalog; binary
presence is discovered at preflight or executor construction.

### Version provenance

`crates/eggbench-drivers/build.rs` scrapes the workspace `Cargo.lock` at build time. The target
table is `build.rs:13-24`; the crate walks `CARGO_MANIFEST_DIR` ancestors for a `Cargo.lock`
(`build.rs:27-31`), parses the first `[[package]]` stanza whose `name` matches
(`build.rs:63-82`), and emits `cargo:rustc-env={var}={version}` (`build.rs:50`) with
`cargo:rerun-if-changed` on the lockfile (`build.rs:39`). Unavailable input yields the literal
`"unknown"` rather than a build failure (`build.rs:35`, and the stated policy at `build.rs:6-7`).

| Constant | Env var | Consumed at | Reaches |
|---|---|---|---|
| `EGGFETCH_CORE_VERSION` | `EGGBENCH_EGGFETCH_CORE_VERSION` | `eggstack/mod.rs:48` | descriptors `eggstack/mod.rs:110`, `143`; evidence `fetch.rs:380`, `1272`, `http_corpus.rs:174` |
| `EGGSERVE_SERVER_VERSION` | `EGGBENCH_EGGSERVE_SERVER_VERSION` | `eggstack/mod.rs:50` | descriptor `eggstack/mod.rs:68`; error text `origin.rs:102` |
| `EGGSERVE_PRIMITIVES_VERSION` | `EGGBENCH_EGGSERVE_PRIMITIVES_VERSION` | `eggstack/mod.rs:52` | re-export `lib.rs:38` |
| `GREGG_PROTOCOL_VERSION` | `EGGBENCH_GREGG_PROTOCOL_VERSION` | `gregg/mod.rs:42` | descriptor `gregg/mod.rs:66` |
| `EGGRESS_OUTBOUND_VERSION` | `EGGBENCH_EGGRESS_OUTBOUND_VERSION` | `path/mod.rs:39` | descriptor `path/mod.rs:74`; evidence `path/evidence.rs:407` |
| `EGGRESS_URI_VERSION` | `EGGBENCH_EGGRESS_URI_VERSION` | `path/mod.rs:41` | evidence `path/evidence.rs:408` |
| `EGGCHAOS_CORE_VERSION` | `EGGBENCH_EGGCHAOS_CORE_VERSION` | `path/mod.rs:43` | descriptor `path/mod.rs:92`; evidence |

This exists because a driver upgrade changes behavior without changing any wire schema. Two
snapshots of the same `plan.json` resolved either side of a lockfile bump are different experiments,
and only the concrete `upstream_version` distinguishes them. The mechanism goes one step further at
lowering time: the path dialer rejects a descriptor whose recorded `upstream_version` no longer
equals the compiled constant (`path/dialer.rs:94`, `path/dialer.rs:114`), so evidence cannot be
produced by a descriptor that claims a different upstream than the one actually linked.

## Invariants

- **The catalog is pure data.** `production()` performs no I/O, spawns nothing, and cannot fail.
  Binary availability is discovered later, in `external_binary_present`
  (`external/preflight.rs:41-52`, filesystem-only, for `doctor`) or in `*Workload::resolve()` during
  executor construction (`workload_registry.rs:404`, `workload_registry.rs:411`,
  `workload_registry.rs:418`, `workload_registry.rs:425`, `workload_registry.rs:442`).
- **One default per populated category.** External oracles are `default: false`;
  `eggfetch-http` (`eggstack/mod.rs:116`), `eggserve-origin` (`eggstack/mod.rs:74`),
  `eggress-route` (`path/mod.rs:80`), `eggchaos-stream` (`path/mod.rs:98`) and `gregg`
  (`gregg/mod.rs:72`) each claim a distinct category, and the Telemetry slot is arbitrated at
  compile time by `prometheus_http.rs:1077`.
- **Order is a presentation concern.** Stored order follows the builder; `descriptors()` sorts
  (`catalog.rs:75`) and `path_descriptors()` sorts (`path/mod.rs:107`). `len()`/`is_empty()` report
  the stored vector, not the sorted clone (`catalog.rs:121-129`).
- **External-process drivers register unconditionally**, so a feature-off build is still truthful
  about what it can attempt; it fails at preflight, not at startup.
- **Descriptor constructors cannot fail.** Every `Name::new(...)` is over a string literal with an
  `expect` (`eggstack/mod.rs:65`, `path/mod.rs:71`, `gregg/mod.rs:63`, `prometheus_http.rs:1068`), so
  the panics are unreachable by construction. `#![forbid(unsafe_code)]` holds crate-wide
  (`lib.rs:13`).

## Failure model

| Condition | Detected where | Surface |
|---|---|---|
| Driver name not in catalog | `workload()`/`service()`/… return `None` (`catalog.rs:81-117`); resolution errors first | plan-resolution error before startup |
| Registered, but feature-off | `workload_registry.rs:461-468` returns `"no production executor for workload driver …"` | `missing_driver` / `unsupported_workload` category |
| `network_path` resolved without `eggstack-path` | `workload_registry.rs:500-501` returns an explicit reason; also rejected at resolution because `eggfetch-http` lacks `Capability::NetworkPath` | preflight, before environment collection or bundle preparation |
| Host binary missing | `external_binary_present` → `false` for `doctor`; `resolve()` error at executor construction | before managed startup |
| Tool version below floor | bounded version probe in `run` preflight, plus self-probe on first execution (`external/preflight.rs:4-8`) | preflight |
| Lockfile entry absent | `build.rs:35` substitutes `"unknown"` | descriptor/evidence records `"unknown"`; does not fail the build |

There is no error path *out of* the catalog. Every accessor returns `Option` or a value, so
"unknown driver" is expressed as absence, never as an error value. This is why the catalog cannot be
the place where a missing tool is reported.

## Boundaries

### Why the feature split exists

- **Link surface and binary size.** Every Eggstack dependency is `optional = true`
  (`Cargo.toml:37-50`) and `default = []` (`Cargo.toml:9`), so a default build links only the
  external-command substrate and third-party measurement tools.
- **The optional upstream crates are genuinely optional.** `eggress-*` and `eggchaos-core` are
  pinned with `=` requirements (`Cargo.toml:47-50`), so isolation is not a range-resolution
  accident: an accidental `eggstack-path` would resolve exact, visible versions.
- **The isolation is testable.** Because the boundary is a cargo feature rather than runtime
  configuration, it can be asserted with `cargo tree`.
- **`eggstack-path` implies `eggstack-http`** (`Cargo.toml:20`) structurally: the path dialer is an
  `eggfetch_core::Dialer` implementation (`path/mod.rs:5-6`) and there is no standalone path driver
  to enable on its own.
- **`gregg` is orthogonal.** It adds exactly one Telemetry descriptor (`catalog.rs:55-56`), does not
  imply `eggstack-http`, and its only cross-feature effect is the Telemetry default arbitration
  (`prometheus_http.rs:1077`).
- **`eggress-embed` is a dev-dependency only** (`Cargo.toml:52-55`), so the Eggress embed surface can
  never reach a production link.

### What the catalog does not own

- **Presentation and selection policy.** The CLI projects descriptors into its inventory view
  (`workload_registry.rs:49-64`) and filters to `DriverCategory::Workload` for the workload registry
  (`workload_registry.rs:144-149`, `workload_registry.rs:245-251`).
- **Resolution.** `resolve_plan` consumes descriptors as input; it does not know the catalog exists.
- **The qualification fake.** `DriverCatalog` has no test-only constructor and no fake-registration
  path. The fake lives on the runner side (`eggbench_runner::test_support::FakeWorkload`,
  `workload_registry.rs:17`) and is injected explicitly by test harnesses through
  `with_qualification_fake()` (`workload_registry.rs:271-282`) or `QualificationRuntime`
  (`workload_registry.rs:176`). `main.rs` never selects it (`workload_registry.rs:12`,
  `workload_registry.rs:173`). The production/qualification split is covered in
  `cli-workload-registry.md` and only summarized here.

## Tests and qualification

### Test-as-architecture-boundary

The "Check Eggstack feature isolation" step (`.github/workflows/ci.yml:24-37`) is the enforcement
mechanism for everything in this document. It is worth reading as a specification of the boundary,
not as build hygiene.

| Check | `ci.yml` | Asserts |
|---|---|---|
| `cargo check -p eggbench-drivers --no-default-features` | 28 | the 7-driver catalog compiles with no optional upstream crate linked |
| `--features eggstack-http` | 29 | Eggfetch/EggServe/hdrhistogram link; path still absent |
| `--features gregg` | 30 | Gregg is orthogonal: no path, no EggServe |
| `--features eggstack-path` | 31 | the implying edge works standalone |
| `--features eggstack-path,gregg` | 32 | the two orthogonal axes compose |
| `--features prometheus-http` | 33 | the third axis compiles |
| `cargo check -p eggbench-cli --no-default-features --features eggstack-path` | 34 | CLI forwarding features resolve end-to-end |
| `cargo tree --no-default-features --edges normal \| grep -E 'eggress\|eggchaos'` must find nothing | 35 | **default build does not link the path stack** |
| same, `--features eggstack-http` | 36 | **`eggstack-http` alone does not link the path stack** |
| `cargo run -p eggbench-cli --features eggstack-path -- validate examples/eggstack-path.json` | 37 | a real path plan is schema-valid in a path build, without running traffic |

The two `cargo tree` assertions are the load-bearing ones. A compile-only matrix cannot
distinguish "Eggress is not linked" from "Eggress is linked but unreferenced", so the negative
dependency-tree assertion is what makes the isolation a property rather than a convention. The
`--edges normal` flag excludes dev-dependencies, which is what keeps the `eggress-embed` dev edge
(`Cargo.toml:52-55`) from failing the grep.

Smoke and unit coverage around the catalog:

- `production_catalog_matches_feature` (`catalog.rs:143-202`) rebuilds the expected name set under
  the same `cfg` gates as the production builder (`catalog.rs:154-165`) and compares
  (`catalog.rs:173`). It also pins per-descriptor flags: Gregg default and in-process
  (`catalog.rs:175-181`), oracles external and non-default (`catalog.rs:182-187`), `eggprobe`
  Diagnostic-only (`catalog.rs:188-192`), `eggsec-waf` Correctness-only (`catalog.rs:193-201`).
- `empty_catalog_lookups_return_none` (`catalog.rs:205-214`) exercises all five accessors.
- `crates/eggbench-drivers/tests/oracles.rs:202-204` pins `is_external_workload` membership and
  asserts an in-process driver is not external.
- `crates/eggbench-drivers/tests/eggstack.rs:344-346` asserts the lockfile extraction produced
  non-`"unknown"` values — the qualification check on the provenance mechanism itself.
- `ci.yml:38` runs clippy with `--all-features -D warnings`, and `ci.yml:39` runs the full
  `--all-features` test suite; `ci.yml:40` runs the CLI integration test with
  `--no-default-features`; `ci.yml:93-94` runs `--all-features --test eggstack_path` on Windows.

The `is_external_workload` predicate (`external/preflight.rs:25-34`) is the runtime complement of
the unconditional group: it is a five-name match over `OHA`, `H2LOAD`, `IPERF3`, `EGGREPLAY`, and
`EGGSEC_LOAD`. Those five are exactly the Workload-category members of the unconditional set;
`eggprobe` and `eggsec-waf` are external-process but are *not* workloads, so they are covered by
the sibling predicate `external_binary_present` instead. The five are catalogued unconditionally
because each is an independent measurement oracle: an in-process adapter measuring itself with its
own transport cannot corroborate that adapter.

## Review focus

- **`Route` and `Fault` have no accessor.** `DriverCategory` declares seven variants
  (`crates/eggbench-core/src/resolved.rs:27-47`), but `DriverCatalog` exposes only five accessors —
  `workload` (`catalog.rs:81`), `service` (`catalog.rs:89`), `telemetry` (`catalog.rs:97`),
  `diagnostic` (`catalog.rs:105`), `correctness` (`catalog.rs:113`) — and the impl block ends at
  `catalog.rs:130` with no route or fault lookup. The catalog nevertheless registers `eggress-route`
  as `DriverCategory::Route` (`path/mod.rs:75`, via `catalog.rs:53-54`) and `eggchaos-stream` as
  `DriverCategory::Fault` (`path/mod.rs:93`). Both are therefore unreachable through the
  category-typed API: a caller must scan `descriptors()` (`catalog.rs:73`) and filter by hand, as
  `commands/doctor.rs:499-500` does for the http-corpus correctness driver. There is also no
  name-only `get(&Name)`, which is what makes this an ergonomic break rather than a cosmetic one.
  `DriverCategory::ExecutionProvider` (`resolved.rs:44`) is likewise unhandled, though no driver is
  registered for it, so it is latent rather than live.
- **The module doc under-reports the registration set.** `catalog.rs:3-12` lists the unconditional
  group as `oha`, `h2load`, `iperf3`, `eggreplay-semantic`, `eggprobe`, `eggsec-waf` — omitting
  `eggsec-load` (`catalog.rs:66`) — and never mentions the `eggstack-path` pair (`catalog.rs:53-54`)
  or `prometheus-http` (`catalog.rs:57-58`). The `DriverCatalog` doc at `catalog.rs:20-23` likewise
  omits the `eggbench-http-corpus` correctness descriptor registered at `eggstack/mod.rs:127`.
- **No duplicate-name defence.** `production()` (`catalog.rs:50-68`) performs no uniqueness check and
  the accessors use `find` (`catalog.rs:84`), which returns the first match. Two features registering
  the same name would yield a catalog that silently answers with the first entry. The current matrix
  has no overlap, so this is a guard that does not exist yet.
- **The dispatch/registration duplication is the sharpest fragility.** `production()`'s external list
  (`catalog.rs:59-67`) and `production_workload_executor`'s name arms
  (`workload_registry.rs:402-450`) are two hand-maintained lists of the same fact, and
  `workload_registry.rs:390` claims an invariant the code does not enforce.
- **`lock_version` returns the first matching stanza.** `build.rs:78-79` returns as soon as it sees a
  `version = "…"` after a matching `name` line. If the lockfile ever carried two versions of a
  scraped crate, the reported provenance could be the version this crate does not link. The `=` pins
  (`Cargo.toml:47-50`) make this unreachable today for the four Eggress/Eggchaos crates;
  `eggfetch-core`, `eggserve-*`, and `gregg-protocol` use caret requirements (`Cargo.toml:37-43`).
- **Prometheus provenance contradicts its own descriptor.** `prometheus_http_descriptor` reports
  `upstream_name: "prometheus-text-exposition"` / `upstream_version: Some("bounded-v1")`
  (`prometheus_http.rs:1070-1071`) — hand-written, not lockfile-derived — while the same driver's
  evidence stamps `"transport_version": env!("EGGBENCH_EGGFETCH_CORE_VERSION")`
  (`prometheus_http.rs:451`). `hdrhistogram` has no entry in the target table (`build.rs:13-24`).
- **`is_external_workload` and `external_binary_present` have different scopes.** Five names
  (`external/preflight.rs:25-34`) versus seven (`external/preflight.rs:41-52`). A caller using the
  former to ask "does this driver need a binary?" gets `false` for `eggprobe` and `eggsec-waf`.
  Current call sites (`commands/run.rs:102`, `commands/run.rs:151`, `commands/run.rs:570`,
  `commands/doctor.rs:152`, `commands/doctor.rs:188`) are consistent with the narrow reading.
- **`Default` and `empty()` overlap.** `catalog.rs:24` derives `Default`; `catalog.rs:32-36` provides
  an identical constructor. Two spellings of the M001 empty state.
- **The default-build inventory is compiled but never asserted.** `ci.yml:39` runs the catalog test
  under `--all-features`, so the seven-name default expectation (`catalog.rs:145-153`) never executes;
  `ci.yml:28` only checks that it compiles.

## Related

- [drivers.md](drivers.md) — the driver layer's position in the system; its "Production catalog
  ownership and feature isolation" section is the same boundary at summary altitude.
- [overview.md](overview.md) — system shape and the workspace feature matrix.
- [core-driver-resolution.md](core-driver-resolution.md) — how the catalog's descriptors become a
  resolved, frozen selection.
- [runner-adapter-seams.md](runner-adapter-seams.md) — the trait objects that the name-keyed
  dispatch in the CLI constructs.
- `cli-workload-registry.md` — the production/qualification split and the CLI projection of the
  catalog.
- [../docs/driver-capabilities.md](../docs/driver-capabilities.md) — capability semantics, default
  selection, and the network-path capability matrix.
- [../docs/eggstack-http.md](../docs/eggstack-http.md) — the two-layer Eggstack path, its feature
  boundary, and the Eggress/Eggchaos semantics behind the `eggress-route` and `eggchaos-stream`
  descriptors.
