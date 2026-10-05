# Eggbench architecture overview

Bird's-eye view of the system and an index into per-component deep dives.

Eggbench is a local-first experiment and evidence system for repeatable
software performance and qualification work. It validates versioned
experiment plans, resolves them against typed driver capabilities, runs
managed workloads through explicit trial lifecycles, and finalizes
immutable `.eggb` evidence bundles. Measurement normalization and offline
comparison are separate capabilities: the runner produces evidence, and
comparison reads evidence without ever mutating it.

This file answers "what are the modules, how do they fit together, and
where do I look next". Each linked deep dive answers "how does this
component actually work, and what should a reviewer check".

## How to use this directory

| File | Answers |
|---|---|
| `overview.md` (this file) | Module map, data flow, index |
| [core.md](core.md), [runner.md](runner.md), [drivers.md](drivers.md), [evidence.md](evidence.md) | Why the crates and boundaries are shaped this way (design rationale, ADR-level) |
| `core-*.md`, `runner-*.md`, `drivers-*.md`, `cli-*.md`, `tooling-qualification-ci.md` | How one component works, its invariants, and its review focus |

The four `core.md` / `runner.md` / `drivers.md` / `evidence.md` files are
crate-level ownership documents. The `core-*` / `runner-*` / `drivers-*` /
`cli-*` files are component-level deep dives. The former explain *why* a
boundary exists; the latter explain *what the code does* inside it.

## 1. System shape

Four crates, one direction of dependency, no cycles:

```text
        eggbench-cli                 presentation: argv in, envelope + exit code out
              |
        eggbench-drivers             adapters + production driver catalog
              |
        eggbench-runner              process/service lifecycle, trial orchestration
              |
        eggbench-core                 runtime-free domain contracts (plans, evidence, metrics, comparison)
```

- **`eggbench-core`** owns typed, runtime-independent contracts. It performs
  no process execution, no async waiting, and no network I/O. Its crate
  dependencies are `serde`, `serde_json`, `thiserror`, `toml`, `uuid`,
  `sha2` — deliberately no `tokio`.
- **`eggbench-runner`** is the only crate that owns local side effects:
  process groups, readiness, bounded log capture, teardown, and the trial
  schedule. It depends on core and adds `tokio`.
- **`eggbench-drivers`** is the sole owner of adapter implementations and of
  the production driver catalog. It is the only crate that links the
  external Eggstack family (`Eggfetch`, `EggServe`, `Eggress`, `Eggchaos`)
  and third-party measurement tools.
- **`eggbench-cli`** is a thin presentation adapter. It parses argv, calls
  into core contracts or the runner's preparation/execution seams, and
  renders results. It does not duplicate orchestration and does not own
  driver registration.

`#![forbid(unsafe_code)]` holds in all four crates. Workspace lints enable
`clippy::all` and `clippy::pedantic` at warn, and `missing_docs` at warn.

## 2. End-to-end flow

The artifact chain is strictly one-directional. Each arrow is a typed
contract, and each stage owns exactly one failure domain.

```text
plan file (toml/json)
  │  core-plan-schema: parse + validate        → validated ExperimentPlan (schema v1..v10)
  ▼
  │  core-driver-resolution: resolve            → ResolvedPlan (schema v6, frozen provenance)
  ▼
  │  drivers-catalog-features: production inventory (capability descriptors)
  ▼
  │  runner-evidence-prep: collect + stage      → plan/resolved/environment/subject staged
  ▼
  │  runner-process-lifecycle: startup          → processes + in-process services, readiness
  ▼
  │  runner-orchestration: phase schedule       → warmups, measured trials, resets, cleanup
  ▼        (drivers-*: WorkloadExecutor / TelemetryCollector / CorrectnessExecutor / DiagnosticExecutor)
  ▼
  │  core-metrics: normalize                    → trials/NNN/metrics.json per measured trial
  ▼
  │  core-evidence-bundle: finalize             → manifest-last, atomic, immutable .eggb
  ▼
  │  core-comparison: offline policy            → standalone comparison receipt (never mutates a bundle)
  ▼
cli-*: envelope + exit code
```

Commands map onto this chain:

| Command | Stages run |
|---|---|
| `validate <plan>` | parse + validate only |
| `doctor <plan>` | validate + resolve + preflight, no managed startup |
| `run <plan> <bundle>` | the full chain through bundle finalization |
| `inspect <bundle>` | read-only verification and summary of a finalized bundle |
| `compare <baseline> <candidate>` | offline comparison of finalized bundles |
| `qualify {validate,expand,run,inspect}` | a separate security-qualification profile pipeline that does not run an experiment plan |

`qualify` is a command family rather than a single command, and it is
structurally distinct from the other five: it is declared and dispatched in
the binary (`crates/eggbench-cli/src/main.rs`), it is absent from the
library's `Command` enum, and it has no module under
`crates/eggbench-cli/src/commands/`. It operates on a qualification profile
and publishes a qualification receipt rather than an `.eggb` bundle, so it
sits beside the evidence chain above rather than inside it. The repository
`README.md` lists only the five plan commands; `docs/security-qualification.md`
and `docs/synvoid-qualification.md` document `qualify`. See
[cli-surface.md](cli-surface.md).

## 3. Module map

Line counts are source-only (`crates/*/src`), excluding tests.

### `eggbench-core` — domain contracts (17,975 LOC)

| Module | Files (LOC) | Responsibility | Deep dive |
|---|---|---|---|
| Plan schema | `plan.rs` (3,209), `types.rs` (255) | Versioned plan decoding, `deny_unknown_fields`, semantic validation across schema v1–v10 | [core-plan-schema.md](core-plan-schema.md) |
| Driver resolution | `resolved.rs` (1,463) | Capability descriptors, typed capability values, deterministic selection, `ResolvedPlan` v6 provenance | [core-driver-resolution.md](core-driver-resolution.md) |
| Metrics | `metrics.rs` (1,588) | Metric vocabulary v1, `TrialMetrics` schema v1, pure normalization to observed/missing/invalid | [core-metrics.md](core-metrics.md) |
| Comparison | `comparison.rs` (6,924) | Bundle identity, digest-pinned aliases, comparability, deterministic bootstrap policies, verdicts, receipt | [core-comparison.md](core-comparison.md) |
| Evidence | `evidence.rs` (2,521) | `.eggb` manifest contract, streaming writer, atomic finalize, read-only `BundleReader` | [core-evidence-bundle.md](core-evidence-bundle.md) |
| Qualification & security | `qualification.rs` (1,277), `security.rs` (412), `network_path.rs` (281) | Bounded subject-neutral qualification inputs, security-correctness evidence contract, declarative network-path intent | [core-qualification-security.md](core-qualification-security.md) |

### `eggbench-runner` — lifecycle and orchestration (9,724 LOC)

| Module | Files (LOC) | Responsibility | Deep dive |
|---|---|---|---|
| Orchestration | `orchestration.rs` (3,805) | Phase coordinator, warmups, measured trials, reset hooks, measurement interval, cleanup boundary | [runner-orchestration.md](runner-orchestration.md) |
| Process lifecycle | `session.rs` (1,266), `spec.rs` (529), `platform.rs` (157), `probe.rs` (239) | Process groups, dependency-ordered startup, readiness, bounded logs, reverse teardown | [runner-process-lifecycle.md](runner-process-lifecycle.md) |
| Adapter seams | `service.rs` (290), `telemetry.rs` (513), `correctness.rs` (320), `diagnostics.rs` (358) | Object-safe trait seams and registries for services, telemetry, correctness, diagnostics | [runner-adapter-seams.md](runner-adapter-seams.md) |
| Evidence preparation | `prepare.rs` (498), `environment.rs` (671), `subject.rs` (338), `bundle.rs` (204), `secret.rs` (65), `error.rs` (175) | Environment fingerprint, subject snapshot, pre-run evidence staging, secret indirection | [runner-evidence-prep.md](runner-evidence-prep.md) |

Two deterministic fixtures support lifecycle testing and are not part of the
production surface: `runner/src/bin/eggbench-child-fixture.rs` (185) and
`drivers/src/bin/eggbench_fixture.rs` (80).

### `eggbench-drivers` — adapters and catalog (16,641 LOC)

