# Driver capability contracts and deterministic plan resolution

## Purpose

`crates/eggbench-core/src/resolved.rs` is the seam between a symbolic,
human-authored experiment plan and a runnable experiment. It owns three things
and nothing else:

1. The **declarative** vocabulary a driver must use to describe itself
   (`DriverDescriptor`, `DriverCategory`, `Capability`) — pure data, no
   behavior, no trait, no runtime object.
2. One **side-effect-free** function, `resolve_plan`, mapping (validated plan,
   in-memory descriptor list, explicit caller options) to a `ResolvedPlan` or
   to a fail-closed `ResolveError`.
3. The **frozen provenance** snapshot — the exact descriptor, adapter version,
   upstream version, and caller-supplied executable path of every selected
   driver, copied into evidence.

The rule the module encodes: a driver upgrade may change behavior *without* a
wire-schema change, so the schema cannot carry behavior. It carries identity.
Everything a reader needs to know *which software produced these numbers* is a
data field; everything that *runs* is constructed later, outside core, from that
data. See [core.md](core.md) for why core owns no process, runtime, or network
client, and [drivers.md](drivers.md) for the adapter-side half of the same
boundary.

## Source layout

| Region | Lines | Contents |
|---|---|---|
| Version constants | 11–22 | Current version 6, plus readable v1–v5 |
| `DriverCategory`, `HttpVersion`, `Capability` | 24–115 | Eight roles; 13 typed capability values |
| `DriverDescriptor` | 118–146 | Identity, category, capabilities, defaults |
| `DefaultDriverPolicy`, `ResolutionOptions` | 149–175 | Caller policy and inputs |
| `ResolvedDriver`, `ResolutionWarning` | 178–198 | Per-category selection; non-fatal diagnostics |
| `ResolvedPlan` | 201–253 | The frozen evidence contract |
| Path / paired / defaults DTOs | 255–333 | `ResolvedNetworkPath`, `ResolvedStreamFaults`, `ResolvedPairedDesign`, `ResolvedDefaults` |
| `ResolveError` | 336–395 | Nine fail-closed outcomes |
| `resolve_plan` | 397–760 | The single validation pass |
| `select_driver` | 762–824 | Explicit selection, then deterministic defaulting |
| `validate_driver` | 826–858 | Platform, capability, external-binary coherence |
| `validate_correctness_…`, `validate_network_path_…` | 885–1041 | Canonical M004a / M002 driver contracts |
| `workload_target`, `workload_mode` | 860–869, 1043–1057 | Workload → target service / load mode |
| Tests | 1059–1457 | 11 unit tests over the CLI's own fixtures |

Two serde decisions shape compatibility. `deny_unknown_fields` sits on
`DriverDescriptor` (119) and on every resolved DTO including `ResolvedPlan`
(202), so an unknown field is a decode error, not a warning, and a schema
change needs a version bump plus a compatibility decision. `Capability` is
`tag = "kind"` with `deny_unknown_fields` (65), so an unrecognized capability
*variant* is also a hard decode failure.

## Key types

### Categories

`DriverCategory` (24–47) is not a taxonomy of implementations; it is a set of
**independent resolution axes**. Each has its own slot in
`ResolvedPlan.drivers` (`BTreeMap<DriverCategory, ResolvedDriver>`, 211), and
selection is per category, so resolving a workload never inspects telemetry.

| Variant | Line | Role |
|---|---|---|
| `Service` | 29 | Subject/service lifecycle |
| `Workload` | 31 | Load generation |
| `Telemetry` | 33 | Host or service telemetry |
| `Fault` | 35 | Fault injection |
| `Diagnostic` | 37 | Diagnostic collection |
| `Correctness` | 42 | Security-correctness vs a predeclared expectation |
| `ExecutionProvider` | 44 | Local or future remote execution provider |
| `Route` | 46 | Listener-free Eggress TCP route driver |

`Correctness` is deliberately distinct from `Workload` and `Diagnostic`
(38–42): a correctness check answers "does the behavior meet the declared
expectation", not "what was the load" or "is the environment healthy".
Conflating them would let a load oracle satisfy a correctness gate. `Route` is
likewise a separate axis (45–46) so a path is not modeled as a workload or
service alias. `ExecutionProvider` (43–44) is declared with no resolution logic
yet.

### Capabilities

`Capability` (63–115) is the machine-checkable part of a descriptor.

