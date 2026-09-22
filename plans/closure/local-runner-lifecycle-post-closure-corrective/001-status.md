# Local Runner Post-Closure Corrective C001 — Closure

Disposition: **closed**

Implementation commits: `5cdd12426c092c31bc9c26fb9b7eb75464435276` and follow-up
qualification/portability fixes through `99862de9cf76c0036b3ea1c6f6094f60196536f2`.

Foundation corrective baseline: `f10e03d224a3ca62cec04cd124154ace7685398f`.

## Requirement-to-evidence matrix

| Requirement | Evidence | Outcome |
|---|---|---|
| Workspace root and cwd are resolved through the filesystem before spawn | Runner canonicalizes and validates existing directories; direct, missing, file, lexical escape, absolute escape, outside symlink, nested outside symlink, and in-root symlink tests | Pass |
| Cwd confinement policy and residual race are explicit | Resolved cwd is stored in each process spec; docs state in-root symlinks are allowed and the guarantee is pre-spawn under an operator-controlled workspace, with concurrent mutation remaining a TOCTOU limitation | Pass |
| Child environment is hermetic and explicit | Spawn clears ambient variables and injects only explicit resolved values; parent sentinel absence, explicit injection, ignored service config, and secret-redaction tests | Pass |
| Executable selection does not silently use ambient PATH | Absolute and cwd-relative explicit paths are resolved and checked before spawn; bare names, missing paths, and directories are rejected | Pass |
| Advertised platform support matches qualification | Linux and macOS report Supported; hosted Linux and macOS suites pass. Other Unix reports Unqualified. Windows reports Unsupported for managed execution and tests that capability behavior | Pass |
| Unix process dependency is not required by Windows builds | `nix` is target-scoped to Unix; `cargo tree --target x86_64-pc-windows-msvc` omits it; Windows check and Clippy pass | Pass |
| Existing lifecycle and corrected evidence contracts remain intact | Workspace all-features suite passes; lifecycle evidence records completed execution with no comparison verdict; Windows core fixtures verify after byte-preserving checkout handling | Pass |
| Cross-platform hosted qualification is recorded | GitHub Actions run `35782986030` on `99862de9cf76c0036b3ea1c6f6094f60196536f2`: Linux stable, Linux MSRV, macOS stable, and Windows stable all succeeded | Pass |
| Historical M001 SHA is corrected without rewriting closure history | `plans/closure/local-runner-lifecycle/001-errata.md` records the incorrect SHA, actual implementation SHA, documentation closure SHA, and unchanged substantive matrix | Pass |
| Runner behavior and platform contract are documented | `docs/local-runner-lifecycle.md` describes cwd, environment, executable, and platform semantics | Pass |

## Platform qualification

| Platform | Capability | Qualification evidence |
|---|---|---|
| Linux | Supported | Hosted stable full workspace CI; descendant-cleanup integration test |
| macOS | Supported | Hosted stable full workspace CI; descendant-cleanup and cwd symlink-confinement tests |
| Windows | Managed execution unsupported | Hosted workspace check, Clippy, core contract tests, and explicit unsupported-capability test |
| Other Unix | Unqualified | No hosted qualification claim |

## Verification

- `cargo fmt --all -- --check` — pass
- `cargo check --workspace --all-targets --locked` — pass
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — pass
- `cargo test --workspace --all-features --locked` — pass (55 tests across 7 suites)
- `cargo +1.89.0 check --workspace --all-targets --locked` — pass
- `cargo tree --locked` — pass; Windows target dependency tree excludes `nix`
- Windows-target Clippy — pass
- `git diff --check` — pass before closure commit
- Hosted GitHub Actions run `35782986030` — all four jobs pass

## Compatibility, security, and limitations

- Manifest-v1 evidence remains read-only legacy provenance; new lifecycle bundles use the separated execution-status/comparison-verdict API from Foundation C001.
- Child processes inherit no ambient environment by default. Future allowlists or ordinary non-secret environment configuration require a typed plan-schema change.
- Bare executable names are rejected. External driver work owns explicit binary discovery and version capture.
- Cwd confinement is canonicalized before spawn. A local operator can mutate the filesystem after preflight; race-free fd-relative spawn or sandboxing remains outside this corrective.
- Windows managed process-tree ownership remains unsupported until a qualified Job Object implementation exists.
- No unresolved correctness, security, platform, or plan blockers remain for this corrective.

## Dependency disposition

Local Runner M002 is dependency-ready for a fresh implementation plan against the corrected evidence and process boundary. The plan has not been written; M002 must remain unimplemented until that plan is reviewed. Local Runner M003 remains blocked on M002. External Oracles M001 is ready for planning because the driver boundary and qualified runner command substrate are now present. Measurement/Comparison M001 remains blocked on local trial evidence. Eggstack integration and Security Qualification remain blocked on their measurement/integration prerequisites. Distributed execution remains deferred.
