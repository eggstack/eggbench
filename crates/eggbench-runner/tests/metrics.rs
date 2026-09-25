//! Focused qualification for Measurement M001 trial normalization.
#![cfg(unix)]

use eggbench_core::{
    Aggregation, ArtifactBounds, ArtifactRole, BundleReader, DurationMs, ExecutionStatus, Name,
    PositiveCount, RawHistogramInput, RawMetricObservation, ResolvedPlan, RunId, Sensitivity,
    Subject, TrialExecutionStatus, TrialId, TrialPolicy, Workload, trial_metrics_path,
};
use eggbench_runner::test_support::FakeWorkload;
use eggbench_runner::{
    LocalSession, MapSecretProvider, OrchestrationError, ResetRegistry, RunnerOptions,
    ServiceAdapterRegistry, TelemetryRegistry, UnixPlatform, WorkloadArtifact, execute_run,
};
use std::{collections::BTreeMap, path::Path, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

fn name(text: &str) -> Name {
    Name::new(text).unwrap()
}

fn metric_request(
    metric: &str,
    unit: &str,
    direction: eggbench_core::MetricDirection,
) -> eggbench_core::MetricRequest {
    eggbench_core::MetricRequest {
        name: name(metric),
        unit: name(unit),
        direction,
        intent: eggbench_core::MetricIntent::Primary,
        gate: None,
    }
}

fn plan_with_metrics(metrics: Vec<eggbench_core::MetricRequest>, measured: u32) -> ResolvedPlan {
    let mut timeouts = BTreeMap::new();
    timeouts.insert(name("measurement"), DurationMs::new(500).unwrap());
    timeouts.insert(name("drain"), DurationMs::new(500).unwrap());
    ResolvedPlan {
        schema_version: eggbench_core::RESOLVED_PLAN_SCHEMA_VERSION,
        source_plan_schema_version: eggbench_core::EXPERIMENT_PLAN_SCHEMA_VERSION,
        experiment: name("metrics"),
        drivers: BTreeMap::new(),
        subject: Subject::Label {
            label: name("subject"),
        },
        topology: Vec::new(),
        workload: Workload::FiniteCount {
            target: name("app"),
            requests: PositiveCount::new(4).unwrap(),
            concurrency: PositiveCount::new(1).unwrap(),
        },
        trials: TrialPolicy {
            measured: PositiveCount::new(measured).unwrap(),
            warmup: 0,
            cooldown_ms: None,
            reset: eggbench_core::ResetPolicy::None,
            timeouts,
        },
        telemetry: Vec::new(),
        defaults: eggbench_core::ResolvedDefaults {
            platform: name("test-platform"),
            warmup_trials: 0,
            measured_trials: measured,
        },
        environment_policy: eggbench_core::EnvironmentPolicy::StrictSameTestbed,
        metrics,
        artifact_bounds: ArtifactBounds {
            artifact_count: PositiveCount::new(64).unwrap(),
            artifact_bytes: 1024 * 1024,
            total_bytes: 4 * 1024 * 1024,
        },
        seed: Some(7),
        paired: None,
        network_path: None,
        warnings: Vec::new(),
    }
}

fn runner_options(root: &Path) -> RunnerOptions {
    RunnerOptions {
        workspace_root: root.to_path_buf(),
        secrets: Arc::new(MapSecretProvider::empty()),
        probes: eggbench_runner::ProbeRegistry::with_builtins(),
        platform: Arc::new(UnixPlatform),
        service_adapters: ServiceAdapterRegistry::new(),
    }
}

fn writer_for(root: &Path, resolved: &ResolvedPlan) -> eggbench_core::BundleWriter {
    use eggbench_core::{ArtifactPath, BundleWriter};
    let path = root.join("out.eggb");
    let mut writer = BundleWriter::create(&path, RunId::new(), resolved.artifact_bounds).unwrap();
    for (file, role) in [
        ("plan.json", ArtifactRole::ExperimentPlan),
        ("resolved-plan.json", ArtifactRole::ResolvedPlan),
        ("environment.json", ArtifactRole::EnvironmentFingerprint),
    ] {
        writer
            .add_artifact(
                ArtifactPath::new(file).unwrap(),
                role,
                "application/json",
                Sensitivity::Public,
                br"{}".as_slice(),
            )
            .unwrap();
    }
    writer
}

fn throughput_observation(value: f64) -> RawMetricObservation {
    RawMetricObservation {
        name: "throughput".to_owned(),
        unit: "rps".to_owned(),
        value,
        aggregation: Aggregation::Rate,
        source_field: Some("requests_per_second".to_owned()),
        producer: None,
        producer_version: None,
        raw_artifacts: Vec::new(),
    }
}

fn latency_p99_observation(value: f64) -> RawMetricObservation {
    RawMetricObservation {
        name: "latency_p99".to_owned(),
        unit: "ms".to_owned(),
        value,
        aggregation: Aggregation::Percentile { basis_points: 9900 },
        source_field: Some("p99_ms".to_owned()),
        producer: None,
        producer_version: None,
        raw_artifacts: Vec::new(),
    }
}

#[tokio::test]
async fn measured_trial_stages_metrics_json_with_observed_values() {
    let temp = tempfile::tempdir().unwrap();
    let resolved = plan_with_metrics(
        vec![
            metric_request(
                "throughput",
                "rps",
                eggbench_core::MetricDirection::HigherIsBetter,
            ),
            metric_request(
                "latency_p99",
                "ms",
                eggbench_core::MetricDirection::LowerIsBetter,
            ),
        ],
        1,
    );
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    workload.metrics_by_invocation = vec![Some(vec![
        throughput_observation(1000.0),
        latency_p99_observation(12.5),
    ])];
    let outcome = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer_for(temp.path(), &resolved),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.execution_status, ExecutionStatus::Completed);
    let reader = BundleReader::open(&outcome.bundle_path).unwrap();
    reader.verify().unwrap();
    let metrics = reader
        .trial_metrics(TrialId::new(1).unwrap())
        .unwrap()
        .expect("metrics.json staged");
    assert_eq!(metrics.observations.len(), 2);
    // Deterministic ordering by metric name.
    assert_eq!(metrics.observations[0].name.as_str(), "latency_p99");
    assert_eq!(metrics.observations[1].name.as_str(), "throughput");
    assert!(metrics.observations.iter().all(|observation| {
        matches!(
            observation.state,
            eggbench_core::ObservationState::Observed { .. }
        )
    }));
    // The metrics artifact belongs to the trial descriptor.
    let descriptor = &reader.manifest().trials[0];
    assert!(
        descriptor
            .artifacts
            .contains(&trial_metrics_path(TrialId::new(1).unwrap()).unwrap())
    );
}

