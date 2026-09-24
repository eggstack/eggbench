//! Run-preflight probing for external-process workload drivers.
//!
//! Binary resolution is synchronous and runs in the executor factory (so a
//! missing binary fails before managed startup); version probing spawns the
//! tool once with a bounded timeout and likewise runs before startup from
//! the CLI `run` preflight. Executors additionally self-probe on first
//! execution so direct `execute_run` consumers get the same version floor
//! without CLI preflight.

use super::error::{DriverError, ErrorCategory};
use super::version::ToolVersion;
use super::{H2LOAD_DRIVER_NAME, IPERF3_DRIVER_NAME, OHA_DRIVER_NAME};
use super::{H2loadWorkload, Iperf3Workload, OhaWorkload};
use eggbench_core::Name;
use tokio_util::sync::CancellationToken;

/// Whether a catalog driver name is an external-process workload.
#[must_use]
pub fn is_external_workload(name: &Name) -> bool {
    matches!(
        name.as_str(),
        OHA_DRIVER_NAME | H2LOAD_DRIVER_NAME | IPERF3_DRIVER_NAME
    )
}

/// Filesystem-only binary presence for `doctor` (no process is spawned).
///
/// Returns `None` for in-process drivers; `Some(present)` for external
/// workload drivers.
#[must_use]
pub fn external_binary_present(name: &Name) -> Option<bool> {
    match name.as_str() {
        OHA_DRIVER_NAME => Some(OhaWorkload::resolve().is_ok()),
        H2LOAD_DRIVER_NAME => Some(H2loadWorkload::resolve().is_ok()),
        IPERF3_DRIVER_NAME => Some(Iperf3Workload::resolve().is_ok()),
        _ => None,
    }
}

/// Canonical executable path for resolution pinning, when resolvable.
///
/// Callers (CLI resolution options) insert this as the driver's
/// `executable_path` so plan resolution pins the exact binary before any
/// managed startup. `None` means resolution proceeds without a path and
/// fails with `MissingExecutablePath` — the explicit missing-binary signal.
#[must_use]
pub fn executable_path_for(name: &Name) -> Option<String> {
    let resolved = match name.as_str() {
        OHA_DRIVER_NAME => OhaWorkload::resolve().ok(),
        H2LOAD_DRIVER_NAME => H2loadWorkload::resolve().ok(),
        IPERF3_DRIVER_NAME => Iperf3Workload::resolve().ok(),
        _ => None,
    }?;
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
        _ => Err(DriverError::resolution(
            ErrorCategory::BinaryNotFound,
            format!("no external workload driver named {}", name.as_str()),
        )),
    }
}
