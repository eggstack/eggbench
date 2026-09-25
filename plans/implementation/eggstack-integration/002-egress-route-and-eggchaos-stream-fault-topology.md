# Eggstack Integration M002 — Listener-Free Eggress Route and Eggchaos Stream-Fault Topology

Status: implemented; closure pending hosted qualification

Repository baseline: d870512a5a1af16276ff05286ff0b6e2366b7f8f

Subsystem roadmap:

- plans/subsystems/eggstack-integration-roadmap.md

Controlling architecture:

- plans/000-long-term-specification.md
- plans/001-terminology-and-domain-model.md
- plans/002-long-term-roadmap.md
- plans/adrs/ADR-0001-typed-core-and-driver-boundaries.md
- plans/adrs/ADR-0002-immutable-evidence-and-testbed-provenance.md
- plans/adrs/ADR-0004-eggstack-composition-and-independent-oracles.md

Qualification prerequisite:

- post-M003 hosted qualification corrective C002 is closed;
- closure: plans/closure/post-m003-hosted-qualification-corrective/002-status.md;
- qualifying run: 36029547565;
- current documentation HEAD is also four-lane green at CI run 36030077617.

Primary class: Eggstack integration / topology semantics.

## 1. Objective

Implement Eggbench's first explicit routed and faulted in-process network path for the native Eggfetch HTTP workload.

The initial production path is:

~~~text
Eggfetch workload
   |
   | eggfetch_core::Dialer
   v
Eggbench network-path dialer
   |
   +--> Eggress OutboundConnector
   |       listener-free route/proxy-chain establishment
   |       no direct fallback after requested route failure
   |
   v
established logical target stream
   |
   +--> Eggchaos BidirectionalChaosStream
           deterministic user-space stream faults
           upstream = workload client -> final target
           downstream = final target -> workload client
~~~

Eggbench owns only:

- declarative network-path intent;
- capability/resolution policy;
- lowering into sibling public APIs;
- deterministic seed assignment;
- lifecycle/cancellation integration;
- bounded route/fault provenance;
- evidence and comparability rules.

Eggress remains authoritative for proxy routing, hop negotiation, route failures, and stream establishment.

Eggchaos remains authoritative for deterministic byte-stream fault semantics.

Eggfetch remains authoritative for HTTP framing, pooling, TLS/HTTP behavior, request semantics, and connection reuse.

M002 MUST NOT implement a competing proxy, chaos engine, HTTP stack, or listener.

## 2. Why network path is a first-class contract

Do not encode M002 as a synthetic managed service.

The existing runner ServiceAdapter seam is appropriate for services with start/readiness/shutdown lifecycle and runtime bindings. Listener-free routing and stream wrapping have different semantics:

- no route listener needs to be started;
- no route service owns a socket solely for Eggbench;
- route establishment occurs when Eggfetch needs a physical connection;
- stream faults operate on that physical/logical connection;
- the path participates in request timing but has no independent service lifecycle;
- service adapters do not receive typed dependency streams;
- introducing a loopback bridge would add a hop Eggbench does not need.

Therefore M002 adds a declarative network-path contract associated with workload transport, rather than pretending routing/fault injection is another topology service.

## 3. Sibling seam audit — 2026-09-24

### 3.1 Eggress

Current default branch workspace version: 1.0.10, Rust 1.89.

Latest published GitHub release inspected during planning: v1.0.9. The public listener-free seam required by M002 already exists in v1.0.9:

- crate: eggress-outbound;
- base profile supports ordinary HTTP and SOCKS TCP chains;
- OutboundConnector::from_chain(...);
- OutboundConnector::direct();
- connect_tcp_detailed(...);
- connect_tcp_timeout_detailed(...);
- stable typed route diagnostics through OutboundConnectError:
  - kind;
  - stage;
  - hop_index;
  - protocol;
- OutboundInfo:
  - local_addr;
  - peer_addr;
  - hop_count.

The Eggress contract explicitly states that route failure does not silently fall back to direct.

M002 production code SHOULD use eggress-outbound directly. It MUST NOT use eggress-embed merely as a convenience umbrella.

egress-uri may be used for native proxy-chain parsing/typed construction where required.

Version policy at implementation start:

1. re-check crates.io/GitHub release state;
2. if 1.0.10 has become published and the audited listener-free API remains compatible, pin the exact published 1.0.10 crates;
3. otherwise pin the exact published 1.0.9 seam already audited here;
4. do not use mutable git main merely to obtain unreleased internal changes.

Implementation re-check on 2026-09-24 found Eggress 1.0.10 published with the audited listener-free API unchanged; the implementation pins exact 1.0.10 crates and retains 1.0.9 only as the planning-time reference.