#[tokio::test]
#[allow(clippy::float_cmp)]
async fn warmup_does_not_receive_trial_metrics() {
    let temp = tempfile::tempdir().unwrap();
    let mut resolved = plan_with_metrics(
        vec![metric_request(
            "throughput",
            "rps",
            eggbench_core::MetricDirection::HigherIsBetter,
        )],
        1,
    );
    resolved.trials.warmup = 1;
    resolved.defaults.warmup_trials = 1;
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    // Warmup invocation (ordinal 1) returns observations; they must not become
    // measured evidence. Measured invocation (ordinal 2) returns its own.
    workload.metrics_by_invocation = vec![
        Some(vec![throughput_observation(1.0)]),
        Some(vec![throughput_observation(2.0)]),
    ];
    let outcome = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer_for(temp.path(), &resolved),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.execution_status, ExecutionStatus::Completed);
    let reader = BundleReader::open(&outcome.bundle_path).unwrap();
    let metrics = reader
        .trial_metrics(TrialId::new(1).unwrap())
        .unwrap()
        .expect("measured metrics staged");
    let observed = metrics
        .observations
        .iter()
        .find(|observation| observation.name.as_str() == "throughput")
        .unwrap();
    // Measured value wins; warmup value never appears in trial evidence.
    assert!(matches!(
        observed.state,
        eggbench_core::ObservationState::Observed { value } if value == 2.0
    ));
    let warmup_metrics = reader
        .manifest()
        .artifacts
        .iter()
        .any(|artifact| artifact.path.as_str() == "warmups/001/metrics.json");
    assert!(!warmup_metrics, "warmups must not receive TrialMetrics");
}

