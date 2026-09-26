//! Qualification for the generic telemetry-collector lifecycle.
//!
//! Covers registry behavior, required/optional preflight semantics,
//! trial-window timing exclusion, combined workload+telemetry staging,
//! failure precedence, and drain composition. The deterministic
//! `FakeTelemetryCollector` stands in for backend collectors; backend
//! specifics live in `eggbench-drivers` tests.
#![cfg(unix)]

use eggbench_core::{
    Aggregation, ArtifactBounds, ArtifactRole, BundleReader, DurationMs, ExecutionStatus, Name,
    PositiveCount, RawMetricObservation, ResolvedPlan, RunId, Subject, TrialPolicy, Workload,
};
use eggbench_runner::test_support::{FakeTelemetryCollector, FakeTelemetryHandle, FakeWorkload};
use eggbench_runner::{
    FailureCategory, LocalSession, MapSecretProvider, OrchestrationError, ResetRegistry,
    RunnerOptions, ServiceAdapterRegistry, TelemetryOutput, TelemetryRegistry, UnixPlatform,
    WorkloadArtifact, execute_run,
};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

fn name(text: &str) -> Name {
    Name::new(text).unwrap()
}

fn plan_with_telemetry(
    metrics: Vec<eggbench_core::MetricRequest>,
    telemetry: Vec<eggbench_core::TelemetryRequest>,
    measured: u32,
) -> ResolvedPlan {
    let mut timeouts = BTreeMap::new();
    timeouts.insert(name("measurement"), DurationMs::new(2000).unwrap());
    timeouts.insert(name("drain"), DurationMs::new(2000).unwrap());
    timeouts.insert(name("telemetry"), DurationMs::new(2000).unwrap());
    ResolvedPlan {
        schema_version: eggbench_core::RESOLVED_PLAN_SCHEMA_VERSION,
        source_plan_schema_version: eggbench_core::EXPERIMENT_PLAN_SCHEMA_VERSION,
        experiment: name("telemetry"),
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
        telemetry,
        defaults: eggbench_core::ResolvedDefaults {
            platform: name("test-platform"),
            warmup_trials: 0,
            measured_trials: measured,
        },
        environment_policy: eggbench_core::EnvironmentPolicy::StrictSameTestbed,
        metrics,
        artifact_bounds: ArtifactBounds {
            artifact_count: PositiveCount::new(128).unwrap(),
            artifact_bytes: 1024 * 1024,
            total_bytes: 32 * 1024 * 1024,
        },
        seed: Some(7),
        paired: None,
        network_path: None,
        diagnostics: Vec::new(),
        security_checks: Vec::new(),
        http_corpus_checks: Vec::new(),
        warnings: Vec::new(),
    }
}

fn telemetry_request(source: &str, required: bool) -> eggbench_core::TelemetryRequest {
    eggbench_core::TelemetryRequest {
        source: name(source),
        fields: Vec::new(),
        required,
    }
}

fn metric_request(metric: &str, unit: &str) -> eggbench_core::MetricRequest {
    eggbench_core::MetricRequest {
        name: name(metric),
        unit: name(unit),
        direction: eggbench_core::MetricDirection::LowerIsBetter,
        intent: eggbench_core::MetricIntent::Primary,
        gate: None,
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
    use eggbench_core::{ArtifactPath, BundleWriter, Sensitivity};
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

fn telemetry_output(metric_name: &str) -> TelemetryOutput {
    TelemetryOutput {
        artifacts: vec![WorkloadArtifact {
            name: "collector.ndjson".to_owned(),
            media_type: "application/x-ndjson".to_owned(),
            bytes: b"{\"sample\":1}\n".to_vec(),
        }],
        metrics: vec![RawMetricObservation {
            name: metric_name.to_owned(),
            unit: "percent".to_owned(),
            value: 12.5,
            aggregation: Aggregation::Mean,
            source_field: Some("fake.value".to_owned()),
            producer: Some("fake-telemetry".to_owned()),
            producer_version: Some("0".to_owned()),
            raw_artifacts: vec!["collector.ndjson".to_owned()],
        }],
        warnings: Vec::new(),
    }
}

fn register_fake(
    registry: &mut TelemetryRegistry,
    collector: FakeTelemetryCollector,
) -> FakeTelemetryHandle {
    let (handle, proxy) = FakeTelemetryHandle::wrap(collector);
    registry.register(proxy).expect("register fake");
    handle
}

fn trial_elapsed_ns(bundle: &Path, trial: u32) -> u64 {
    let reader = BundleReader::open(bundle.join("out.eggb")).expect("bundle opens");
    let path = eggbench_core::ArtifactPath::new(format!("trials/{trial:03}/result.json")).unwrap();
    let mut file = reader.open_artifact(&path).expect("result opens");
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).expect("result reads");
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("result json");
    value["measurement_elapsed_ns"].as_u64().expect("elapsed")
}

#[test]
fn duplicate_telemetry_source_registration_rejected() {
    let mut registry = TelemetryRegistry::new();
    registry
        .register(Box::new(FakeTelemetryCollector::new(
            "fake-telemetry",
            TelemetryOutput::default(),
        )))
        .expect("first");
    let error = registry
        .register(Box::new(FakeTelemetryCollector::new(
            "fake-telemetry",
            TelemetryOutput::default(),
        )))
        .unwrap_err();
    assert!(error.contains("fake-telemetry"));
    assert_eq!(registry.sources(), vec!["fake-telemetry"]);
}

#[tokio::test]
async fn required_preflight_failure_prevents_startup() {
    let temp = tempfile::tempdir().unwrap();
    let resolved = plan_with_telemetry(vec![], vec![telemetry_request("fake-telemetry", true)], 1);
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut telemetry = TelemetryRegistry::new();
    register_fake(
        &mut telemetry,
        FakeTelemetryCollector::new("fake-telemetry", TelemetryOutput::default())
            .with_preflight_failure(),
    );
    let mut workload = FakeWorkload::default();
    let error = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut telemetry,
        writer_for(temp.path(), &resolved),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(error, OrchestrationError::Preflight(_)),
        "required failure prevents measurement, got {error:?}"
    );
    // No managed startup occurred and the workload never ran.
    assert!(session.events().is_empty());
    assert!(workload.invocations.is_empty());
}

