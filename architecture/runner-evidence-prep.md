# Runner evidence preparation

## Purpose

What `eggbench-runner` does to establish evidence *before* and *around* managed
execution: environment fingerprinting, subject identity capture, primary and
lifecycle artifact staging, secret indirection, and the resulting error surface.

The concern is ordering and provenance, not measurement. This layer answers one
question before a single measured trial runs — *can the two things this run intends
to compare be compared on this host at all?* — because afterwards the host state,
the binary on disk, and the plan bytes cannot be recovered with the same fidelity.
Three properties are load-bearing, and all three are established structurally
rather than by convention:

1. Fingerprinting and subject capture precede any spawn, so a failure here cannot
   leave orphan descendants.
2. Primary evidence is staged into a writer that is *not yet published*, so a
   partially populated bundle can never be read as evidence.
3. Lifecycle evidence is staged through the same writer API as everything else, so
   the runner adds no second evidence format.

## Source layout

| File | Lines | Role |
|---|---|---|
| [`environment.rs`](../crates/eggbench-runner/src/environment.rs) | 671 | `LocalEnvironmentCollector`, host fact collection, `EnvironmentError` |
| [`prepare.rs`](../crates/eggbench-runner/src/prepare.rs) | 498 | `BundlePreparation`, `prepare_bundle`, `SubjectSnapshotError`, `PrepareError` |
| [`subject.rs`](../crates/eggbench-runner/src/subject.rs) | 338 | `SubjectSnapshot`, observed executable digest, snapshot schema version |
| [`bundle.rs`](../crates/eggbench-runner/src/bundle.rs) | 204 | `stage_lifecycle_logs`, `stage_lifecycle_metadata`, `stage_run_evidence`, `stage_runtime_topology` |
| [`secret.rs`](../crates/eggbench-runner/src/secret.rs) | 65 | `SecretProvider`, `MapSecretProvider` |
| [`error.rs`](../crates/eggbench-runner/src/error.rs) | 175 | `RunnerError`, `CleanupFailure` |

External contracts consumed, not re-defined: `EnvironmentFingerprint` and its three
comparability classes
([evidence.rs:694](../crates/eggbench-core/src/evidence.rs)); `Subject`
([plan.rs:226](../crates/eggbench-core/src/plan.rs)); and `BundleWriter`
([evidence.rs:839](../crates/eggbench-core/src/evidence.rs)) — the single artifact
write path. That method is `add_artifact`; the core crate has no `stage_artifact`,
and nothing in the runner invents a second one.

## Key types

**`LocalEnvironmentCollector`** is a zero-sized unit struct
([environment.rs:39](../crates/eggbench-runner/src/environment.rs)) deriving
`Default, Clone, Copy`. It performs no I/O at construction; all probing happens
inside `collect()` ([environment.rs:48](../crates/eggbench-runner/src/environment.rs)).
"No side effects until asked" is therefore a property of the type, not a habit of
the caller.

**`EnvironmentFingerprint`** (schema v1) is a
`BTreeMap<Name, EnvironmentField>` with a fixed `SchemaVersion(1)`
([evidence.rs:22](../crates/eggbench-core/src/evidence.rs),
[evidence.rs:706](../crates/eggbench-core/src/evidence.rs)). `BTreeMap` is the
determinism mechanism: serialization order is fixed by key order, so two
collections on an unchanged host produce byte-identical JSON. Each field carries
its own `EnvironmentFieldClass` — `comparison_critical` (a mismatch blocks a strict
same-testbed comparison), `warning_only` (surfaced, not disqualifying), or
`informational` (context only). The class is *data attached to a fact*, not a branch
in the collector: this layer has no comparison policy, only a record of which facts
a later policy must treat as load-bearing. Boundedness is the core's job
([evidence.rs:727](../crates/eggbench-core/src/evidence.rs)) — at most 128 fields,
names ≤ 128 bytes, values ≤ 4096 bytes, no control characters — and `collect()` runs
that validation itself before returning
([environment.rs:74](../crates/eggbench-runner/src/environment.rs)).

**`SubjectSnapshot`** carries eight `Option`-bearing fields: `schema_version`,
`subject`, `resolved_executable`, `executable_sha256`, `declared_revision`,
`declared_digest`, `declared_digest_matches`, `identity_label`
([subject.rs:24](../crates/eggbench-runner/src/subject.rs) through
[subject.rs:38](../crates/eggbench-runner/src/subject.rs)). The `Option`s are
meaningful: "no observed digest" differs from "observed digest that does not match".
Two constants bound it — `SUBJECT_SNAPSHOT_SCHEMA_VERSION = SchemaVersion(1)`
([subject.rs:47](../crates/eggbench-runner/src/subject.rs)) and
`MAX_SUBJECT_SNAPSHOT_BYTES = 4 MiB`
([subject.rs:16](../crates/eggbench-runner/src/subject.rs)). `subject.rs` defines
its *own* `SchemaVersion(pub u32)` newtype
([subject.rs:44](../crates/eggbench-runner/src/subject.rs)) rather than reusing the
core one: the snapshot is versioned independently of the evidence manifest and the
resolved plan.

**`BundlePreparation<'a>`**
([prepare.rs:16](../crates/eggbench-runner/src/prepare.rs)) is a borrowed input
bundle for `prepare_bundle`: destination, `RunId`, verbatim source plan bytes, the
source plan's media type, the resolved plan, the fingerprint, the snapshot, and
`ArtifactBounds`. It holds no `SecretProvider` — secret values are not in scope for
staging at all.