Initial feature profile is the base TCP profile only. Do not enable pproxy-compat, extended, ssh, quic, udp, insecure-tls, or full embed features for M002.

### 3.2 Eggchaos

Published release: v0.1.0, Rust 1.89+.

The release publishes eggchaos-core and other libraries on crates.io and has release qualification evidence across supported platforms.

The M002 stream seam is eggchaos-core, not the Eggchaos server/control plane.

Relevant public types include:

- FaultPlan;
- FaultSpec;
- FaultKind;
- LatencyConfig;
- BandwidthConfig;
- BlackholeConfig;
- LimitDataConfig;
- SlowCloseConfig;
- SliceConfig;
- DisconnectConfig;
- Probability;
- RngVersion;
- LivePolicy;
- BidirectionalChaosStream;
- Direction.

Direction semantics are sibling-owned and MUST remain:

- Upstream = workload client -> target;
- Downstream = target -> workload client.

eggchaos-eggfetch is intentionally NOT the production seam for M002. Its ChaosDialer owns a direct TCP dial and then applies Eggchaos. Eggbench needs route-then-fault composition, so it must wrap the stream returned by Eggress using eggchaos-core rather than creating a second direct-dial path.

Eggchaos default branch has additional post-release datagram work. M002 does not consume that subsystem. It remains stream-only.

Initial dependency policy:

- exact eggchaos-core = 0.1.0 unless a later published version is re-audited before implementation;
- no eggchaos-server;
- no eggchaos-toxiproxy;
- no eggchaos-eggfetch production dependency.

## 4. M002 scope

M002 delivers:

1. ExperimentPlan schema v3 network-path intent;
2. ResolvedPlan representation of that path and concrete route/fault driver identities;
3. route/fault driver categories and capability validation;
4. an opt-in eggstack-path build feature;
5. listener-free Eggress TCP route composition;
6. deterministic Eggchaos bidirectional stream wrapping;
7. Eggfetch custom-dialer integration while preserving one client per run;
8. route/fault evidence and comparison-critical provenance;
9. CLI validate/doctor/run/inspect support sufficient to expose and fail closed on path configuration;
10. deterministic integration tests including proof that requested routing never falls back direct;
11. fresh four-lane hosted qualification.

## 5. Explicit non-goals

M002 does not add:

- a managed/listening Eggress proxy created by Eggbench;
- a loopback bridge between Eggfetch, Eggress, and Eggchaos;
- Eggress full runtime/embed lifecycle;
- Eggress extended protocols;
- SSH;
- QUIC/H3;
- UDP routing;
- Eggchaos datagram faults;
- Toxiproxy API/control-plane integration;
- Eggchaos server/API/CLI integration;
- live fault mutation during a run;
- scenarios/time-varying fault schedules;
- probabilistic per-connection fault activation;
- hard-reset fault requests;
- paired/interleaved network-path experiments;
- external-oracle routing through this path;
- Linux tc/netem;
- packet-loss claims;
- new normalized path-performance metrics;
- security-scanner semantics;
- a generic arbitrary Dialer plugin system.

External Oracles M003 remains the future netem/system-level impairment milestone.

## 6. Core schema v3

### 6.1 Versioning

Add:

- EXPERIMENT_PLAN_SCHEMA_VERSION_3 = SchemaVersion(3);
- RESOLVED_PLAN_SCHEMA_VERSION_3 if the resolved-plan wire shape changes.

Schema v1 and v2 parsing, validation, and serialization behavior MUST remain supported and regression-tested.

Do not reinterpret a v1/v2 plan as carrying a default path. Absence remains explicit.

### 6.2 ExperimentPlan field

Add an optional field:

~~~text
network_path: Option<NetworkPathRequest>
~~~

Rules:

- schema v1: network_path must be absent;
- schema v2: network_path must be absent;
- schema v3: network_path may be absent or present;
- unknown fields remain rejected.

### 6.3 Typed request model

Preferred core-owned model:

~~~text
NetworkPathRequest
  route: RouteRequest
  stream_faults: Option<StreamFaultPlanRequest>

RouteRequest
  driver: Name
  mode: RouteMode

RouteMode
  Direct
  ProxyChain { chain: String }

StreamFaultPlanRequest
  driver: Name
  upstream: Vec<StreamFaultRequest>
  downstream: Vec<StreamFaultRequest>

StreamFaultRequest
  id: Name
  kind: StreamFaultKind
~~~

StreamFaultKind should expose only the stable M002 semantic subset:

~~~text
Latency
  delay_ms: DurationMs
  jitter_ms: DurationMs
  max_buffer_bytes: Positive/nonzero bounded integer