#[tokio::test]
async fn failed_trial_marks_metrics_missing_but_still_stages() {
    let temp = tempfile::tempdir().unwrap();
    let resolved = plan_with_metrics(
        vec![metric_request(
            "latency_p99",
            "ms",
            eggbench_core::MetricDirection::LowerIsBetter,
        )],
        1,
    );
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    workload.fail_on = Some(1);
    let outcome = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer_for(temp.path(), &resolved),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.execution_status, ExecutionStatus::Failed);
    let reader = BundleReader::open(&outcome.bundle_path).unwrap();
    let metrics = reader
        .trial_metrics(TrialId::new(1).unwrap())
        .unwrap()
        .expect("failed trial still stages metrics.json");
    assert!(matches!(
        metrics.observations[0].state,
        eggbench_core::ObservationState::Missing {
            reason: eggbench_core::MissingReason::TrialNotCompleted
        }
    ));
    // Failed trials keep diagnostic evidence but expose no valid observation.
    assert_eq!(reader.manifest().trials.len(), 1);
}

#[tokio::test]
async fn semantic_invalid_metric_still_finalizes_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let resolved = plan_with_metrics(
        vec![metric_request(
            "latency_p99",
            "ms",
            eggbench_core::MetricDirection::LowerIsBetter,
        )],
        1,
    );
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    // Unit mismatch: raw `us` against requested `ms`. No implicit conversion.
    workload.metrics_by_invocation = vec![Some(vec![RawMetricObservation {
        name: "latency_p99".to_owned(),
        unit: "us".to_owned(),
        value: 100.0,
        aggregation: Aggregation::Percentile { basis_points: 9900 },
        source_field: None,
        producer: None,
        producer_version: None,
        raw_artifacts: Vec::new(),
    }])];
    let outcome = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer_for(temp.path(), &resolved),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.execution_status, ExecutionStatus::Completed);
    let reader = BundleReader::open(&outcome.bundle_path).unwrap();
    reader.verify().unwrap();
    let metrics = reader
        .trial_metrics(TrialId::new(1).unwrap())
        .unwrap()
        .expect("metrics staged");
    assert!(matches!(
        metrics.observations[0].state,
        eggbench_core::ObservationState::Invalid {
            reason: eggbench_core::InvalidReason::UnitMismatch,
            ..
        }
    ));
}

#[tokio::test]
async fn structural_metric_overflow_uses_evidence_error_cleanup_path() {
    let temp = tempfile::tempdir().unwrap();
    let resolved = plan_with_metrics(
        vec![metric_request(
            "latency_p99",
            "ms",
            eggbench_core::MetricDirection::LowerIsBetter,
        )],
        1,
    );
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    // 300 raw observations exceed the per-invocation structural cap (256).
    let observations: Vec<RawMetricObservation> = (0..300)
        .map(|index| RawMetricObservation {
            name: format!("custom-{index}"),
            unit: "widgets".to_owned(),
            value: 1.0,
            aggregation: Aggregation::Direct,
            source_field: None,
            producer: None,
            producer_version: None,
            raw_artifacts: Vec::new(),
        })
        .collect();
    workload.metrics_by_invocation = vec![Some(observations)];
    workload.delay = Duration::from_millis(1);
    let error = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer_for(temp.path(), &resolved),
        &CancellationToken::new(),
    )
    .await
    .expect_err("structural overflow must fail");
    assert!(
        matches!(error, OrchestrationError::Evidence { .. }),
        "expected evidence error, got {error}"
    );
    // Mandatory cleanup tail still ran.
    assert!(workload.drained);
    assert!(!session.is_running());
}

