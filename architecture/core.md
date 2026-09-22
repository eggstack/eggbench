# Core architecture

`eggbench-core` owns runtime-free domain contracts: typed experiment plans, stable names and bounds, schema versions, and validation. It must not own process execution, asynchronous runtimes, command-line presentation, or concrete networking clients. Adapters resolve symbolic requests and perform side effects outside core.

The experiment-plan v1 decoder rejects unknown fields (`deny_unknown_fields`). Schema changes therefore require an explicit version update and compatibility decision. Ordered maps keep snapshots deterministic. Plan inputs carry references to secrets, never credential values.

The boundary is: human-authored plan → validated plan → resolved plan → (future runner) → immutable evidence. Driver descriptors and resolution are typed, serializable contracts; actual driver implementations and side effects remain outside core. Evidence APIs own streaming artifact staging, finalization, digest verification, and read-only inspection without a database.