| Variant | Line | Operational meaning at resolution time |
|---|---|---|
| `HttpVersion { version }` | 68 | Acceptable protocol; reachable only via caller `required_capabilities`, never derived from the plan |
| `LoadMode { mode }` | 73 | Load model the driver can generate; **derived from the plan** for every non-replay, non-corpus workload (438–446) |
| `CorrectedLatency` | 78 | Corrects coordinated omission in latency reporting |
| `ProxyRouting` | 80 | Can select an explicit proxy/route; required of `Route` when `network_path` is requested (454–457) |
| `FaultFamily { family }` | 82 | Supports a named fault family |
| `TelemetryField { field }` | 87 | Emits one named field; drives the per-request field check (622–633) |
| `ExternalBinary` | 92 | Uses an external binary; **implied by** `external_process` (847–856) |
| `NetworkPath` | 94 | Owns a custom HTTP dialer; required of the workload for a path (450–453) |
| `StreamFaultPlan` | 97 | Produces a static deterministic stream-fault plan; required of `Fault` only when faults are requested (458–468) |
| `SemanticReplay` | 101 | Executes one immutable `EggReplay` fixture as one trial; substitutes for a load-mode requirement (425–430) |
| `HttpCorpus` | 103 | Executes a deterministic HTTP security-corpus schedule |
| `DiagnosticProbe { probe }` | 105 | Executes one diagnostic probe family; one requirement per requested family (483–499) |
| `SecurityCheck { family }` | 111 | Executes one correctness family; `waf_bypass` initially (500–514) |

`Capability`, `DriverCategory`, `LoadMode`, and `HttpVersion` all derive
`PartialOrd + Ord`, and capabilities live in a `BTreeSet` (132). Ordering is
therefore canonical, which is what makes the serialized snapshot byte-stable
and `AmbiguousSelection.candidates` a stable list (816).

### Descriptors and options

`DriverDescriptor` (118–146) is what a driver publishes about itself: canonical
`name`, `adapter_version`, `upstream_name`, optional `upstream_version`, one
`category`, `capabilities`, `supported_platforms`, optional
`machine_output_schema`, `external_process`, `default`, and
`compatible_service_types`. Two defaults are load-bearing: an **empty
`supported_platforms` means portable** (133, enforced at 831–833 by checking
only when non-empty) and an **empty `compatible_service_types` means no
restriction** (143, enforced at 675–677). "Unset" is permissive, not
restrictive.

`ResolutionOptions` (159–175) is the caller's entire input surface: explicit
`selections` per category, `default_policy`, the current `platform` label,
`executable_paths` keyed by driver name, and extra `required_capabilities` per
category. The doc comment states the rule directly: "paths are supplied by the
caller, never discovered here" (158).

`ResolvedDriver` (178–185) is a full descriptor **clone** plus an optional
non-secret `executable_path`. Cloning the descriptor rather than referencing
it by name is what lets a snapshot stand alone after the driver is upgraded.

## How it works

`resolve_plan` (407–760) is one pass, annotated as such at 406.

**1. Validate the plan first** (412). Everything downstream assumes a plan that
already satisfies its own schema and semantic rules — including constraints
resolution never re-checks, such as the paired-design / `network_path`
incompatibility, which lives in plan validation.

**2. Build the registry, reject duplicate identities** (413–421). Descriptors
go into a `BTreeMap<Name, &DriverDescriptor>`; a second descriptor with the
same name is `DuplicateDriver`. Identity is the name alone — not
name+category, not name+upstream. Two descriptors claiming one name is an
authoring error, not a resolution choice.

**3. Derive required capabilities per category** (423–520). This is the
contract between plan semantics and driver vocabulary:

| Plan signal | Required capability | Line |
|---|---|---|
| `Workload::SemanticReplay` | `SemanticReplay`, replacing the load-mode requirement | 425–430 |
| `Workload::HttpCorpus` | `HttpCorpus` **and** an explicit `LoadMode::ClosedLoop` | 431–437 |
| Any other workload | the `LoadMode` its variant implies (`workload_mode`) | 438–446 |
| `network_path` present | `Workload: NetworkPath`, `Route: ProxyRouting` | 450–457 |
| …with `stream_faults` | `Fault: StreamFaultPlan` | 458–468 |
| `Subject::External` + path | rejected: `workload_path_incompatible` | 471–478 |
| Services declared, or `ManagedCommand` | `Service` category present (no capability) | 480–482 |
| `diagnostics` non-empty | one `DiagnosticProbe` per requested family | 483–499 |
| `security_checks` non-empty | `SecurityCheck { family: waf_bypass }` | 500–514 |
| Caller `required_capabilities` | unioned in last, so a caller can only tighten | 515–520 |

