# Core architecture

`eggbench-core` owns runtime-free domain contracts: typed experiment plans, stable names and bounds, schema versions, and validation. It must not own process execution, asynchronous runtimes, command-line presentation, or concrete networking clients. Adapters resolve symbolic requests and perform side effects outside core.

The experiment-plan v1 decoder rejects unknown fields (`deny_unknown_fields`). Schema changes therefore require an explicit version update and compatibility decision. Ordered maps keep snapshots deterministic. Plan inputs carry references to secrets, never credential values.

The boundary is: human-authored plan → validated plan → (future driver resolution) → (future runner) → immutable evidence. Timing, driver selection, and evidence persistence are outside M001.

