//! Raw artifact helpers: deterministic `WorkloadArtifact` candidates from one
//! command outcome. No tool-specific metric normalization happens here.

use super::command::ExternalCommandOutcome;
use eggbench_runner::{WorkloadArtifact, WorkloadOutput};
use serde::Serialize;

/// Convert one command outcome into deterministic workload-artifact
/// candidates: `stdout.raw`, `stderr.raw`, and a small
/// `command-metadata.json` with tool/version identity, executable digest,
/// exit code, truncation counters, and parser id where applicable.
#[must_use]
pub fn artifact_candidates(
    outcome: &ExternalCommandOutcome,
    parser_id: Option<&str>,
) -> Vec<WorkloadArtifact> {
    vec![
        WorkloadArtifact {
            name: "stdout.raw".to_owned(),
            media_type: "application/octet-stream".to_owned(),
            bytes: outcome.stdout.retained().to_vec(),
        },
        WorkloadArtifact {
            name: "stderr.raw".to_owned(),
            media_type: "application/octet-stream".to_owned(),
            bytes: outcome.stderr.retained().to_vec(),
        },
        WorkloadArtifact {
            name: "command-metadata.json".to_owned(),
            media_type: "application/json".to_owned(),
            bytes: command_metadata_json(outcome, parser_id).into_bytes(),
        },
    ]
}

/// Merge command artifacts into a [`WorkloadOutput`] with no metrics.
#[must_use]
pub fn workload_output_from_outcome(
    outcome: &ExternalCommandOutcome,
    parser_id: Option<&str>,
) -> WorkloadOutput {
    WorkloadOutput {
        artifacts: artifact_candidates(outcome, parser_id),
        metrics: Vec::new(),
        histograms: Vec::new(),
        error_counts: Vec::new(),
    }
}

/// Deterministic command-metadata JSON bytes.
#[must_use]
pub fn command_metadata_json(outcome: &ExternalCommandOutcome, parser_id: Option<&str>) -> String {
    #[derive(Serialize)]
    struct Metadata<'a> {
        schema_version: u32,
        tool: &'a str,
        executable_sha256: &'a str,
        executable_bytes: u64,
        argc: usize,
        exit_code: Option<i32>,
        stdout_retained_bytes: u64,
        stdout_dropped_bytes: u64,
        stdout_truncated: bool,
        stderr_retained_bytes: u64,
        stderr_dropped_bytes: u64,
        stderr_truncated: bool,
        parser_id: Option<&'a str>,
    }
    let metadata = Metadata {
        schema_version: 1,
        tool: &outcome.executable.logical_tool,
        executable_sha256: &outcome.executable.sha256_hex,
        executable_bytes: outcome.executable.file_size,
        argc: outcome.argc,
        exit_code: outcome.exit_code,
        stdout_retained_bytes: outcome.stdout.retained_bytes(),
        stdout_dropped_bytes: outcome.stdout.dropped_bytes(),
        stdout_truncated: outcome.stdout.truncated(),
        stderr_retained_bytes: outcome.stderr.retained_bytes(),
        stderr_dropped_bytes: outcome.stderr.dropped_bytes(),
        stderr_truncated: outcome.stderr.truncated(),
        parser_id,
    };
    serde_json::to_string_pretty(&metadata).unwrap_or_else(|_| "{}".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external::resolver::ResolvedExecutable;
    use std::time::Duration;

    #[test]
    fn artifact_names_are_safe_single_components() {
        let outcome = ExternalCommandOutcome {
            executable: ResolvedExecutable {
                logical_tool: "tool".to_owned(),
                selected_path: "/tmp/tool".into(),
                canonical_path: "/tmp/tool".into(),
                sha256_hex: "ab".repeat(32),
                file_size: 10,
                executable_class: "test".to_owned(),
            },
            argc: 2,
            exit_code: Some(0),
            stdout: eggbench_test_capture(b"hello"),
            stderr: eggbench_test_capture(b""),
            duration: Duration::from_millis(1),
            cancelled: false,
            timed_out: false,
            cleanup_notes: Vec::new(),
        };
        let artifacts = artifact_candidates(&outcome, Some("parser/v1"));
        assert_eq!(artifacts.len(), 3);
        for artifact in &artifacts {
            assert!(!artifact.name.is_empty());
            assert!(!artifact.name.contains('/'));
            assert!(!artifact.name.contains('\\'));
        }
        assert_eq!(artifacts[2].name, "command-metadata.json");
    }

    fn eggbench_test_capture(bytes: &[u8]) -> crate::external::command::CapturedStream {
        crate::external::command::CapturedStream::collect(bytes.to_vec(), bytes.len() as u64, 65536)
    }
}
