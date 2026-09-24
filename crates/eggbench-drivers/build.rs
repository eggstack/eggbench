//! Build-time Eggstack dependency provenance.
//!
//! Reads the workspace `Cargo.lock` and emits the exact resolved versions of
//! the Eggstack sibling crates as compile-time environment values. Runtime
//! evidence and driver descriptors must report these values instead of a
//! hardcoded patch version. When the lockfile or an entry is unavailable the
//! value falls back to `unknown` rather than failing the build.

use std::env;
use std::fs;
use std::path::PathBuf;

const TARGETS: &[(&str, &str)] = &[
    ("eggfetch-core", "EGGBENCH_EGGFETCH_CORE_VERSION"),
    ("eggserve-server", "EGGBENCH_EGGSERVE_SERVER_VERSION"),
    (
        "eggserve-primitives",
        "EGGBENCH_EGGSERVE_PRIMITIVES_VERSION",
    ),
];

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let lockfile = manifest
        .ancestors()
        .map(|dir| dir.join("Cargo.lock"))
        .find(|path| path.is_file());

    let mut versions: Vec<(&str, String)> = TARGETS
        .iter()
        .map(|(name, _)| (*name, "unknown".to_owned()))
        .collect();

    if let Some(path) = lockfile {
        println!("cargo:rerun-if-changed={}", path.display());
        if let Ok(text) = fs::read_to_string(&path) {
            for (name, version) in &mut versions {
                if let Some(found) = lock_version(&text, name) {
                    *version = found;
                }
            }
        }
    }

    for ((_, var), (_, version)) in TARGETS.iter().zip(versions.iter()) {
        println!("cargo:rustc-env={var}={version}");
    }
}

/// Extract the version of one `[[package]]` entry from lockfile text.
///
/// Expected stanza shape:
///
/// ```toml
/// [[package]]
/// name = "<name>"
/// version = "<version>"
/// ```
fn lock_version(lock: &str, package: &str) -> Option<String> {
    let needle = format!("name = \"{package}\"");
    let mut current: Option<bool> = None;
    for line in lock.lines() {
        let trimmed = line.trim();
        if trimmed == "[[package]]" {
            current = None;
            continue;
        }
        if current.is_none() {
            if trimmed == needle {
                current = Some(true);
            }
            continue;
        }
        if let Some(version) = trimmed.strip_prefix("version = \"") {
            return version.strip_suffix('"').map(str::to_owned);
        }
    }
    None
}