**`SecretProvider`** ([secret.rs:15](../crates/eggbench-runner/src/secret.rs)) is a
`Send + Sync + Debug` trait with one method,
`resolve(&self, reference: &Name) -> Option<String>`. `Debug` is a supertrait
rather than an afterthought: a provider must be loggable, so the trait forces every
implementation to make a redaction decision. `MapSecretProvider` implements it by
hand, printing reference names and `"[REDACTED]"` for the value map
([secret.rs:29](../crates/eggbench-runner/src/secret.rs)).

**`RunnerError`** has 14 variants split by *phase*; only the post-spawn group
carries `cleanup: Vec<CleanupFailure>`. `CleanupFailure` is a two-field record,
`service` and `reason`
([error.rs:13](../crates/eggbench-runner/src/error.rs)).

## How it works

### Fact collection and comparability classification

`collect()` assembles facts from five groups
([environment.rs:101](../crates/eggbench-runner/src/environment.rs)) and filters
them in one pass: `None` values are dropped
([environment.rs:52](../crates/eggbench-runner/src/environment.rs)), as are values
empty after trimming
([environment.rs:53](../crates/eggbench-runner/src/environment.rs)), each surviving
name is validated through `Name::new`
([environment.rs:57](../crates/eggbench-runner/src/environment.rs)), and the result
is schema-validated before return. Facts are *candidates*: each is reached only if
its data source yields a value, so presence is platform-conditional.

| Field | Class | Read from | Present when |
|---|---|---|---|
| `os_family` | `comparison_critical` | `cfg!(target_os)` chain, [environment.rs:225](../crates/eggbench-runner/src/environment.rs) | Always |
| `architecture` | `comparison_critical` | `std::env::consts::ARCH` via an 8-entry whitelist, [environment.rs:249](../crates/eggbench-runner/src/environment.rs) | Arch is one of the 8 listed |
| `target_family` | `comparison_critical` | `cfg!(target_family)`, [environment.rs:239](../crates/eggbench-runner/src/environment.rs) | Always |
| `cpu_model` | `comparison_critical` | Linux: `/proc/cpuinfo` `model name` → `Hardware` → `Processor` ([environment.rs:308](../crates/eggbench-runner/src/environment.rs)); macOS: `sysctl machdep.cpu.brand_string` | Source exposes a model string |
| `logical_cpu_count` | `comparison_critical` | Linux: `/sys/devices/system/cpu/online` → `cpuN` dir count → `/proc/cpuinfo` `processor` lines ([environment.rs:332](../crates/eggbench-runner/src/environment.rs)); macOS: `sysctl hw.ncpu`; Windows: `NUMBER_OF_PROCESSORS` | Any source yields a positive count |
| `physical_cpu_count` | `comparison_critical` | Linux: `/proc/cpuinfo` `physical id` grouped, `cpu cores` (else `siblings`) max-per-package, summed ([environment.rs:395](../crates/eggbench-runner/src/environment.rs)); macOS: `sysctl hw.physicalcpu`; Windows: never ([environment.rs:448](../crates/eggbench-runner/src/environment.rs)) | Linux/macOS only |
| `total_memory_bytes` | `comparison_critical` | Linux: `/proc/meminfo` `MemTotal` × 1024 ([environment.rs:493](../crates/eggbench-runner/src/environment.rs)); macOS: `sysctl hw.memsize` | Linux/macOS only |
| `kernel_release` | `comparison_critical` | `uname -r` ([environment.rs:294](../crates/eggbench-runner/src/environment.rs)) | Unix, and `uname` spawns successfully |
| `os_version` | `warning_only` | Linux: `/etc/os-release` `PRETTY_NAME` ([environment.rs:263](../crates/eggbench-runner/src/environment.rs)); macOS: `"macos {kernel_release}"` | Linux/macOS only; always absent on Windows |
| `current_cpu_frequency_mhz` | `warning_only` | Linux: max over `cpuN/cpufreq/cpuinfo_max_freq` ÷ 1000 ([environment.rs:462](../crates/eggbench-runner/src/environment.rs)); macOS: `sysctl hw.cpufrequency_max` | cpufreq or sysctl available |
| `eggbench_collector_version` | `informational` | `env!("CARGO_PKG_VERSION")` ([environment.rs:203](../crates/eggbench-runner/src/environment.rs)) | Always |
| `rust_target` | `informational` | `std::env::consts::ARCH` ([environment.rs:208](../crates/eggbench-runner/src/environment.rs)) | Always |
| `rustc_version_runtime` | `informational` | Stub returning `None` ([environment.rs:576](../crates/eggbench-runner/src/environment.rs)) | Never — always dropped |
| `build_profile` | `informational` | `cfg!(debug_assertions)` ([environment.rs:583](../crates/eggbench-runner/src/environment.rs)) | Always |

Three consequences follow. `current_cpu_frequency_mhz` is `warning_only` and never
higher ([environment.rs:169](../crates/eggbench-runner/src/environment.rs)) — the
module doc states that transient load or frequency is warning-only at best
([environment.rs:12](../crates/eggbench-runner/src/environment.rs)) and a test pins
it ([environment.rs:662](../crates/eggbench-runner/src/environment.rs)); its source
is `cpuinfo_max_freq`, a *static* ceiling rather than a live read, which is also why
the determinism test holds. Subject revision and digest are absent by design
([environment-fingerprint.md:11](../docs/environment-fingerprint.md)) because
baseline and candidate binaries are expected to differ, so a comparison-critical
subject digest would fail every legitimate A/B comparison. And
`rustc_version_runtime` is dead weight by decision — its comment
([environment.rs:577](../crates/eggbench-runner/src/environment.rs)) rejects a new
dependency for one informational label, so the field is declared and never emitted,
which is why it is missing from the published field table.

### The collect-before-startup ordering guarantee

