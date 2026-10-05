# Evidence bundles: the `.eggb` contract

## Purpose

`crates/eggbench-core/src/evidence.rs` (2,487 lines) owns one contract: an
`.eggb` directory is a self-contained, immutable set of files plus one
authoritative `manifest.json` written after every other byte it describes, and
published into place by a single rename that refuses an existing destination.

The module holds four things: a **writer** that streams and hashes artifacts into
a hidden staging sibling
([evidence.rs:839](../crates/eggbench-core/src/evidence.rs)), the **manifest DTO**
and its validator
([evidence.rs:377](../crates/eggbench-core/src/evidence.rs)), a **read-only
reader** ([evidence.rs:1134](../crates/eggbench-core/src/evidence.rs)), and one
error enum covering creation, path, schema, and verification failures
([evidence.rs:750](../crates/eggbench-core/src/evidence.rs)).

Two properties drive every decision in the file. **The manifest is
authoritative:** it never embeds artifact contents, and an entry is metadata only
— confined path, logical role, media type, sensitivity label, exact byte count,
SHA-256 digest
([evidence.rs:256](../crates/eggbench-core/src/evidence.rs)) — so readers decide
what exists from the manifest and directory naming is a human convention, never a
definition of evidence. **There is no mutation API for a published bundle:** the
only `remove_*` call deletes a partially written artifact inside the writer's own
staging directory
([evidence.rs:1026](../crates/eggbench-core/src/evidence.rs)), and no `Drop`
repairs or cleans a staging tree, so a failed `finalize` leaves it on disk for
explicit operator disposal.

Crate-level rationale is in [evidence.md](evidence.md); the user-facing contract is
[docs/evidence-bundle.md](../docs/evidence-bundle.md).

## Source layout

| Region | Lines (evidence.rs) | Contents |
|---|---|---|
| Limits, identity | 19–33 | `EVIDENCE_MANIFEST_SCHEMA_VERSION = 2`, `ENVIRONMENT_FINGERPRINT_SCHEMA_VERSION = 1`, `MAX_MANIFEST_BYTES`, `MAX_ARTIFACT_COUNT`, `MAX_ARTIFACT_BYTES`, `MAX_TOTAL_ARTIFACT_BYTES`, `MAX_PATH_BYTES`, `MAX_PATH_DEPTH`, `HASH_BUFFER_BYTES` |
| Bounded primitives | 35–162 | `RunId`, `TrialId`, `ArtifactPath` |
| Status vocabularies | 164–206 | `LegacyRunStatus`, `ExecutionStatus`, `ComparisonVerdict` |
| Artifact and trial types | 208–373 | `ArtifactRole`, `Sensitivity`, `ArtifactRecord`, `TrialDescriptor`, `TrialExecutionResult`, `TrialArm`, `TrialExecutionStatus`, `TrialExecutionFailure`, `PairedRunRecord` |
| Manifest DTOs | 375–492 | `BundleManifest` + `validate_inner`, `LegacyManifestV1` + `into_current_view` |
| Environment fingerprint | 681–746 | `EnvironmentFingerprint`, `EnvironmentFieldClass`, `EnvironmentField`, `validate` |
| Errors | 748–801 | `BundleError` |
| Writer | 803–1131 | `validate_bounds`, `is_windows_device_name`, `BundleWriter::create/add_artifact/finalize` |
| Reader | 1133–1319 | `BundleReader::open/verify/trial_metrics/open_artifact/validate_paths` |
| Filesystem helpers | 1321–1610 | `unix_time_ms`, `to_hex`, `hash_reader`, Unix/non-Unix `secure_open`, Linux/non-Linux `publish_staging`, `create_safe_parents`, `check_no_symlink_components`, `verify_artifact_tree`, `collect_files`, `sync_directory` |
| Resolved-plan gate | 1612–1771 | `validate_resolved_network_path*`, `validate_resolved_plan_bytes` |
| Tests | 1773–2487 | 16 unit tests plus shared helpers |

## Key types

`RunId` is a UUID v4, explicitly independent of host process identifiers
([evidence.rs:36](../crates/eggbench-core/src/evidence.rs)). `TrialId` is a
`NonZeroU32`, so trial identity is numeric and never inferred from directory
ordering ([evidence.rs:68](../crates/eggbench-core/src/evidence.rs)).

`ArtifactPath` is the security-relevant type. `ArtifactPath::new` rejects a
value unless every rule below holds, because the path must stay portable and
confined on all supported filesystems
([evidence.rs:111](../crates/eggbench-core/src/evidence.rs)):