| Module | Files (LOC) | Responsibility | Deep dive |
|---|---|---|---|
| Catalog & features | `catalog.rs` (341), `lib.rs` (63) | `DriverCatalog::production`, feature-gated registration, no-PATH-search contract | [drivers-catalog-features.md](drivers-catalog-features.md) |
| Eggstack HTTP | `eggstack/fetch.rs` (1,520), `origin.rs` (265), `http_corpus.rs` (202), `mod.rs` (178) | Eggfetch closed-loop workload, EggServe controlled origin, fixed-corpus correctness | [drivers-eggstack-http.md](drivers-eggstack-http.md) |
| Network path | `eggstack/path/{route,fault,dialer,evidence,mod}.rs` (1,360) | Route-first/fault-second Eggress + Eggchaos lowering, per-run path evidence | [drivers-network-path.md](drivers-network-path.md) |
| External substrate | `external/{mod,resolver,command,version,parser,artifact,preflight,common,error}.rs` (2,334) | Trusted binary resolution, argv-only bounded execution, version policy, parser contract | [drivers-external-substrate.md](drivers-external-substrate.md) |
| Load tool adapters | `external/{oha,h2load,iperf3}.rs` (2,552) | Independent external load generators and their report semantics | [drivers-external-load-tools.md](drivers-external-load-tools.md) |
| Oracle tool adapters | `external/{eggsec,eggsec_load,eggreplay,eggprobe}.rs` (4,953) | WAF correctness, security load, semantic replay, pre/post diagnostics | [drivers-external-oracle-tools.md](drivers-external-oracle-tools.md) |
| Telemetry adapters | `gregg/{mod,collect,endpoint}.rs` (1,049), `prometheus_http.rs` (1,744) | Trial-synchronized host telemetry, bounded Prometheus text-format parsing | [drivers-telemetry-adapters.md](drivers-telemetry-adapters.md) |

### `eggbench-cli` — presentation (5,511 LOC)

| Module | Files (LOC) | Responsibility | Deep dive |
|---|---|---|---|
| CLI surface | `main.rs` (913), `lib.rs` (227), `envelope.rs` (738), `error.rs` (124), `plan_input.rs` (93) | Argv parsing, JSON envelope compatibility surface, exit-code mapping, plan input loading | [cli-surface.md](cli-surface.md) |
| Commands | `commands/{run,doctor,inspect,compare,validate,mod}.rs` (2,361) | One module per command; pipeline wiring, never orchestration logic | [cli-commands.md](cli-commands.md) |
| Workload registry | `workload_registry.rs` (1,055) | Production/qualification driver-inventory split, adapter construction from capabilities | [cli-workload-registry.md](cli-workload-registry.md) |

## 4. Cross-cutting contracts

Every serialized artifact carries an explicit `SchemaVersion` under the
`org.eggstack.eggbench.core` namespace. A version bump is required when a
field changes interpretation; additive top-level fields stay backward
compatible, nested and variant fields stay strict.

| Contract | Location | Versions |
|---|---|---|
| Experiment plan | core `plan.rs` | v1 → v10 (v2 paired design, v3 network path, v4 semantic replay, v5 diagnostics, v6 security checks, v7 static service binding, v8 HTTP corpus, v9 generic runtime binding + security load, v10 HTTP security corpus) |
| Resolved plan | core `resolved.rs` | v6 current; v1–v5 readable for legacy evidence |
| Metric vocabulary | core `metrics.rs` | v1 |
| Trial metrics | core `metrics.rs` | v1 |
| Bundle manifest | core `evidence.rs` | v2 current, v1 read through a legacy DTO |
| Environment fingerprint | core + runner | v1 |
| Runtime topology | runner `service.rs` | `RUNTIME_TOPOLOGY_SCHEMA_VERSION` |
| Subject snapshot | runner `subject.rs` | `SUBJECT_SNAPSHOT_SCHEMA_VERSION` |
| Network path evidence | drivers `path/evidence.rs` | `NETWORK_PATH_EVIDENCE_SCHEMA_VERSION` |
| Comparison receipt | core `comparison.rs` | policy identities `eggbench.trial-bootstrap.v1`, `…-network-path.v1`, `…-paired.v1` |

