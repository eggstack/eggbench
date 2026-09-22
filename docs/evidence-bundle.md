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

Environment data has a separate schema version (`EnvironmentFingerprint` v1). Each selected, non-secret field is classified as comparison-critical, warning-only, or informational. The schema does not fingerprint the host automatically.

## Staging and finalization

`BundleWriter` creates a sibling staging directory. Each artifact is copied and hashed with a fixed 64 KiB buffer and checked against the plan's count, per-artifact, total-byte, path-length, and path-depth limits. Hard caps are 10,000 artifacts, 256 MiB per artifact, 2 GiB total, a 4 MiB manifest, 1,024 path bytes, and 16 path components. Finalization checks required primary roles and references, verifies staged content again, flushes files, writes the manifest last, and atomically renames the directory into place. A destination collision fails. If the filesystem cannot perform the same-directory atomic rename, finalization reports an error; it never copies a partial tree and labels it atomic. On Windows, the library flushes each artifact and manifest file, while directory-entry durability follows OS/filesystem behavior because portable directory syncing is unavailable through `std`.

Interrupted staging directories have a `.staging-` name and are not accepted by `BundleReader`. They are never repaired or treated as completed evidence. A caller may explicitly discard them after operational review.

## Inspection, corruption, and immutability

`BundleReader::open` parses and validates the manifest; `verify` enumerates the exact file set and checks every required path, regular-file type, size, and SHA-256 digest. It rejects extra files and symlinks, including links that would lead outside the bundle. `open_artifact` is read-only and only opens manifest-listed paths.

The ordinary API has no mutation operation for a finalized bundle. Verification reports missing, wrong-size, wrong-digest, extra-file, incomplete, unsupported-schema, and unsafe-path conditions without rewriting data. Manifest v1 ignores unknown top-level additive metadata and rejects unknown nested fields/variants. Any field that changes interpretation requires an explicit schema compatibility decision.

Retention, garbage collection, indexing, and database-backed search are outside this milestone. `Sensitivity` is metadata for future policies; raw security payload retention policy is deferred to security integrations.