Bandwidth
  bytes_per_second: Positive/nonzero integer
  burst_bytes: Positive/nonzero integer

Blackhole
  close_after_ms: Option<DurationMs>

LimitData
  bytes: Positive/nonzero integer

SlowClose
  delay_ms: DurationMs

Slice
  average_size: Positive/nonzero integer
  variation: u64
  delay_ms: DurationMs

Disconnect
  after_ms: DurationMs
~~~

M002 stream faults have implicit activation probability 1.0.

Do not expose a probability field in schema v3 M002.

Do not expose hard_reset in schema v3 M002.

This keeps path determinism tied to the experiment seed and avoids ambiguous connection-pool sampling behavior in the first integration.

### 6.4 Bounds

Add explicit validation:

- maximum 128 faults per direction, matching the sibling evidence identity bound;
- unique fault IDs within each direction;
- no zero buffer/rate/limit/burst values;
- slice variation < average size;
- durations remain within existing DurationMs bounds;
- route chain string has a conservative maximum byte length;
- no control characters.

Use existing typed bounded primitives where possible rather than raw integers.

## 7. Credential and secret policy

M002 route configuration MUST be safe to persist in source/resolved evidence.

For ProxyChain input:

- reject URI userinfo/embedded passwords/tokens;
- reject any chain representation whose redacted form cannot be proven before persistence;
- do not accept secrets via arbitrary Service.config strings;
- do not copy Eggress credential values into network-path evidence.

If a useful route requires credentials, M002 must fail preflight with a stable credentials-not-supported category.

A future milestone may introduce typed SecretRef-backed route credentials with a non-secret resolved identity. Do not improvise that mechanism in M002.

## 8. Paired experiment policy

M002 MUST reject network_path together with paired design.

Reason:

- M003 paired execution keeps both arms live;
- Eggfetch owns a connection pool across warmups/trials;
- route/fault policies attach to physical connections;
- alternating logical trial arms do not imply fresh physical connections;
- per-arm route/fault identity would therefore be ambiguous without an explicit connection-lifecycle contract.

The validation failure must occur before startup.

A later plan may define per-arm network paths together with deterministic physical connection reset/reuse semantics.

Do not weaken the M003 paired contract to make M002 fit.

## 9. Driver categories and capability model

### 9.1 New categories

Add two explicit driver categories:

- Route;
- Fault.

These are provenance/capability categories. They are not Workload or Service aliases.

ResolvedPlan drivers should record concrete selected descriptors for both requested path layers.

### 9.2 Route descriptor

Canonical name:

~~~text
eggress-route
~~~

Descriptor:

- category Route;
- upstream name eggress-outbound;
- exact resolved upstream version from Cargo.lock;
- non-external-process;
- base TCP route capability;
- no UDP claim;
- no extended/SSH/QUIC claim.

### 9.3 Fault descriptor

Canonical name:

~~~text
eggchaos-stream
~~~

Descriptor:

- category Fault;
- upstream name eggchaos-core;
- exact resolved upstream version from Cargo.lock;
- non-external-process;
- deterministic stream-fault capability;
- RngVersion v1 provenance;
- no datagram capability.

### 9.4 Workload compatibility

Add a workload capability representing custom network-path dialing, for example:

~~~text
Capability::NetworkPath
~~~

or an equally explicit typed variant.

The eggfetch-http workload advertises it only when eggstack-path is enabled.

oha, h2load, and iperf3 do not advertise this capability in M002.

When network_path is requested:

- resolution requires a workload driver with NetworkPath;
- resolution requires selected Route and, when stream_faults is present, Fault descriptors;
- failure occurs before environment collection, service startup, or bundle creation.

Requested routed behavior MUST NOT silently execute direct.

## 10. Feature isolation and dependency shape

Add an Eggbench drivers feature:

~~~text
eggstack-path = [
  eggstack-http,
  dep:eggress-outbound,
  dep:eggress-uri,
  dep:eggchaos-core
]
~~~

The CLI exposes a corresponding eggstack-path feature forwarding to eggbench-drivers.

Required feature properties:

- default build does not link Eggress or Eggchaos;
- eggstack-http alone keeps current M001 behavior and does not link Eggress/Eggchaos;
- gregg remains orthogonal;
- eggstack-path implies eggstack-http because M002 initially composes only with eggfetch-http;
- all-features remains valid.

Production dependencies SHOULD be exact published versions after implementation-time re-audit.

Do not depend on eggress-embed or eggchaos-server in production.

