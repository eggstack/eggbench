//! Focused lifecycle evidence for Local Runner M001.
//!
//! Each test maps to a required verification bullet: dependency-ordered
//! startup and reverse teardown, readiness success and named-probe failure,
//! spawn failure and readiness timeout, cancellation, graceful then forced
//! cleanup, descendant cleanup, cleanup-failure preservation, bounded pipe
//! draining, external-service observation, secret handling, and
//! inconclusive zero-trial evidence.

use eggbench_core::{
    ArtifactBounds, ArtifactRole, BundleWriter, ComparisonVerdict, DurationMs, ExecutionStatus,
    Lifecycle, Name, PositiveCount, Readiness, ResolvedPlan, RunId, Service, ServiceKind, Shutdown,
    Subject, TrialPolicy,
};
use eggbench_runner::{
    LifecycleEventKind, LocalSession, MapSecretProvider, PlatformAdapter, PlatformSupport,
    ProbeRegistry, RunnerError, RunnerOptions, UnixPlatform, UnsupportedPlatform, is_process_alive,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

fn name(value: &str) -> Name {
    Name::new(value).unwrap()
}

fn fixture() -> String {
    env!("CARGO_BIN_EXE_eggbench-child-fixture").to_owned()
}

fn argv(args: &[&str]) -> Vec<String> {
    std::iter::once(fixture())
        .chain(args.iter().map(|arg| (*arg).to_owned()))
        .collect()
}

fn base_resolved() -> ResolvedPlan {
    ResolvedPlan {
        schema_version: eggbench_core::RESOLVED_PLAN_SCHEMA_VERSION,
        source_plan_schema_version: eggbench_core::EXPERIMENT_PLAN_SCHEMA_VERSION,
        experiment: name("lifecycle"),
        drivers: BTreeMap::new(),
        subject: Subject::Label {
            label: name("bench"),
        },
        topology: Vec::new(),
        workload: eggbench_core::Workload::FiniteCount {
            target: name("app"),
            requests: PositiveCount::new(10).unwrap(),
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
        defaults: eggbench_core::ResolvedDefaults {
            platform: name("linux-x86_64"),
            warmup_trials: 0,
            measured_trials: 1,
        },
        environment_policy: eggbench_core::EnvironmentPolicy::StrictSameTestbed,
        metrics: Vec::new(),
        artifact_bounds: ArtifactBounds {
            artifact_count: PositiveCount::new(256).unwrap(),
            artifact_bytes: 16 * 1024 * 1024,
            total_bytes: 64 * 1024 * 1024,
        },
        seed: None,
        warnings: Vec::new(),
    }
}

fn managed(service: &str, args: &[&str], depends_on: &[&str], log_limit_bytes: u64) -> Service {
    Service {
        name: name(service),
        kind: ServiceKind::Command { argv: argv(args) },
        lifecycle: Lifecycle::Managed,
        depends_on: depends_on.iter().map(|dep| name(dep)).collect(),
        config: BTreeMap::new(),
        readiness: None,
        shutdown: None,
        working_directory: None,
        log_limit_bytes,
    }
}

fn delay_ms(value: u64) -> DurationMs {
    DurationMs::new(value).unwrap()
}

fn options(root: &std::path::Path) -> RunnerOptions {
    RunnerOptions {
        workspace_root: root.to_path_buf(),
        secrets: Arc::new(MapSecretProvider::empty()),
        probes: ProbeRegistry::with_builtins(),
        platform: Arc::new(UnixPlatform),
    }
}

fn temp_root() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

#[tokio::test]
async fn dependency_order_startup_and_reverse_teardown() {
    let root = temp_root();
    let mut resolved = base_resolved();
    resolved.topology = vec![
        managed("c", &["sleep", "30000"], &["b"], 4096),
        managed("a", &["sleep", "30000"], &[], 4096),
        managed("b", &["sleep", "30000"], &["a"], 4096),
    ];
    let mut session = LocalSession::prepare(&resolved, options(root.path())).unwrap();
    assert_eq!(session.spawn_order(), vec!["a", "b", "c"]);
    assert_eq!(session.teardown_order(), vec!["c", "b", "a"]);

    let cancel = CancellationToken::new();
    let report = session.startup(&cancel).await.unwrap();
    assert_eq!(report.started, vec!["a", "b", "c"]);
    let spawned: Vec<String> = session
        .events()
        .iter()
        .filter(|event| event.kind == LifecycleEventKind::Spawned)
        .map(|event| event.identity.clone())
        .collect();
    assert_eq!(spawned, vec!["a", "b", "c"]);
    for event in session.events() {
        assert!(event.pid.is_some(), "pids are recorded as diagnostics");
    }

    let shutdown = session.shutdown().await;
    assert_eq!(shutdown.stopped_order, vec!["c", "b", "a"]);
    assert!(shutdown.failures.is_empty());
    assert!(!session.is_running());
}

#[tokio::test]
async fn readiness_delay_and_fake_ok_succeed() {
    let root = temp_root();
    let mut resolved = base_resolved();
    let mut first = managed("first", &["sleep", "30000"], &[], 4096);
    first.readiness = Some(Readiness::Delay {
        after_ms: delay_ms(100),
    });
    let mut second = managed("second", &["sleep", "30000"], &[], 4096);
    second.readiness = Some(Readiness::Probe {
        probe: name("fake-ok"),
        timeout_ms: delay_ms(1000),
    });
    resolved.topology = vec![first, second];
    let mut session = LocalSession::prepare(&resolved, options(root.path())).unwrap();
    let outcome = session.run(&CancellationToken::new()).await.unwrap();
    assert_eq!(outcome.started, vec!["first", "second"]);
    assert!(outcome.cleanup.is_empty());
    assert!(!session.is_running());
}

#[tokio::test]
async fn unknown_probe_fails_before_spawn() {
    let root = temp_root();
    let mut resolved = base_resolved();
    let mut service = managed("probed", &["sleep", "30000"], &[], 4096);
    service.readiness = Some(Readiness::Probe {
        probe: name("nope"),
        timeout_ms: delay_ms(1000),
    });
    resolved.topology = vec![service];
    let mut session = LocalSession::prepare(&resolved, options(root.path())).unwrap();
    let error = session
        .startup(&CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, RunnerError::UnsupportedProbe { .. }));
    assert!(error.cleanup().is_empty());
    assert!(session.events().is_empty());
    assert!(!session.is_running());
}

#[tokio::test]
async fn spawn_failure_before_readiness() {
    let root = temp_root();
    let mut resolved = base_resolved();
    resolved.topology = vec![Service {
        name: name("broken"),
        kind: ServiceKind::Command {
            argv: vec!["/nonexistent/eggbench-fixture-program".to_owned()],
        },
        lifecycle: Lifecycle::Managed,
        depends_on: Vec::new(),
        config: BTreeMap::new(),
        readiness: None,
        shutdown: None,
        working_directory: None,
        log_limit_bytes: 4096,
    }];
    let mut session = LocalSession::prepare(&resolved, options(root.path())).unwrap();
    let error = session
        .startup(&CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, RunnerError::SpawnFailed { .. }));
    assert!(error.cleanup().is_empty());
    assert!(!session.is_running());
}