#[tokio::test]
async fn missing_required_collector_prevents_startup() {
    let temp = tempfile::tempdir().unwrap();
    let resolved = plan_with_telemetry(vec![], vec![telemetry_request("gregg", true)], 1);
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
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
    .unwrap_err();
    assert!(matches!(error, OrchestrationError::Preflight(_)));
    assert!(session.events().is_empty());
}

#[tokio::test]
async fn optional_preflight_gap_warns_and_leaves_metrics_missing() {
    let temp = tempfile::tempdir().unwrap();
    let metrics = vec![metric_request("host_cpu_percent", "percent")];
    let resolved = plan_with_telemetry(metrics, vec![telemetry_request("gregg", false)], 1);
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
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
    let reader = BundleReader::open(&outcome.bundle_path).expect("bundle opens");
    let trial_metrics = reader
        .trial_metrics(eggbench_core::TrialId::new(1).unwrap())
        .expect("metrics read")
        .expect("metrics present");
    // Requested host metric normalizes as missing, never zero.
    let observation = trial_metrics
        .observations
        .iter()
        .find(|observation| observation.name.as_str() == "host_cpu_percent")
        .expect("host metric");
    assert!(
        matches!(
            observation.state,
            eggbench_core::ObservationState::Missing { .. }
        ),
        "optional gap stays missing"
    );
    assert!(
        trial_metrics
            .warnings
            .iter()
            .any(|warning| warning.category == "telemetry_disabled")
    );
}

#[tokio::test]
async fn telemetry_window_stays_outside_measured_elapsed() {
    let temp = tempfile::tempdir().unwrap();
    let resolved = plan_with_telemetry(vec![], vec![telemetry_request("fake-telemetry", true)], 2);
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    // Workload takes ~50ms; telemetry adds 300ms on each side of the timer.
    let mut workload = FakeWorkload::default();
    workload.delay = Duration::from_millis(50);
    let mut telemetry = TelemetryRegistry::new();
    let handle = register_fake(
        &mut telemetry,
        FakeTelemetryCollector::new("fake-telemetry", TelemetryOutput::default())
            .with_start_delay(Duration::from_millis(300))
            .with_stop_delay(Duration::from_millis(300)),
    );
    let outcome = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut telemetry,
        writer_for(temp.path(), &resolved),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.execution_status, ExecutionStatus::Completed);
    assert_eq!(handle.starts().await, vec![1, 2]);
    assert_eq!(handle.stops().await, vec![1, 2]);
    assert_eq!(handle.preflights().await, 1);
    // 600ms of telemetry latency per trial must not inflate the ~50ms
    // measured interval.
    for trial in 1..=2 {
        let elapsed = trial_elapsed_ns(temp.path(), trial);
        assert!(
            elapsed < 300_000_000,
            "trial {trial} elapsed {elapsed}ns excludes telemetry delays"
        );
    }
}

