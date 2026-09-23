# Environment fingerprint

The local environment fingerprint is a versioned, bounded collection of
non-secret host facts. The fingerprint is collected before managed startup
so a failed collection cannot leave descendant processes running. Every
field carries an explicit comparability class. Missing optional fields stay
absent; the collector never fabricates placeholders such as `unknown`.

Schema version 1 is used for both the typed core DTO and the on-disk
artifact. The schema does not change merely because new producers exist;
new fields are additive. Subject revision/digest intentionally never
appear as comparison-critical environment fields: candidate and baseline
binaries are expected to differ.

## Field classes

| Class | Meaning |
|-------|---------|
| `comparison_critical` | A mismatch prevents strict same-testbed comparison. |
| `warning_only` | A mismatch should be surfaced as a warning. |
| `informational` | Recorded for context without affecting comparability. |

## Field table

| Field | Class | Notes |
|-------|-------|-------|
| `os_family` | comparison_critical | `linux`, `macos`, `windows`, `unix-other`. |
| `architecture` | comparison_critical | Normalized label such as `x86_64` or `aarch64`. |
| `target_family` | comparison_critical | `unix` or `windows`. |
| `cpu_model` | comparison_critical | Best-effort CPU model label; absent on bare-metal Linux without `/proc/cpuinfo` or on macOS without `sysctl`. |
| `logical_cpu_count` | comparison_critical | Total logical CPUs. |
| `physical_cpu_count` | comparison_critical | Total physical cores when reliably discoverable. |
| `total_memory_bytes` | comparison_critical | Bytes of installed memory. |
| `kernel_release` | comparison_critical | `uname -r` on Unix. |
| `os_version` | warning_only | Pretty OS version (for example `/etc/os-release PRETTY_NAME`). |
| `current_cpu_frequency_mhz` | warning_only | Maximum current frequency in MHz. Transient — never comparison-critical. |
| `eggbench_collector_version` | informational | `eggbench` package version. |
| `rust_target` | informational | Rust `cfg!(target_arch)` value. |
| `build_profile` | informational | `debug` or `release` based on `cfg!(debug_assertions)`. |

Fields are omitted when their underlying data source is unavailable. ARM/SBC
hosts that do not expose an x86-style model string simply omit `cpu_model`
without a placeholder.

## Security and privacy

The collector never reads the process environment, never inspects
`PATH` or similar search paths, and never queries network interfaces. It
uses only filesystem, `sysctl` on macOS, and a small number of read-only
system commands that take no arguments. Shell strings are never executed.

Subject secret references remain references in subject snapshots and never
flow into the environment artifact. Hostnames, MAC/IP inventories, and
secret-bearing environment variables are not part of v1.

## Subject fingerprint

The companion subject fingerprint lives under `subject.json` with the
`ArtifactRole::Subject` role. For managed subjects the snapshot records the
resolved executable path, its observed SHA-256, the declared revision/digest
hints, and whether the declared digest matched the observed digest. External
and label subjects keep declared identity only and never fabricate a
binary digest.

A `declared_digest_matches == false` is reported by `eggbench doctor` and
fails `eggbench run` before managed startup with a stable
`subject_digest_mismatch` category.