#[tokio::test]
async fn readiness_timeout_tears_down_started_process() {
    let root = temp_root();
    let mut resolved = base_resolved();
    let mut service = managed("hanging", &["sleep", "30000"], &[], 4096);
    service.readiness = Some(Readiness::Probe {
        probe: name("fake-never"),
        timeout_ms: delay_ms(150),
    });
    resolved.topology = vec![service];
    let mut session = LocalSession::prepare(&resolved, options(root.path())).unwrap();
    let error = session
        .startup(&CancellationToken::new())
        .await
        .unwrap_err();
    let RunnerError::ReadinessTimeout {
        service,
        timeout_ms,
        cleanup,
    } = &error
    else {
        panic!("expected readiness timeout, got {error:?}");
    };
    assert_eq!(service, "hanging");
    assert_eq!(*timeout_ms, 150);
    assert!(cleanup.is_empty(), "single-service teardown must succeed");
    assert!(!session.is_running());
    assert!(session.logs("hanging").await.is_some());
}

#[tokio::test]
async fn child_failure_before_readiness() {
    let root = temp_root();
    let mut resolved = base_resolved();
    let mut service = managed("failing", &["exit-after", "50", "3"], &[], 4096);
    service.readiness = Some(Readiness::Delay {
        after_ms: delay_ms(5000),
    });
    resolved.topology = vec![service];
    let mut session = LocalSession::prepare(&resolved, options(root.path())).unwrap();
    let error = session
        .startup(&CancellationToken::new())
        .await
        .unwrap_err();
    assert!(
        matches!(error, RunnerError::ProcessExitedEarly { .. }),
        "got {error:?}"
    );
    assert!(!session.is_running());
}