A dev-dependency on a narrow Eggress test/runtime crate is permitted solely for proving real routed traffic in integration tests if a simpler local proxy fixture would duplicate too much protocol behavior. Prefer sibling testkit or documented embed surface rather than writing an HTTP CONNECT/SOCKS proxy in Eggbench tests.

## 11. Dependency provenance

Extend crates/eggbench-drivers/build.rs Cargo.lock extraction with:

- eggress-outbound;
- eggress-uri if directly depended upon;
- eggchaos-core.

Expose compile-time exact resolved versions analogous to M001:

- EGGBENCH_EGGRESS_OUTBOUND_VERSION;
- EGGBENCH_EGGRESS_URI_VERSION;
- EGGBENCH_EGGCHAOS_CORE_VERSION.

Unknown lockfile values may preserve the existing build-script fallback behavior, but production doctor/evidence should identify unknown provenance clearly.

Do not hardcode a patch version in runtime evidence.

## 12. Network-path lowering

Add a driver-owned lowering layer, e.g.:

~~~text
eggstack/path/
  mod.rs
  route.rs
  fault.rs
  dialer.rs
  evidence.rs
~~~

Exact filenames are not contractual; ownership is.

### 12.1 Route lowering

Direct:

~~~text
RouteMode::Direct
  -> OutboundConnector::direct()
~~~

ProxyChain:

~~~text
RouteMode::ProxyChain
  -> validate credential-free chain
  -> eggress_uri native chain parse
  -> OutboundConnector::from_chain(...)
~~~

Use native Eggress construction.

Do not use pproxy compatibility syntax merely because it is convenient.

Unsupported hop/protocol configuration fails preflight.

### 12.2 Fault lowering

Convert core StreamFaultRequest values to eggchaos-core values.

For every fault:

- FaultId comes from the stable Eggbench ID;
- Probability is exactly 1.0;
- fault order is preserved;
- sibling validation is authoritative after Eggbench validates its own schema;
- no unknown fallback conversion is permitted.

Construct independent upstream and downstream FaultPlan values.

When no faults exist for a direction, use FaultPlan::empty().

### 12.3 Seed contract

If stream_faults is present and either direction is non-empty:

- ExperimentPlan.seed MUST be Some(...);
- missing seed is a validation/preflight failure;
- pass the experiment seed as the Eggchaos LivePolicy seed namespace;
- record Eggchaos RngVersion;
- do not draw an OS-random seed.

The same run/plan/connection ordinal must produce the same Eggchaos policy-seed derivation subject to sibling v1 semantics.

## 13. EggstackPathDialer

Add a small Eggbench-owned Dialer implementation behind eggstack-path.

Conceptual state:

~~~text
EggstackPathDialer
  route: Arc<OutboundConnector>
  upstream_policy: Option<LivePolicy>
  downstream_policy: Option<LivePolicy>
  connection_ordinal: AtomicU64
  connect_timeout: Duration
  diagnostics: bounded shared PathDiagnostics
~~~

It implements eggfetch_core::Dialer.

### 13.1 Dial path

For each physical Eggfetch dial:

1. allocate a deterministic monotonically increasing connection ordinal;
2. call Eggress connect_tcp_timeout_detailed(host, port, timeout);
3. on failure:
   - capture bounded stable kind/stage/hop/protocol facts;
   - map failure into Eggfetch DialError;
   - return the error;
   - NEVER attempt a direct fallback;
4. on success:
   - capture bounded OutboundInfo diagnostics;
   - if faults are configured, wrap the returned stream with BidirectionalChaosStream::new_live;
   - box the resulting stream as Eggfetch DialStream;
   - return exactly one stream.

No shell, subprocess, listener, or second route decision exists in this path.

### 13.2 Route/fault ordering

Faults apply AFTER Eggress establishes the routed logical connection.

That means M002 faults affect the end-to-end application byte stream carried through the established route; they do not independently fault:

- DNS lookup;
- proxy TCP connect;
- SOCKS/HTTP CONNECT handshake bytes;
- individual proxy hops;
- TLS handshake unless Eggfetch performs TLS over the returned logical stream.

This ordering MUST be recorded in evidence.

### 13.3 Direction meaning

For the wrapped logical stream:

- upstream = Eggfetch/workload client -> final target;
- downstream = final target -> Eggfetch/workload client.

Do not reinterpret directions relative to proxy hops.

## 14. Timing semantics

M002 must preserve the runner's measured interval.

Untimed preflight:

- schema validation;
- route-chain parsing/validation;
- credential rejection;
- descriptor/capability resolution;
- fault-plan lowering/validation;
- route/fault dependency provenance construction.

Timed when it occurs during workload execution:

