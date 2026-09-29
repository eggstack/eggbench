# Security qualification

M001a adds a subject-neutral profile and fixed HTTP corpus input format. The profile is independent of the ordinary experiment plan and contains explicit relative scenario plan paths plus corpus and target configuration references. M001b adds an ordinary-plan correctness check that executes the corpus through Eggfetch.

```sh
eggbench qualify validate examples/security-profile.json --json
eggbench qualify expand examples/security-profile.json --json
eggbench qualify run examples/security-profile.json --output target/security-qualification
eggbench qualify inspect target/security-qualification/qualification-receipt.json --json
```

The current CLI treats the profile's containing directory as the explicit workspace root. Every profile, plan, corpus, body, target configuration, and optional baseline path is confined to that root. Profile schema v1 remains supported. Schema v2 may add `"workload_driver": "eggfetch-http"` to a scenario; expansion freezes this selection and qualification invokes the ordinary run path with it. An unavailable selected driver does not fall back. A scenario may declare `"baseline_bundle": "baselines/reference.eggb"`; omitting it selects absolute-only comparison. Baselines are verified and their manifest identities frozen during expansion, before any candidate scenario starts. There is no automatic baseline lookup.

`qualify run` executes scenarios serially in profile order through ordinary `run` and `compare`. It stages `expansion.json`, ordinary scenario bundles and comparison receipts, and publishes `qualification-receipt.json` last by atomic directory rename. Existing output directories are rejected. Fail and Inconclusive comparisons continue to later scenarios; invalid run/evidence and cancellation stop later starts. Incomplete receipts are marked Invalid and cannot claim execution completion. The receipt references immutable evidence and aggregates only the comparison's typed verdicts using Invalid > Fail > Inconclusive > Pass. `qualify inspect` verifies referenced identities and digests before displaying the receipt.

Corpus requests use relative origin-form paths, bounded methods and headers, and owner-authored exact or allowed-set status expectations. Credential-bearing headers, absolute request targets, fragments, and symlinked content are rejected. Category labels are opaque owner metadata.

An HTTP corpus plan uses schema version 8 and `http_corpus_checks`. Each check pins a workspace-relative corpus file, its M001a content-tree digest, target service, whole-check timeout, and per-case timeout. Run the CLI with the `eggstack-http` feature enabled to include the Eggfetch executor:

```sh
cargo run -p eggbench-cli --features eggstack-http -- validate examples/security-http-corpus-plan.json
cargo run -p eggbench-cli --features eggstack-http -- run examples/security-http-corpus-plan.json --bundle target/http-corpus.eggb
```

Cases run serially after readiness and before warmups. They do not enter trial metrics. Only HTTP status, owner-defined case IDs, case/request hashes, expectation, and stable dispositions enter the evidence artifact; request and response bodies and server error text are omitted. Transport failures and timeouts are Invalid. Status mismatches are correctness Fail while performance trials continue.

Targets come from the named service's runtime `http_url` binding and must be loopback, RFC1918, IPv6 ULA, or an explicit `.localhost` name. Eggfetch uses HTTP/1.1 with redirects and retries absent from its selected feature profile. HTTP corpus checks reject paired runs and `network_path` composition. Comparison receipts use schema v4 and `eggbench.security-correctness.v2` when this family is present; M004 WAF-only evidence keeps the immutable v1 correctness policy.

Service plans may declare an `http_url` on schema version 7 or later. Plan schema v9 also supports a bounded `static_bindings` map; a matching `static_bindings.http_url` is accepted alongside the legacy field, while disagreement fails validation. Managed command services and external services publish these non-secret values through `RuntimeBindings`; named adapters may publish them too, while conflicts fail closed. Typed `binding_args` and `binding_env` references allow a managed command to consume a declared dependency's binding after readiness. Runtime topology records each consumed value's digest and source/destination identity without copying secret values.

Schema v10 adds `http_corpus` as a finite closed-loop security-load shape. It reuses the same pinned corpus format, requires an explicit seed, limits schedules to one million requests and concurrency to 256, and requires a primary absolute-zero `expected_outcome_mismatch_rate` gate. Method/path/headers/body and expected statuses stay owner-authored. Transport errors and status mismatches are emitted as separate metrics; a correct expected block is a completed response. The runtime binding contributes only the origin authority. See [experiment-plan schema v10](experiment-plan.md#schema-v10-http_corpus) for fields, comparison identity, and the native/oha connection behavior.

## Generic Prometheus subject telemetry

Build with `--features prometheus-http` to enable the `prometheus-http` telemetry source. A plan declares one external named source service with `service_type: "prometheus-http"` and string config values `target_service`, `binding_key`, `mapping_ref`, `mapping_sha256`, and `poll_interval_ms`. The named target publishes the URL through `static_bindings` (or `http_url` when `binding_key` is `http_url`). This keeps the scrape authority tied to a declared service binding. Poll intervals are bounded from 100 ms through 60 seconds.

The workspace mapping is immutable and its content-tree SHA-256 must match before startup. Example mapping:

```json
{
  "schema_version": 1,
  "source": "prometheus",
  "fields": [{
    "output_name": "subject_event_loop_lag_ms",
    "prometheus_name": "synvoid_event_loop_lag_ms",
    "kind": "gauge",
    "unit": "ms",
    "aggregation": "max",
    "required": true
  }]
}
```

Mappings support `gauge` with explicit `mean`, `max`, or `min`, and `counter` with nonnegative trial delta. Optional exact label selectors are bounded to 16 labels and 256 bytes per value; wildcard matching and ambiguous duplicate samples are rejected. The collector accepts scalar Prometheus text samples only, caps a scrape at 1 MiB and 10,000 lines, and retains normalized observations plus bounded provenance rather than raw scrapes. Subject output names must use the `subject_` prefix; Gregg host metrics remain `host_*`. Required mapped fields missing at preflight or during a trial, and counter resets, fail closed.

Prometheus endpoints must use HTTP and loopback/private literal IPs or `.localhost` names resolving only to loopback. The collector snapshots counters at trial start, polls only during the runner-owned trial window, takes a final snapshot, and drains or aborts its poll task within the stop bound. Mapping digest and binding source are recorded with each trial. SynVoid metric names and mappings remain owned by the upstream SynVoid telemetry contract; the generic collector contains no SynVoid field vocabulary.

The identity implementation is shared with EggReplay fixture hashing. Existing fixture records retain the same canonical path, length, and SHA-256 input sequence.