| Rejected | Rule |
|---|---|
| empty | `value.is_empty()` |
| overlong / over-deep | `> MAX_PATH_BYTES` (1,024) / `> MAX_PATH_DEPTH` (16) segments |
| absolute | starts with `/` |
| Windows-illegal | contains `\`, or a segment contains `:` ([evidence.rs:124](../crates/eggbench-core/src/evidence.rs)) |
| control characters | any `char::is_control` |
| dot segments | any segment equal to `.` or `..` |
| trailing space or dot | Windows path-normalization hazards |
| Windows device name | `CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9` ([evidence.rs:824](../crates/eggbench-core/src/evidence.rs)) |
| reserved root name | equals `manifest.json`, case-insensitively ([evidence.rs:130](../crates/eggbench-core/src/evidence.rs)) |

The reserved-name test is an exact whole-path comparison, so a nested
`trials/001/manifest.json` is *not* reserved. That is safe: the root
`manifest.json` is the only file the reader opens by literal name
([evidence.rs:1172](../crates/eggbench-core/src/evidence.rs)).

`ArtifactRole` is internally tagged (`kind`) with `deny_unknown_fields`, so the
role set is closed plus one named escape hatch: `experiment_plan`,
`resolved_plan`, `environment_fingerprint`, `topology`, `subject`,
`trial_result`, `telemetry`, `stdout`, `stderr`, `trial_artifact`,
`comparison`, `report`, and `other { label: Name }`
([evidence.rs:210](../crates/eggbench-core/src/evidence.rs)).
`Sensitivity` (`public` / `redacted` / `sensitive`) is a stored label only; the
manifest never carries secret contents, and the label is metadata for a future
integration policy ([evidence.rs:244](../crates/eggbench-core/src/evidence.rs)).

`BundleManifest` is the authoritative DTO
([evidence.rs:377](../crates/eggbench-core/src/evidence.rs)): `schema_version`,
`run_id`, `execution_status`, `comparison_verdict`, `subject`,
`created_unix_ms` / `finalized_unix_ms`, the three required references (`plan`,
`resolved_plan`, `environment`), `trials`, optional `comparison`, optional
`paired`, optional `report`, `drivers`, `limits`
([plan.rs:618](../crates/eggbench-core/src/plan.rs)), the full `artifacts` list,
and `finalized`.

`TrialDescriptor` (`id`, `result`, `artifacts`) and `TrialExecutionResult`
separate *what* a trial was from *what happened* to it. The latter holds only
execution facts — schema version, trial id, measurement start offset and elapsed
nanoseconds, `terminal_status`, optional `failure_category`, and the additive
`arm` / `pair_id` fields that stay absent for schema-v1 evidence
([evidence.rs:289](../crates/eggbench-core/src/evidence.rs)). No metric
interpretation lives in either type; normalized metrics arrive as a separate
artifact read through `trial_metrics`.

`EnvironmentFingerprint` is a versioned map of `Name → EnvironmentField`, each
field carrying a value and an `EnvironmentFieldClass` (`comparison_critical` /
`warning_only` / `informational`)
([evidence.rs:684](../crates/eggbench-core/src/evidence.rs),
[docs/environment-fingerprint.md](../docs/environment-fingerprint.md)). Its
`validate` bounds the field count to 128 and each value to 4 KiB with no control
characters ([evidence.rs:727](../crates/eggbench-core/src/evidence.rs)).

## How it works

### The writer, and why the order is the contract

**1. Staging creation.** `BundleWriter::create`
([evidence.rs:855](../crates/eggbench-core/src/evidence.rs)) requires the
destination to end in `.eggb`, normalizes a bare file name to a `.` parent,
refuses an existing destination via `symlink_metadata`, requires a real parent
directory, validates the declared bounds *before* creating anything, then
creates a hidden sibling `.{name}.eggb.staging-{uuid}`
([evidence.rs:892](../crates/eggbench-core/src/evidence.rs)). Sibling placement
is load-bearing — publication needs a same-filesystem rename — and the hidden
`.staging-` marker is what lets the reader refuse in-progress work.

**2. Streaming an artifact.** `add_artifact`
([evidence.rs:957](../crates/eggbench-core/src/evidence.rs)) validates the media
type, rejects duplicate paths and count overruns, creates parent directories
while refusing to follow a symlinked component
([evidence.rs:1466](../crates/eggbench-core/src/evidence.rs)), and opens the
file with `create_new` so a pre-existing staging file can never be silently
overwritten ([evidence.rs:983](../crates/eggbench-core/src/evidence.rs)).

The copy loop reads and hashes in one pass over a fixed 64 KiB buffer, so an
artifact is never buffered in full regardless of size
([evidence.rs:990](../crates/eggbench-core/src/evidence.rs)). Per-chunk
arithmetic uses `checked_add`, and both the per-artifact and total-byte bounds are
enforced *before* each write
([evidence.rs:1000](../crates/eggbench-core/src/evidence.rs)): a violation aborts
with the file uncommitted rather than after a truncated-then-oversized write. On
success the file is `sync_all`-flushed and the handle dropped; on failure the
partial file is removed and the running byte total is left untouched
([evidence.rs:1018](../crates/eggbench-core/src/evidence.rs),
[evidence.rs:1026](../crates/eggbench-core/src/evidence.rs)). Records go into a
`BTreeMap`, so the manifest's artifact list is canonically path-ordered.

**3. Finalization.** `finalize` consumes `self`, so a writer can be finalized at
most once ([evidence.rs:1051](../crates/eggbench-core/src/evidence.rs)). It
then, in this exact order:

1. Discovers the plan, resolved-plan, and environment references by **role**, not
   from caller-supplied paths
   ([evidence.rs:1061](../crates/eggbench-core/src/evidence.rs)).
2. Canonically sorts trial artifacts, trials by id, and drivers by name
   ([evidence.rs:1076](../crates/eggbench-core/src/evidence.rs)).
3. Builds the manifest with `finalized: true` and validates it
   ([evidence.rs:1102](../crates/eggbench-core/src/evidence.rs)).
4. Re-verifies the staged tree with `manifest_present: false`
   ([evidence.rs:1103](../crates/eggbench-core/src/evidence.rs)) — every staged
   file must be manifest-listed, regular, exact-sized, and hash-matching.
5. Serializes the manifest, enforces the 4 MiB cap, writes it `create_new`, and
   flushes it ([evidence.rs:1104](../crates/eggbench-core/src/evidence.rs)).
6. Drops the manifest handle, syncs the staging directory, publishes, syncs the
   parent, then re-opens and verifies the published bundle
   ([evidence.rs:1121](../crates/eggbench-core/src/evidence.rs),
   [evidence.rs:1127](../crates/eggbench-core/src/evidence.rs)).

**The ordering argument.** Nothing names evidence before the bytes exist and
have been hashed, because the manifest is the only thing that names anything; and
the manifest lands after the last content check, so a staging directory is either
manifest-less — refused as `IncompleteBundle`
([evidence.rs:1151](../crates/eggbench-core/src/evidence.rs)) — or
manifest-bearing with a manifest describing a tree verified microseconds
earlier. Publication is a single rename, so a `.eggb` path never exists
half-populated. That is the manifest-last publication property: a bundle cannot
contain evidence of its own publication ([overview.md](overview.md)).

### Publication is one rename, with no copy fallback

| Platform | Mechanism | Existence / failure handling | Lines |
|---|---|---|---|
| Linux | `renameat_with(RENAME_NOREPLACE)` on a parent handle opened `RDONLY\|DIRECTORY\|CLOEXEC\|NOFOLLOW` | `EXDEV`→`DestinationExists`; `XDEV`/`INVAL`/`NOSYS`/`OPNOTSUPP`→`AtomicRenameUnsupported`; other errnos→`Io` | 1389–1422 |
| Everything else | `create_new` lock `.{name}.finalize-lock`, `symlink_metadata` re-check, then `fs::rename` | lock `AlreadyExists`→`DestinationExists`; `CrossesDevices`→`AtomicRenameUnsupported` | 1424–1457 |

Linux uses the no-replace flag, so the existence check and the rename are one
syscall with no check-then-act window
([evidence.rs:1408](../crates/eggbench-core/src/evidence.rs)). The portable path
cannot express "rename but do not replace" through `std`, so it emulates the
reservation with an atomically created sentinel, drops its handle immediately,
re-checks the destination, and only then renames
([evidence.rs:1431](../crates/eggbench-core/src/evidence.rs)).

**No cross-filesystem copy fallback exists, by design.** A copy cannot be atomic,
so it would produce a destination that looks finalized while holding a partially
copied tree — the exact failure the rename exists to prevent. When the filesystem
cannot support the rename, finalization fails loudly
(`AtomicRenameUnsupported`) instead of degrading
([evidence.rs:1046](../crates/eggbench-core/src/evidence.rs),
[evidence.rs:1417](../crates/eggbench-core/src/evidence.rs)).

**Windows durability.** `sync_directory` is a documented no-op on Windows
because no portable directory sync exists through `std`
([evidence.rs:1606](../crates/eggbench-core/src/evidence.rs)). Artifact and
manifest file *contents* are still flushed individually
([evidence.rs:1018](../crates/eggbench-core/src/evidence.rs),
[evidence.rs:1117](../crates/eggbench-core/src/evidence.rs)); only
directory-entry durability is delegated to the OS and filesystem. The manifest
handle is explicitly closed before the directory sync and rename, because Windows
cannot rename a directory while a handle without delete-sharing is open
([evidence.rs:1121](../crates/eggbench-core/src/evidence.rs)).

### The read path is a security boundary

`BundleReader::open` ([evidence.rs:1145](../crates/eggbench-core/src/evidence.rs))
runs a fixed rejection sequence (all ranges in `evidence.rs`):

| Check | Failure | Lines |
|---|---|---|
| name contains `.staging-`, or extension is not `.eggb` | `IncompleteBundle` | 1151–1155 |
| root is a symlink, or not a directory | `Symlink` / `IncompleteBundle` | 1156–1162 |
| `manifest.json` is a symlink | `Symlink` | 1164–1168 |
| `manifest.json` larger than 4 MiB, before any read | `BoundExceeded` | 1169–1171 |
| manifest read through the no-follow opener, `take(cap + 1)` | `Io` / `BoundExceeded` | 1172–1182 |
| `schema_version` absent or not a number | `ManifestParse` | 1183–1187 |
| schema version not 1 or 2 | `UnsupportedManifestVersion` | 1188–1210 |
| manifest invariants | `InvalidManifest` / `MissingArtifact` / `IncompleteBundle` | 1211–1215 |
| symlink in any artifact path component | `Symlink` / `UnsafeArtifactPath` | 1221 |

`verify` re-runs the path check, then enumerates the tree
([evidence.rs:1250](../crates/eggbench-core/src/evidence.rs)): `collect_files`
rejects any symlink and any entry that is neither a regular file nor a
directory, so sockets, FIFOs, and devices surface as `ExtraFile`
([evidence.rs:1571](../crates/eggbench-core/src/evidence.rs)). The on-disk set
must equal the manifest set exactly in both directions, with `manifest.json`
excluded from the on-disk side
([evidence.rs:1530](../crates/eggbench-core/src/evidence.rs)). Each artifact is
re-`symlink_metadata`d, required to be a regular file, compared on size, opened
through the no-follow opener, and re-hashed in 64 KiB chunks
([evidence.rs:1541](../crates/eggbench-core/src/evidence.rs)). `open_artifact`
refuses any path that is not manifest-listed before touching the filesystem
([evidence.rs:1300](../crates/eggbench-core/src/evidence.rs)); no API opens a
caller-supplied path blindly.

**On Unix, opens are relative to directory handles with no-follow flags**
([evidence.rs:1354](../crates/eggbench-core/src/evidence.rs)). The root is
opened `RDONLY|DIRECTORY|CLOEXEC|NOFOLLOW`; every intermediate component is then
walked with `openat` under the same flags, so a component replaced by a symlink
after the walk began cannot redirect the open; the leaf is opened with
`openat(RDONLY|CLOEXEC|NOFOLLOW|NONBLOCK)`
([evidence.rs:1356](../crates/eggbench-core/src/evidence.rs)). Two consequences
deserve precision:

- The no-follow guarantee covers symlinks at **every** component including the
  leaf, and it is enforced by the kernel at open time, not by a prior `stat`.
- "Must be a regular file" is *not* part of `open_artifact`; it is enforced by
  `verify_artifact_tree` ([evidence.rs:1547](../crates/eggbench-core/src/evidence.rs)).
  `NONBLOCK` is what stops opening a substituted FIFO from blocking, and digest
  verification is what detects it.

On non-Unix, `secure_open` falls back to a metadata walk plus a plain
`File::open` ([evidence.rs:1460](../crates/eggbench-core/src/evidence.rs)): the
symlink check precedes the open, so no-follow there is a check-then-act sequence.

### Manifest v1 versus v2

The reader sniffs the top-level `schema_version` from a generic JSON value
before choosing a DTO
([evidence.rs:1183](../crates/eggbench-core/src/evidence.rs)).

**v1** decodes into the explicit `LegacyManifestV1` DTO carrying the overloaded
`status: LegacyRunStatus`
([evidence.rs:421](../crates/eggbench-core/src/evidence.rs)). That enum's
`Inconclusive` variant is documented as ambiguous between "no comparison" and
"an inconclusive comparison"
([evidence.rs:177](../crates/eggbench-core/src/evidence.rs)). The normalized view
maps `Succeeded` **and** `Inconclusive` to `ExecutionStatus::Completed` and
always sets `comparison_verdict: None`
([evidence.rs:461](../crates/eggbench-core/src/evidence.rs)), while the original
value stays reachable through `BundleReader::legacy_status`
([evidence.rs:1236](../crates/eggbench-core/src/evidence.rs)). A legacy
`inconclusive` therefore never becomes a verdict — and a consumer that reads only
`manifest().execution_status` will over-report such a run as a clean completion.

**v2** separates the axes, and both coupling directions are enforced, at
publication and at read:

- verdict without a comparison artifact → `InvalidManifest`
  ([evidence.rs:518](../crates/eggbench-core/src/evidence.rs));
- comparison artifact without a verdict → `InvalidManifest`
  ([evidence.rs:523](../crates/eggbench-core/src/evidence.rs));
- `failed` or `cancelled` execution with any verdict → `InvalidManifest`
  ([evidence.rs:528](../crates/eggbench-core/src/evidence.rs)).

All three are skipped for a legacy view via the `legacy` parameter
([evidence.rs:509](../crates/eggbench-core/src/evidence.rs)), because a v1
manifest has no verdict field to couple. A completed run that never compared is
therefore representable and unambiguous: `execution_status: completed`, no
verdict, no comparison artifact
([docs/evidence-bundle.md](../docs/evidence-bundle.md)).

### Versioning policy

`BundleManifest`
([evidence.rs:376](../crates/eggbench-core/src/evidence.rs)) and
`LegacyManifestV1` ([evidence.rs:420](../crates/eggbench-core/src/evidence.rs))
both lack `deny_unknown_fields`, so unknown **top-level** keys are ignored in
either version. Strictness sits in the nested types, which each declare
`deny_unknown_fields`: `ArtifactRecord`
([evidence.rs:257](../crates/eggbench-core/src/evidence.rs)), `TrialDescriptor`
([evidence.rs:275](../crates/eggbench-core/src/evidence.rs)),
`TrialExecutionResult` ([evidence.rs:288](../crates/eggbench-core/src/evidence.rs)),
`PairedRunRecord` ([evidence.rs:359](../crates/eggbench-core/src/evidence.rs)),
`EnvironmentFingerprint`
([evidence.rs:683](../crates/eggbench-core/src/evidence.rs)),
`EnvironmentField` ([evidence.rs:705](../crates/eggbench-core/src/evidence.rs)),
and `ArtifactBounds` ([plan.rs:617](../crates/eggbench-core/src/plan.rs)). Enum
variants are closed by serde; `ArtifactRole` additionally denies unknown keys
inside its tagged representation
([evidence.rs:210](../crates/eggbench-core/src/evidence.rs)).

| Change | Requires a version bump | Enforced by |
|---|---|---|
| New top-level manifest field | No | no `deny_unknown_fields` on the manifest DTO |
| New nested field or enum variant | Yes, or the bundle is rejected | `deny_unknown_fields` on nested types |
| Field that changes interpretation | Yes | policy plus the `validate_inner` version gate ([evidence.rs:509](../crates/eggbench-core/src/evidence.rs)) |
| New artifact role | Yes | `deny_unknown_fields` on the tagged `ArtifactRole` |
| New `trials` / `paired` / `drivers` entry | No | `#[serde(default)]` on those fields ([evidence.rs:399](../crates/eggbench-core/src/evidence.rs)) |