- physical route connect;
- proxy handshake;
- Eggchaos stream delay/throttling/drop behavior;
- Eggfetch request dispatch through full response-body consumption.

Untimed after workload completion:

- diagnostic snapshot serialization;
- normalization;
- evidence staging;
- comparison.

Do not subtract route connect or fault delay from request latency. They are part of the observed workload path.

## 15. Eggfetch workload integration

Preserve the M001 invariant: one Eggfetch client per workload executor for the whole run.

Add construction such as:

~~~text
EggfetchWorkload::with_dialer(...)
~~~

or a typed path constructor.

Do not create a client per request/trial.

Warmups may therefore populate Eggfetch connection state and measured trials may reuse physical connections exactly as in existing M001 behavior.

M002 must make this inspectable:

- record physical dial attempts/successes;
- preserve maximum request concurrency evidence;
- preserve one-client-per-run method provenance.

Do not force a new connection per trial solely to make faults easier to explain.

Because policies are static for M002, a pooled connection sees the same fault plan throughout its lifetime.

## 16. Production executor construction

Current production_workload_executor accepts only the selected workload driver name.

Refactor minimally so production executor construction can consume the already-resolved network path.

Preferred direction:

~~~text
production_workload_executor(
    resolved_plan,
    workload_driver
)
~~~

or a narrower typed resolved-path argument.

Requirements:

- no route/fault parsing after managed startup;
- no main.rs string-switch;
- driver factory remains behind the existing registry/driver module boundary;
- external-process oracle construction remains behaviorally unchanged;
- eggfetch-http without network_path remains behaviorally unchanged.

The current run path resolves before constructing the executor; use that existing pre-start boundary instead of creating a second hidden resolution.

If run_impl's second resolution creates divergence risk, reconcile it narrowly and prove source/resolved consistency; do not create a third plan representation.

## 17. Evidence model

### 17.1 Run-level network path evidence

Add a versioned artifact:

~~~text
network-path.json
~~~

Suggested schema identifier:

~~~text
NETWORK_PATH_EVIDENCE_SCHEMA_VERSION = 1
~~~

It records only non-secret deterministic/provenance facts:

Route:

- selected route driver;
- Eggbench adapter version;
- eggress-outbound exact resolved version;
- optional eggress-uri exact resolved version;
- route mode;
- credential-free canonical/redacted chain representation;
- chain/config digest;
- declared hop count where deterministically known.

Fault:

- selected fault driver;
- Eggbench adapter version;
- eggchaos-core exact resolved version;
- Eggchaos RngVersion;
- seed namespace;
- exact ordered upstream typed faults;
- exact ordered downstream typed faults;
- static-policy marker.

Semantics:

- route-first/fault-second;
- stream-layer, not packet-layer;
- upstream/downstream definitions.

The artifact must not contain:

- passwords;
- tokens;
- route URI userinfo;
- unbounded error strings;
- payload bytes.

### 17.2 Per-invocation method evidence

Extend the existing eggfetch-method.json additively with a network_path object when configured.

Include bounded diagnostics:

- physical dial attempts;
- successful routed dials;
- route failures by stable Eggress kind/stage/protocol;
- observed hop-count distribution or bounded set;
- whether fault wrapping was active;
- connection ordinal range/count.

Do not make ephemeral local/peer socket addresses comparison-critical.

Avoid writing raw upstream error Display strings when stable categories suffice.

### 17.3 Fault evidence limitation

Do not claim per-connection Eggchaos activation counters unless the public BidirectionalChaosStream seam exposes them safely.

M002 closure requires plan/seed/RNG provenance and behavioral tests, not invented internal instrumentation.

If a small upstream Eggchaos API addition becomes necessary solely for bounded read-only evidence, STOP and write the needed upstream plan in eggchaos rather than reaching into private internals or copying the engine.

## 18. Bundle and topology representation

Network path is experiment topology intent but not a managed Service.

Preferred evidence treatment:

- source plan contains network_path;
- resolved-plan contains resolved network path plus selected Route/Fault drivers;
- network-path.json contains runtime/method provenance;
- runtime-topology.json remains service lifecycle evidence unless a backward-compatible schema bump explicitly adds a separate paths section.

Do not insert a fake service entry merely to make runtime-topology.json mention the route.

If runtime-topology is extended:

- bump its schema;
- retain reader compatibility;
- separate services from network_paths;
- do not overload ServiceOwnership.

A standalone network-path.json is sufficient for M002 and is the lower-risk default.

## 19. Comparison-critical identity

M002 network path is comparison-critical configuration.

Comparison must consider at least:

- presence/absence of network_path;
- route driver name/version;
- route mode;
- canonical route-chain identity/config digest;
- fault driver name/version;
- Eggchaos RNG version;
- experiment fault seed;
- ordered upstream fault plan;
- ordered downstream fault plan;
- route/fault ordering semantics version.

A mismatch must flow through the existing environment/comparability policies rather than invent a new verdict system.

Expected existing policy behavior:

- StrictSameTestbed: critical path mismatch makes baseline-relative primary gating invalid;
- WarnOnMismatch: preserve descriptive effects, suppress relative/statistical gate verdict;
- CrossTestbedDescriptive: relative/statistical baseline gate remains descriptive/suppressed;
- absolute candidate-only gates may remain eligible according to existing Measurement M002 semantics.

Informational runtime facts such as ephemeral ports, socket addresses, physical dial counts, and observed hop metadata MUST NOT make otherwise identical configurations incomparable.

## 20. CLI behavior

### 20.1 validate

Validate schema v3 and all network-path invariants.

### 20.2 doctor

Report:

- eggstack-path compiled/enabled state;
- route driver name/upstream version;
- fault driver name/upstream version;
- supported M002 route/fault capabilities;
- explicit unsupported capabilities.

Doctor must not establish a real route or start load.

### 20.3 run

Fail before managed startup on:

- unsupported_network_path;
- missing_route_driver;
- missing_fault_driver;
- invalid_route;
- invalid_fault_plan;
- missing_fault_seed;
- route_credentials_not_supported;
- workload_path_incompatible;
- paired_network_path_not_supported.

Map these to the existing CapabilityPreflight exit class/code 3 unless an existing more precise parse/schema category applies.

Do not add a new broad exit-code family.

### 20.4 inspect

When practical, surface the retained network path identity from source/resolved/path evidence.

Inspection must remain read-only.

## 21. Initial supported route matrix

M002 production support is intentionally small.

Required:

- Direct TCP through OutboundConnector::direct;
- native single/multi-hop proxy chain supported by the base eggress-outbound TCP profile;
- ordinary HTTP CONNECT and SOCKS routes covered by the base published seam.

Explicitly unsupported in M002:

- extended protocol features;
- SSH;
- QUIC/H3;
- Eggress UDP;
- pproxy compatibility syntax;
- insecure TLS feature;
- reverse proxy;
- listener-bound routing;
- routing retries/fallbacks invented by Eggbench.

An unsupported requested route must fail closed.

## 22. Initial supported fault matrix

Required static stream faults:

- latency;
- bandwidth;
- blackhole;
- limit-data;
- slow-close;
- slice;
- graceful disconnect.

Restrictions:

- probability = 1.0;
- no hard reset;
- no live mutation;
- no scenarios;
- no datagram loss/duplication/reordering/corruption;
- no packet terminology.

Every fault kind must have deterministic lowering and at least one behavior test.

## 23. Cancellation and cleanup

Eggbench cancellation remains runner-owned.

A cancelled workload must:

- stop issuing new requests under the existing Eggfetch workload contract;
- drop/cancel in-flight dial futures cleanly;
- not leave an Eggress listener, because M002 creates none;
- not leave detached route/fault tasks owned by Eggbench;
- continue through existing workload drain and service teardown.

Eggress connection futures are cancelled by future drop unless the sibling API documents stronger cleanup.

Eggchaos wrappers are owned by Eggfetch physical streams and drop with them.

No new process-group or service-shutdown path should be introduced by M002.

## 24. Security posture

M002 is intended for controlled/private experiments.

Requirements:

- no shell execution;
- no route credential serialization;
- no implicit public listener;
- no hidden MITM/TLS interception;
- no direct fallback after proxy failure;
- no arbitrary dynamic plugin loading;
- bounded chain text;
- bounded fault count;
- bounded diagnostic event counts;
- bounded evidence artifacts;
- stable credential-safe Eggress typed errors preferred over raw strings;
- Debug output of path objects must be credential-safe.

The implementation must include a source/evidence grep-style regression proving configured secret-like test values never appear in persisted network-path evidence.

## 25. Testing strategy

### 25.1 Core schema

Add fixtures/tests for:

- v1 unchanged;
- v2 unchanged;
- v3 without network_path;
- v3 direct path;
- v3 proxy-chain path;
- v3 route + upstream/downstream faults;
- unknown-field rejection;
- v1/v2 network_path rejection;
- duplicate fault IDs;
- invalid bounds;
- missing seed;
- credential-bearing chain rejection;
- paired + network_path rejection.

Old v1/v2 goldens MUST remain byte/semantic stable where existing tests promise that.

### 25.2 Resolution

Prove:

- network_path selects Route/Fault categories;
- eggfetch-http with eggstack-path resolves;
- eggfetch-http compiled without eggstack-path fails;
- external oracle + network_path fails before startup;
- missing route/fault descriptor fails;
- unsupported route/fault capability fails;
- exact upstream versions survive resolved-plan serialization.

### 25.3 Dependency/feature isolation

Required checks:

- default;
- eggstack-http only;
- gregg only;
- eggstack-path;
- eggstack-path + gregg;
- all-features.

Verify cargo tree does not pull Eggress/Eggchaos into default or eggstack-http-only builds.

### 25.4 Route integration

Use a real sibling-owned proxy test seam.

At minimum prove:

1. controlled EggServe origin;
2. local test proxy;
3. Eggfetch workload configured with network_path;
4. traffic reaches the origin through Eggress;
5. route evidence records the expected hop count/config identity.

Do not implement an Eggbench SOCKS/HTTP CONNECT proxy simply for this test if Eggress testkit/embed can provide the fixture.

### 25.5 No-fallback acceptance test

Critical test:

- origin is directly reachable;
- configured proxy route is intentionally unavailable/failing;
- workload attempts the route;
- request fails;
- origin observes no direct request;
- evidence reports typed route failure;
- execution never silently succeeds direct.

This is an M002 release-blocking invariant.

### 25.6 Fault integration

At minimum prove deterministic behavior for each supported fault kind.

Also prove one full composed path:

~~~text
Eggfetch
 -> Eggress route
 -> Eggchaos bidirectional stream
 -> EggServe
~~~

Use deterministic seeds and bounded timeouts.

### 25.7 Directionality

Use distinct upstream/downstream effects to prove:

- upstream faults affect request-side byte flow;
- downstream faults affect response-side byte flow;
- the labels are final-target relative, not proxy-hop relative.

### 25.8 Pooling/warmup

Prove:

- one Eggfetch client is retained across warmups/trials;
- physical dial count can be lower than request count under reuse;
- static fault policy remains attached to reused physical connections;
- evidence does not pretend every request created a new fault connection.

### 25.9 Evidence

Golden/test:

- network-path schema v1;
- exact sibling versions;
- route-first/fault-second semantic marker;
- no secrets;
- deterministic fault config + seed;
- bounded route diagnostics;
- no packet-loss wording;
- no ephemeral address in comparison identity.

### 25.10 Comparison

Construct otherwise identical bundles and vary one item at a time:

- route mode;
- route chain;
- Eggress version;
- fault presence;
- Eggchaos version;
- seed;
- upstream plan;
- downstream plan.

Verify existing comparability policy produces the intended strict/warn/descriptive disposition.

## 26. Performance and measurement guardrails

M002 is not a performance-optimization milestone, but it must avoid obvious harness distortion.

Record enough method evidence to separate:

- request count;
- physical dial count;
- route hop count;
- fault configuration.

Do not set a numeric overhead budget in this implementation plan without measured evidence.

Add a diagnostic no-fault route smoke comparing:

- existing direct Eggfetch;
- eggstack-path Direct with empty fault plan.

The purpose is to catch order-of-magnitude regressions or accidental extra listeners/copies, not to create a release gate from an invented percentage.

If the no-fault path demonstrates substantial unexplained overhead, record measurements and stop for a dedicated optimization plan rather than optimizing blindly inside M002.

## 27. Documentation requirements

Update:

- README feature/capability summary;
- doctor/help text as needed;
- Eggstack integration roadmap;
- network-path plan examples;
- evidence documentation;
- terminology stating stream faults are not packet faults.

Include one complete example plan showing:

- EggServe origin;
- Eggfetch closed-loop workload;
- Eggress route;
- deterministic Eggchaos upstream/downstream faults;
- explicit seed.

Also include an unsupported example documenting why paired + network_path is rejected in M002.

## 28. Build and verification matrix

Before closure, run:

~~~text
cargo fmt --all -- --check

cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked

cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
EGGBENCH_PARENT_SENTINEL_26CE=set-in-parent-process cargo test --workspace --all-targets --all-features --locked

cargo +1.89.0 check --workspace --all-targets --locked
cargo +1.89.0 check --workspace --all-targets --all-features --locked
cargo +1.89.0 test -p eggbench-core --all-features --locked
cargo +1.89.0 test -p eggbench-drivers --all-features --locked

cargo tree --locked
git diff --check
~~~

Also run the targeted feature-isolation matrix from section 25.3.

## 29. Hosted qualification

Require a fresh GitHub Actions run on the exact implementation/closure candidate.

Required lanes:

- Linux stable;
- Linux Rust 1.89;
- macOS stable;
- Windows stable.