A `Subject::External` experiment carrying `network_path` fails here because
external targets own no workload-side dialer, so path assertions would have
nothing to attach to.

**4. Select and validate one driver per required category** (522–541):
`select_driver`, `validate_driver`, the external-binary check, then insert.
`select_driver` (762–824) resolves in this order:

- An explicit `selections` entry wins immediately. A name absent from the
  registry is `MissingDriver`; a name whose descriptor belongs to another
  category is `CategoryMismatch` (768–781). Note the precedence: a
  wrong-category driver reports the mismatch, not a capability failure.
- Otherwise candidates are every descriptor of that category (782–786).
  `capable_candidates` (791–799) is a prefilter applied **only** for
  `Correctness`, and only when non-empty (800–804) — comment 787–790 explains
  that correctness registries may hold one descriptor per family. Every other
  category keeps the category-wide set so a capability miss still reports
  `UnsupportedCapability` naming the specific capability instead of degrading
  to "no such driver".
- `ExplicitOnly` infers nothing: `MissingDriver` (805–807).
- Otherwise: exactly one `default` wins (810); else exactly one candidate wins
  (812); else no candidate is `MissingDriver` (813); else `AmbiguousSelection`
  with the full candidate list (814–822). Two or more marked defaults is also
  `AmbiguousSelection` (819–822) — the resolver never breaks a tie by sort
  order.

`validate_driver` (826–858) checks platform (831–838, only when
`supported_platforms` is non-empty), then every required capability (839–846),
then external coherence (847–856): `external_process == true` **requires**
`Capability::ExternalBinary`. The asymmetry matters — a driver cannot be
process-backed without declaring the capability, so "needs a binary" is never
inferred from a flag alone.

The external-binary *path* requirement is a separate step (527–533): a
process-backed driver with no path, or with an empty string, is
`MissingExecutablePath` — `is_none_or(str::is_empty)` treats `""` as absent.

**5. Post-selection canonical contracts** (543–553). Two helpers run after the
generic loop because they constrain a driver more tightly than capabilities
can. `validate_network_path_driver_contract` (941–1041) pins name, non-empty
`adapter_version` bounded to 128 chars, `upstream_name`, a non-empty
`upstream_version`, the required capability, `external_process == false`, and
`executable_path.is_none()`, for all three of workload (956–974), route
(989–1007), and fault (1020–1038). `validate_correctness_driver_contract`
(885–939) does the same for `eggsec-waf`: canonical name constant, `eggsec`
upstream, the `waf_bypass` family, `external_process == true`, and a pinned
executable path (914–930). The native/external polarity is the point: a path
driver must be in-process *and* pathless, while the correctness driver must be
external *and* have a path. Adapter contract validation, not capability
advertisement, is what enforces this.

**6. Telemetry** (555–657) is separate because of its required/optional split:

- No telemetry descriptor in the registry: a `required: true` request is
  `MissingDriver` (562–566); otherwise **every** request becomes an
  `OptionalTelemetryOmitted` warning listing all its fields (567–572).
- Otherwise select, preferring the descriptor *named by* the plan's telemetry
  `source` when there is exactly one distinct source and no explicit selection
  (578–606) — so `source` doubles as a driver name in that case. Otherwise
  fall back to normal `select_driver`, then `validate_driver` (607).
- Field-level check per request (615–648). `prometheus-http` is skipped
  (615–621): its fields come from a workspace-pinned mapping the CLI loads, so
  its descriptor cannot enumerate owner-defined metric names (616–618). A new
  dynamic-mapping source needs the same explicit exemption. Missing fields on
  a `required` request are `UnsupportedCapability` naming `missing[0]`
  (634–641) — one concrete field, not the set. On an optional request they
  become a warning carrying the whole `missing` list (642–647).
- The request is never dropped. `ResolvedPlan.telemetry` retains all requests
  including omitted ones (220–221); the omission lives in `warnings`.