The claim is that a collection failure cannot leave managed processes running
because startup has not begun
([environment.rs:3](../crates/eggbench-runner/src/environment.rs)). It is
guaranteed by call order, not by the type system. The authoritative sequence is in
the CLI run command ([run.rs:338](../crates/eggbench-cli/src/commands/run.rs)):

| Step | Site | Spawns? |
|---|---|---|
| Resolve the plan | [run.rs:345](../crates/eggbench-cli/src/commands/run.rs) | No |
| Resolve the managed executable path | [run.rs:353](../crates/eggbench-cli/src/commands/run.rs) | No |
| `collect_local_environment(...)` → fingerprint + snapshot | [run.rs:365](../crates/eggbench-cli/src/commands/run.rs) | No |
| Reject a declared/observed digest mismatch | [run.rs:376](../crates/eggbench-cli/src/commands/run.rs) | No |
| `LocalSession::prepare(...)` — build spawn specs | [run.rs:398](../crates/eggbench-cli/src/commands/run.rs) | **No** — "No process starts here" ([session.rs:290](../crates/eggbench-runner/src/session.rs)) |
| `prepare_bundle(...)` — stage four primary artifacts | [run.rs:408](../crates/eggbench-cli/src/commands/run.rs) | No |
| `execute_run_with_diagnostics(...)` | [run.rs:447](../crates/eggbench-cli/src/commands/run.rs) | Yes — first spawn, first measurement |

Three structural facts carry it. Collection is pure until called, so there is no
constructor side effect to unwind
([environment.rs:101](../crates/eggbench-runner/src/environment.rs)).
`collect_local_environment`
([prepare.rs:253](../crates/eggbench-runner/src/prepare.rs)) precedes
`LocalSession::prepare`, which is documented and implemented as the non-spawning
preflight ([session.rs:288](../crates/eggbench-runner/src/session.rs)), so a
collection error occurs where the process table is provably untouched by this
runner. And the orchestrator cannot re-enter preparation: `execute_run` receives an
already-populated `BundleWriter`
([run.rs:455](../crates/eggbench-cli/src/commands/run.rs)) with no access to the
plan bytes or the collector, so it cannot defer or repeat the step.

The honest limit: this is a property of one call graph, not a type-level
constraint. An embedder that hand-builds a `BundleWriter` and calls `execute_run`
would not inherit the ordering. Making `prepare_bundle` a free function that owns
the whole staging contract reduces the risk; nothing prevents bypassing it.

### `SubjectSnapshot::build()`

`build()` ([subject.rs:60](../crates/eggbench-runner/src/subject.rs)) dispatches on
the `Subject` variant ([subject.rs:74](../crates/eggbench-runner/src/subject.rs)):

| Variant | `resolved_executable` | `executable_sha256` | `declared_revision` / `declared_digest` | `identity_label` |
|---|---|---|---|---|
| `ManagedCommand` with a path supplied | Set to `path.display()` | Computed | Copied from the plan | — |
| `ManagedCommand` with `None` | `None` | `None` | Copied from the plan | — |
| `External` | `None` | **never** | Copied from the plan | `target` |
| `Label` | `None` | **never** | none — the variant has no such fields | `label` |

Only `ManagedCommand` produces an observed digest: the executable is streamed
through a 64 KiB buffer into SHA-256
([subject.rs:214](../crates/eggbench-runner/src/subject.rs),
[subject.rs:17](../crates/eggbench-runner/src/subject.rs)) and hex-encoded in
lowercase ([subject.rs:234](../crates/eggbench-runner/src/subject.rs)). `External`
and `Label` are declared identity only — the runner does not own those binaries and
cannot truthfully hash them.

The match flag is `digest.as_deref() == Some(sha.as_str())`, stored as `Some(bool)`
([subject.rs:83](../crates/eggbench-runner/src/subject.rs)). Because the comparison
is against a non-empty `Some`, a managed subject declaring **no** digest yields
`Some(false)`, not `None`: the snapshot cannot distinguish "declared digest absent"
from "declared digest wrong", and the CLI fails both at preflight
([run.rs:376](../crates/eggbench-cli/src/commands/run.rs)). A managed-command
subject with no declared digest is not runnable through this path.

`validate()` ([subject.rs:122](../crates/eggbench-runner/src/subject.rs)) enforces
rules that matter more than the field checks: version exactly 1
([subject.rs:123](../crates/eggbench-runner/src/subject.rs)); a recorded digest of
64 lowercase hex characters
([subject.rs:136](../crates/eggbench-runner/src/subject.rs)); an `External` or
`Label` snapshot carrying `executable_sha256` rejected outright
([subject.rs:160](../crates/eggbench-runner/src/subject.rs)); a `ManagedCommand`
snapshot with a digest but no resolved executable rejected
([subject.rs:169](../crates/eggbench-runner/src/subject.rs)); declared digest
≤ 128 bytes and a non-empty identity label.

The bound in `build()` is measured against a **compact** encoding
([subject.rs:101](../crates/eggbench-runner/src/subject.rs)) while the staged
artifact is serialized **pretty**
([subject.rs:113](../crates/eggbench-runner/src/subject.rs)), so the published
artifact can exceed `MAX_SUBJECT_SNAPSHOT_BYTES` by the whitespace delta. The
effective limit is the writer's `artifact_bytes` bound, checked separately.

### `prepare_bundle()`

The function validates everything it can before creating any filesystem state, then
creates the writer, then stages:

```rust
preparation.environment.validate()?;                 // prepare.rs:118
preparation.subject_snapshot.validate()?;            // prepare.rs:119
validate_source_plan(bytes, media_type, resolved)?;  // prepare.rs:120
let mut writer = BundleWriter::create(...)?;         // prepare.rs:126
```

