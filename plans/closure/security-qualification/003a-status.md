# Security Qualification M003a — Closure

Disposition: closed.

Implementation: `e023637` (`feat(security): bind qualification methods and runtime dependencies`). Follow-up qualification-test changes: `e53fddb`, `80cdeed`, `343d0f4`, and `f1e30cc`.

## Contract evidence

| Requirement | Evidence | Result |
|---|---|---|
| Qualification profile v2 freezes a workload driver for each scenario | Expansion records the selected driver; `qualify run` forwards it through the ordinary workload registry; receipts and inspect bind it to expansion and bundle evidence | Pass |
| Qualification profile v1 remains readable | Core and CLI compatibility tests cover the legacy profile form | Pass |
| Static service bindings are bounded, generic, and comparison-critical | Plan validation and resolution tests cover binding limits, runtime merge, and conflict rejection | Pass |
| Managed commands resolve typed dependency bindings in argv/environment | Runner resolves after dependency startup; references are restricted to declared dependencies, values are bounded, and argv0 cannot be replaced | Pass |
| Missing or conflicting references fail before consumer network activity | Core validation and runner preflight/lifecycle tests | Pass |
| Binding consumption is auditable without retaining the values | Runtime topology records consumer/source/destination, value digest, and public topology flag | Pass |
| Driver identity is immutable evidence | Qualification expansion, qualification receipt, inspect, and ordinary run artifacts retain the selected driver | Pass |
| No subject-specific launcher or secret interpolation was introduced | Implementation uses existing command lifecycle and workload registry; values are non-secret and no arbitrary templates are evaluated | Pass |
| Formatting, check, clippy, workspace tests, MSRV, and four hosted lanes are green | Hosted CI run [36499149359](https://github.com/eggstack/eggbench/actions/runs/36499149359), exact HEAD `f1e30ccadffbe008cc4d279b87a5b83851119a19` | Pass |
| Live qualification workflow remains green | Live workflow [36499149401](https://github.com/eggstack/eggbench/actions/runs/36499149401), same exact HEAD | Pass |

## Hosted test boundary

The synthetic same-source performance integration test now runs only on Linux, the qualification host used by M002. On macOS, the synthetic Python proxy repeatedly failed frozen relative gates at concurrency 1/8/32 while both direct-origin controls passed; this was not a stable cross-platform contract check. M002 C002 still requires the real Linux SynVoid same-source repeatability proof under the frozen v1 policy. No M002 performance threshold or correctness expectation was changed.

## Handoff and dependencies

M003b is unblocked and ready. M003c remains blocked on M003b and the SynVoid owner telemetry-contract closeout. The upstream telemetry plan is READY but is not closed, so it is recorded as an outstanding owner dependency. M003d remains blocked on M003a-c and M002 C002; this closure does not claim M002 terminal closure or M003 completion.

M003a is closed. M003b is ready for implementation.