This is the "additive top-level fields stay compatible, nested and variant
fields stay strict" rule from [overview.md](overview.md), implemented by
convention about *where* the attribute sits rather than by a per-field policy.

### The resolved-plan gate

`validate_resolved_plan_bytes`
([evidence.rs:1745](../crates/eggbench-core/src/evidence.rs)) is the public seam
that decodes and version-gates a resolved plan before a caller stages it under
`ArtifactRole::ResolvedPlan`. It accepts the current version plus v5, v4, v3, v2,
and v1 — legacy evidence stays readable and is never rewritten
([evidence.rs:1748](../crates/eggbench-core/src/evidence.rs)). If a plan carries
a `network_path`, it must be current-or-v5, unpaired, transport-owning, and
non-external-subject, with a specific `eggfetch-http` workload descriptor plus a
matching `egress-route` (and, when faults are configured, `eggchaos-stream`)
descriptor, a seed present if faults are configured, and matching
semantics/RNG versions
([evidence.rs:1612](../crates/eggbench-core/src/evidence.rs),
[evidence.rs:1664](../crates/eggbench-core/src/evidence.rs)). A legacy resolved
plan derived from a schema-v3 source plan may not omit the network path
([evidence.rs:1759](../crates/eggbench-core/src/evidence.rs)).