#[tokio::test]
async fn combined_workload_and_telemetry_metrics_normalize() {
    let temp = tempfile::tempdir().unwrap();
    let metrics = vec![
        metric_request("throughput", "rps"),
        metric_request("host_cpu_percent", "percent"),
    ];
    let resolved = plan_with_telemetry(metrics, vec![telemetry_request("fake-telemetry", true)], 1);
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    workload.metrics_by_invocation = vec![Some(vec![RawMetricObservation {
        name: "throughput".to_owned(),
        unit: "rps".to_owned(),
        value: 100.0,
        aggregation: Aggregation::Rate,
        source_field: Some("fake.rps".to_owned()),
        producer: None,
        producer_version: None,
        raw_artifacts: Vec::new(),
    }])];
    let mut telemetry = TelemetryRegistry::new();
    register_fake(
        &mut telemetry,
        FakeTelemetryCollector::new("fake-telemetry", telemetry_output("host_cpu_percent")),
    );
    let outcome = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut telemetry,
        writer_for(temp.path(), &resolved),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.execution_status, ExecutionStatus::Completed);
    let reader = BundleReader::open(&outcome.bundle_path).expect("bundle opens");
    let trial_metrics = reader
        .trial_metrics(eggbench_core::TrialId::new(1).unwrap())
        .expect("metrics read")
        .expect("metrics present");
    // Both producers observed with their own attribution.
    let producer_of = |name: &str| {
        trial_metrics
            .observations
            .iter()
            .find(|observation| observation.name.as_str() == name)
            .expect("metric")
            .provenance
            .producer
            .as_str()
            .to_owned()
    };
    assert_eq!(producer_of("throughput"), "unknown-workload");
    assert_eq!(producer_of("host_cpu_percent"), "fake-telemetry");
    // Telemetry artifact stages under its own namespace.
    let paths: Vec<String> = reader
        .manifest()
        .artifacts
        .iter()
        .map(|artifact| artifact.path.to_string())
        .collect();
    assert!(
        paths
            .iter()
            .any(|path| path == "trials/001/telemetry/00-00-collector.ndjson"),
        "telemetry artifact staged, got {paths:?}"
    );
}

#[tokio::test]
async fn telemetry_start_failure_fails_trial_without_measurement() {
    let temp = tempfile::tempdir().unwrap();
    let resolved = plan_with_telemetry(vec![], vec![telemetry_request("fake-telemetry", true)], 2);
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let mut telemetry = TelemetryRegistry::new();
    let handle = register_fake(
        &mut telemetry,
        FakeTelemetryCollector::new("fake-telemetry", TelemetryOutput::default())
            .with_start_failure(1),
    );
    let outcome = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut telemetry,
        writer_for(temp.path(), &resolved),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        outcome.primary_failure,
        Some(FailureCategory::TelemetryFailed)
    );
    assert_eq!(handle.starts().await, vec![1]);
    assert!(workload.invocations.is_empty());
    let reader = BundleReader::open(&outcome.bundle_path).expect("bundle opens");
    let path = eggbench_core::ArtifactPath::new("trials/001/result.json").unwrap();
    let mut file = reader.open_artifact(&path).expect("result opens");
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).expect("result reads");
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("result json");
    assert_eq!(value["failure_category"], "telemetry_failed");
    assert_eq!(value["measurement_elapsed_ns"], 0);
}

#[tokio::test]
async fn telemetry_stop_failure_after_success_is_primary_with_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    let resolved = plan_with_telemetry(vec![], vec![telemetry_request("fake-telemetry", true)], 1);
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let mut telemetry = TelemetryRegistry::new();
    register_fake(
        &mut telemetry,
        FakeTelemetryCollector::new("fake-telemetry", TelemetryOutput::default())
            .with_stop_failure(1),
    );
    let outcome = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut telemetry,
        writer_for(temp.path(), &resolved),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        outcome.primary_failure,
        Some(FailureCategory::TelemetryFailed)
    );
    assert_eq!(outcome.cleanup_failures.len(), 1);
    assert_eq!(outcome.cleanup_failures[0].service, "fake-telemetry");
}

#[tokio::test]
async fn workload_failure_plus_stop_failure_keeps_workload_primary() {
    let temp = tempfile::tempdir().unwrap();
    let resolved = plan_with_telemetry(vec![], vec![telemetry_request("fake-telemetry", true)], 1);
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    workload.fail_on = Some(1);
    let mut telemetry = TelemetryRegistry::new();
    let handle = register_fake(
        &mut telemetry,
        FakeTelemetryCollector::new("fake-telemetry", TelemetryOutput::default())
            .with_stop_failure(1),
    );
    let outcome = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut telemetry,
        writer_for(temp.path(), &resolved),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        outcome.primary_failure,
        Some(FailureCategory::WorkloadFailed)
    );
    assert_eq!(outcome.cleanup_failures.len(), 1);
    assert_eq!(outcome.cleanup_failures[0].service, "fake-telemetry");
    // Stop was still attempted after the workload failure.
    assert_eq!(handle.stops().await, vec![1]);
}

#[tokio::test]
async fn telemetry_drain_failure_follows_drain_precedence() {
    let temp = tempfile::tempdir().unwrap();
    let resolved = plan_with_telemetry(vec![], vec![telemetry_request("fake-telemetry", true)], 1);
    let mut session = LocalSession::prepare(&resolved, runner_options(temp.path())).unwrap();
    let mut workload = FakeWorkload::default();
    let mut telemetry = TelemetryRegistry::new();
    let handle = register_fake(
        &mut telemetry,
        FakeTelemetryCollector::new("fake-telemetry", TelemetryOutput::default())
            .with_drain_failure(),
    );
    let outcome = execute_run(
        &mut session,
        &resolved,
        &mut workload,
        &ResetRegistry::default(),
        &mut telemetry,
        writer_for(temp.path(), &resolved),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        outcome.primary_failure,
        Some(FailureCategory::TelemetryFailed)
    );
    assert_eq!(handle.drains().await, 1);
}