#[tokio::test]
async fn histogram_reference_resolves_to_same_trial_artifact() {
    let temp = tempfile::tempdir().unwrap();
    let resolved = plan_with_metrics(
        vec![metric_request(
            "latency_p99",
            "ms",
            eggbench_core::MetricDirection::LowerIsBetter,
        )],
        1,
    );
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    workload.artifacts_by_invocation = vec![Some(vec![WorkloadArtifact {
        name: "latency.hist".to_owned(),
        media_type: "application/octet-stream".to_owned(),
        bytes: vec![1, 2, 3, 4],
    }])];
    workload.metrics_by_invocation = vec![Some(vec![RawMetricObservation {
        name: "latency_p99".to_owned(),
        unit: "ms".to_owned(),
        value: 9.0,
        aggregation: Aggregation::Percentile { basis_points: 9900 },
        source_field: Some("p99_ms".to_owned()),
        producer: None,
        producer_version: None,
        raw_artifacts: vec!["latency.hist".to_owned()],
    }])];
    workload.histograms_by_invocation = vec![Some(vec![RawHistogramInput {
        metric: "latency".to_owned(),
        artifact_name: "latency.hist".to_owned(),
        format: "fake-hist-v1".to_owned(),
        unit: "ms".to_owned(),
        method: Some("none".to_owned()),
    }])];
    workload.error_counts_by_invocation = vec![Some(vec![
        ("timeout".to_owned(), 2_u64),
        ("refused".to_owned(), 1_u64),
    ])];
    let outcome = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer_for(temp.path(), &resolved),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.execution_status, ExecutionStatus::Completed);
    let reader = BundleReader::open(&outcome.bundle_path).unwrap();
    reader.verify().unwrap();
    let metrics = reader
        .trial_metrics(TrialId::new(1).unwrap())
        .unwrap()
        .expect("metrics staged");
    assert_eq!(metrics.histograms.len(), 1);
    assert_eq!(metrics.histograms[0].metric.as_str(), "latency");
    // Histogram path is a manifest-listed same-trial artifact.
    let histogram_path = &metrics.histograms[0].path;
    assert!(
        reader
            .manifest()
            .artifacts
            .iter()
            .any(|artifact| &artifact.path == histogram_path)
    );
    assert!(histogram_path.as_str().starts_with("trials/001/"));
    // Provenance references the same staged artifact.
    assert_eq!(
        metrics.observations[0].provenance.raw_artifacts.as_slice(),
        std::slice::from_ref(histogram_path)
    );
    // Error distribution is sorted and descriptive.
    assert_eq!(metrics.error_distribution.len(), 2);
    assert_eq!(metrics.error_distribution[0].category.as_str(), "refused");
    assert_eq!(metrics.error_distribution[1].category.as_str(), "timeout");
}

#[tokio::test]
async fn unknown_histogram_reference_is_dropped_without_claim() {
    let temp = tempfile::tempdir().unwrap();
    let resolved = plan_with_metrics(
        vec![metric_request(
            "latency_p99",
            "ms",
            eggbench_core::MetricDirection::LowerIsBetter,
        )],
        1,
    );
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    workload.metrics_by_invocation = vec![Some(vec![latency_p99_observation(5.0)])];
    workload.histograms_by_invocation = vec![Some(vec![RawHistogramInput {
        metric: "latency".to_owned(),
        artifact_name: "ghost.hist".to_owned(),
        format: "fake-hist-v1".to_owned(),
        unit: "ms".to_owned(),
        method: None,
    }])];
    let outcome = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer_for(temp.path(), &resolved),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.execution_status, ExecutionStatus::Completed);
    let reader = BundleReader::open(&outcome.bundle_path).unwrap();
    let metrics = reader
        .trial_metrics(TrialId::new(1).unwrap())
        .unwrap()
        .expect("metrics staged");
    assert!(
        metrics.histograms.is_empty(),
        "unknown histogram refs must not be claimed"
    );
    assert!(matches!(
        metrics.observations[0].state,
        eggbench_core::ObservationState::Observed { .. }
    ));
}

