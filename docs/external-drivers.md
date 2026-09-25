# External drivers

External Oracles M001 establishes the shared `eggbench-drivers` crate and a
secure, bounded, testable command-adapter substrate. Oracles M002 adds the
first tool adapters on that substrate — oha/h2load/iperf3 mapping, with
tc/netem still future and no installer, shell execution, or remote
execution. See [external oracles](external-oracles.md).

## Catalog ownership

Production driver inventory lives in `eggbench-drivers::DriverCatalog`. The
catalog always registers the external-process drivers (`oha`, `h2load`,
`iperf3`, `eggreplay-semantic`); native drivers join per feature. `doctor`
truthfully reports the inventory (including per-driver `binary_present`
without spawning tools) and production `run` fails before startup with
`missing_driver`/`unsupported_workload`/`ambiguous_selection`/`missing_executable_path`
when no driver resolves. The CLI no longer owns the
authoritative registry. The qualification fake stays test-only.

Feature policy: `default = []`, `external-command = [...]` (substrate only).
Default builds contain no protocol client/server dependency.

## Resolution policy

- Explicit absolute paths only; relative explicit paths are rejected unless
  the caller resolved them against a trusted root first.
- `PATH` search enumerates components manually: empty and relative
  components are skipped, there is no implicit current-directory lookup, and
  the first match in trusted absolute order wins.
- Unix: candidate must be a regular file with executable mode bits.
- Windows: only `.exe`/`.com` direct targets; `.bat`/`.cmd` wrappers are
  rejected (they need shell semantics). Current directory is never searched
  implicitly.
- Symlink input is accepted but identity/hash cover the canonical target;
  both selected and canonical paths are recorded.
- Executable identity is SHA-256 of canonical bytes plus file size and
  platform classification. The digest is diagnostic provenance, not a trust
  signature.
- argv metacharacters stay literal; environment is `env_clear` plus explicit
  driver variables only (deterministic `LC_ALL=C`/`LANG=C` for probes).
  Secrets never appear in `Debug`/error output.

## Execution

- argv[0] is the resolved path; no shell, no glob, no inherited cwd.
- stdout/stderr are drained concurrently with independent caps; draining
  continues after the cap so children cannot block on a full pipe.
  Truncation is explicit (`truncated`, retained/dropped/total counters).
- Cancellation token plus explicit driver timeout are always observed. Unix
  uses a dedicated process group (TERM then KILL, reusing runner semantics);
  Windows cleans up the direct child and reports `direct_child_only`.
- Raw output is retained (`stdout.raw`, `stderr.raw`,
  `command-metadata.json`) before any semantic normalization. Parsers are
  versioned, fixture-testable, and independent of spawning; a parser failure
  never mutates the invocation into a different load/protocol semantic.

## Tests

Deterministic fixture executable (`eggbench_fixture` binary, test-only)
covers version text/bytes/exit/sleep/malformed/child-spawn behaviors so
Windows qualifies the same argv-only contract without shell scripts.