Because the three checks precede `BundleWriter::create`, a rejected input leaves no
staging directory behind — asserted at
[prepare.rs:448](../crates/eggbench-runner/src/prepare.rs).

`validate_source_plan` ([prepare.rs:35](../crates/eggbench-runner/src/prepare.rs))
is the anti-forgery check between the two retained plan artifacts. It decodes the
source plan by media type, accepting only `application/json` and `application/toml`
([prepare.rs:42](../crates/eggbench-runner/src/prepare.rs)), then requires field
equality with the resolved plan across `schema_version`, `experiment`, `subject`,
`services`↔`topology`, `workload`, `trials`, `telemetry`, `metrics`,
`environment_policy`, `seed`, and `bounds`↔`artifact_bounds`
([prepare.rs:77](../crates/eggbench-runner/src/prepare.rs)). `plan.json` is the
operator's input, `resolved-plan.json` is the machine's view, and disagreement
blocks publication. Two checks are structural rather than scalar: `path_matches`
compares only `network_path.route` and `network_path.stream_faults.request`
([prepare.rs:54](../crates/eggbench-runner/src/prepare.rs)), and `paired_matches`
additionally requires the resolved schedule to equal `PAIRED_SCHEDULE_V1`
([prepare.rs:66](../crates/eggbench-runner/src/prepare.rs)).

The four primary artifacts:

| Path | Role | Media type | Sensitivity | Bytes from | Staged at |
|---|---|---|---|---|---|
| `plan.json` | `ExperimentPlan` | Caller's (`toml`/`json`) | `Redacted` | `source_plan_bytes`, verbatim | [prepare.rs:135](../crates/eggbench-runner/src/prepare.rs) |
| `resolved-plan.json` | `ResolvedPlan` | `application/json` | `Redacted` | `to_vec_pretty` + `validate_resolved_plan_bytes` | [prepare.rs:147](../crates/eggbench-runner/src/prepare.rs) |
| `environment.json` | `EnvironmentFingerprint` | `application/json` | `Redacted` | `to_vec_pretty` | [prepare.rs:159](../crates/eggbench-runner/src/prepare.rs) |
| `subject.json` | `Subject` | `application/json` | `Public` | `snapshot.to_json_bytes()` | [prepare.rs:170](../crates/eggbench-runner/src/prepare.rs) |

Two decisions are worth naming. `plan.json` is staged as raw bytes rather than
re-serialized, so the retained bytes are provably what the operator supplied — and
the parsed value `validate_source_plan` returns is discarded by its only caller.
The sensitivity split is asymmetric — three `Redacted`, one `Public` — which is
correct because the snapshot holds only a path, a digest, and declared labels,
non-secret by construction since its own `validate()` rejects fabrication.

Paired plans stage two extra snapshots (`subject-arm-baseline.json`,
`subject-arm-candidate.json`) and then set the manifest's paired record
([prepare.rs:182](../crates/eggbench-runner/src/prepare.rs)). The arm snapshots
pass `None` as the executable path
([prepare.rs:192](../crates/eggbench-runner/src/prepare.rs)) so they are
declared-only by construction, and a managed-command arm is rejected before staging
([prepare.rs:187](../crates/eggbench-runner/src/prepare.rs)): the runner never
launches or digests a paired arm subject, so accepting one would imply a guarantee
it cannot make. Multiple `Subject`-role artifacts are legal because the "exactly
once" rule covers only the three required primary roles
([evidence.rs:587](../crates/eggbench-core/src/evidence.rs)), and
`set_paired_record` must precede `finalize`
([evidence.rs:911](../crates/eggbench-core/src/evidence.rs)).

#### Why "still unpublished" is the point

`BundleWriter::create` refuses a pre-existing destination
([evidence.rs:879](../crates/eggbench-core/src/evidence.rs)) and creates a sibling
`.<name>.staging-<uuid>` directory
([evidence.rs:892](../crates/eggbench-core/src/evidence.rs)). The `.eggb` path
materializes exactly once, at the rename in `finalize`
([evidence.rs:1124](../crates/eggbench-core/src/evidence.rs)), and `BundleReader`
refuses to open any path containing `.staging-`
([evidence.rs:1151](../crates/eggbench-core/src/evidence.rs)). Staging before the
measured run buys four things:

- **No half-populated bundle can be mistaken for evidence.** A crash leaves a
  dot-prefixed staging directory that readers reject outright, and
  [evidence-bundle.md:67](../docs/evidence-bundle.md) documents it as reviewable
  but never repaired.
- **Bundle-level rejections cost no measurement time.** Count, per-artifact, and
  total-byte bounds are enforced inside `add_artifact`
  ([evidence.rs:1006](../crates/eggbench-core/src/evidence.rs)), so a too-small
  `ArtifactBounds` fails during preparation rather than after an hour of trials.
- **The manifest's required roles are guaranteed satisfiable.** `finalize` locates
  the three primary roles by role lookup
  ([evidence.rs:1067](../crates/eggbench-core/src/evidence.rs)) and manifest
  validation requires each exactly once
  ([evidence.rs:587](../crates/eggbench-core/src/evidence.rs)), so finalization
  cannot fail for a missing primary.
- **CLI input parsing stays out of the execution engine.** The CLI owns format
  selection and byte reading
  ([run.rs:338](../crates/eggbench-cli/src/commands/run.rs)), converts that into a
  borrowed `BundlePreparation`, and hands the orchestrator a finished writer
  ([run.rs:408](../crates/eggbench-cli/src/commands/run.rs),
  [run.rs:455](../crates/eggbench-cli/src/commands/run.rs)). `execute_run` receives
  no plan text, no media type, and no collector — it cannot parse, and cannot decide
  to re-parse, anything.