Two callers use it, which is the intended shape: the runner validates before
staging ([prepare.rs:102](../crates/eggbench-runner/src/prepare.rs)) and
comparison re-validates while loading a bundle
([comparison.rs:2108](../crates/eggbench-core/src/comparison.rs)). Note that the
manifest layer itself never decodes plan, resolved-plan, environment, or trial
result bytes — it enforces roles, references, and digests only. Decoding is a
consumer decision, and `EnvironmentFingerprint::validate` is likewise called by
producers and consumers, never by `finalize`.

## Invariants

| # | Invariant | Enforced at |
|---|---|---|
| 1 | A published `.eggb` is complete: the staged tree is re-verified before the manifest write, and the published bundle re-opened and verified after | [evidence.rs:1103](../crates/eggbench-core/src/evidence.rs), [evidence.rs:1127](../crates/eggbench-core/src/evidence.rs) |
| 2 | `manifest.json` is written last and is the only file opened by literal name | [evidence.rs:1109](../crates/eggbench-core/src/evidence.rs) |
| 3 | A staging directory is never readable as evidence: the name carries `.staging-` | [evidence.rs:1151](../crates/eggbench-core/src/evidence.rs) |
| 4 | Publication never replaces: `RENAME_NOREPLACE` on Linux, a `create_new` reservation plus re-check elsewhere | [evidence.rs:1408](../crates/eggbench-core/src/evidence.rs), [evidence.rs:1431](../crates/eggbench-core/src/evidence.rs) |
| 5 | The manifest-listed set equals the on-disk set exactly; extra and missing both fail | [evidence.rs:1533](../crates/eggbench-core/src/evidence.rs) |
| 6 | Every artifact is byte-exact: size compared before digest; digest is lowercase hex of exactly 64 characters | [evidence.rs:1553](../crates/eggbench-core/src/evidence.rs), [evidence.rs:559](../crates/eggbench-core/src/evidence.rs) |
| 7 | Paths are portable and confined: not absolute, no dot segment, no backslash, no control character, ≤1,024 bytes, ≤16 segments, no Windows device name, no root `manifest.json` | [evidence.rs:111](../crates/eggbench-core/src/evidence.rs) |
| 8 | No symlink is ever followed, in staging or at read | [evidence.rs:1482](../crates/eggbench-core/src/evidence.rs), [evidence.rs:1508](../crates/eggbench-core/src/evidence.rs), [evidence.rs:1544](../crates/eggbench-core/src/evidence.rs) |
| 9 | Exactly one artifact carries each primary role (`experiment_plan`, `resolved_plan`, `environment_fingerprint`); at most one carries `comparison` or `report` | [evidence.rs:587](../crates/eggbench-core/src/evidence.rs), [evidence.rs:598](../crates/eggbench-core/src/evidence.rs) |
| 10 | The three primary references exist in the artifact table *and* carry the matching role | [evidence.rs:605](../crates/eggbench-core/src/evidence.rs) |
| 11 | Trial ids are unique; each trial `result` is a manifest-listed `trial_result`; extra trial artifacts are listed with no duplicate references | [evidence.rs:618](../crates/eggbench-core/src/evidence.rs) |
| 12 | Verdict and comparison artifact are mutually required; `failed` and `cancelled` execution cannot carry a verdict | [evidence.rs:518](../crates/eggbench-core/src/evidence.rs) |
| 13 | A bundle is finalized or it is not a bundle: `finalized: false` is `IncompleteBundle` | [evidence.rs:539](../crates/eggbench-core/src/evidence.rs) |
| 14 | Declared bounds are self-consistent and inside hard caps: non-zero per-artifact bytes, total ≥ per-artifact, count ≤ 10,000, per-artifact ≤ 256 MiB, total ≤ 2 GiB, `artifacts.len()` within the declared count | [evidence.rs:542](../crates/eggbench-core/src/evidence.rs), [evidence.rs:810](../crates/eggbench-core/src/evidence.rs) |
| 15 | Canonical ordering — artifacts path-sorted, trials id-sorted, drivers name-sorted — so identical evidence yields identical manifest bytes, which is what makes the manifest digest usable as bundle identity | [evidence.rs:1076](../crates/eggbench-core/src/evidence.rs), [comparison.rs:2231](../crates/eggbench-core/src/comparison.rs) |
| 16 | Secrets never enter: `Subject` carries `SecretRef` references only, and `Sensitivity` is a label | [types.rs:80](../crates/eggbench-core/src/types.rs) |
| 17 | The reader writes nothing: `manifest`, `root`, `legacy_status`, `verify`, `trial_metrics`, `open_artifact` — no mutation, no repair | [evidence.rs:1134](../crates/eggbench-core/src/evidence.rs) |

