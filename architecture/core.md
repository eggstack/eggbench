# Core architecture

`eggbench-core` owns runtime-free domain contracts: typed experiment plans, stable names and bounds, schema versions, and validation. It must not own process execution, asynchronous runtimes, command-line presentation, or concrete networking clients. Adapters resolve symbolic requests and perform side effects outside core.

The experiment-plan v1 decoder rejects unknown fields (`deny_unknown_fields`). Schema changes therefore require an explicit version update and compatibility decision. Ordered maps keep snapshots deterministic. Plan inputs carry references to secrets, never credential values. Plan schema v2 adds the optional predeclared `paired` baseline/candidate design (alternating arms, even measured count, label subject); v1 plans validate exactly as before. Plan schema v3 adds the optional bounded, credential-free `network_path` contract for native route-first/fault-second stream composition; v1/v2 remain readable and reject the field. Later schemas are additive only: v4 semantic replay, v5 diagnostics, v6 security checks, v7 service `http_url`, v8 corpus checks, v9 static/bindings references, v10 `http_corpus` workload (see `docs/experiment-plan.md`).

The boundary is: human-authored plan → validated plan → resolved plan → (runner) → immutable evidence. Driver descriptors and resolution are typed, serializable contracts; actual driver implementations and side effects remain outside core. Evidence APIs own streaming artifact staging, finalization, digest verification, and read-only inspection without a database.

The `EnvironmentFingerprint` schema v1 defines a bounded map of
non-secret host facts with an explicit comparability class
(`comparison_critical`, `warning_only`, or `informational`). The schema does
not change merely because real producers exist; new fields are additive.

The `Subject` enum is preserved in `BundleManifest` for trial provenance.
The companion `SubjectSnapshot` (lives in `eggbench-runner` because it
hashes executables) extends the manifest with the resolved executable path
and its SHA-256 digest, plus declared/observed digest matching.

Measurement M001 adds a dependency-light `metrics` module: metric vocabulary
v1, `TrialMetrics` schema v1, and a pure normalization function. Core owns
validation and the normalized evidence type; drivers own parsing upstream
output and never write `TrialMetrics` JSON themselves. `BundleReader`
exposes `trial_metrics(trial_id)` so later comparison and CLI surfaces can
load normalized evidence without reconstructing paths.

Measurement M002 adds a dependency-light `comparison` module: immutable
bundle identities, digest-pinned baseline aliases, typed comparability,
deterministic trial-level bootstrap policy v1
(`eggbench.trial-bootstrap.v1`), network-path-aware policy identity
(`eggbench.trial-bootstrap-network-path.v1`), paired policy identity
(`eggbench.trial-bootstrap-paired.v1`, M003), per-metric and aggregate
verdicts, and the standalone comparison receipt (schema v4; v1-v3 readable).
Core stays free of Tokio/process/network dependencies; bundle reads use
bounded synchronous filesystem I/O. No p-value or bundle mutation exists;
paired inference is confined to the paired policy over one bundle's two arms.