### Lifecycle evidence staging

All four helpers in [`bundle.rs`](../crates/eggbench-runner/src/bundle.rs) take
`&mut BundleWriter` and register through `writer.add_artifact` — no second writer,
no sidecar manifest, no bespoke envelope. The module-header claim
([bundle.rs:3](../crates/eggbench-runner/src/bundle.rs)) verifies: five
`add_artifact` call sites ([bundle.rs:60](../crates/eggbench-runner/src/bundle.rs),
[82](../crates/eggbench-runner/src/bundle.rs),
[114](../crates/eggbench-runner/src/bundle.rs),
[123](../crates/eggbench-runner/src/bundle.rs),
[176](../crates/eggbench-runner/src/bundle.rs)) and no other write path.

| Function | Artifact path | Role | Media type | Sensitivity | Sync? |
|---|---|---|---|---|---|
| `stage_runtime_topology` | `lifecycle/runtime-topology.json` | `Other { runtime-topology }` | `application/json` | `Redacted` | Yes |
| `stage_run_evidence` | `evidence.name()` | `Other { evidence.role_label() }` | `evidence.media_type()` | `evidence.sensitivity()` | Yes |
| `stage_lifecycle_logs` | `lifecycle/logs/<identity>.stdout` / `.stderr` | `Stdout` / `Stderr` | `application/octet-stream` | `Redacted` | `async` |
| `stage_lifecycle_metadata` | `lifecycle/lifecycle.json` | `Other { lifecycle }` | `application/json` | `Redacted` | Yes |

Three details are load-bearing. **Role and sensitivity travel with the data, not
the staging site:** for `stage_run_evidence`
([bundle.rs:77](../crates/eggbench-runner/src/bundle.rs)) the path, role label,
media type, and sensitivity all come off the `RunEvidenceArtifact` itself
([bundle.rs:81](../crates/eggbench-runner/src/bundle.rs)), so `bundle.rs` has no
literal it could get wrong. **Identity strings are sanitized before becoming path
components:** `sanitize_identity`
([bundle.rs:188](../crates/eggbench-runner/src/bundle.rs)) replaces every character
outside `[A-Za-z0-9_-]` with `_` and substitutes `unnamed` for an empty result —
defense in depth against a service name escaping the logs directory, since
plan-declared names are already `Name`-validated. The cost is a collision
possibility: `a.b` and `a-b` map to one filename and the second registration is
rejected as a duplicate path
([evidence.rs:972](../crates/eggbench-core/src/evidence.rs)) — loud, not silent.
**Staging reads retained state, so teardown order does not matter:** both
`stage_runtime_topology` and `stage_lifecycle_metadata` read the session after
shutdown ([bundle.rs:56](../crates/eggbench-runner/src/bundle.rs),
[bundle.rs:161](../crates/eggbench-runner/src/bundle.rs)); `LocalSession` retains
spawn order, spools, events, and topology so final evidence can be assembled once
processes are gone.

One writer means lifecycle artifacts inherit the primary artifacts' guarantees for
free: the 64 KiB streaming copy and SHA-256 registration
([evidence.rs:989](../crates/eggbench-core/src/evidence.rs)), bounds enforcement,
safe-parent creation, duplicate rejection, and the staging-tree re-verification done
in `finalize`
([evidence.rs:1103](../crates/eggbench-core/src/evidence.rs)). A truncated log is
retained byte-for-byte from the session spool, so the spool's truncation is what
becomes evidence — `bundle.rs` does not re-interpret it. A lifecycle-only run is
still a complete bundle: the three primary roles are already staged, trials are
optional, and `comparison_verdict` stays `None`
([bundle.rs:5](../crates/eggbench-runner/src/bundle.rs)) — and a verdict without a
comparison artifact is itself rejected
([evidence.rs:518](../crates/eggbench-core/src/evidence.rs)).

### Secret indirection

Plans reference secrets by name only: `Subject::ManagedCommand.environment` is a
`BTreeMap<String, SecretRef>`
([plan.rs:232](../crates/eggbench-core/src/plan.rs)), `SecretRef`'s `Debug` renders
redacted (pinned at [subject.rs:331](../crates/eggbench-runner/src/subject.rs)), and
no value type appears in the subject declaration at all.

Resolution happens in exactly one place, the spawn-plan build
([spec.rs:221](../crates/eggbench-runner/src/spec.rs)): for each declared
`(key, secret_ref)` the builder calls
`options.secrets.resolve(&secret_ref.reference)` and, on `None`, fails with
`RunnerError::MissingSecret` carrying the *reference name*
([spec.rs:225](../crates/eggbench-runner/src/spec.rs)). On success the value goes
into the in-memory `ProcessSpec.env`
([spec.rs:237](../crates/eggbench-runner/src/spec.rs)) while the sorted reference
*names* are recorded separately in `secret_references`
([spec.rs:238](../crates/eggbench-runner/src/spec.rs)).

So the indirection holds where it matters: values are read from the caller's
provider *after* plan resolution, so they cannot be in the plan, cannot be in the
`ResolvedPlan`, and cannot be in `resolved-plan.json`. The only trace in evidence is
the names-only `secret_references` list. `prepare.rs` and `bundle.rs` have no access
to a provider at all — `BundlePreparation` holds no secrets — so no code path lets a
value reach a staged artifact. Two honest limits: this is an *evidence* invariant,
not a memory one (the resolved value is a plain `String` in a `ProcessSpec` and is
not zeroed on drop), and the CLI currently wires an empty provider
([run.rs:387](../crates/eggbench-cli/src/commands/run.rs)), so a managed subject
declaring any environment secret fails with `MissingSecret` in preflight. The
indirection is fully wired; the value source is not yet populated outside tests and
embeddings.