**7. Compatible service types** (659–685). For the workload target, plus both
paired arm services when a paired design exists (663–667), resolution looks the
service up in `plan.services` and maps `ServiceKind::Command` to the literal
`"command"` and `ServiceKind::Named` to its declared type (670–673). A
non-empty `compatible_service_types` not containing that type is
`IncompatibleService` (675–682). Checking the arms too is what makes a driver
that can drive only one variant fail *before* a trial runs.

**8. Freeze** (687–759). Paired design: arms resolve by pure copy
(`ResolvedPairedArm::resolve`, 312–321 — arms carry no driver selection of
their own), `schedule` is the `PAIRED_SCHEDULE_V1` constant (690), and
`pairs = measured / 2` (691) by integer division; plan validation owns the
even-count rule. Network path: the `Route` and, when faults are present,
`Fault` entries must exist and their descriptor names must equal the names in
the request (701–705, 713–717), else `MissingDriver` — the plan cannot
redirect a selection. `defaults` freezes the platform label actually used for
the check plus warmup/measured counts (744–748), so the compatibility decision
stays auditable after the fact.

`ResolvedPlan` derives `PartialEq` but not `Eq` (201), unlike the nested DTOs —
its `workload` field is not `Eq`-comparable.

### ResolvedPlan schema and legacy readability

`ResolvedPlan` (201–253) freezes: `schema_version` (6) and
`source_plan_schema_version`, the `experiment` name, the full `drivers` map,
`subject`, normalized `topology`, `workload`, `trials`, all `telemetry`
requests, `defaults`, `environment_policy`, `metrics`, `artifact_bounds`,
`seed`, optional `paired`, optional `network_path`, `diagnostics`,
`security_checks`, `http_corpus_checks`, and `warnings`. Metric and artifact
entries are frozen as *requests* — no computed verdicts (226–229).

Version history is encoded in the type itself:

| Field | Line | Serde |
|---|---|---|
| `paired` | 233–234 | `#[serde(default)]` |
| `network_path` | 236–241 | `default` + `skip_serializing_if` + `deserialize_present_optional` |
| `diagnostics` | 243–244 | `#[serde(default)]` |
| `security_checks` | 246–247 | `#[serde(default)]` |
| `http_corpus_checks` | 249–250 | `#[serde(default)]` |

Legacy readability is **additive optionality plus a version allowlist** — not a
versioned enum, not separate per-version DTOs, not an untagged wrapper. The
constants at 12–22 name the readable versions; the gate that enforces them
lives in the evidence layer, `validate_resolved_plan_bytes`
(`crates/eggbench-core/src/evidence.rs:1745`), which accepts v1–v6 and
additionally rejects a legacy snapshot that omits `network_path` while its
source plan was schema v3 or later (1759–1769). Older bundles are read and
validated, never rewritten.

`ResolvedPlan` deliberately excludes secret values and callable/runtime
objects. There is no place for a credential, socket, client, or trait object:
every field is owned, bounded data. The path contract reinforces this at the
type level rather than by convention — `RouteMode`'s hand-written `Serialize`
refuses to emit a proxy chain containing `@`, `%`, `?`, `#`, control
characters, or over 1024 bytes, and its `Debug` redacts the chain
(`crates/eggbench-core/src/network_path.rs:51`, `network_path.rs:85`); a
credentialed chain cannot be serialized and its error text does not echo the
input. Concrete adapter and upstream versions are frozen precisely *because* a
driver upgrade can change behavior without a wire-schema change: the schema
describes shape, and only the version strings distinguish two adapters that
satisfy the same shape.

### Resolved network path provenance

`ResolvedNetworkPath` (258–270) is a first-class descriptor of the lowered
path, not a managed service. It retains the credential-free `route` request
(725), the selected `route_driver` (726), the stable `route-first-fault-second-v1`
semantics identity (727, from `NETWORK_PATH_SEMANTICS_VERSION`), and optionally
`stream_faults`. The field is skipped when absent, so a non-path plan's
snapshot carries no key at all.

`ResolvedStreamFaults` (277–286) retains the original `request` so
per-invocation evidence can roll the fault plan forward without reconstructing
the plan (272–276), the selected `fault_driver`, and the Eggchaos RNG identity
`splitmix64-v1` (721, `NETWORK_PATH_RNG_VERSION`).

The experiment seed is retained in `ResolvedPlan.seed` whenever the plan carries
one, independent of faults (752). The *fault-conditional* seed retention is a
per-run evidence concern, not a resolved-plan one: the path evidence builder
writes `seed_namespace` only when the fault plan is active, i.e. at least one
upstream or downstream entry
(`crates/eggbench-drivers/src/eggstack/path/evidence.rs:395`, `evidence.rs:399`;
field at `evidence.rs:131`).

