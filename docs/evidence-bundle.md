# Evidence bundles

An `.eggb` bundle is a self-contained directory with one authoritative `manifest.json` and the files named by that manifest. The manifest is written after all other artifacts. It records the schema version, run ID, execution status, optional comparison verdict, subject identity, timestamps, plan/resolved-plan/environment references, trial identities, driver inventory, artifact roles/media types/sensitivity labels, sizes, digests, and persisted bounds.

| Manifest | Read/write behavior | Status semantics |
|---|---|---|
| v1 | Read and fully verify; never rewritten | Legacy `RunStatus` retained explicitly. `inconclusive` is ambiguous and is not converted into a comparison verdict. |
| v2 | Current read and write format | `execution_status` is required. `comparison_verdict` is absent when comparison was not performed and requires a comparison artifact when present. |

For example, lifecycle completion without measurement is `execution_status: completed`, no comparison verdict, and zero trials. It does not report a performance pass or an invented inconclusive verdict.

## Example tree

The checked-in [`example.eggb`](../crates/eggbench-core/tests/fixtures/example.eggb) is a synthetic, verified bundle:

```text
example.eggb/
  manifest.json
  plan.json
  resolved-plan.json
  environment.json
  trials/001/result.json
```

`plan.json` preserves the source plan, including an optional schema-v3 `network_path`. New resolutions write `resolved-plan.json` as ResolvedPlan schema v3, which records selected Route/Fault descriptors and their upstream provenance. ResolvedPlan v1 and v2 remain readable for legacy evidence; they are not rewritten or silently upgraded. A v1/v2 source plan remains compatible when it has no network path.

## Network-path evidence

A run with `network_path` stages a root-level `network-path.json` artifact. It is a run-level, redacted JSON document with schema v1 and the stable manifest role label `network-path`; it is not a managed service and does not require a new `runtime-topology.json` schema.

The artifact retains bounded, credential-free facts:

- route driver name, adapter version, exact `eggress-outbound` and `eggress-uri` versions, mode, canonical redacted chain, chain configuration digest, and configured hop count;
- optional fault driver and exact upstream version;
- `route_first_fault_second` ordering, `user_space_stream` fault layer, and final-target-relative upstream/downstream directions;
- the ordered typed fault plan, static-policy marker, explicit seed namespace, and Eggchaos RNG version;
- bounded physical-dial attempts, successful dials, wrapped connections, stable route-failure buckets, hop-count distribution, and connection ordinal range.

The evidence never contains passwords, tokens, URI userinfo, payload bytes, or unbounded raw error strings. A path plan that does not produce this artifact is an evidence failure rather than a successful path run.

## Per-invocation method evidence

The existing per-measured-invocation `eggfetch-method.json` remains in place. When a path is configured, it is extended additively with a `network_path` object containing bounded invocation diagnostics such as physical dial attempts, successful routed dials, route failures, hop-count distribution, fault wrapping, and connection ordinals. Existing method fields and artifact names remain unchanged.

The method artifact distinguishes request count from physical connection count. One Eggfetch client is retained for the whole run, so warmups and measured trials may reuse pooled connections; the evidence does not imply one faulted connection per request. Network-path diagnostics are evidence, not new normalized metric semantics.

Measured trials stage `trials/NNN/metrics.json` (`ArtifactRole::TrialArtifact`, `Public`): normalized per-trial metric evidence owned by Measurement M001. Each file carries the metric schema and vocabulary versions, one `observed`/`missing`/`invalid` record per requested metric in deterministic name order, raw histogram references, an optional error-category distribution, and bounded warnings. Warmups never receive this artifact. Pre-M001 bundles simply have no `metrics.json`; `BundleReader::trial_metrics(trial_id)` returns `None` for them. See [`metrics.md`](metrics.md).

Environment data has a separate schema version (`EnvironmentFingerprint` v1). Each selected, non-secret field is classified as comparison-critical, warning-only, or informational. The schema does not fingerprint the host automatically.

