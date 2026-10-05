//! Run-preflight probing for external-process workload drivers.
//!
//! Binary resolution is synchronous and runs in the executor factory (so a
//! missing binary fails before managed startup); version probing spawns the
//! tool once with a bounded timeout and likewise runs before startup from
//! the CLI `run` preflight. Executors additionally self-probe on first
//! execution so direct `execute_run` consumers get the same version floor
//! without CLI preflight.

use super::error::{DriverError, ErrorCategory};
use super::resolver::ResolvedExecutable;
use super::version::ToolVersion;
use super::{
    EGGPROBE_DRIVER_NAME, EGGREPLAY_DRIVER_NAME, EGGSEC_DRIVER_NAME, EGGSEC_LOAD_DRIVER_NAME,
    H2LOAD_DRIVER_NAME, IPERF3_DRIVER_NAME, OHA_DRIVER_NAME,
};
use super::{
    EggProbeExecutor, EggReplayWorkload, EggsecLoadWorkload, H2loadWorkload, Iperf3Workload,
    OhaWorkload,
};
use eggbench_core::Name;
use tokio_util::sync::CancellationToken;

/// Every registered driver that runs as an external process.
///
/// This is the single source for the external-driver gate: the load oracles
/// (`oha`, `h2load`, `iperf3`), the semantic-replay workload, the load
/// workload, the `eggprobe` diagnostic driver, and the `eggsec-waf`
/// correctness driver all shell out to a binary. All three predicates below
/// consult this list, so the gate cannot silently under-cover a registered
/// driver again.
const EXTERNAL_DRIVER_NAMES: [&str; 7] = [
    OHA_DRIVER_NAME,
    H2LOAD_DRIVER_NAME,
    IPERF3_DRIVER_NAME,
    EGGREPLAY_DRIVER_NAME,
    EGGSEC_LOAD_DRIVER_NAME,
    EGGPROBE_DRIVER_NAME,
    EGGSEC_DRIVER_NAME,
];

/// Resolve an external driver by catalog name.
///
/// `None` for an in-process driver (nothing to resolve); `Some(result)`
/// keeps the per-driver resolution error instead of collapsing it, because a
/// registered external driver whose binary is absent still needs an answer.
fn resolve_external_driver(name: &str) -> Option<Result<ResolvedExecutable, DriverError>> {
    Some(match name {
        OHA_DRIVER_NAME => OhaWorkload::resolve(),
        H2LOAD_DRIVER_NAME => H2loadWorkload::resolve(),
        IPERF3_DRIVER_NAME => Iperf3Workload::resolve(),
        EGGREPLAY_DRIVER_NAME => EggReplayWorkload::resolve(),
        EGGPROBE_DRIVER_NAME => EggProbeExecutor::resolve(),
        EGGSEC_DRIVER_NAME => super::EggsecWafExecutor::resolve(),
        EGGSEC_LOAD_DRIVER_NAME => EggsecLoadWorkload::resolve(),
        _ => return None,
    })
}

/// Whether a catalog driver name is an external-process driver.
///
/// True for every name in [`EXTERNAL_DRIVER_NAMES`]: the three load oracles,
/// the semantic-replay workload, the load workload, the `eggprobe`
/// diagnostic driver, and the `eggsec-waf` correctness driver.
#[must_use]
pub fn is_external_workload(name: &Name) -> bool {
    EXTERNAL_DRIVER_NAMES.contains(&name.as_str())
}

/// Filesystem-only binary presence for `doctor` (no process is spawned).
///
/// Returns `None` for in-process drivers; `Some(present)` for every external
/// workload, diagnostic, and correctness driver.
#[must_use]
pub fn external_binary_present(name: &Name) -> Option<bool> {
    resolve_external_driver(name.as_str()).map(|resolved| resolved.is_ok())
}

/// Canonical executable path for resolution pinning, when resolvable.
///
/// Callers (CLI resolution options) insert this as the driver's
/// `executable_path` so plan resolution pins the exact binary before any
/// managed startup. `None` means resolution proceeds without a path and
/// fails with `MissingExecutablePath` — the explicit missing-binary signal.
#[must_use]
pub fn executable_path_for(name: &Name) -> Option<String> {
    let resolved = resolve_external_driver(name.as_str())?.ok()?;
    Some(resolved.canonical_path.to_string_lossy().into_owned())
}