### Error surface and secondary cleanup failures

`RunnerError` variants split cleanly by phase. Pre-spawn variants — `InvalidPlan`,
`UnsupportedService`, `UnsupportedProbe`, `UnsupportedPlatform`, `MissingSecret`,
`InvalidWorkingDirectory`, `InvalidExecutablePath`, `EmptyArgv`,
`CancelledBeforeSpawn` — describe conditions detected before anything started.
Post-spawn variants — `SpawnFailed`, `ReadinessFailed`, `ReadinessTimeout`,
`ProcessExitedEarly`, `Cancelled` — describe failures that leave teardown work
behind. Only the post-spawn group carries `cleanup: Vec<CleanupFailure>`
([error.rs:104](../crates/eggbench-runner/src/error.rs),
[114](../crates/eggbench-runner/src/error.rs),
[124](../crates/eggbench-runner/src/error.rs),
[134](../crates/eggbench-runner/src/error.rs),
[144](../crates/eggbench-runner/src/error.rs)). `RunnerError::cleanup()` returns
that vector, or `&[]` for every pre-spawn variant
([error.rs:154](../crates/eggbench-runner/src/error.rs)).

The primary cause is preserved structurally, not by convention. The primary failure
*is* the variant: there is no "cleanup error" variant that could displace it and no
flattening step that could prefer one over the other. Each variant's `Display`
renders only service and message
([error.rs:104](../crates/eggbench-runner/src/error.rs)), so a `{}` print can never
substitute a teardown complaint for the initiating failure; cleanup detail is
retrieved separately and structurally through `cleanup()`. `CleanupFailure` has its
own `Display` — `cleanup(<service>): <reason>`
([error.rs:31](../crates/eggbench-runner/src/error.rs)) — for callers that want to
report teardown trouble alongside the primary.

`CleanupFailure::reason` is documented as a *stable machine-readable* reason
([error.rs:17](../crates/eggbench-runner/src/error.rs)) and flows verbatim into
`lifecycle.json` as a `CleanupSummary.reason`
([bundle.rs:157](../crates/eggbench-runner/src/bundle.rs)). Reason strings are
therefore part of the evidence contract, not free-form log text: they must stay
non-secret, and stable across releases if downstream tooling keys off them.

Preparation errors are separate and smaller. `EnvironmentError`
([environment.rs:81](../crates/eggbench-runner/src/environment.rs)) covers a
rejected field name or a failed schema validation. `SubjectSnapshotError`
([prepare.rs:221](../crates/eggbench-runner/src/prepare.rs)) is the stable
CLI-facing category for a digest mismatch or a build failure. `PrepareError`
([prepare.rs:264](../crates/eggbench-runner/src/prepare.rs)) aggregates the three
pre-start domains — environment, snapshot, bundle — and `collect_local_environment`
returns on the first failure
([prepare.rs:253](../crates/eggbench-runner/src/prepare.rs)).

## Invariants

| # | Invariant | Enforced at |
|---|---|---|
| 1 | Fingerprint and subject snapshot are established before any spawn | Call order from [run.rs:365](../crates/eggbench-cli/src/commands/run.rs); no side effects in the collector type |
| 2 | A failed collection leaves nothing running | Same ordering; `LocalSession::prepare` starts no process ([session.rs:290](../crates/eggbench-runner/src/session.rs)) |
| 3 | Missing optional facts stay absent; no placeholder values | `None` dropped at [environment.rs:52](../crates/eggbench-runner/src/environment.rs); blank-after-trim dropped at [environment.rs:53](../crates/eggbench-runner/src/environment.rs) |
| 4 | No transient value is ever comparison-critical | Classification at [environment.rs:165](../crates/eggbench-runner/src/environment.rs); test at [environment.rs:662](../crates/eggbench-runner/src/environment.rs) |
| 5 | Fingerprint output is byte-deterministic on an unchanged host | `BTreeMap` at [environment.rs:50](../crates/eggbench-runner/src/environment.rs); test at [environment.rs:612](../crates/eggbench-runner/src/environment.rs) |
| 6 | Fingerprint is bounded and schema-validated before use | [environment.rs:74](../crates/eggbench-runner/src/environment.rs) → [evidence.rs:727](../crates/eggbench-core/src/evidence.rs) |
| 7 | Only managed subjects receive an observed digest | [subject.rs:75](../crates/eggbench-runner/src/subject.rs); rejected otherwise at [subject.rs:160](../crates/eggbench-runner/src/subject.rs) |
| 8 | A retained source plan may not contradict the resolved plan | [prepare.rs:77](../crates/eggbench-runner/src/prepare.rs) |
| 9 | Rejected preparation leaves no staging directory | Validation at [prepare.rs:118](../crates/eggbench-runner/src/prepare.rs) precedes `create` at [prepare.rs:126](../crates/eggbench-runner/src/prepare.rs) |
| 10 | The `.eggb` path exists only after an atomic rename | [evidence.rs:892](../crates/eggbench-core/src/evidence.rs), [evidence.rs:1124](../crates/eggbench-core/src/evidence.rs) |
| 11 | One evidence format; one write path | Five `add_artifact` sites in [bundle.rs](../crates/eggbench-runner/src/bundle.rs) |
| 12 | Secret values never reach a plan, resolved plan, or manifest | `SecretRef` by name at [plan.rs:232](../crates/eggbench-core/src/plan.rs); resolution at [spec.rs:221](../crates/eggbench-runner/src/spec.rs); no provider in `BundlePreparation` |
| 13 | A secondary cleanup failure never replaces the primary cause | Field, not variant; [error.rs:154](../crates/eggbench-runner/src/error.rs) |
| 14 | Paired arm subjects are declared-only and never managed | [prepare.rs:187](../crates/eggbench-runner/src/prepare.rs) |

