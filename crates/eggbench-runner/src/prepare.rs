//! Pre-start bundle preparation helper.
//!
//! Stages the source plan, resolved plan, environment fingerprint, and
//! subject snapshot artifacts into one still-unpublished [`BundleWriter`]
//! so M002 [`execute_run`] can hand a complete writer into orchestration.
//! All four primary artifacts are validated before the writer is returned.

use crate::{LocalEnvironmentCollector, SubjectSnapshot};
use eggbench_core::{
    ArtifactBounds, ArtifactPath, ArtifactRole, BundleError, BundleWriter, EnvironmentFingerprint,
    ResolvedPlan, RunId, Sensitivity,
};
use std::path::Path;

/// Inputs required to stage the four primary evidence artifacts.
pub struct BundlePreparation<'a> {
    /// Destination `.eggb` bundle path. The caller must not have created it.
    pub destination: &'a Path,
    /// Stable run identity used to seed the writer.
    pub run_id: RunId,
    /// Source plan bytes to retain as-is.
    pub source_plan_bytes: &'a [u8],
    /// Media type for the source plan artifact (`application/toml`, `application/json`, ...).
    pub source_plan_media_type: &'a str,
    /// Resolved plan to serialize canonically.
    pub resolved_plan: &'a ResolvedPlan,
    /// Environment fingerprint produced before startup.
    pub environment: &'a EnvironmentFingerprint,
    /// Subject snapshot evidence.
    pub subject_snapshot: &'a SubjectSnapshot,
    /// Artifact bounds for the new bundle.
    pub bounds: ArtifactBounds,
}

/// Serialize the resolved plan to canonical JSON for artifact staging.
fn resolved_plan_bytes(resolved: &ResolvedPlan) -> Result<Vec<u8>, BundleError> {
    serde_json::to_vec_pretty(resolved)
        .map_err(|error| BundleError::ManifestParse(error.to_string()))
}

fn environment_bytes(environment: &EnvironmentFingerprint) -> Result<Vec<u8>, BundleError> {
    serde_json::to_vec_pretty(environment)
        .map_err(|error| BundleError::ManifestParse(error.to_string()))
}

/// Stage source plan, resolved plan, environment, and subject snapshot evidence
/// before managed startup.
///
/// # Errors
/// Returns [`BundleError`] for writer creation, source plan decoding,
/// validation, or staging failures.
pub fn prepare_bundle(preparation: &BundlePreparation<'_>) -> Result<BundleWriter, BundleError> {
    preparation.environment.validate()?;
    preparation.subject_snapshot.validate()?;

    let mut writer = BundleWriter::create(
        preparation.destination,
        preparation.run_id,
        preparation.bounds,
    )?;

    let plan_path = ArtifactPath::new("plan.json").map_err(|error| {
        BundleError::ManifestParse(format!("source plan path rejected: {error}"))
    })?;
    writer.add_artifact(
        plan_path,
        ArtifactRole::ExperimentPlan,
        preparation.source_plan_media_type.to_owned(),
        Sensitivity::Redacted,
        preparation.source_plan_bytes,
    )?;

    let resolved_path = ArtifactPath::new("resolved-plan.json").map_err(|error| {
        BundleError::ManifestParse(format!("resolved plan path rejected: {error}"))
    })?;
    let resolved_bytes = resolved_plan_bytes(preparation.resolved_plan)?;
    writer.add_artifact(
        resolved_path,
        ArtifactRole::ResolvedPlan,
        "application/json".to_owned(),
        Sensitivity::Redacted,
        resolved_bytes.as_slice(),
    )?;

    let environment_path = ArtifactPath::new("environment.json").map_err(|error| {
        BundleError::ManifestParse(format!("environment path rejected: {error}"))
    })?;
    let environment_bytes = environment_bytes(preparation.environment)?;
    writer.add_artifact(
        environment_path,
        ArtifactRole::EnvironmentFingerprint,
        "application/json".to_owned(),
        Sensitivity::Redacted,
        environment_bytes.as_slice(),
    )?;

    let subject_path = ArtifactPath::new("subject.json")
        .map_err(|error| BundleError::ManifestParse(format!("subject path rejected: {error}")))?;
    let subject_bytes = preparation.subject_snapshot.to_json_bytes()?;
    writer.add_artifact(
        subject_path,
        ArtifactRole::Subject,
        "application/json".to_owned(),
        Sensitivity::Public,
        subject_bytes.as_slice(),
    )?;

    Ok(writer)
}

