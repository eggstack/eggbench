# SynVoid upstream asset manifest (expected import contract)

M002a consumes the closed SynVoid-owned qualification asset contract at
`dbowm91/synvoid:plans/eggbench_security_qualification_asset_contract.md`,
not a reconstruction inside Eggbench. That upstream plan is **open** at the
time of writing, so this document freezes the import boundary M002a will
verify once the upstream closes. The routine-test fixture at
`materialized/provenance.json` matches this schema and is explicitly
synthetic.

## Expected materialized layout (plan section 3)

```text
synvoid-qualification/
  config/
    main.toml
    sites/...
  corpus.json
  provenance.json
```

## Required `provenance.json` fields

| Field | Meaning |
|---|---|
| `policy_id` | Upstream policy identifier (expected `synvoid-qualification-assets.v1`) |
| `policy_version` | Upstream materializer policy version |
| `materializer` | Materializer name/version that generated the export |
| `source_sha` | Exact SynVoid source SHA the export was derived from |
| `package_version` | SynVoid package version (planning baseline `1.1.0`) |
| `cargo_lock_sha256` | SHA-256 of the source tree `Cargo.lock` |
| `source_fixture_digest` | Digest over the selected source fixtures + exclusion list |
| `excluded_fixture_ids` | Source fixture IDs deliberately excluded with reasons |
| `corpus_digest` | SHA-256 of the exported `corpus.json` |
| `config_digest` | Digest over the generated `config/` tree |
| `listen` | `{host, port}` of the SynVoid data plane under test |
| `origin` | `{url}` of the controlled origin the config proxies to |
| `mapping` | Observable block/pass mapping (Detect -> statuses, Pass -> status) |

## Verification procedure (plan sections 3, 10)

Before any candidate run, the harness MUST verify, and fail closed on
mismatch:

1. `policy_id` equals the pinned expectation;
2. `source_sha` equals the pinned live-qualification SHA;
3. `corpus_digest` recomputes over the exported `corpus.json`;
4. `config_digest` recomputes over the exported `config/` tree;
5. the scenario plan's SynVoid static `http_url` host/port equals
   `listen`;
6. the materialized config's upstream origin equals `origin.url`.

Port/policy/digest mismatches are evidence failures (`Invalid`), never
silent downgrades. The live harness
(`scripts/qualification/synvoid-m002/run-live-qualification.sh`) performs
checks 1-6; routine tests cover the same rejections against synthetic
fixtures.