/// Resolve, probe, and version-check an external workload driver.
///
/// # Errors
/// Returns resolution, probe, or `unsupported_version` failures. Callers
/// surface these as capability-preflight failures before managed startup.
pub async fn probe_external_workload(
    name: &Name,
    cancel: &CancellationToken,
) -> Result<ToolVersion, DriverError> {
    match name.as_str() {
        EGGSEC_LOAD_DRIVER_NAME => {
            let executable = EggsecLoadWorkload::resolve()?;
            EggsecLoadWorkload::probe(&executable, cancel).await
        }
        OHA_DRIVER_NAME => {
            let executable = OhaWorkload::resolve()?;
            let probed = OhaWorkload::probe(&executable, cancel).await?;
            OhaWorkload::new(executable, probed.version.clone())?;
            Ok(probed)
        }
        H2LOAD_DRIVER_NAME => {
            let executable = H2loadWorkload::resolve()?;
            let probed = H2loadWorkload::probe(&executable, cancel).await?;
            H2loadWorkload::new(executable, probed.version.clone())?;
            Ok(probed)
        }
        IPERF3_DRIVER_NAME => {
            let executable = Iperf3Workload::resolve()?;
            let probed = Iperf3Workload::probe(&executable, cancel).await?;
            Iperf3Workload::new(executable, probed.version.clone())?;
            Ok(probed)
        }
        EGGREPLAY_DRIVER_NAME => {
            let executable = EggReplayWorkload::resolve()?;
            let probed = EggReplayWorkload::probe(&executable, cancel).await?;
            EggReplayWorkload::new(
                executable,
                probed.version.clone(),
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
                "fixture".to_owned(),
            )?;
            Ok(probed)
        }
        _ => Err(DriverError::resolution(
            ErrorCategory::BinaryNotFound,
            format!("no external workload driver named {}", name.as_str()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three predicates must agree for every external driver: a name in
    /// the shared list is gated as external, is answerable by the
    /// filesystem-only presence check, and yields a pinned path exactly when
    /// that check says the binary is present.
    #[test]
    fn external_predicates_agree_over_the_registered_set() {
        for tool in EXTERNAL_DRIVER_NAMES {
            let name = Name::new(tool).expect("static driver name");
            assert!(is_external_workload(&name), "{tool} is not gated");
            let present = external_binary_present(&name)
                .unwrap_or_else(|| panic!("{tool} has no binary-presence answer"));
            assert_eq!(
                executable_path_for(&name).is_some(),
                present,
                "{tool} path/presence disagree",
            );
            assert!(
                resolve_external_driver(tool).is_some(),
                "{tool} is not resolvable",
            );
        }
    }

    /// The list must name every registered external-process driver, and only
    /// those: it is the gate for "does this driver need a binary".
    #[test]
    fn shared_list_matches_the_registered_external_drivers() {
        let catalog = crate::production_catalog();
        let registered: Vec<String> = catalog
            .descriptors()
            .iter()
            .filter(|descriptor| descriptor.external_process)
            .map(|descriptor| descriptor.name.as_str().to_owned())
            .collect();
        assert!(!registered.is_empty(), "no external drivers registered");

        for name in &registered {
            assert!(
                EXTERNAL_DRIVER_NAMES.contains(&name.as_str()),
                "registered external driver {name} is missing from the shared list",
            );
            let parsed = Name::new(name).expect("static driver name");
            assert!(is_external_workload(&parsed));
            assert!(external_binary_present(&parsed).is_some());
        }

        for tool in EXTERNAL_DRIVER_NAMES {
            assert!(
                registered.iter().any(|name| name == tool),
                "{tool} is gated as external but is not a registered external driver",
            );
        }

        // An in-process driver is outside the list and answers `None`.
        let in_process = Name::new("eggfetch-http").expect("static driver name");
        assert!(!is_external_workload(&in_process));
        assert!(external_binary_present(&in_process).is_none());
        assert!(executable_path_for(&in_process).is_none());
    }
}