#[tokio::test]
async fn cancellation_during_readiness_tears_down() {
    let root = temp_root();
    let mut resolved = base_resolved();
    let mut slow = managed("slow", &["sleep", "30000"], &[], 4096);
    slow.readiness = Some(Readiness::Delay {
        after_ms: delay_ms(30000),
    });
    resolved.topology = vec![managed("fast", &["sleep", "30000"], &[], 4096), slow];
    let mut session = LocalSession::prepare(&resolved, options(root.path())).unwrap();
    let cancel = CancellationToken::new();
    let canceller = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        canceller.cancel();
    });
    let error = session.startup(&cancel).await.unwrap_err();
    assert!(matches!(error, RunnerError::Cancelled { .. }));
    assert!(!session.is_running());
}

#[tokio::test]
async fn cancellation_before_spawn_reports_cleanly() {
    let root = temp_root();
    let mut resolved = base_resolved();
    resolved.topology = vec![managed("app", &["sleep", "30000"], &[], 4096)];
    let mut session = LocalSession::prepare(&resolved, options(root.path())).unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let error = session.startup(&cancel).await.unwrap_err();
    assert!(matches!(error, RunnerError::CancelledBeforeSpawn));
    assert!(session.events().is_empty());
}

#[tokio::test]
async fn graceful_shutdown_path() {
    let root = temp_root();
    let mut resolved = base_resolved();
    let mut service = managed("graceful", &["term-exit"], &[], 4096);
    service.shutdown = Some(Shutdown {
        grace_ms: delay_ms(5000),
        method: None,
    });
    resolved.topology = vec![service];
    let mut session = LocalSession::prepare(&resolved, options(root.path())).unwrap();
    let outcome = session.run(&CancellationToken::new()).await.unwrap();
    assert_eq!(outcome.started, vec!["graceful"]);
    assert!(outcome.cleanup.is_empty());
}

#[tokio::test]
async fn forced_cleanup_after_ignored_termination() {
    let root = temp_root();
    let mut resolved = base_resolved();
    let mut service = managed("stubborn", &["term-ignore"], &[], 4096);
    service.shutdown = Some(Shutdown {
        grace_ms: delay_ms(200),
        method: None,
    });
    resolved.topology = vec![service];
    let mut session = LocalSession::prepare(&resolved, options(root.path())).unwrap();
    let outcome = session.run(&CancellationToken::new()).await.unwrap();
    assert_eq!(outcome.started, vec!["stubborn"]);
    assert!(
        outcome.cleanup.is_empty(),
        "forced SIGKILL cleanup must succeed"
    );
    assert!(!session.is_running());
}

