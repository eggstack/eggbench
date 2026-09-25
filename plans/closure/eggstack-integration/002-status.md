# Eggstack Integration M002 — Listener-Free Eggress Route and Eggchaos Stream-Fault Topology — Closure

Disposition: **closed**
Closed: 2026-09-25
Implementation commit: `f816a65` (`feat(eggstack): implement M002 network path topology`), on top of planning baseline `d870512a5a1af16276ff05286ff0b6e2366b7f8f`.
Hosted qualification: GitHub Actions run `36085136434` passed Linux stable, Linux Rust 1.89, macOS stable, and Windows stable.

## 1. Requirement-to-evidence matrix

| Plan requirement | Evidence | Outcome |
|---|---|---|
| Schema-v3 `network_path` with v1/v2 compatibility | `crates/eggbench-core/src/network_path.rs`, `plan.rs`, `resolved.rs`; legacy/null/rejection and round-trip tests | Pass |
| Route and fault capability resolution before startup | `DriverCategory::Route`/`Fault`, `Capability::ProxyRouting`/`StreamFaultPlan`; missing, wrong-category, unsupported, external, and version-provenance tests | Pass |
| Canonical native route and fault descriptors | `crates/eggbench-drivers/src/eggstack/path/mod.rs`, build-time lockfile versions; exact `eggress 1.0.10` and `eggchaos-core 0.1.0` pins | Pass |
| Credential and secret policy | Route URI userinfo/query/fragment rejection, safe `Debug` and serialization, approved non-secret service-config keys, evidence grep regression, CLI no-bundle test | Pass |
| Route-first/fault-second stream composition | `path/dialer.rs`, safe async stream adapter, real HTTP/SOCKS/multi-hop traffic, no-fallback test | Pass |
| Seven stream-fault kinds and direction semantics | `path/fault.rs`, all-fault activation/directionality/slow-close tests, bounded user-space stream semantics | Pass |
| Pooling, warmup reuse, cancellation, and drain | One-client-per-run integration tests, pooled fault connection test, cancellation/drain tests, `MeasurementSignal` post-processing boundary | Pass |
| Bounded diagnostics and method evidence | Per-invocation `network_path` object includes active-fault state, dial/failure/hop/ordinal counters, and dropped-bucket count; run-level counters use checked arithmetic | Pass |
| Versioned `network-path.json` | `path/evidence.rs`, manifest role/media/sensitivity/bounds, post-drain staging, inspect and comparison loaders | Pass |
| Comparison-critical path identity | `comparison.rs` path-aware policy `eggbench.trial-bootstrap-network-path.v1`; full bundle matrix varies mode, chain, versions, faults, seed, and ordered fault plans | Pass |
| CLI preflight and evidence UX | `validate`, `doctor`, `run`, and `inspect`; feature-disabled gates, stable categories, source/resolved consistency, trial identity checks | Pass |
| Feature and dependency isolation | CI matrix for default, HTTP-only, Gregg-only, path, path+Gregg, all-feature, MSRV, and normal-edge dependency trees | Pass |
| Documentation and examples | `docs/`, `architecture/`, `README.md`, `examples/eggstack-path*.json`; CI validates the runnable example | Pass |

## 2. Verification record

- `cargo fmt --all -- --check` — pass.
- `EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-targets --all-features --locked` — pass: **410 tests across 20 suites**.
- `EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-targets --locked` — pass: **353 tests across 20 suites**.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass.
- Default-feature Clippy with `-D warnings` — pass.
- Feature checks for default, `eggstack-http`, `gregg`, `eggstack-path`, `eggstack-path,gregg`, and CLI `eggstack-path` — pass.
- `cargo tree --edges normal` isolation checks for default and HTTP-only graphs — pass; neither graph contains Eggress or Eggchaos.
- `cargo +1.89.0 check --workspace --all-targets --all-features --locked` — pass.
- Rust 1.89 core tests (**106 unit + 23 network-path integration tests**) — pass.
- Rust 1.89 driver tests, including real path integration — pass.
- `cargo run -p eggbench-cli --features eggstack-path -- validate examples/eggstack-path.json` — pass.
- `git diff --check` — pass.
- Hosted run `36085136434` — pass on Linux stable, Linux MSRV, macOS stable, and Windows stable.

## 3. Schema, compatibility, and provenance

- Plan schema v3 is additive; v1/v2 plans remain readable and reject an explicit `network_path` field, including explicit null.
- Resolved-plan schema v3 records the credential-free request plus exact Route/Fault descriptors, semantics identity, RNG identity, and seed namespace; legacy resolved snapshots remain readable.
- `network-path.json` schema v1 records exact sibling versions, canonical route identity, ordered fault plans, static policy, and bounded diagnostics without payload bytes or credentials.
- The comparison loader requires a path artifact, binds its route/fault provenance to `resolved-plan.json`, recomputes canonical route identity, validates trial result identity, and rejects forged or ambiguous evidence.
- Network-path comparisons use a separately identified policy while path-free unpaired receipts retain `eggbench.trial-bootstrap.v1`; paired policy identity is unchanged.
- Exact production pins are `eggress-outbound 1.0.10`, `eggress-uri 1.0.10`, `eggress-core 1.0.10`, `eggress-embed 1.0.10`, and `eggchaos-core =0.1.0`.

## 4. Security and lifecycle evidence

- No route credentials, userinfo, query/fragment secrets, arbitrary path-plan service-config secrets, shell execution, listener, MITM/TLS interception, UDP, or packet-loss claim is introduced.
- Route failures never fall back to Direct; failed physical dials remain typed request failures.
- Route/fault lowering and descriptor/provenance checks occur before managed startup.
- Diagnostics snapshots, method evidence, run evidence, and evidence staging are bounded and occur outside the measured workload interval through the measurement signal and post-drain hook.
- Cancellation, drain, and teardown preserve existing runner cleanup semantics; no route-owned process or listener is left behind.

## 5. Known limitations and follow-up boundaries

- M002 remains stream-only: no UDP/datagram impairment, netem, packet-loss semantics, route credentials, or paired path experiments.
- Network-path identity is static for one run; live policy mutation and replay/diagnostic policy belong to M003.
- Independent external oracles remain available but do not advertise `NetworkPath`.
- Future secret-bearing authenticated proxy routes require a separately designed typed `SecretRef` contract; M002 fails closed instead.

## 6. Disposition

**Closed.** M002 delivers the schema-v3 network-path contract, native Eggress routing, deterministic Eggchaos stream-fault composition, bounded post-drain evidence, path-aware comparison identity, CLI surfaces, feature isolation, documentation, local qualification, and four-lane hosted qualification. Eggstack M003 may proceed against the closed contracts.