Other contracts that cut across every module:

- **Capability model.** A driver declares a stable identity, exactly one
  category (service, workload, telemetry, fault, diagnostic, execution
  provider, route), a platform label, and typed capability values. Core
  stores descriptors; it never instantiates runtime traits.
- **Selection determinism.** Default selection accepts a single marked
  default or a sole category candidate. Anything ambiguous fails rather than
  guessing. Required capability or telemetry gaps fail closed; missing
  optional telemetry is retained as a structured warning.
- **Secret indirection.** Plans reference secrets by name; values are
  supplied at runtime through `SecretProvider` and never enter a plan,
  resolved plan, or manifest.
- **Normalize-once rule.** Drivers parse upstream output and produce
  protocol-neutral raw observations. Only `core::metrics` writes
  `TrialMetrics`. No driver writes normalized metric JSON itself.
- **No silent fallback.** Route selection never degrades to a direct
  connection; a failed route is a failed dial. A missing external binary
  fails before managed startup.
- **Manifest-last publication.** A bundle can never contain evidence of its
  own publication.

### Known drift and open findings

The deep dives were written by reading the code, and comparing the code against
its own prose surfaced 38 findings. Every one has since been re-verified
against source and dispositioned, so this section is a **live** index of what is
still open — not a record of what was found. Findings that are fixed have been
removed rather than left to rot into a list that contradicts its own
disposition note.

The original table of pre-existing documents that contradicted the code has
been retired: all sixteen of its rows were corrected (`f61df14`). Three further
audit claims did not survive verification and were never acted on; they are
listed in the Disposition section below rather than dropped. The record lives in
`plans/subsystems/architecture-deep-dive-corrective-addendum.md` and
`plans/closure/architecture-deep-dive-corrective/`. What remains are the
source-level findings below.

**Source-level findings worth a reviewer's attention.** Each is documented
in full, with surrounding context, in the linked deep dive's *Review focus*.

| Area | Finding | Deep dive |
|---|---|---|
| Dispatch | Catalog registration and CLI dispatch are two hand-maintained name lists, so a driver can reach the catalog with no executor arm. The comment stating dispatch is by name is now truthful, and a guard test asserts the two sets agree — so this is a maintainability cost, not a live divergence | [cli-workload-registry.md](cli-workload-registry.md) |
| Evidence | `BundleReader::open` performs no declared-size verification: path validation checks symlink components and manifest validation checks declared *bounds*, but an artifact whose on-disk bytes exceed its declared size is not rejected at open | [core-evidence-bundle.md](core-evidence-bundle.md) |
| Evidence | A stale `.{name}.finalize-lock` from a crash is reported as `DestinationExists` even when the destination is free (`C003-a`) | [core-evidence-bundle.md](core-evidence-bundle.md) |
| Seams | `diagnostics.rs` claims sibling-neutrality but hardcodes `driver == "eggprobe"` and `machine_schema == "0.3"`, duplicating a pin the drivers crate already exports | [runner-adapter-seams.md](runner-adapter-seams.md) |
| Substrate | `ErrorCategory::OutputTruncated` and `ExternalParseError` are public and exported but never constructed — a documented reservation, not a defect | [drivers-external-substrate.md](drivers-external-substrate.md) |
| CLI | Public failure categories are derived by substring-matching driver error text, so rewording a message silently changes a public category | [cli-commands.md](cli-commands.md) |
| CLI | `ResolutionOptions` and `workload_load_mode` are built independently in `run.rs` and `doctor.rs`, so `doctor` can disagree with `run` (`C003-i`) | [cli-commands.md](cli-commands.md) |
| Build | `build.rs` returns the first matching `[[package]]` stanza, so version provenance is wrong if the lockfile ever holds two versions of a scraped crate (`C003-e`) | [drivers-catalog-features.md](drivers-catalog-features.md) |
| Provisioning | `OHA_SHA256_aarch64` is empty, so `provision-external-oracles.sh` is x86_64-only and cannot provision aarch64 or the Windows lane. It fails closed rather than installing unpinned | [tooling-qualification-ci.md](tooling-qualification-ci.md) |
| Qualification | The live-tools negative control accepts any nonzero exit as "rejected" without checking the reason, so an unrelated crash would pass | [tooling-qualification-ci.md](tooling-qualification-ci.md) |