## Failure model

`BundleError` ([evidence.rs:750](../crates/eggbench-core/src/evidence.rs)) is
the single failure vocabulary; each variant is raised at one phase.

| Variant | Raised when | Phase |
|---|---|---|
| `Io { path, source }` | Any filesystem call fails, with the offending path | create / add / finalize / read |
| `DestinationExists` | Destination exists at create, on `EXDEV` at rename, or on a taken finalization lock | create / publish |
| `AtomicRenameUnsupported` | `XDEV`/`INVAL`/`NOSYS`/`OPNOTSUPP` (Linux) or `CrossesDevices` (portable) | publish |
| `UnsafeArtifactPath` | Rejected `ArtifactPath`, or a non-`Normal` component seen by a filesystem helper | add / read |
| `Symlink` | Symlink at the root, at the manifest, at a path component, in a staged parent, or during tree verification | add / finalize / read |
| `ExtraFile` | Unmanifested regular file, or an entry that is neither file nor directory | finalize / verify |
| `MissingArtifact` | A manifest reference is absent from the table or from disk | validate / verify |
| `ArtifactVerification` | Not a regular file, size mismatch, SHA-256 mismatch | finalize / verify |
| `IncompleteBundle` | Staging name, non-`.eggb` extension, not a directory, or `finalized: false` | create / read |
| `UnsupportedManifestVersion` | Manifest schema other than 1 or 2 | read |
| `ManifestParse` | JSON decode failure, missing `schema_version`, or a typed decode failure (`trial_metrics`, resolved plan) | read |
| `InvalidManifest` | Any cross-field inconsistency, or a bad media type, bounds, path, or duplicate | create / add / finalize / read |
| `BoundExceeded` | Declared count/byte bound exceeded, or the manifest exceeds 4 MiB | add / finalize / read |