/// Stable subject snapshot error category for CLI presentation.
#[derive(Debug, thiserror::Error, Clone)]
pub enum SubjectSnapshotError {
    /// Subject digest did not match the observed executable digest.
    #[error("subject declared digest does not match observed executable digest")]
    DeclaredDigestMismatch,
    /// Subject snapshot could not be built.
    #[error("subject snapshot build failed: {0}")]
    Build(String),
}

/// Build a fresh subject snapshot for the resolved plan.
///
/// For managed commands, the resolved executable path is resolved through the
/// workspace root by the runner's `prepare` step so the caller passes the
/// filesystem-resolved path. External and label subjects never fabricate
/// executable digests.
///
/// # Errors
/// Returns [`SubjectSnapshotError`] when the snapshot cannot be built for
/// the supplied subject.
pub fn build_subject_snapshot(
    resolved: &ResolvedPlan,
    managed_executable: Option<&Path>,
) -> Result<SubjectSnapshot, SubjectSnapshotError> {
    SubjectSnapshot::build(&resolved.subject, managed_executable)
        .map_err(|error| SubjectSnapshotError::Build(error.to_string()))
}

/// Convenience: collect the environment fingerprint and the subject snapshot.
///
/// # Errors
/// Returns the first failure observed when collecting environment or building
/// the subject snapshot.
pub fn collect_local_environment(
    resolved: &ResolvedPlan,
    managed_executable: Option<&Path>,
) -> Result<(EnvironmentFingerprint, SubjectSnapshot), PrepareError> {
    let environment = LocalEnvironmentCollector.collect()?;
    let snapshot = build_subject_snapshot(resolved, managed_executable)?;
    Ok((environment, snapshot))
}

/// Aggregated pre-start preparation error.
#[derive(Debug, thiserror::Error)]
pub enum PrepareError {
    /// Environment collection failed.
    #[error("environment collection failed: {0}")]
    Environment(crate::EnvironmentError),
    /// Subject snapshot failed.
    #[error("subject snapshot failed: {0}")]
    Snapshot(SubjectSnapshotError),
    /// Bundle staging failed before startup.
    #[error("bundle preparation failed: {0}")]
    Bundle(BundleError),
}

impl From<crate::EnvironmentError> for PrepareError {
    fn from(value: crate::EnvironmentError) -> Self {
        Self::Environment(value)
    }
}

impl From<SubjectSnapshotError> for PrepareError {
    fn from(value: SubjectSnapshotError) -> Self {
        Self::Snapshot(value)
    }
}