### Disposition of the audit findings

`13eb443` recorded the 38 findings as open leads. The corrective pass
(`plans/subsystems/architecture-deep-dive-corrective-addendum.md`,
milestones C001/C002) closed the following classes, whose rows have been removed
from the table above rather than left to read as live defects:

- **Documentation drift** — every "pre-existing documents that contradict the
  code" row was corrected. Schema versions, readable ranges, the driver
  registration set, the unconditional-drain contract, the timeout allowlist,
  the external-substrate error and cwd semantics, the Prometheus retention
  model, and the manifest field-tolerance description all now match the code.
- **Silent evidence loss** — comparison records a `metric_receipt_truncated`
  warning instead of truncating quietly; per-trial metric semantics are now
  checked across the whole baseline rather than first-wins; `trial_metrics`
  bounds its read before allocating; the comparison manifest re-read now
  inherits the reader's no-follow resolution and byte cap.
- **Unreachable observability** — `cancelled`/`timed_out` derive from the real
  deadline outcome, `cleanup_notes` are computed before the early returns,
  `join_pipes` fails closed instead of returning empty output, `CapturedStream::collect`'s
  dead `limit` parameter is gone, `workload_entered` is removed, and both
  correctness/diagnostic registries reject a duplicate registration.
- **Loud-by-omission** — the phase-event reservation is now the exact
  required count rather than one short, `FakeDiagnosticExecutor` reports
  `DiagnosticFailed`, and `TelemetryError` can no longer be built past its
  documented bound.
- **Unreachable descriptors** — every `DriverCategory` variant now has a
  catalog accessor, so `Route` and `Fault` descriptors are no longer reachable
  only by scanning the flat list.

Three claims did not survive verification and were **not** acted on:
`docs/equivalence.md` never existed and the `ComparisonReceipt` field
strictness it was said to contradict is already in place; the catalog and CLI
dispatch driver-name sets are identical (only the comment was wrong, and a
guard test now keeps them equal); and the histogram/scalar
unresolvable-reference asymmetry is a stated design decision at
`metrics.rs:1028`.

The remainder are decision-gated and enumerated as C003-a through C003-o in
the addendum, which is the authoritative record — several of them, including
C003-c and C003-f through C003-o, have no row in the table above and are
tracked only there. The most consequential are the `doctor`/`run` disagreement
for `prometheus-http` fields (C003-d), stale `finalize-lock` reporting
(C003-a), and whether the v2 manifest should be a closed contract (C003-b).

## 5. Feature matrix

Features are additive and isolated; the CLI forwards each one to
`eggbench-drivers`. CI enforces the isolation explicitly.

| Feature | Links | Registers |
|---|---|---|
| *(default)* | core, runner, external command substrate | 7 external-process drivers: `oha`, `h2load`, `iperf3`, `eggreplay-semantic`, `eggprobe`, `eggsec-waf`, `eggsec-load` |
| `eggstack-http` | Eggfetch, EggServe, hdrhistogram | `eggserve-origin` service, `eggfetch-http` workload, `eggbench-http-corpus` correctness |
| `eggstack-path` | implies `eggstack-http`; adds Eggress, Eggchaos | `eggress-route` (route), `eggchaos-stream` (fault), network-path dialer |
| `gregg` | gregg-protocol, Eggfetch | `gregg` telemetry collector |
| `prometheus-http` | Eggfetch | `prometheus-http` telemetry collector |

Counts are cumulative: 7 drivers with no features, 14 with all features
enabled. The external group is unconditional because a missing binary is a
resolution/preflight failure, not a link-time one. Default and
`eggstack-http`-only builds must not link Eggress or Eggchaos; CI greps the
dependency tree to prove it. See
[drivers-catalog-features.md](drivers-catalog-features.md).

## 6. Evidence bundle layout