Three behaviors worth naming explicitly. **A failed `finalize` keeps the staging
directory:** nothing removes it, it stays unreadable as evidence until an operator
discards it, and the only cleanup in the file is the partial-artifact removal
inside `add_artifact`
([evidence.rs:1026](../crates/eggbench-core/src/evidence.rs)). **`create` fails
before touching disk** for a wrong suffix, an existing destination, a missing
parent, or an over-declared bound, so a rejected attempt leaves no residue
([evidence.rs:861](../crates/eggbench-core/src/evidence.rs)). **On the non-Linux
publication path a stale lock is indistinguishable from a real collision:** the
`create_new` sentinel is removed on both the success and error paths, but a crash
between the rename and that cleanup leaves `.{name}.finalize-lock` behind, and
the next attempt for that name then reports `DestinationExists` even though the
destination is free
([evidence.rs:1431](../crates/eggbench-core/src/evidence.rs),
[evidence.rs:1455](../crates/eggbench-core/src/evidence.rs)).

Verification is all-or-nothing per run: the first missing, extra, wrong-size,
wrong-digest, symlink, or path violation aborts with a single error. There is no
partial-repair or continue-on-error mode.

## Boundaries

**Inside.** Path confinement, streaming hashing and byte accounting, manifest
construction and validation, atomic publication, read-only verification, the
v1/v2 compatibility gate, and the resolved-plan version allowlist.