## Failure model

| Domain | Type | Raised when | Managed processes at risk? |
|---|---|---|---|
| Fingerprint | `EnvironmentError::InvalidName` | A field name fails `Name` validation ([environment.rs:57](../crates/eggbench-runner/src/environment.rs)) | No |
| Fingerprint | `EnvironmentError::Schema` | Resulting fingerprint fails schema/bounds validation | No |
| Subject | `SubjectSnapshotError::DeclaredDigestMismatch` | Declared digest ≠ observed digest | No |
| Subject | `SubjectSnapshotError::Build` | Snapshot cannot be built, e.g. the executable cannot be hashed ([subject.rs:215](../crates/eggbench-runner/src/subject.rs)) | No |
| Preparation | `PrepareError::Bundle` | Writer creation, plan decode, contradiction, or staging failure | No |
| Lifecycle, pre-spawn | `InvalidPlan`, `UnsupportedService`, `UnsupportedProbe`, `UnsupportedPlatform`, `InvalidWorkingDirectory`, `InvalidExecutablePath`, `EmptyArgv`, `MissingSecret`, `CancelledBeforeSpawn` | Preflight rejection ([error.rs:44](../crates/eggbench-runner/src/error.rs)) | No — `cleanup()` is `&[]` |
| Lifecycle, post-spawn | `SpawnFailed`, `ReadinessFailed`, `ReadinessTimeout`, `ProcessExitedEarly`, `Cancelled` | Failure after at least one spawn ([error.rs:104](../crates/eggbench-runner/src/error.rs)) | Yes — teardown performed; residual trouble in `cleanup()` |

The split is the useful part: `RunnerError::cleanup()` returning an empty slice is a
reliable signal that nothing was left running, because the post-spawn group is
exactly the set of variants that can leave a process tree behind
([error.rs:154](../crates/eggbench-runner/src/error.rs)). Consumers need not
reconstruct which phase a failure belonged to. Three fail-closed gates sit before
measurement rather than after: an environment or snapshot failure returns before
`LocalSession::prepare`
([run.rs:368](../crates/eggbench-cli/src/commands/run.rs)); a declared/observed
digest mismatch returns before the writer is even created
([run.rs:376](../crates/eggbench-cli/src/commands/run.rs)); and a paired
managed-command arm is rejected inside `prepare_bundle`
([prepare.rs:187](../crates/eggbench-runner/src/prepare.rs)).

## Boundaries

- **No comparison.** The three classes are recorded, not applied. Nothing here
  diffs two fingerprints or refuses a run on a `comparison_critical` mismatch;
  `EnvironmentPolicy` ([plan.rs:607](../crates/eggbench-core/src/plan.rs)) is a
  resolved-plan input this layer only cross-checks for agreement
  ([prepare.rs:85](../crates/eggbench-runner/src/prepare.rs)).
- **No metric normalization and no driver selection.** `environment.json` and
  `subject.json` are run-scoped context, not measurement output.
- **No CLI concerns.** Format selection, file reading, and exit-code mapping are
  the CLI's ([run.rs:338](../crates/eggbench-cli/src/commands/run.rs),
  [run.rs:371](../crates/eggbench-cli/src/commands/run.rs)). The runner takes bytes
  and a media-type string.
- **No shell execution.** Every external probe is a fixed `argv` spawned with
  `std::process::Command`, child environment cleared and stderr discarded
  ([environment.rs:557](../crates/eggbench-runner/src/environment.rs)); a failed
  probe degrades to an absent field rather than propagating.
- **No secret material.** No function in `environment.rs`, `subject.rs`,
  `prepare.rs`, or `bundle.rs` can see a secret value.
- **No lifecycle authority.** `bundle.rs` stages what the session retained; it does
  not decide when to stop anything. Process ownership and cleanup live in the
  session and platform layers.

## Tests and qualification

| Test | Site | Qualifies |
|---|---|---|
| `fingerprint_has_no_secret_values` | [environment.rs:596](../crates/eggbench-runner/src/environment.rs) | No field value matches secret-ish patterns |
| `fingerprint_is_deterministic` | [environment.rs:612](../crates/eggbench-runner/src/environment.rs) | Two consecutive collections are equal |
| `fingerprint_validates` | [environment.rs:619](../crates/eggbench-runner/src/environment.rs) | Schema version and bounds |
| `parse_cpu_range_count_handles_union_and_single` | [environment.rs:630](../crates/eggbench-runner/src/environment.rs) | Linux CPU range parsing |
| `windows_missing_version_stays_absent` | [environment.rs:637](../crates/eggbench-runner/src/environment.rs) | Absent rather than fabricated, on Windows |
| `macos_cpuinfo_helper_ignores_key_without_unused_warning` | [environment.rs:646](../crates/eggbench-runner/src/environment.rs) | cfg-gated helper compiles cleanly |
| `build_profile_is_present` | [environment.rs:652](../crates/eggbench-runner/src/environment.rs) | Informational field presence |
| `transient_load_never_marked_comparison_critical` | [environment.rs:662](../crates/eggbench-runner/src/environment.rs) | Class ceiling for frequency |
| `prepare_bundle_stages_all_four_primary_artifacts` | [prepare.rs:393](../crates/eggbench-runner/src/prepare.rs) | All four present in staging |
| `prepare_bundle_rejects_invalid_environment` | [prepare.rs:418](../crates/eggbench-runner/src/prepare.rs) | Oversized value rejected, **and no destination created** ([prepare.rs:448](../crates/eggbench-runner/src/prepare.rs)) |
| `prepare_bundle_rejects_unsafe_subject` | [prepare.rs:452](../crates/eggbench-runner/src/prepare.rs) | Forged digest on an external subject rejected |
| `collect_local_environment_produces_snapshot_and_environment` | [prepare.rs:484](../crates/eggbench-runner/src/prepare.rs) | Both outputs validate |
| `resolved_plan_artifacts_can_be_serialized_and_loaded` | [prepare.rs:492](../crates/eggbench-runner/src/prepare.rs) | Canonical round-trip |
| `managed_subject_records_resolved_digest_and_mismatch` | [subject.rs:268](../crates/eggbench-runner/src/subject.rs) | Observed digest + `Some(false)` |
| `declared_digest_match_is_detected` | [subject.rs:287](../crates/eggbench-runner/src/subject.rs) | `Some(true)` path |
| `external_subject_has_no_executable_digest` | [subject.rs:307](../crates/eggbench-runner/src/subject.rs) | No fabrication |
| `label_subject_is_diagnostic_only` | [subject.rs:315](../crates/eggbench-runner/src/subject.rs) | Label-only identity |
| `declared_digest_matches_returns_helper` | [subject.rs:325](../crates/eggbench-runner/src/subject.rs) | `None` for external |
| `secret_reference_debug_redacts` | [subject.rs:331](../crates/eggbench-runner/src/subject.rs) | `SecretRef` redaction |

