//! `eggbench inspect <bundle>` command.

use crate::envelope::{
    CliOutput, DriverSummary, EnvironmentSummary, NetworkPathInspectSummary,
    PresentedCommandResult, SemanticReplayInspectSummary, SubjectSummary, TrialSummary,
};
use crate::error::CliError;
use eggbench_core::BundleReader;
use std::io::Read;
use std::path::Path;

/// Open, verify, and summarize a finalized bundle.
///
/// # Errors
/// Returns [`CliError`] when the bundle cannot be opened or verified.
pub fn run(bundle: &Path, emit_manifest_json: bool) -> Result<PresentedCommandResult, CliError> {
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
            adapter_version: driver.adapter_version.clone(),
            upstream_name: driver.upstream_name.clone(),
            upstream_version: driver.upstream_version.clone(),
            capabilities: driver
                .capabilities
                .iter()
                .map(|capability| format!("{capability:?}"))
                .collect(),
            // Bundle manifests predate presence probing; the binary state
            // on this host says nothing about the recording host.
            binary_present: None,
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
        .map(|descriptor| trial_summary(&reader, descriptor))
        .collect::<Result<_, _>>()?;

    let artifact_count = manifest.artifacts.len();
    let artifact_bytes = manifest
        .artifacts
        .iter()
        .map(|artifact| artifact.byte_size)
        .sum();

    let network_path_artifact_present = manifest.artifacts.iter().any(|artifact| {
        artifact.path.as_str() == "network-path.json"
            || matches!(
                &artifact.role,
                eggbench_core::ArtifactRole::Other { label } if label.as_str() == "network-path"
            )
    });
    if network_path_artifact_present {
        eggbench_core::load_comparison_input(&reader).map_err(|error| {
            CliError::Bundle(eggbench_core::BundleError::ManifestParse(error.to_string()))
        })?;
    }
    let network_path = network_path_inspect_summary(&reader, network_path_artifact_present)?;
    let semantic_replay = semantic_replay_inspect_summary(&reader)?;

    let manifest_json = if emit_manifest_json {
        serde_json::to_string_pretty(manifest).ok()
    } else {
        None
    };

    Ok(PresentedCommandResult::success(
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
            network_path: Some(network_path),
            semantic_replay: Some(semantic_replay),
        },
    ))
}

fn trial_summary(
    reader: &BundleReader,
    descriptor: &eggbench_core::TrialDescriptor,
) -> Result<TrialSummary, CliError> {
    let mut file = reader
        .open_artifact(&descriptor.result)
        .map_err(CliError::Bundle)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|error| {
        CliError::Bundle(eggbench_core::BundleError::Io {
            path: std::path::PathBuf::from(descriptor.result.as_str()),
            source: error,
        })
    })?;
    let Ok(result) = serde_json::from_slice::<eggbench_core::TrialExecutionResult>(&bytes) else {
        return Ok(TrialSummary {
            id: descriptor.id.get(),
            terminal_status: "completed".to_owned(),
            measurement_elapsed_ns: None,
            semantic_findings: None,
        });
    };
    let semantic_findings =
        reader
            .trial_metrics(descriptor.id)
            .ok()
            .flatten()
            .and_then(|metrics| {
                metrics.observations.iter().find_map(|observation| {
                    if observation.name.as_str() == "semantic_findings" {
                        match observation.state {
                            eggbench_core::ObservationState::Observed { value } => Some(value),
                            _ => None,
                        }
                    } else {
                        None
                    }
                })
            });
    Ok(TrialSummary {
        id: descriptor.id.get(),
        terminal_status: format!("{:?}", result.terminal_status).to_lowercase(),
        measurement_elapsed_ns: Some(result.measurement_elapsed_ns),
        semantic_findings,
    })
}