#[tokio::test]
async fn unrequested_raw_metric_never_becomes_gate_eligible() {
    let temp = tempfile::tempdir().unwrap();
    let resolved = plan_with_metrics(
        vec![metric_request(
            "throughput",
            "rps",
            eggbench_core::MetricDirection::HigherIsBetter,
        )],
        1,
    );
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    workload.metrics_by_invocation = vec![Some(vec![
        throughput_observation(500.0),
        RawMetricObservation {
            name: "secret_custom".to_owned(),
            unit: "widgets".to_owned(),
            value: 99.0,
            aggregation: Aggregation::Direct,
            source_field: None,
            producer: None,
            producer_version: None,
            raw_artifacts: Vec::new(),
        },
    ])];
    let outcome = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut TelemetryRegistry::new(),
        writer_for(temp.path(), &resolved),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.execution_status, ExecutionStatus::Completed);
    let reader = BundleReader::open(&outcome.bundle_path).unwrap();
    let metrics = reader
        .trial_metrics(TrialId::new(1).unwrap())
        .unwrap()
        .expect("metrics staged");
    // Only the requested metric is normalized; the extra raw value is a warning.
    assert_eq!(metrics.observations.len(), 1);
    assert_eq!(metrics.observations[0].name.as_str(), "throughput");
    assert!(
        metrics
            .warnings
            .iter()
            .any(|warning| warning.category == "unrequested_raw_metric"),
        "expected unrequested-metric warning"
    );
}

#[tokio::test]
async fn trial_is_the_statistical_unit_not_requests() {
    // Two plans with different per-trial request counts but the same number of
    // measured trials must expose exactly one scalar observation per requested
    // metric per completed trial — never one row per request.
    for requests in [4_u32, 10_000_u32] {
        let temp = tempfile::tempdir().unwrap();
        let mut resolved = plan_with_metrics(
            vec![metric_request(
                "throughput",
                "rps",
                eggbench_core::MetricDirection::HigherIsBetter,
            )],
            3,
        );
        if let Workload::FiniteCount {
            requests: count, ..
        } = &mut resolved.workload
        {
            *count = PositiveCount::new(requests).unwrap();
        }
        let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
        let mut workload = FakeWorkload::default();
        workload.metrics_by_invocation = vec![
            Some(vec![throughput_observation(100.0)]),
            Some(vec![throughput_observation(200.0)]),
            Some(vec![throughput_observation(300.0)]),
        ];
        let outcome = execute_run(
            &mut session,
            &resolved,
            &mut workload,
            &ResetRegistry::default(),
            &mut TelemetryRegistry::new(),
            writer_for(temp.path(), &resolved),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(outcome.execution_status, ExecutionStatus::Completed);
        let reader = BundleReader::open(&outcome.bundle_path).unwrap();
        assert_eq!(reader.manifest().trials.len(), 3);
        for trial_number in 1..=3 {
            let metrics = reader
                .trial_metrics(TrialId::new(trial_number).unwrap())
                .unwrap()
                .expect("metrics staged");
            assert_eq!(
                metrics.observations.len(),
                1,
                "requests={requests}: exactly one scalar per trial"
            );
        }
    }
}

#[tokio::test]
async fn legacy_bundle_without_metrics_returns_none() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("eggbench-core")
        .join("tests")
        .join("fixtures")
        .join("current-v2.eggb");
    let reader = BundleReader::open(&fixture).unwrap();
    let metrics = reader.trial_metrics(TrialId::new(1).unwrap()).unwrap();
    assert!(
        metrics.is_none(),
        "pre-M001 bundles carry no normalized metrics"
    );
    // Unknown trial identities also return None rather than an error.
    let missing = reader.trial_metrics(TrialId::new(999).unwrap()).unwrap();
    assert!(missing.is_none());
}

#[test]
fn trial_metrics_path_is_stable() {
    let path = trial_metrics_path(TrialId::new(7).unwrap()).unwrap();
    assert_eq!(path.as_str(), "trials/007/metrics.json");
    assert_eq!(
        TrialExecutionStatus::Completed as u8,
        TrialExecutionStatus::Completed as u8
    );
}