**Outside, deliberately.** No async runtime, no process execution, no network
I/O — the imports are `std` filesystem plus `serde`/`sha2`/`uuid`/`thiserror`
([evidence.rs:2](../crates/eggbench-core/src/evidence.rs)) — and bundle reads use
bounded synchronous filesystem I/O, so the same code runs under any caller
runtime. Plan parsing and validation belong to
[core-plan-schema.md](core-plan-schema.md), metric normalization to
[core-metrics.md](core-metrics.md), the verdict policy to
[core-comparison.md](core-comparison.md), environment collection to
[runner-evidence-prep.md](runner-evidence-prep.md). Retention, garbage collection,
indexing, and search are explicitly out of scope
([docs/evidence-bundle.md](../docs/evidence-bundle.md)).

**Read-only consumer surface.** Comparison is the heaviest consumer and uses only
the narrow reader API: `verify()` before loading anything
([comparison.rs:2097](../crates/eggbench-core/src/comparison.rs)), `manifest()`
for references, `trial_metrics()` per trial
([comparison.rs:2118](../crates/eggbench-core/src/comparison.rs)), and
`open_artifact()` for every decoded evidence document
([comparison.rs:868](../crates/eggbench-core/src/comparison.rs)); it never writes
to a bundle. One boundary detail for reviewers: comparison re-reads the manifest
with a plain `std::fs::File::open(reader.root().join("manifest.json"))` under its
own 8 MiB cap
([comparison.rs:4092](../crates/eggbench-core/src/comparison.rs)), so that one
re-read sits outside the reader's no-follow open and outside its 4 MiB cap.

## Tests and qualification

In-file unit tests (`#[cfg(test)]`, 1773–2487):

| Test | Line | Contract pinned |
|---|---|---|
| `trial_result_v1_parses_with_defaulted_arm_and_pair` | 1794 | Additive `arm`/`pair_id` absent in v1 |
| `trial_result_v2_round_trips_arm_and_pair` | 1812 | v2 trial result round trip |
| `finalize_reopen_verify_and_manifest_is_written_last` | 1917 | No `manifest.json` in staging before finalize; reopen + verify |
| `committed_synthetic_bundle_fixture_opens_and_verifies` | 1960 | v1 fixture; `legacy_status == Inconclusive`; no verdict |
| `current_v2_fixture_opens_and_records_execution_without_comparison` | 1974 | v2 fixture; completed with no verdict |
| `execution_outcomes_and_verdicts_are_independent_and_coherent` | 1988 | All four verdicts round-trip; both coupling directions reject |
| `environment_fingerprint_is_versioned_and_bounded` | 2061 | Fingerprint version + round trip |
| `destination_collision_and_interrupted_staging_fail_closed` | 2082 | `DestinationExists`; staging unreadable; over-declared bounds rejected at create |
| `staging_tampering_is_detected_before_publication` | 2110 | Post-registration mutation fails finalize and leaves no destination |
| `empty_trial_failed_bundle_is_valid_and_multiple_trials_are_stable` | 2140 | Zero-trial bundle valid; trial ordering |
| `missing_wrong_size_wrong_digest_and_extra_files_are_detected` | 2151 | `MissingArtifact`, size mismatch, digest mismatch, `ExtraFile` |
| `path_collision_absolute_and_parent_traversal_are_rejected` | 2186 | Every `ArtifactPath` rejection rule; duplicate path |
| `symlink_escape_is_rejected_without_following` (unix) | 2226 | Symlink at an artifact path fails at open |
| `unsupported_version_and_unknown_fields_are_rejected` | 2242 | Version 99 rejected; unknown top-level field accepted and still verifies |
| `secret_references_and_manifest_labels_do_not_embed_secret_values` | 2266 | No secret value in plan, resolved plan, environment, or manifest |
| `large_artifact_hashing_uses_bounded_stream_buffers` | 2447 | Largest read request equals `HASH_BUFFER_BYTES` |