## Runtime-topology evidence (Eggstack HTTP)

`lifecycle/runtime-topology.json` (schema v1, `Redacted`) records one entry per launch-order service identity plus externally managed services: ownership kind (`process`/`adapter`/`external`), the named service type for adapter-owned services, and non-secret startup-established runtime bindings such as the origin's `http_url`, `bound_addr`, and `bound_port`. It stages from retained session state after teardown, so topology evidence survives service shutdown. Network paths remain separate run evidence; they are never inserted as fake service entries.

## Telemetry artifacts (Eggstack M001b)

Per measured trial, `trials/NNN/telemetry/` holds collector artifacts in `{collector:02}-{artifact:02}-{name}` slots (for example `trials/001/telemetry/00-00-gregg.ndjson`): the raw series plus a provenance document, staged with the same name-safety rules as workload artifacts and collision-checked against them. Telemetry observations join workload observations in the same `metrics.json` normalization with per-observation producer attribution. See [Gregg telemetry](gregg-telemetry.md).

## Staging and finalization

`BundleWriter` creates a sibling staging directory. Each artifact is copied and hashed with a fixed 64 KiB buffer and checked against the plan's count, per-artifact, total-byte, path-length, and path-depth limits. Hard caps are 10,000 artifacts, 256 MiB per artifact, 2 GiB total, a 4 MiB manifest, 1,024 path bytes, and 16 path components. Finalization checks required primary roles and references, verifies staged content again, flushes files, writes the manifest last, and atomically renames the directory into place. A destination collision fails. If the filesystem cannot perform the same-directory atomic rename, finalization reports an error; it never copies a partial tree and labels it atomic. On Windows, the library flushes each artifact and manifest file, while directory-entry durability follows OS/filesystem behavior because portable directory syncing is unavailable through `std`.

Interrupted staging directories have a `.staging-` name and are not accepted by `BundleReader`. They are never repaired or treated as completed evidence. A caller may explicitly discard them after operational review.

## Inspection, corruption, and immutability

`BundleReader::open` parses and validates the manifest; `verify` enumerates the exact file set and checks every required path, regular-file type, size, and SHA-256 digest. It rejects extra files and symlinks, including links that would lead outside the bundle. `open_artifact` is read-only and only opens manifest-listed paths.

The ordinary API has no mutation operation for a finalized bundle. Verification reports missing, wrong-size, wrong-digest, extra-file, incomplete, unsupported-schema, and unsafe-path conditions without rewriting data. Manifest v1 ignores unknown top-level additive metadata and rejects unknown nested fields/variants. Any field that changes interpretation requires an explicit schema compatibility decision.

A feature-enabled `eggbench inspect` verifies and decodes `network-path.json`, reporting its schema, route/fault provenance, ordering/layer markers, fault count, and bounded dial counters. A feature-disabled binary can still verify the bundle and report that a path artifact is present, but detailed path decoding is unavailable. Inspection never dials a route or mutates a bundle.

Retention, garbage collection, indexing, and database-backed search are outside this milestone. `Sensitivity` is metadata for future policies; raw security payload retention policy is deferred to security integrations.

## Offline comparison receipts

`eggbench compare` never mutates either bundle. Unpaired path-free comparison emits a standalone versioned JSON receipt (schema v1, policy `eggbench.trial-bootstrap.v1`); comparisons involving a network path use the path-aware policy `eggbench.trial-bootstrap-network-path.v1`. Both carry bundle identities, the baseline reference, environment policy, typed comparability, seed, per-metric estimates/intervals/thresholds/verdicts, the aggregate verdict, and warnings. Paired comparison emits schema v2 under `eggbench.trial-bootstrap-paired.v1`; schema-v1 receipts remain readable. The manifest `comparison_verdict` field remains reserved for future run-time comparison performed before finalization. Network-path configuration is included in comparison identity as described in [comparison](comparison.md).
