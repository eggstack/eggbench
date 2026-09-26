# Security qualification inputs

M001a adds a subject-neutral profile and fixed HTTP corpus input format. The profile is independent of the ordinary experiment plan and contains explicit relative scenario plan paths plus corpus and target configuration references.

```sh
eggbench qualify validate examples/security-profile.json --json
eggbench qualify expand examples/security-profile.json --json
```

The current CLI treats the profile's containing directory as the explicit workspace root. Every profile, plan, corpus, body, and target configuration path is confined to that root. Expansion hashes exact plan bytes and normalized content-tree records; it does not execute a scenario or infer expectations from a baseline.

Corpus requests use relative origin-form paths, bounded methods and headers, and owner-authored exact or allowed-set status expectations. Credential-bearing headers, absolute request targets, fragments, and symlinked content are rejected. Category labels are opaque owner metadata.

Service plans may declare an `http_url` on schema version 7. Managed command services and external services publish this static non-secret value through `RuntimeBindings`; named adapters may publish the same value, while conflicts fail closed. Values are included in runtime topology evidence. M001b applies the local/private destination policy before making requests.

The identity implementation is shared with EggReplay fixture hashing. Existing fixture records retain the same canonical path, length, and SHA-256 input sequence.
