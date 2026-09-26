# Security qualification

M001a adds a subject-neutral profile and fixed HTTP corpus input format. The profile is independent of the ordinary experiment plan and contains explicit relative scenario plan paths plus corpus and target configuration references. M001b adds an ordinary-plan correctness check that executes the corpus through Eggfetch.

```sh
eggbench qualify validate examples/security-profile.json --json
eggbench qualify expand examples/security-profile.json --json
```

The current CLI treats the profile's containing directory as the explicit workspace root. Every profile, plan, corpus, body, and target configuration path is confined to that root. Expansion hashes exact plan bytes and normalized content-tree records; it does not execute a scenario or infer expectations from a baseline.

Corpus requests use relative origin-form paths, bounded methods and headers, and owner-authored exact or allowed-set status expectations. Credential-bearing headers, absolute request targets, fragments, and symlinked content are rejected. Category labels are opaque owner metadata.

An HTTP corpus plan uses schema version 8 and `http_corpus_checks`. Each check pins a workspace-relative corpus file, its M001a content-tree digest, target service, whole-check timeout, and per-case timeout. Run the CLI with the `eggstack-http` feature enabled to include the Eggfetch executor:

```sh
cargo run -p eggbench-cli --features eggstack-http -- validate examples/security-http-corpus-plan.json
cargo run -p eggbench-cli --features eggstack-http -- run examples/security-http-corpus-plan.json --bundle target/http-corpus.eggb
```

Cases run serially after readiness and before warmups. They do not enter trial metrics. Only HTTP status, owner-defined case IDs, case/request hashes, expectation, and stable dispositions enter the evidence artifact; request and response bodies and server error text are omitted. Transport failures and timeouts are Invalid. Status mismatches are correctness Fail while performance trials continue.

Targets come from the named service's runtime `http_url` binding and must be loopback, RFC1918, IPv6 ULA, or an explicit `.localhost` name. Eggfetch uses HTTP/1.1 with redirects and retries absent from its selected feature profile. HTTP corpus checks reject paired runs and `network_path` composition. Comparison receipts use schema v4 and `eggbench.security-correctness.v2` when this family is present; M004 WAF-only evidence keeps the immutable v1 correctness policy.

Service plans may declare an `http_url` on schema version 7 or later. Managed command services and external services publish this static non-secret value through `RuntimeBindings`; named adapters may publish the same value, while conflicts fail closed. Values are included in runtime topology evidence.

The identity implementation is shared with EggReplay fixture hashing. Existing fixture records retain the same canonical path, length, and SHA-256 input sequence.