fn semantic_replay_inspect_summary(
    reader: &BundleReader,
) -> Result<SemanticReplayInspectSummary, CliError> {
    let manifest = reader.manifest();
    let record = manifest.artifacts.iter().find(|artifact| {
        artifact.path.as_str() == "semantic-replay.json"
            || matches!(
                &artifact.role,
                eggbench_core::ArtifactRole::Other { label } if label.as_str() == "semantic-replay"
            )
    });
    let Some(record) = record else {
        return Ok(SemanticReplayInspectSummary {
            artifact_present: false,
            driver: None,
            fixture_digest: None,
            fixture_session_schema: None,
            envelope_schema: None,
            report_schema: None,
            executable_version: None,
            flow_count: None,
        });
    };
    let mut file = reader
        .open_artifact(&record.path)
        .map_err(CliError::Bundle)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|error| {
        CliError::Bundle(eggbench_core::BundleError::Io {
            path: std::path::PathBuf::from(record.path.as_str()),
            source: error,
        })
    })?;
    let evidence: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
        CliError::Bundle(eggbench_core::BundleError::ManifestParse(error.to_string()))
    })?;
    Ok(SemanticReplayInspectSummary {
        artifact_present: true,
        driver: evidence
            .get("driver")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        fixture_digest: evidence
            .get("fixture_digest")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        fixture_session_schema: evidence
            .get("fixture_session_schema")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        envelope_schema: evidence
            .get("envelope_schema")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        report_schema: evidence
            .get("report_schema")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        executable_version: evidence
            .get("executable_version")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        flow_count: evidence
            .get("flow_count")
            .and_then(serde_json::Value::as_u64),
    })
}

#[allow(clippy::unnecessary_wraps)] // Feature-off inspection is infallible; feature-on loading is verified.
fn network_path_inspect_summary(
    reader: &BundleReader,
    artifact_present: bool,
) -> Result<NetworkPathInspectSummary, CliError> {
    #[cfg(feature = "eggstack-path")]
    {
        let evidence =
            eggbench_drivers::load_network_path_evidence(reader).map_err(CliError::Bundle)?;
        let Some(evidence) = evidence else {
            return Ok(NetworkPathInspectSummary {
                artifact_present,
                detailed_evidence_available: true,
                schema_version: None,
                route_driver: None,
                route_upstream_version: None,
                route_mode: None,
                chain_config_digest: None,
                fault_driver: None,
                fault_upstream_version: None,
                fault_count: None,
                ordering: None,
                fault_layer: None,
                policy_mode: None,
                physical_dial_attempts: None,
                successful_dials: None,
                fault_wrapped_connections: None,
                configured_hop_count: None,
            });
        };
        let fault_count = evidence
            .stream_faults
            .as_ref()
            .map(|faults| faults.request.upstream.len() + faults.request.downstream.len());
        let fault_driver = evidence
            .fault_driver
            .as_ref()
            .map(|driver| driver.name.clone());
        let fault_upstream_version = evidence
            .fault_driver
            .as_ref()
            .and_then(|driver| driver.upstream_version.clone());
        Ok(NetworkPathInspectSummary {
            artifact_present,
            detailed_evidence_available: true,
            schema_version: Some(evidence.schema_version.0),
            route_driver: Some(evidence.route_driver.name),
            route_upstream_version: evidence.route_driver.upstream_version,
            route_mode: Some(match &evidence.route.mode {
                eggbench_core::RouteMode::Direct => "direct".to_owned(),
                eggbench_core::RouteMode::ProxyChain { .. } => "proxy_chain".to_owned(),
            }),
            chain_config_digest: evidence.chain_config_digest,
            fault_driver,
            fault_upstream_version,
            fault_count,
            ordering: Some(match evidence.semantics.ordering {
                eggbench_drivers::PathOrdering::RouteFirstFaultSecond => {
                    "route_first_fault_second".to_owned()
                }
            }),
            fault_layer: Some(match evidence.semantics.fault_layer {
                eggbench_drivers::FaultLayer::UserSpaceStream => "user_space_stream".to_owned(),
            }),
            policy_mode: Some(match evidence.policy_mode {
                eggbench_drivers::PathPolicyMode::Static => "static".to_owned(),
            }),
            physical_dial_attempts: Some(evidence.diagnostics.physical_dial_attempts),
            successful_dials: Some(evidence.diagnostics.successful_dials),
            fault_wrapped_connections: Some(evidence.diagnostics.fault_wrapped_connections),
            configured_hop_count: Some(evidence.configured_hop_count),
        })
    }
    #[cfg(not(feature = "eggstack-path"))]
    {
        let _ = reader;
        Ok(NetworkPathInspectSummary {
            artifact_present,
            detailed_evidence_available: false,
            schema_version: None,
            route_driver: None,
            route_upstream_version: None,
            route_mode: None,
            chain_config_digest: None,
            fault_driver: None,
            fault_upstream_version: None,
            fault_count: None,
            ordering: None,
            fault_layer: None,
            policy_mode: None,
            physical_dial_attempts: None,
            successful_dials: None,
            fault_wrapped_connections: None,
            configured_hop_count: None,
        })
    }
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