All four must pass on the same commit.

Hosted qualification must include:

- workspace check;
- all-feature Clippy with -D warnings on stable lanes;
- relevant workspace/core/driver tests;
- Windows supported subset;
- macOS process/filesystem regressions already present;
- the new schema/resolution/evidence tests.

If real proxy runtime integration is capability-gated on a hosted OS, document the exact skipped portion and retain deterministic fixture/unit coverage. Prefer a portable loopback fixture so the composed route test runs on all three stable OSes.

## 30. Closure record

After implementation and hosted qualification, create:

plans/closure/eggstack-integration/002-status.md

It must record:

- implementation commit(s);
- exact Eggress crate/version used;
- exact Eggchaos crate/version used;
- why those seams were selected;
- feature/dependency-tree evidence;
- schema/resolved-plan version changes;
- route/fault capability matrix;
- no-fallback test result;
- credential-redaction result;
- deterministic fault results;
- composed Eggfetch -> Eggress -> Eggchaos -> EggServe result;
- pooling/warmup result;
- comparison-critical identity tests;
- local default/all-feature test counts;
- Rust 1.89 result;
- hosted run ID;
- four hosted job conclusions;
- unresolved findings by severity;
- final disposition.

M002 is not closed from compilation alone.

## 31. Acceptance criteria

M002 closes only when all are true:

1. schema v3 network_path is implemented while v1/v2 remain supported;
2. network path is not modeled as a fake managed service;
3. Route and Fault driver identities are present in resolved provenance;
4. default and eggstack-http-only builds do not link Eggress/Eggchaos;
5. eggstack-path uses published narrow crates;
6. no Eggress listener is started by production M002;
7. Eggress route failure never falls back direct;
8. credential-bearing route input fails before startup;
9. all supported fault kinds lower through eggchaos-core;
10. fault activation is deterministic/static with probability 1.0;
11. non-empty faults require an explicit experiment seed;
12. paired + network_path fails before startup;
13. external-oracle + network_path fails before startup;
14. route-first/fault-second semantics are documented and evidenced;
15. upstream/downstream meaning is proven by tests;
16. one Eggfetch client remains alive across the run;
17. physical dial reuse is observable and not confused with request count;
18. network-path evidence is bounded, versioned, and secret-free;
19. network-path configuration is comparison-critical;
20. no packet-level claims are made for stream faults;
21. no new normalized metric semantics are invented;
22. default/all-feature Clippy passes with -D warnings;
23. Rust 1.89 remains green;
24. Linux stable hosted CI passes;
25. Linux 1.89 hosted CI passes;
26. macOS stable hosted CI passes;
27. Windows stable hosted CI passes;
28. closure evidence is committed;
29. no unresolved correctness/security/portability finding remains.

## 32. Stop conditions

Stop for planning review if:

- the currently published Eggress release no longer exposes the audited listener-free route seam;
- route composition requires eggress-embed/full runtime in production;
- the Eggress BoxStream cannot be wrapped by BidirectionalChaosStream and returned as Eggfetch DialStream without copying protocol code or introducing unsafe code;
- satisfying M002 requires an unpublished mutable git dependency;
- route credentials would be serialized into source/resolved/evidence artifacts;
- the only way to provide fault evidence is to reach into Eggchaos private internals;
- a requested route needs silent direct fallback to pass tests;
- the custom dialer would change Eggfetch HTTP ownership/pooling semantics;
- paired support requires changing connection reuse/trial semantics;
- a schema change beyond the scoped v3 path contract becomes necessary;
- a sibling API defect is discovered that should be fixed upstream;
- hosted CI exposes a substantive correctness/lifecycle defect rather than a narrow portability hygiene issue.

If an upstream change is required, write and register a sibling-repository implementation plan and block M002 on that exact dependency instead of copying or patching sibling internals inside Eggbench.

## 33. Handoff sequence

Implementation should proceed in this order:

~~~text
WP1  schema v3 + validation
  -> WP2 Route/Fault driver descriptors + provenance
  -> WP3 path lowering and deterministic fault plans
  -> WP4 EggstackPathDialer route-first/fault-second composition
  -> WP5 Eggfetch workload factory integration
  -> WP6 evidence + comparability
  -> WP7 CLI/doctor/inspect
  -> WP8 composed/no-fallback/security tests
  -> WP9 feature/MSRV/default/all-feature qualification
  -> WP10 four-lane hosted qualification
  -> closure record + registry reconciliation
~~~

Do not start M003 replay/diagnostics implementation until M002 closes unless the work is demonstrably independent and touches no shared network-path/driver/evidence contracts.
