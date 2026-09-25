//! Lifecycle evidence staged through the immutable bundle writer.
//!
//! The runner adds no second evidence format. Diagnostic logs and lifecycle
//! metadata are staged as ordinary artifacts with the M003 [`BundleWriter`];
//! finalization still requires the plan, resolved-plan, and environment
//! primary roles. A lifecycle-only run records no trials and finalizes with
//! execution completed and no comparison verdict.

use crate::RunEvidenceArtifact;
use crate::session::{LifecycleOutcome, LocalSession};
use eggbench_core::{ArtifactPath, ArtifactRole, BundleError, BundleWriter, Name, Sensitivity};
use serde::Serialize;

/// Lifecycle metadata staged alongside bounded logs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LifecycleMetadata {
    started: Vec<String>,
    stopped_order: Vec<String>,
    cleanup: Vec<CleanupSummary>,
    events: Vec<EventSummary>,
    external_observed: Vec<String>,
}

/// Redaction-safe cleanup summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct CleanupSummary {
    service: String,
    reason: String,
}

/// Redaction-safe event summary with diagnostic process identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct EventSummary {
    seq: u64,
    identity: String,
    kind: String,
    pid: Option<u32>,
    elapsed_ms: u64,
}

/// Stage versioned runtime-topology evidence as a JSON artifact.
///
/// The artifact lands at `lifecycle/runtime-topology.json` with role
/// [`ArtifactRole::Other`] labeled `runtime-topology` and
/// [`Sensitivity::Redacted`]. It records schema version, service identities,
/// adapter/process ownership kind, adapter provenance, and the non-secret
/// startup-established runtime bindings. Staging reads retained session
/// state, so it remains available through teardown for final evidence.
///
/// # Errors
/// Returns [`BundleError`] when artifact registration violates bundle bounds.
pub fn stage_runtime_topology(
    session: &LocalSession,
    writer: &mut BundleWriter,
) -> Result<ArtifactPath, BundleError> {
    let topology = session.runtime_topology();
    let bytes = serde_json::to_vec_pretty(&topology)
        .map_err(|error| BundleError::ManifestParse(error.to_string()))?;
    let path = ArtifactPath::new("lifecycle/runtime-topology.json")?;
    writer.add_artifact(
        path.clone(),
        ArtifactRole::Other {
            label: Name::new("runtime-topology")
                .map_err(|_| BundleError::InvalidManifest("bad label"))?,
        },
        "application/json",
        Sensitivity::Redacted,
        bytes.as_slice(),
    )?;
    Ok(path)
}

/// Stage one protocol-neutral run evidence artifact.
///
/// # Errors
/// Returns [`BundleError`] when the path or artifact violates bundle bounds.
pub fn stage_run_evidence(
    writer: &mut BundleWriter,
    evidence: &RunEvidenceArtifact,
) -> Result<ArtifactPath, BundleError> {
    let path = ArtifactPath::new(evidence.name().to_owned())?;
    writer.add_artifact(
        path.clone(),
        ArtifactRole::Other {
            label: evidence.role_label().clone(),
        },
        evidence.media_type(),
        evidence.sensitivity(),
        evidence.bytes(),
    )?;
    Ok(path)
}

/// Stage bounded stdout/stderr logs for every identity that ever started.
///
/// Artifacts land under `lifecycle/logs/<identity>.stdout` and `.stderr`
/// with [`ArtifactRole::Stdout`]/[`ArtifactRole::Stderr`] and
/// [`Sensitivity::Redacted`]. Truncation is preserved byte-for-byte from the
/// session spools.
///
/// # Errors
/// Returns [`BundleError`] when artifact registration violates bundle bounds.
pub async fn stage_lifecycle_logs(
    session: &LocalSession,
    writer: &mut BundleWriter,
) -> Result<Vec<ArtifactPath>, BundleError> {
    let mut staged = Vec::new();
    for identity in session.spawn_order() {
        let Some(logs) = session.logs(&identity).await else {
            continue;
        };
        let safe = sanitize_identity(&identity);
        let stdout_path = ArtifactPath::new(format!("lifecycle/logs/{safe}.stdout"))?;
        writer.add_artifact(
            stdout_path.clone(),
            ArtifactRole::Stdout,
            "application/octet-stream",
            Sensitivity::Redacted,
            logs.stdout.data.as_slice(),
        )?;
        staged.push(stdout_path);
        let stderr_path = ArtifactPath::new(format!("lifecycle/logs/{safe}.stderr"))?;
        writer.add_artifact(
            stderr_path.clone(),
            ArtifactRole::Stderr,
            "application/octet-stream",
            Sensitivity::Redacted,
            logs.stderr.data.as_slice(),
        )?;
        staged.push(stderr_path);
    }
    Ok(staged)
}

/// Stage lifecycle metadata (order, events, cleanup) as a JSON artifact.
///
/// The artifact lands at `lifecycle/lifecycle.json` with role
/// [`ArtifactRole::Other`] labeled `lifecycle` and
/// [`Sensitivity::Redacted`]. It carries identities, diagnostic process
/// identifiers, and cleanup reasons only; secret values never enter it.
///
/// # Errors
/// Returns [`BundleError`] when artifact registration violates bundle bounds.
pub fn stage_lifecycle_metadata(
    session: &LocalSession,
    outcome: &LifecycleOutcome,
    writer: &mut BundleWriter,
) -> Result<ArtifactPath, BundleError> {
    let metadata = LifecycleMetadata {
        started: outcome.started.clone(),
        stopped_order: outcome.stopped_order.clone(),
        cleanup: outcome
            .cleanup
            .iter()
            .map(|failure| CleanupSummary {
                service: failure.service.clone(),
                reason: failure.reason.clone(),
            })
            .collect(),
        events: session
            .events()
            .iter()
            .map(|event| EventSummary {
                seq: event.seq,
                identity: event.identity.clone(),
                kind: format!("{:?}", event.kind),
                pid: event.pid,
                elapsed_ms: event.elapsed_ms,
            })
            .collect(),
        external_observed: session.external_services().to_vec(),
    };
    let bytes = serde_json::to_vec_pretty(&metadata)
        .map_err(|error| BundleError::ManifestParse(error.to_string()))?;
    let path = ArtifactPath::new("lifecycle/lifecycle.json")?;
    writer.add_artifact(
        path.clone(),
        ArtifactRole::Other {
            label: Name::new("lifecycle").map_err(|_| BundleError::InvalidManifest("bad label"))?,
        },
        "application/json",
        Sensitivity::Redacted,
        bytes.as_slice(),
    )?;
    Ok(path)
}

fn sanitize_identity(identity: &str) -> String {
    let cleaned: String = identity
        .chars()
        .map(|char| {
            if char.is_ascii_alphanumeric() || char == '-' || char == '_' {
                char
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "unnamed".to_owned()
    } else {
        cleaned
    }
}