impl From<BundleError> for PrepareError {
    fn from(value: BundleError) -> Self {
        Self::Bundle(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eggbench_core::{
        ArtifactBounds, EnvironmentField, EnvironmentFieldClass, EnvironmentFingerprint, Name,
        PositiveCount, ResolvedDefaults, ResolvedPlan, SchemaVersion, Subject, TrialPolicy,
    };
    use std::collections::BTreeMap;

    fn bounds() -> ArtifactBounds {
        ArtifactBounds {
            artifact_count: PositiveCount::new(100).unwrap(),
            artifact_bytes: 16 * 1024 * 1024,
            total_bytes: 64 * 1024 * 1024,
        }
    }

    fn environment() -> EnvironmentFingerprint {
        let mut fields = BTreeMap::new();
        fields.insert(
            Name::new("os_family").unwrap(),
            EnvironmentField {
                value: "linux".into(),
                class: EnvironmentFieldClass::ComparisonCritical,
            },
        );
        EnvironmentFingerprint::new(fields)
    }

    fn resolved_plan_for_external_subject() -> ResolvedPlan {
        ResolvedPlan {
            schema_version: SchemaVersion(1),
            source_plan_schema_version: SchemaVersion(1),
            experiment: Name::new("smoke").unwrap(),
            drivers: BTreeMap::new(),
            subject: Subject::External {
                target: Name::new("api").unwrap(),
                revision: None,
                digest: None,
            },
            topology: Vec::new(),
            workload: eggbench_core::Workload::FiniteCount {
                target: Name::new("api").unwrap(),
                requests: PositiveCount::new(1).unwrap(),
                concurrency: PositiveCount::new(1).unwrap(),
            },
            trials: TrialPolicy {
                measured: PositiveCount::new(1).unwrap(),
                warmup: 0,
                cooldown_ms: None,
                reset: eggbench_core::ResetPolicy::None,
                timeouts: BTreeMap::new(),
            },
            telemetry: Vec::new(),
            defaults: ResolvedDefaults {
                platform: Name::new("linux-x86_64").unwrap(),
                warmup_trials: 0,
                measured_trials: 1,
            },
            environment_policy: eggbench_core::EnvironmentPolicy::StrictSameTestbed,
            metrics: Vec::new(),
            artifact_bounds: bounds(),
            seed: None,
            warnings: Vec::new(),
        }
    }

    fn subject_snapshot_for(resolved: &ResolvedPlan) -> SubjectSnapshot {
        SubjectSnapshot::build(&resolved.subject, None).unwrap()
    }

    #[test]
    fn prepare_bundle_stages_all_four_primary_artifacts() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("run.eggb");
        let resolved = resolved_plan_for_external_subject();
        let snapshot = subject_snapshot_for(&resolved);
        let environment = environment();
        let plan_bytes = br#"{"experiment":"smoke"}"#;
        let preparation = BundlePreparation {
            destination: &destination,
            run_id: RunId::new(),
            source_plan_bytes: plan_bytes,
            source_plan_media_type: "application/json",
            resolved_plan: &resolved,
            environment: &environment,
            subject_snapshot: &snapshot,
            bounds: bounds(),
        };
        let writer = prepare_bundle(&preparation).expect("prepare");
        assert!(writer.staging_path().join("plan.json").is_file());
        assert!(writer.staging_path().join("resolved-plan.json").is_file());
        assert!(writer.staging_path().join("environment.json").is_file());
        assert!(writer.staging_path().join("subject.json").is_file());
    }

    #[test]
    fn prepare_bundle_rejects_invalid_environment() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("invalid-env.eggb");
        let resolved = resolved_plan_for_external_subject();
        let snapshot = subject_snapshot_for(&resolved);
        let mut fields = BTreeMap::new();
        fields.insert(
            Name::new("toolong").unwrap(),
            EnvironmentField {
                value: "x".repeat(5_000),
                class: EnvironmentFieldClass::ComparisonCritical,
            },
        );
        let environment = EnvironmentFingerprint::new(fields);
        let preparation = BundlePreparation {
            destination: &destination,
            run_id: RunId::new(),
            source_plan_bytes: br#"{"experiment":"smoke"}"#,
            source_plan_media_type: "application/json",
            resolved_plan: &resolved,
            environment: &environment,
            subject_snapshot: &snapshot,
            bounds: bounds(),
        };
        match prepare_bundle(&preparation) {
            Err(BundleError::InvalidManifest(_)) => {}
            Err(error) => panic!("expected invalid manifest error, got {error}"),
            Ok(_) => panic!("expected invalid manifest error, got success"),
        }
        assert!(!destination.exists());
    }

    #[test]
    fn prepare_bundle_rejects_unsafe_subject() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("unsafe.eggb");
        let mut resolved = resolved_plan_for_external_subject();
        resolved.subject = Subject::External {
            target: Name::new("api").unwrap(),
            revision: None,
            digest: None,
        };
        let snapshot = subject_snapshot_for(&resolved);
        let environment = environment();
        let mut written = snapshot.clone();
        written.executable_sha256 = Some("deadbeef".repeat(8));
        let preparation = BundlePreparation {
            destination: &destination,
            run_id: RunId::new(),
            source_plan_bytes: br#"{"experiment":"smoke"}"#,
            source_plan_media_type: "application/json",
            resolved_plan: &resolved,
            environment: &environment,
            subject_snapshot: &written,
            bounds: bounds(),
        };
        match prepare_bundle(&preparation) {
            Err(BundleError::InvalidManifest(_)) => {}
            Err(error) => panic!("expected invalid manifest error, got {error}"),
            Ok(_) => panic!("expected invalid manifest error, got success"),
        }
    }

    #[test]
    fn collect_local_environment_produces_snapshot_and_environment() {
        let resolved = resolved_plan_for_external_subject();
        let (environment, snapshot) = collect_local_environment(&resolved, None).expect("collect");
        environment.validate().unwrap();
        snapshot.validate().unwrap();
    }

    #[test]
    fn resolved_plan_artifacts_can_be_serialized_and_loaded() {
        let resolved = resolved_plan_for_external_subject();
        let bytes = resolved_plan_bytes(&resolved).expect("serialize");
        let decoded: ResolvedPlan = serde_json::from_slice(&bytes).expect("decode");
        assert_eq!(decoded.experiment.as_str(), "smoke");
    }
}