#[cfg(unix)]
#[tokio::test]
async fn descendant_cleanup_reaches_process_group() {
    let root = temp_root();
    let pidfile = root.path().join("grandchild.pid");
    let mut resolved = base_resolved();
    resolved.topology = vec![managed(
        "parent",
        &["descendant", pidfile.to_str().unwrap(), "30000"],
        &[],
        4096,
    )];
    let mut session = LocalSession::prepare(&resolved, options(root.path())).unwrap();
    session.startup(&CancellationToken::new()).await.unwrap();
    let grandchild: u32 = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(text) = std::fs::read_to_string(&pidfile)
                && let Ok(pid) = text.trim().parse()
            {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    assert!(is_process_alive(grandchild));
    let shutdown = session.shutdown().await;
    assert!(shutdown.failures.is_empty());
    tokio::time::timeout(Duration::from_secs(10), async {
        while is_process_alive(grandchild) {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("grandchild must be reaped by process-group cleanup");
}

/// Platform adapter that always fails forced cleanup, to prove the primary
/// failure is preserved when teardown also fails.
#[derive(Debug, Clone, Copy, Default)]
struct FailKillPlatform;

impl PlatformAdapter for FailKillPlatform {
    fn support(&self) -> PlatformSupport {
        PlatformSupport::Supported
    }

    fn label(&self) -> &'static str {
        "test-fail-kill"
    }

    fn is_alive(&self, pid: u32) -> bool {
        is_process_alive(pid)
    }

    fn terminate_group(&self, _pid: u32) -> Result<(), String> {
        Err("injected terminate failure".to_owned())
    }

    fn kill_group(&self, _pid: u32) -> Result<(), String> {
        Err("injected kill failure".to_owned())
    }
}

#[tokio::test]
async fn cleanup_failure_preserves_primary_failure() {
    let root = temp_root();
    let mut resolved = base_resolved();
    let mut steady = managed("steady", &["sleep", "30000"], &[], 4096);
    steady.shutdown = Some(Shutdown {
        grace_ms: delay_ms(150),
        method: None,
    });
    let mut failing = managed("failing", &["exit-after", "50", "3"], &["steady"], 4096);
    failing.readiness = Some(Readiness::Delay {
        after_ms: delay_ms(5000),
    });
    resolved.topology = vec![steady, failing];
    let mut session = LocalSession::prepare(
        &resolved,
        RunnerOptions {
            workspace_root: root.path().to_path_buf(),
            secrets: Arc::new(MapSecretProvider::empty()),
            probes: ProbeRegistry::with_builtins(),
            platform: Arc::new(FailKillPlatform),
        },
    )
    .unwrap();
    let error = session
        .startup(&CancellationToken::new())
        .await
        .unwrap_err();
    let RunnerError::ProcessExitedEarly {
        service, cleanup, ..
    } = &error
    else {
        panic!("expected primary early-exit failure, got {error:?}");
    };
    assert_eq!(service, "failing");
    assert_eq!(cleanup.len(), 1, "steady teardown must be reported");
    assert_eq!(cleanup[0].service, "steady");
    assert!(!session.is_running());
}

#[tokio::test]
async fn bounded_output_truncates_but_keeps_draining() {
    let root = temp_root();
    let mut resolved = base_resolved();
    resolved.topology = vec![managed(
        "noisy",
        &["emit-sleep", "131072", "30000"],
        &[],
        4096,
    )];
    let mut session = LocalSession::prepare(&resolved, options(root.path())).unwrap();
    session.startup(&CancellationToken::new()).await.unwrap();
    // Wait until the drain tasks have observed the full emission; the
    // process stays alive afterwards so shutdown timing cannot truncate it.
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(logs) = session.logs("noisy").await
                && logs.stdout.total_bytes == 131_072
                && logs.stderr.total_bytes == 131_072
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("pipes must be fully drained");
    let shutdown = session.shutdown().await;
    assert!(shutdown.failures.is_empty());
    let logs = session.logs("noisy").await.unwrap();
    assert_eq!(logs.stdout.retained_bytes, 4096);
    assert_eq!(logs.stderr.retained_bytes, 4096);
    assert!(logs.stdout.truncated);
    assert!(logs.stderr.truncated);
    assert_eq!(logs.stdout.total_bytes, 131_072);
    assert_eq!(logs.stderr.total_bytes, 131_072);
    assert_eq!(
        logs.stdout.dropped_bytes,
        131_072 - 4096,
        "draining must continue past the cap"
    );
}

#[tokio::test]
async fn external_services_are_observed_but_never_owned() {
    let root = temp_root();
    let mut external = managed("db", &["sleep", "30000"], &[], 4096);
    external.lifecycle = Lifecycle::External;
    external.kind = ServiceKind::Named {
        service_type: name("postgres"),
    };
    let mut resolved = base_resolved();
    resolved.topology = vec![external, managed("app", &["sleep", "30000"], &["db"], 4096)];
    // Pre-existing externally managed process the runner must not touch.
    let mut dummy = tokio::process::Command::new(fixture())
        .arg("sleep")
        .arg("30000")
        .spawn()
        .unwrap();
    let dummy_pid = dummy.id().unwrap();
    let mut session = LocalSession::prepare(&resolved, options(root.path())).unwrap();
    assert_eq!(session.external_services(), &["db".to_owned()]);
    assert_eq!(session.spawn_order(), vec!["app"]);
    let outcome = session.run(&CancellationToken::new()).await.unwrap();
    assert_eq!(outcome.started, vec!["app"]);
    assert!(
        session.events().iter().all(|event| event.identity != "db"),
        "external services must never appear in lifecycle events"
    );
    assert!(is_process_alive(dummy_pid), "external process left running");
    dummy.kill().await.unwrap();
    dummy.wait().await.unwrap();
}

#[tokio::test]
async fn missing_secret_fails_before_spawn_and_values_stay_redacted() {
    let root = temp_root();
    let secret_value = "super-secret-xyz-12345";
    let mut resolved = base_resolved();
    resolved.subject = Subject::ManagedCommand {
        argv: argv(&["exit", "0"]),
        environment: BTreeMap::from([(
            "HARNESS_TOKEN".to_owned(),
            eggbench_core::SecretRef {
                reference: name("TOKEN"),
            },
        )]),
        revision: None,
        digest: None,
    };
    resolved.topology = vec![managed("app", &["sleep", "30000"], &[], 4096)];

    let error = LocalSession::prepare(&resolved, options(root.path())).unwrap_err();
    assert!(matches!(error, RunnerError::MissingSecret { .. }));

    let provider = MapSecretProvider::with(&name("TOKEN"), secret_value);
    let mut session = LocalSession::prepare(
        &resolved,
        RunnerOptions {
            workspace_root: root.path().to_path_buf(),
            secrets: Arc::new(provider),
            probes: ProbeRegistry::with_builtins(),
            platform: Arc::new(UnixPlatform),
        },
    )
    .unwrap();
    let outcome = session.run(&CancellationToken::new()).await.unwrap();
    assert_eq!(
        outcome.started,
        vec!["subject".to_owned(), "app".to_owned()]
    );

    let debug = format!(
        "{session:?} {outcome:?} {}",
        session.spawn_order().join(",")
    );
    assert!(!debug.contains(secret_value));
    for identity in session.spawn_order() {
        if let Some(logs) = session.logs(&identity).await {
            let stdout = String::from_utf8_lossy(&logs.stdout.data);
            let stderr = String::from_utf8_lossy(&logs.stderr.data);
            assert!(!stdout.contains(secret_value));
            assert!(!stderr.contains(secret_value));
        }
    }
}

fn add_required_artifacts(writer: &mut BundleWriter) {
    use eggbench_core::{ArtifactPath, ArtifactRole, Sensitivity};
    for (path, role) in [
        ("plan.json", ArtifactRole::ExperimentPlan),
        ("resolved-plan.json", ArtifactRole::ResolvedPlan),
        ("environment.json", ArtifactRole::EnvironmentFingerprint),
    ] {
        writer
            .add_artifact(
                ArtifactPath::new(path).unwrap(),
                role,
                "application/json",
                Sensitivity::Redacted,
                &b"{}"[..],
            )
            .unwrap();
    }
}

#[tokio::test]
async fn zero_trial_lifecycle_evidence_has_no_comparison_verdict() {
    let root = temp_root();
    let mut resolved = base_resolved();
    resolved.topology = vec![managed("app", &["emit-stdout", "64"], &[], 4096)];
    let mut session = LocalSession::prepare(&resolved, options(root.path())).unwrap();
    let outcome = session.run(&CancellationToken::new()).await.unwrap();

    let destination = root.path().join("lifecycle.eggb");
    let mut writer = BundleWriter::create(
        &destination,
        RunId::new(),
        ArtifactBounds {
            artifact_count: PositiveCount::new(256).unwrap(),
            artifact_bytes: 16 * 1024 * 1024,
            total_bytes: 64 * 1024 * 1024,
        },
    )
    .unwrap();
    add_required_artifacts(&mut writer);
    let staged_logs = eggbench_runner::stage_lifecycle_logs(&session, &mut writer)
        .await
        .unwrap();
    assert!(!staged_logs.is_empty());
    let metadata =
        eggbench_runner::stage_lifecycle_metadata(&session, &outcome, &mut writer).unwrap();
    let _ = metadata;
    let bundle = writer
        .finalize(
            ExecutionStatus::Completed,
            None,
            Subject::Label {
                label: name("bench"),
            },
            Vec::new(),
            Vec::new(),
            None,
            None,
        )
        .unwrap();
    bundle.verify().unwrap();
    assert_eq!(
        bundle.manifest().execution_status,
        Some(ExecutionStatus::Completed),
        "lifecycle completion is independent from comparison"
    );
    assert_eq!(bundle.manifest().comparison_verdict, None);
    assert_ne!(
        bundle.manifest().comparison_verdict,
        Some(ComparisonVerdict::Pass)
    );
    assert!(bundle.manifest().trials.is_empty());
    let stdout_role = bundle
        .manifest()
        .artifacts
        .iter()
        .find(|artifact| artifact.role == ArtifactRole::Stdout)
        .expect("bounded stdout must be staged");
    assert!(stdout_role.byte_size <= 4096);
}

#[tokio::test]
async fn working_directory_escapes_are_rejected() {
    let root = temp_root();
    for requested in ["../escape", "/tmp", "../../etc"] {
        let mut resolved = base_resolved();
        let mut service = managed("app", &["sleep", "30000"], &[], 4096);
        service.working_directory = Some(requested.to_owned());
        resolved.topology = vec![service];
        let error = LocalSession::prepare(&resolved, options(root.path())).unwrap_err();
        assert!(
            matches!(error, RunnerError::InvalidWorkingDirectory { .. }),
            "got {error:?} for {requested}"
        );
    }
}

#[tokio::test]
async fn unsupported_service_and_platform_fail_explicitly() {
    let root = temp_root();
    let mut resolved = base_resolved();
    resolved.topology = vec![Service {
        name: name("queue"),
        kind: ServiceKind::Named {
            service_type: name("ampq"),
        },
        lifecycle: Lifecycle::Managed,
        depends_on: Vec::new(),
        config: BTreeMap::new(),
        readiness: None,
        shutdown: None,
        working_directory: None,
        log_limit_bytes: 4096,
    }];
    let error = LocalSession::prepare(&resolved, options(root.path())).unwrap_err();
    assert!(matches!(error, RunnerError::UnsupportedService { .. }));

    let mut resolved = base_resolved();
    resolved.topology = vec![managed("app", &["sleep", "30000"], &[], 4096)];
    let error = LocalSession::prepare(
        &resolved,
        RunnerOptions {
            workspace_root: root.path().to_path_buf(),
            secrets: Arc::new(MapSecretProvider::empty()),
            probes: ProbeRegistry::with_builtins(),
            platform: Arc::new(UnsupportedPlatform),
        },
    )
    .unwrap_err();
    assert!(matches!(error, RunnerError::UnsupportedPlatform { .. }));
}

#[tokio::test]
async fn subject_managed_command_spawns_first() {
    let root = temp_root();
    let mut resolved = base_resolved();
    resolved.subject = Subject::ManagedCommand {
        argv: argv(&["sleep", "30000"]),
        environment: BTreeMap::new(),
        revision: None,
        digest: None,
    };
    resolved.topology = vec![managed("app", &["sleep", "30000"], &[], 4096)];
    let mut session = LocalSession::prepare(&resolved, options(root.path())).unwrap();
    assert_eq!(session.spawn_order(), vec!["subject", "app"]);
    let outcome = session.run(&CancellationToken::new()).await.unwrap();
    assert_eq!(outcome.stopped_order, vec!["app", "subject"]);
    assert!(outcome.cleanup.is_empty());
}

#[test]
fn supported_platform_matrix_is_truthful() {
    assert_eq!(
        UnixPlatform.support(),
        if cfg!(unix) {
            PlatformSupport::Supported
        } else {
            PlatformSupport::Unsupported
        }
    );
    assert_eq!(UnsupportedPlatform.support(), PlatformSupport::Unsupported);
}