```text
<name>.eggb/
  manifest.json            written last; authoritative; no embedded artifact contents
  plan.json                validated source plan
  resolved-plan.json       frozen resolution provenance
  environment.json         versioned fingerprint with comparability class per field
  subject.json             resolved executable path + SHA-256, declared/observed match
  lifecycle/
    runtime-topology.json  ownership kind, adapter provenance, non-secret bindings
    runner-phases.json     terminal phase state
  trials/NNN/
    result.json            versioned trial execution facts (schema v2 carries arm + pair_id)
    metrics.json           normalized TrialMetrics
    telemetry/…            collector output
  comparison.json          standalone receipt; only for a comparison run
  report.json              human/machine report
```

Artifact roles are typed (`ExperimentPlan`, `ResolvedPlan`,
`EnvironmentFingerprint`, `Subject`, `TrialResult`, `Telemetry`, `Stdout`,
`Stderr`, `TrialArtifact`, `Comparison`, `Report`, `Other`), each with a
media type, a sensitivity label, an exact byte count, and a SHA-256 digest.
Paths are confined and relative. The manifest is authoritative: directory
naming never defines what evidence exists.

## 7. Non-Rust surfaces

| Surface | Purpose | Deep dive |
|---|---|---|
| `docs/` (17 files) | Contract documentation: plan schema, capabilities, evidence, lifecycle, metrics, comparison, eggstack, gregg, oracles, security, qualification, CLI | referenced from each deep dive |
| `plans/` | ADRs, subsystem roadmaps, implementation plans, closure records, registry | [tooling-qualification-ci.md](tooling-qualification-ci.md) |
| `qualification/synvoid/{v1,v2}/` | Versioned qualification assets: scenarios, corpora, profiles, telemetry contracts, baselines | [tooling-qualification-ci.md](tooling-qualification-ci.md) |
| `scripts/qualification/` | Live qualification harnesses and oracle assertions | [tooling-qualification-ci.md](tooling-qualification-ci.md) |
| `examples/` | Worked plan examples, including intentionally rejected cases | [tooling-qualification-ci.md](tooling-qualification-ci.md) |
| `.github/workflows/` | `ci.yml` (4 lanes: Linux stable, Linux MSRV 1.89.0, macOS, Windows) and `live-tools.yml` (5 real-binary interoperability jobs: live-tools, live-eggsec, live-m004b, live-synvoid, live-m003) | [tooling-qualification-ci.md](tooling-qualification-ci.md) |

## 8. Suggested review order

For a first pass over an unfamiliar area, this order moves from contracts
that everything depends on toward leaves:

1. [core-plan-schema.md](core-plan-schema.md) — the input contract and the
   version history that explains most downstream features.
2. [core-driver-resolution.md](core-driver-resolution.md) — how a plan
   becomes a frozen, capability-checked resolution.
3. [core-metrics.md](core-metrics.md) — what a number means and how a
   raw observation becomes an evidence record.
4. [core-evidence-bundle.md](core-evidence-bundle.md) — the immutability
   and publication contract everything else must satisfy.
5. [runner-orchestration.md](runner-orchestration.md) — the phase schedule
   and the single cleanup boundary.
6. [core-comparison.md](core-comparison.md) — the statistical policy and
   verdict semantics, the largest single module in the workspace.
7. Driver and CLI deep dives, as needed by the change under review.

## 9. Keeping this directory accurate

These documents cite `file.rs:LINE` for their load-bearing claims, so they
rot as the code moves. `scripts/verify-architecture-docs.py` checks the
mechanical invariants and should be run after editing anything here:

```bash
python3 scripts/verify-architecture-docs.py
```

It verifies that every relative link resolves, every `file:line` citation is
in range, every deep dive carries the ten standard sections, and this file
links every deep dive. It does not judge whether a citation points at the
*right* line — that still needs a human or a careful read.

## 10. Deferred boundaries

Two boundaries are named in the accepted ADRs but intentionally unbuilt:
a remote/distributed execution provider (ADR-0005, `ExecutionProvider` /
`Eggwork` boundary) and the `execution provider` and `route` driver
categories beyond the local route adapter. `plans/subsystems/` records
distributed execution as deferred until a local lifecycle is stable and a
concrete remote provider exists.