All `Line` values are in `evidence.rs`. Checked-in fixtures
`tests/fixtures/example.eggb` (manifest v1) and
`tests/fixtures/current-v2.eggb` are opened and verified by the module, so the
compatibility promise is pinned to real files rather than synthesized JSON, and
the resolved-plan allowlist is pinned in
[tests/network_path.rs](../crates/eggbench-core/tests/network_path.rs), which
asserts v1–v5 remain readable and that legacy plans lacking a required network
path are rejected. Not covered by an in-file test: the 4 MiB manifest cap, the
1,024-byte and 16-segment path bounds, the 256-entry driver cap, the portable
finalization-lock path (exercised only on non-Linux CI), the
`AtomicRenameUnsupported` mapping, and the run-level `paired` record. Nothing was
executed for this deep dive; the review focus below lists those as gaps.

## Review focus

1. **Any reordering inside `finalize`.** Only reorderings that keep the staged
   verification and the manifest write after the last content mutation are safe
   ([evidence.rs:1103](../crates/eggbench-core/src/evidence.rs)). Writing the
   manifest earlier turns a crash into a bundle that looks complete.
2. **Bound arithmetic.** `remaining_total_bytes` subtracts without
   `saturating_*` ([evidence.rs:941](../crates/eggbench-core/src/evidence.rs))
   and relies on the per-chunk check never letting `total_bytes` exceed
   `bounds.total_bytes`
   ([evidence.rs:1009](../crates/eggbench-core/src/evidence.rs)). Any new path
   that mutates `total_bytes` must preserve that.
3. **`trial_metrics` reads before it bounds.** `read_to_end` completes at
   [evidence.rs:1279](../crates/eggbench-core/src/evidence.rs) and the 256 MiB
   check follows at
   [evidence.rs:1281](../crates/eggbench-core/src/evidence.rs). `open` does not
   verify artifact sizes, so a hand-authored bundle can make this allocate more
   than the bound before the bound applies. Bound the read itself if that matters
   to the threat model.
4. **The security boundary is the opener, not the checker.** On Unix the
   guarantee is `NOFOLLOW` on `openat` for every component
   ([evidence.rs:1356](../crates/eggbench-core/src/evidence.rs)); on non-Unix it
   degrades to a metadata walk plus `File::open`
   ([evidence.rs:1461](../crates/eggbench-core/src/evidence.rs)). Keep the
   `is_file` assertion in `verify_artifact_tree`; do not assume `open_artifact`
   implies it.
5. **Role-count rules are a compatibility surface.** A new producer staging a
   second plan, comparison, or report artifact fails at
   [evidence.rs:587](../crates/eggbench-core/src/evidence.rs) /
   [evidence.rs:598](../crates/eggbench-core/src/evidence.rs) until those rules
   are revisited deliberately.
6. **Versioning is positional, not declarative.** Forward compatibility comes
   from *where* `deny_unknown_fields` is absent
   ([evidence.rs:376](../crates/eggbench-core/src/evidence.rs)); a new top-level
   field on a nested type, or a new enum variant, silently becomes a hard
   rejection.
7. **The legacy marker is easy to lose.**
   `LegacyRunStatus::Inconclusive` normalizes to `Completed`
   ([evidence.rs:461](../crates/eggbench-core/src/evidence.rs)); any consumer
   reading only `manifest().execution_status` over-reports a legacy ambiguous run
   as a clean completion.
8. **The non-Linux lock is a reservation, not a lock.** A stale sentinel both
   blocks reuse of the name and is reported as a destination collision
   ([evidence.rs:1437](../crates/eggbench-core/src/evidence.rs)).
9. **Redundant path check.** The `segments.first().ends_with(':')` test at
   [evidence.rs:129](../crates/eggbench-core/src/evidence.rs) cannot produce a
   distinct outcome: a `:`-containing first segment is already rejected by the
   segment rule at
   [evidence.rs:124](../crates/eggbench-core/src/evidence.rs).

## Related

- [overview.md](overview.md) — module map, end-to-end flow, bundle layout,
  cross-cutting version policy
- [evidence.md](evidence.md) — crate-level ownership rationale for the `.eggb`
  contract
- [core.md](core.md) · [core-plan-schema.md](core-plan-schema.md) ·
  [core-driver-resolution.md](core-driver-resolution.md) ·
  [core-metrics.md](core-metrics.md) · [core-comparison.md](core-comparison.md) ·
  [runner-evidence-prep.md](runner-evidence-prep.md) — the surrounding
  producer/consumer contracts
- [../docs/evidence-bundle.md](../docs/evidence-bundle.md) — user-facing bundle
  contract; [../docs/metrics.md](../docs/metrics.md);
  [../docs/environment-fingerprint.md](../docs/environment-fingerprint.md);
  [../docs/comparison.md](../docs/comparison.md)
- Source: [../crates/eggbench-core/src/evidence.rs](../crates/eggbench-core/src/evidence.rs) ·
  [../crates/eggbench-core/src/types.rs](../crates/eggbench-core/src/types.rs) ·
  [../crates/eggbench-core/src/plan.rs](../crates/eggbench-core/src/plan.rs)