## Invariants

- **No I/O, no PATH search, no runtime instantiation.** `resolve_plan` takes
  slices and maps and performs no filesystem, process, or network operation.
  Executable paths are supplied by the caller (158, 171); resolution records
  the path, it never resolves, probes, or execs it. No driver trait object is
  constructed — the descriptor is data, and construction happens in the
  runner/driver layer. This is what lets `doctor <plan>` resolve without
  starting anything.
- **Fail closed.** Every unsupported, missing, mismatched, ambiguous, or
  incompatible condition is an error, not a fallback. There is no "try the next
  candidate" loop and no degradation path.
- **Determinism.** Same plan + descriptors + options ⇒ same `ResolvedPlan`,
  via `BTreeMap`/`BTreeSet` ordering throughout and by resolving ambiguity as an
  error rather than a sorted choice (816).
- **The caller can only tighten** (515–520).
- **The snapshot is self-contained**: the full descriptor is cloned
  (536–539), so evidence does not depend on a later catalog state.
- **In-process drivers carry no executable path; process drivers carry a
  non-empty one.** Both directions are enforced (531–533, 612–614, 967–968,
  1000–1001, 1031–1032).
- **The plan cannot redirect a resolved selection** (701–705, 713–717).
- **Secrets stay out.** Plan inputs carry secret *references*; the resolved
  contract carries no credential values, and the path chain is serializable only
  in credential-free shape.
- **No panics.** The only `expect` calls are on statically valid names (512,
  905, 913), and the doc comment claims this explicitly (404–405).

## Failure model

`ResolveError` (336–395) is fail-closed and stable. `InvalidPlan` wraps
`PlanError` transparently (339–340), so plan-validation categories such as
`workload_path_incompatible` (472–477) and `unsupported_network_path`
(946–949) surface as a `PlanError::Validation` category rather than a new
resolver variant.

| Variant | Line | Raised when |
|---|---|---|
| `InvalidPlan` | 339 | The plan failed its own validation, or a canonical path/correctness contract was violated |
| `MissingDriver` | 342 | No candidate for a required category; `ExplicitOnly`; name absent; route/fault name mismatch |
| `CategoryMismatch` | 348 | An explicit selection named a driver of a different category |
| `AmbiguousSelection` | 358 | ≥2 candidates and no unique marked default |
| `UnsupportedCapability` | 366 | A required capability is absent; `external_process` without `ExternalBinary`; a required telemetry field is missing |
| `UnsupportedPlatform` | 374 | Non-empty `supported_platforms` excludes the caller's `platform` |
| `MissingExecutablePath` | 383 | A process-backed driver has no non-empty caller path |
| `IncompatibleService` | 386 | The workload driver excludes a target or paired-arm service type |
| `DuplicateDriver` | 393 | Two descriptors share one name |

Non-fatal outcomes are warnings, and there is exactly one
(`ResolutionWarning::OptionalTelemetryOmitted`, 188–198): optional telemetry
was dropped, with the source and the missing field names. `warnings` is a
required (non-defaulted) field, so a snapshot always accounts for it.

## Boundaries

- **Core ↔ drivers.** Core owns the descriptor *language* and selection;
  `eggbench-drivers` owns every descriptor instance and all side effects. The
  production inventory is `DriverCatalog::production`
  (`crates/eggbench-drivers/src/catalog.rs:49`): feature-gated native drivers
  plus unconditionally registered external oracles, because a missing binary is
  a runtime resolution error, not a build configuration (41–47). Qualification
  fakes are test-only and never linked through the catalog (13–14).
- **Core ↔ runner.** The runner receives a `ResolvedPlan` and constructs
  adapter objects from it. `crates/eggbench-runner/src/spec.rs:199` rejects a
  resolved plan whose `schema_version` is not the current one, so the
  legacy-readable v1–v5 snapshots are an evidence-read concern only.
- **No universal driver trait.** Categories are separate axes precisely so a
  single trait cannot become a lowest common denominator: a route driver is not
  a workload driver, and a correctness driver is not an oracle load generator.
- **Not resolvable here:** tool install, binary discovery, adapter
  construction, connection pooling, and route/fault dialing.

## Tests and qualification