Coverage gaps worth naming. [`secret.rs`](../crates/eggbench-runner/src/secret.rs),
[`error.rs`](../crates/eggbench-runner/src/error.rs), and
[`bundle.rs`](../crates/eggbench-runner/src/bundle.rs) contain **no** in-file test
module, so `MapSecretProvider` redaction, the `cleanup()` phase split,
`sanitize_identity` collisions, and the `stage_lifecycle_*` artifact paths and roles
are not pinned by unit tests at this layer. And "no process starts before
collection" is verified only by call-graph inspection: a regression reordering
`collect_local_environment` after `LocalSession::startup` would compile and pass
every test in this table.

## Review focus

Ordered by how quietly a change breaks something.

1. **Reordering around `LocalSession::prepare`.** Nothing enforces invariant 2. If
   collection moves after startup, every test still passes and orphan processes
   become possible. Check the order at
   [run.rs:365](../crates/eggbench-cli/src/commands/run.rs) explicitly.
2. **Adding or reclassifying an environment fact.** Classification is a deliberate
   decision with comparison consequences, not a default. Adding a fact means
   updating [environment-fingerprint.md:25](../docs/environment-fingerprint.md) in
   the same change. Never mark a live-read value `ComparisonCritical` — and note
   that switching `current_cpu_frequency_mhz` to a real `scaling_cur_freq` read
   would break the determinism test at
   [environment.rs:612](../crates/eggbench-runner/src/environment.rs).
3. **Changing the declared/observed digest semantics.** `declared_digest_matches` is
   `Some(false)` both for a genuine mismatch and for a managed subject that
   declared no digest
   ([subject.rs:83](../crates/eggbench-runner/src/subject.rs)); a consumer
   distinguishing those cases needs a tri-state, not a `bool`.
4. **The `network_path` cross-check is partial.** `path_matches` compares only
   `route` and `stream_faults.request`
   ([prepare.rs:54](../crates/eggbench-runner/src/prepare.rs)). A new
   `network_path` field requires an explicit decision about whether it joins the
   source/resolved contradiction check.
5. **Adding a second artifact write path.** Any new `fs::write` into the staging
   tree bypasses bounds, digest registration, and final re-verification. Use
   `add_artifact`.
6. **Paired-run artifact growth.** Arm snapshots add two more `Subject`-role
   artifacts before any measurement, bounded by `artifact_count` — which the
   prepare tests set to 100
   ([prepare.rs:305](../crates/eggbench-runner/src/prepare.rs)). Keep that in mind
   if the primary-artifact count grows.
7. **Secret resolution sites.** A `SecretProvider` read anywhere reachable from
   `prepare.rs` or `bundle.rs` would break invariant 12. Values belong only in
   `ProcessSpec.env` ([spec.rs:237](../crates/eggbench-runner/src/spec.rs)).
8. **Error `Display` strings and `CleanupFailure::reason`.** Both reach
   `lifecycle.json` as evidence
   ([bundle.rs:157](../crates/eggbench-runner/src/bundle.rs)); they must stay
   non-secret, and `reason` must stay stable.

## Related

- [`overview.md`](./overview.md) — crate layering and the evidence bundle layout.
- [`runner.md`](./runner.md) — the M003 pre-start preparation section this document
  details.
- [`../docs/environment-fingerprint.md`](../docs/environment-fingerprint.md) —
  schema v1 field table and class semantics.
- [`../docs/evidence-bundle.md`](../docs/evidence-bundle.md) — staging,
  finalization, and the `.staging-` discard rule.
- [`../docs/local-runner-lifecycle.md`](../docs/local-runner-lifecycle.md) —
  platform process-ownership guarantees bounding the lifecycle error variants.
- Sources: [`environment.rs`](../crates/eggbench-runner/src/environment.rs),
  [`prepare.rs`](../crates/eggbench-runner/src/prepare.rs),
  [`subject.rs`](../crates/eggbench-runner/src/subject.rs),
  [`bundle.rs`](../crates/eggbench-runner/src/bundle.rs),
  [`secret.rs`](../crates/eggbench-runner/src/secret.rs),
  [`error.rs`](../crates/eggbench-runner/src/error.rs).
