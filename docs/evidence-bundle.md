# Evidence bundles

An `.eggb` bundle is a self-contained directory with one authoritative `manifest.json` and the files named by that manifest. The manifest is written after all other artifacts. Its schema version, run ID, execution status, optional comparison verdict, subject identity, timestamps (Unix milliseconds), plan/resolved-plan/environment references, trial identities, driver inventory, artifact roles/media types/sensitivity labels, sizes, digests, and persisted bounds make the evidence inspectable without a database.

| Manifest | Read/write behavior | Status semantics |
|---|---|---|
| v1 | Read and fully verify; never rewritten | Legacy `RunStatus` retained explicitly. `inconclusive` is ambiguous and is not converted into a comparison verdict. |
| v2 | Current read and write format | `execution_status` is required. `comparison_verdict` is absent when comparison was not performed and requires a comparison artifact when present. |

For example, lifecycle completion without measurement is `execution_status: completed`, no comparison verdict, and zero trials. It does not report a performance pass or an invented inconclusive verdict.

## Example tree

The checked-in [`example.eggb`](../crates/eggbench-core/tests/fixtures/example.eggb) is a synthetic, verified bundle:

~~~text
example.eggb/
  manifest.json
  plan.json
  resolved-plan.json
  environment.json
  trials/001/result.json
~~~

Measured trials additionally stage `trials/NNN/metrics.json`
(`ArtifactRole::TrialArtifact`, `Public`): normalized per-trial metric
evidence owned by Measurement M001. Each file carries the metric schema and
vocabulary versions, one `observed`/`missing`/`invalid` record per requested
metric in deterministic name order, raw histogram references, an optional
error-category distribution, and bounded warnings. Warmups never receive
this artifact. Pre-M001 bundles simply have no `metrics.json`;
`BundleReader::trial_metrics(trial_id)` returns `None` for them. See
[`metrics.md`](metrics.md).

Environment data has a separate schema version (`EnvironmentFingerprint` v1). Each selected, non-secret field is classified as comparison-critical, warning-only, or informational. The schema does not fingerprint the host automatically.

## Runtime-topology evidence (Eggstack M001a)

`lifecycle/runtime-topology.json` (schema v1, `Redacted`) records one entry
per launch-order service identity plus externally managed services: ownership
kind (`process`/`adapter`/`external`), the named service type for
adapter-owned services, and the non-secret startup-established runtime
bindings (for example the origin's `http_url`, `bound_addr`, `bound_port`).
It stages from retained session state after teardown, so topology evidence
survives service shutdown, and its failure still routes through the M002
evidence-safety cleanup invariant. See [Eggstack HTTP](eggstack-http.md).

## Telemetry artifacts (Eggstack M001b)

Per measured trial, `trials/NNN/telemetry/` holds collector artifacts in
`{collector:02}-{artifact:02}-{name}` slots (for example
`trials/001/telemetry/00-00-gregg.ndjson`): the raw series plus a
provenance document, staged with the same name-safety rules as workload
artifacts and collision-checked against them. Telemetry observations join
workload observations in the same `metrics.json` normalization with
per-observation producer attribution. See
[Gregg telemetry](gregg-telemetry.md).

## Staging and finalization

`BundleWriter` creates a sibling staging directory. Each artifact is copied and hashed with a fixed 64 KiB buffer and checked against the plan's count, per-artifact, total-byte, path-length, and path-depth limits. Hard caps are 10,000 artifacts, 256 MiB per artifact, 2 GiB total, a 4 MiB manifest, 1,024 path bytes, and 16 path components. Finalization checks required primary roles and references, verifies staged content again, flushes files, writes the manifest last, and atomically renames the directory into place. A destination collision fails. If the filesystem cannot perform the same-directory atomic rename, finalization reports an error; it never copies a partial tree and labels it atomic. On Windows, the library flushes each artifact and manifest file, while directory-entry durability follows OS/filesystem behavior because portable directory syncing is unavailable through `std`.

Interrupted staging directories have a `.staging-` name and are not accepted by `BundleReader`. They are never repaired or treated as completed evidence. A caller may explicitly discard them after operational review.

## Inspection, corruption, and immutability

`BundleReader::open` parses and validates the manifest; `verify` enumerates the exact file set and checks every required path, regular-file type, size, and SHA-256 digest. It rejects extra files and symlinks, including links that would lead outside the bundle. `open_artifact` is read-only and only opens manifest-listed paths.

The ordinary API has no mutation operation for a finalized bundle. Verification reports missing, wrong-size, wrong-digest, extra-file, incomplete, unsupported-schema, and unsafe-path conditions without rewriting data. Manifest v1 ignores unknown top-level additive metadata and rejects unknown nested fields/variants. Any field that changes interpretation requires an explicit schema compatibility decision.

Retention, garbage collection, indexing, and database-backed search are outside this milestone. `Sensitivity` is metadata for future policies; raw security payload retention policy is deferred to security integrations.

## Offline comparison receipts

`eggbench compare` never mutates either bundle. It emits a standalone
versioned JSON receipt (schema v1, policy `eggbench.trial-bootstrap.v1`)
carrying both bundle identities, the baseline reference, environment policy,
typed comparability, seed, per-metric estimates/intervals/thresholds/
verdicts, the aggregate verdict, and warnings. The manifest
`comparison_verdict` field remains reserved for future run-time comparison
performed before finalization. See [comparison](comparison.md) and
[baselines](baselines.md).
