//! `eggbench inspect <bundle>` command.

use crate::envelope::{
    CliEnvelope, CliOutput, DriverSummary, EnvironmentSummary, ExitCode, SubjectSummary,
    TrialSummary,
};
use crate::error::CliError;
use eggbench_core::BundleReader;
use std::io::Read;
use std::path::Path;

/// Open, verify, and summarize a finalized bundle.
///
/// # Errors
/// Returns [`CliError`] when the bundle cannot be opened or verified.
pub fn run(bundle: &Path, emit_manifest_json: bool) -> Result<CliEnvelope, CliError> {
    let reader = BundleReader::open(bundle).map_err(CliError::Bundle)?;
    reader.verify().map_err(CliError::Bundle)?;

    let manifest = reader.manifest();
    let mut drivers: Vec<DriverSummary> = manifest
        .drivers
        .iter()
        .map(|driver| DriverSummary {
            name: driver.name.as_str().to_owned(),
            category: format!("{:?}", driver.category),
            default: driver.default,
            external_process: driver.external_process,
        })
        .collect();
    drivers.sort_by(|left, right| left.name.cmp(&right.name));

    let env_path = manifest
        .artifacts
        .iter()
        .find(|artifact| {
            matches!(
                artifact.role,
                eggbench_core::ArtifactRole::EnvironmentFingerprint
            )
        })
        .map(|artifact| artifact.path.clone());

    let env_fields =
        env_path
            .as_ref()
            .map_or(Vec::new(), |path| match reader.open_artifact(path) {
                Ok(mut file) => {
                    let mut bytes = Vec::new();
                    if file.read_to_end(&mut bytes).is_ok()
                        && let Ok(env) =
                            serde_json::from_slice::<eggbench_core::EnvironmentFingerprint>(&bytes)
                    {
                        return env
                            .fields
                            .iter()
                            .map(|(name, field)| EnvironmentSummary {
                                name: name.as_str().to_owned(),
                                value: field.value.clone(),
                                class: format!("{:?}", field.class),
                            })
                            .collect();
                    }
                    Vec::new()
                }
                Err(_) => Vec::new(),
            });

    let subject = SubjectSummary {
        label: subject_label(&manifest.subject),
        revision: subject_revision(&manifest.subject),
        declared_digest: subject_digest(&manifest.subject),
    };

    let trials: Vec<TrialSummary> = manifest
        .trials
        .iter()
        .map(|descriptor| TrialSummary {
            id: descriptor.id.get(),
            terminal_status: "completed".to_owned(),
            measurement_elapsed_ns: None,
        })
        .collect();

    let artifact_count = manifest.artifacts.len();
    let artifact_bytes = manifest
        .artifacts
        .iter()
        .map(|artifact| artifact.byte_size)
        .sum();

    let manifest_json = if emit_manifest_json {
        serde_json::to_string_pretty(manifest).ok()
    } else {
        None
    };

    let envelope = CliEnvelope::ok(
        "inspect",
        CliOutput::Inspect {
            manifest_schema_version: manifest.schema_version.0,
            run_id: manifest.run_id.to_string(),
            execution_status: manifest
                .execution_status
                .map(|status| format!("{status:?}").to_lowercase()),
            comparison_verdict: manifest
                .comparison_verdict
                .map(|verdict| format!("{verdict:?}").to_lowercase()),
            legacy_status: reader
                .legacy_status()
                .map(|status| format!("{status:?}").to_lowercase()),
            subject,
            drivers,
            environment_fields: env_fields,
            trials,
            artifact_count,
            artifact_bytes,
            manifest_json,
        },
    );
    let _ = ExitCode::Success;
    Ok(envelope)
}

fn subject_label(subject: &eggbench_core::Subject) -> String {
    use eggbench_core::Subject;
    match subject {
        Subject::ManagedCommand { argv, .. } => argv
            .first()
            .cloned()
            .unwrap_or_else(|| "<managed>".to_owned()),
        Subject::External { target, .. } => target.as_str().to_owned(),
        Subject::Label { label } => label.as_str().to_owned(),
    }
}

fn subject_revision(subject: &eggbench_core::Subject) -> Option<String> {
    use eggbench_core::Subject;
    match subject {
        Subject::ManagedCommand { revision, .. } | Subject::External { revision, .. } => {
            revision.clone()
        }
        Subject::Label { .. } => None,
    }
}

fn subject_digest(subject: &eggbench_core::Subject) -> Option<String> {
    use eggbench_core::Subject;
    match subject {
        Subject::ManagedCommand { digest, .. } | Subject::External { digest, .. } => digest.clone(),
        Subject::Label { .. } => None,
    }
}