Eleven unit tests (1059–1457) cover the contract, using the same fixtures the
CLI reads (`tests/fixtures/minimal.json`, `driver-capabilities.json`,
`sample-resolved-plan.json`, `multi-service-open-loop.json`):

| Test | Line | Qualifies |
|---|---|---|
| `exact_selection_and_deterministic_default_keep_provenance` | 1102 | Explicit and default selection yield the same snapshot; `upstream_version` survives serialization |
| `capability_matrix_and_resolved_snapshot_fixtures_are_current` | 1128 | The descriptor matrix still resolves to the committed snapshot fixture byte-for-byte |
| `resolution_fails_closed_on_missing_ambiguous_wrong_category_capability_and_platform` | 1148 | All five generic fail-closed paths |
| `optional_telemetry_warns_and_required_telemetry_fails` | 1191 | The required/optional split: warning vs `MissingDriver` |
| `prometheus_mapping_source_selects_its_dynamic_descriptor` | 1212 | Source-name-based telemetry selection |
| `required_telemetry_capability_fails_before_resolution` | 1234 | Field-level capability failure |
| `compatible_service_relation_is_checked` | 1252 | `IncompatibleService` |
| `external_driver_requires_explicit_binary_capability_and_path` | 1272 | `ExternalBinary` + non-empty path, and that the path is frozen |
| `paired_resolution_records_schedule_pairs_and_source_version` | 1342 | Schedule constant, `pairs`, arm provenance, `source_plan_schema_version` |
| `paired_resolution_fails_closed_when_driver_cannot_drive_one_arm` | 1361 | The candidate arm is compatibility-checked before any trial |
| `semantic_replay_requires_semantic_capability_and_pins_binary` | 1400 | `SemanticReplay` substitutes for a load mode; the external path is required |

The snapshot-fixture test (1128–1145) is the qualification hinge: any change to
`ResolvedPlan`'s serialized shape, to derived requirements, or to a descriptor
in the matrix breaks a checked-in artifact rather than passing silently. The
network-path and correctness canonical contracts have no dedicated unit test in
this file; they are exercised through the drivers crate's feature-gated
`eggstack-path` coverage.

## Review focus

- Selection precedence in `select_driver` (768–823): explicit → category
  candidates → unique default → sole candidate → error. Any change that
  introduces a tie-break breaks the determinism invariant.
- The `Correctness`-only `capable_candidates` prefilter (800–804). Extending it
  to other categories would change error specificity for existing plans.
- Required-capability derivation (423–520), especially `HttpCorpus` forcing
  `LoadMode::ClosedLoop` (431–437) and `SemanticReplay` suppressing a load-mode
  requirement (425–430).
- The `is_none_or(str::is_empty)` external-path checks (531, 612): an empty
  caller path must count as missing, not as a valid path.
- The `prometheus-http` capability-check bypass (615–621).
- Both native/external polarity checks (967–968 vs 923–924): loosening either
  lets a process driver satisfy an in-process path contract.
- Paired-arm service compatibility (664–667): a third arm source means updating
  this list.
- Version constants (11–22) and the `evidence.rs:1745` allowlist must move
  together; a version added here but not there is silently unreadable.
- `pairs = measured / 2` (691) has no local guard — it relies on plan validation
  for the even-count rule.

## Related

- [overview.md](overview.md) — system shape, cross-cutting contracts, core
  module map
- [core.md](core.md) — why core owns runtime-free domain contracts
- [drivers.md](drivers.md) — adapter-side ownership, including the
  "Resolution and resolved-plan provenance" contract this module implements
- [../docs/driver-capabilities.md](../docs/driver-capabilities.md) —
  capability matrices, network-path requirements, ResolvedPlan compatibility
- `crates/eggbench-core/src/plan.rs` — `ExperimentPlan`, `LoadMode`,
  `TelemetryRequest`, `PairedArm`, `deserialize_present_optional`
- `crates/eggbench-core/src/network_path.rs` — the credential-free
  `RouteRequest` / `StreamFaultPlanRequest` contract and its redaction rules
- `crates/eggbench-core/src/evidence.rs` — `validate_resolved_plan_bytes`, the
  v1–v6 read gate
- `crates/eggbench-drivers/src/catalog.rs` — the production descriptor set
  resolution consumes
- `crates/eggbench-drivers/src/eggstack/path/evidence.rs` — per-run path
  evidence, including the fault-conditional `seed_namespace`
