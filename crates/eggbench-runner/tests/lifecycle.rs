//! Focused lifecycle evidence for Local Runner M001.
//!
//! Each test maps to a required verification bullet: dependency-ordered
//! startup and reverse teardown, readiness success and named-probe failure,
//! spawn failure and readiness timeout, cancellation, graceful then forced
//! cleanup, descendant cleanup, cleanup-failure preservation, bounded pipe
//! draining, external-service observation, secret handling, and completed
//! lifecycle-only evidence with no comparison verdict.

#![cfg(unix)]

use eggbench_core::{
    ArtifactBounds, ArtifactRole, BundleWriter, ComparisonVerdict, DurationMs, ExecutionStatus,
    Lifecycle, Name, PositiveCount, Readiness, ResolvedPlan, RunId, Service, ServiceKind, Shutdown,
    Subject, TrialPolicy,
};
use eggbench_runner::{
    LifecycleEventKind, LocalSession, MapSecretProvider, PlatformAdapter, PlatformSupport,
    ProbeRegistry, RunnerError, RunnerOptions, ServiceAdapterRegistry, UnixPlatform,
    UnsupportedPlatform, is_process_alive,
};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
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
        paired: None,
        network_path: None,
        diagnostics: Vec::new(),
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
        service_adapters: ServiceAdapterRegistry::new(),
    }
}

fn prepare_options(root: &std::path::Path) -> eggbench_runner::PrepareOptions {
    eggbench_runner::PrepareOptions {
        workspace_root: root.to_path_buf(),
        secrets: Arc::new(MapSecretProvider::empty()),
        platform: Arc::new(UnixPlatform),
        service_adapters: ServiceAdapterRegistry::new(),
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
    let non_executable = root.path().join("non-executable");
    std::fs::write(&non_executable, b"not executable").unwrap();
    std::fs::set_permissions(&non_executable, std::fs::Permissions::from_mode(0o600)).unwrap();
    resolved.topology = vec![Service {
        name: name("broken"),
        kind: ServiceKind::Command {
            argv: vec![non_executable.to_string_lossy().into_owned()],
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
            service_adapters: ServiceAdapterRegistry::new(),
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
        argv: argv(&["sleep", "30000"]),
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
            service_adapters: ServiceAdapterRegistry::new(),
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

#[tokio::test]
#[allow(clippy::too_many_lines)] // Exercises the environment boundary through finalized evidence.
async fn managed_environment_is_hermetic_and_explicit_secrets_stay_redacted() {
    const PARENT_SENTINEL: &str = "EGGBENCH_PARENT_SENTINEL_26CE";
    assert!(
        std::env::var_os(PARENT_SENTINEL).is_some(),
        "CI/test command must set the parent sentinel"
    );

    let root = temp_root();
    let secret_value = "secret-environment-probe-value-73b4";
    let mut resolved = base_resolved();
    resolved.subject = Subject::ManagedCommand {
        argv: argv(&["has-env", "EGGBENCH_SUBJECT_TOKEN", "30000"]),
        environment: BTreeMap::from([(
            "EGGBENCH_SUBJECT_TOKEN".to_owned(),
            eggbench_core::SecretRef {
                reference: name("TOKEN"),
            },
        )]),
        revision: None,
        digest: None,
    };
    let mut app = managed("app", &["has-env", PARENT_SENTINEL, "30000"], &[], 4096);
    app.readiness = Some(Readiness::Delay {
        after_ms: delay_ms(100),
    });
    let mut configured = managed(
        "config-check",
        &["has-env", "EGGBENCH_CONFIG_SENTINEL", "30000"],
        &[],
        4096,
    );
    configured.config.insert(
        "EGGBENCH_CONFIG_SENTINEL".to_owned(),
        "from-config".to_owned(),
    );
    configured.readiness = Some(Readiness::Delay {
        after_ms: delay_ms(100),
    });
    resolved.topology = vec![app, configured];
    let provider = MapSecretProvider::with(&name("TOKEN"), secret_value);
    let mut session = LocalSession::prepare(
        &resolved,
        RunnerOptions {
            workspace_root: root.path().to_path_buf(),
            secrets: Arc::new(provider),
            probes: ProbeRegistry::with_builtins(),
            platform: Arc::new(UnixPlatform),
            service_adapters: ServiceAdapterRegistry::new(),
        },
    )
    .unwrap();
    let debug = format!("{session:?}");
    assert!(!debug.contains(secret_value));
    assert!(debug.contains("[REDACTED]"));
    let outcome = session.run(&CancellationToken::new()).await.unwrap();
    assert_eq!(outcome.started, vec!["subject", "app", "config-check"]);
    assert_eq!(
        session.logs("subject").await.unwrap().stdout.data,
        b"present\n"
    );
    assert_eq!(session.logs("app").await.unwrap().stdout.data, b"absent\n");
    assert_eq!(
        session.logs("config-check").await.unwrap().stdout.data,
        b"absent\n",
        "opaque service config is not copied into the child environment"
    );

    let destination = root.path().join("secret-lifecycle.eggb");
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
    eggbench_runner::stage_lifecycle_logs(&session, &mut writer)
        .await
        .unwrap();
    eggbench_runner::stage_lifecycle_metadata(&session, &outcome, &mut writer).unwrap();
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
    let manifest = serde_json::to_string(bundle.manifest()).unwrap();
    assert!(!manifest.contains(secret_value));
    assert!(
        !format!(
            "{:?}",
            RunnerError::MissingSecret {
                service: "subject".to_owned(),
                reference: "TOKEN".to_owned()
            }
        )
        .contains(secret_value)
    );
    for record in &bundle.manifest().artifacts {
        let bytes = std::io::Read::bytes(bundle.open_artifact(&record.path).unwrap())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains(secret_value));
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

#[test]
fn workspace_and_cwd_are_resolved_through_the_filesystem() {
    let root = temp_root();
    std::fs::create_dir(root.path().join("real")).unwrap();
    std::fs::create_dir(root.path().join("nested")).unwrap();
    #[cfg(unix)]
    {
        let mut direct_resolved = base_resolved();
        let mut direct_service = managed("direct", &["sleep", "30000"], &[], 4096);
        direct_service.working_directory = Some("real".to_owned());
        direct_resolved.topology = vec![direct_service];
        let direct_plan =
            eggbench_runner::prepare(&direct_resolved, &prepare_options(root.path())).unwrap();
        assert_eq!(
            direct_plan.specs[0].cwd,
            root.path().join("real").canonicalize().unwrap()
        );

        std::os::unix::fs::symlink(root.path().join("real"), root.path().join("inside-link"))
            .unwrap();
        let outside = temp_root();
        std::os::unix::fs::symlink(outside.path(), root.path().join("outside-link")).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("nested/outside-link"))
            .unwrap();

        let mut resolved = base_resolved();
        let mut service = managed("app", &["sleep", "30000"], &[], 4096);
        service.working_directory = Some("inside-link".to_owned());
        resolved.topology = vec![service];
        let plan = eggbench_runner::prepare(&resolved, &prepare_options(root.path())).unwrap();
        assert_eq!(
            plan.specs[0].cwd,
            root.path().join("real").canonicalize().unwrap()
        );

        for requested in ["outside-link", "nested/outside-link"] {
            let mut resolved = base_resolved();
            let mut service = managed("app", &["sleep", "30000"], &[], 4096);
            service.working_directory = Some(requested.to_owned());
            resolved.topology = vec![service];
            assert!(matches!(
                eggbench_runner::prepare(&resolved, &prepare_options(root.path())),
                Err(RunnerError::InvalidWorkingDirectory { .. })
            ));
        }
    }

    let file = root.path().join("not-a-directory");
    std::fs::write(&file, b"file").unwrap();
    for requested in ["missing", "not-a-directory"] {
        let mut resolved = base_resolved();
        let mut service = managed("app", &["sleep", "30000"], &[], 4096);
        service.working_directory = Some(requested.to_owned());
        resolved.topology = vec![service];
        assert!(matches!(
            eggbench_runner::prepare(&resolved, &prepare_options(root.path())),
            Err(RunnerError::InvalidWorkingDirectory { .. })
        ));
    }
}

#[test]
fn workspace_root_must_exist_and_be_a_directory() {
    let root = temp_root();
    let missing = root.path().join("missing-root");
    assert!(matches!(
        eggbench_runner::prepare(&base_resolved(), &prepare_options(&missing)),
        Err(RunnerError::InvalidWorkingDirectory { .. })
    ));
    let file = root.path().join("root-file");
    std::fs::write(&file, b"file").unwrap();
    assert!(matches!(
        eggbench_runner::prepare(&base_resolved(), &prepare_options(&file)),
        Err(RunnerError::InvalidWorkingDirectory { .. })
    ));
}

#[tokio::test]
async fn explicit_executable_paths_are_resolved_and_bare_names_rejected() {
    let root = temp_root();
    let fixture_path = std::path::PathBuf::from(fixture());
    let bin = root.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let relative_fixture = bin.join("fixture");
    std::fs::copy(&fixture_path, &relative_fixture).unwrap();

    let mut resolved = base_resolved();
    let mut relative = Service {
        name: name("relative"),
        kind: ServiceKind::Command {
            argv: vec![
                "bin/fixture".to_owned(),
                "emit-sleep".to_owned(),
                "1".to_owned(),
                "30000".to_owned(),
            ],
        },
        lifecycle: Lifecycle::Managed,
        depends_on: Vec::new(),
        config: BTreeMap::new(),
        readiness: None,
        shutdown: None,
        working_directory: None,
        log_limit_bytes: 4096,
    };
    relative.readiness = Some(Readiness::Delay {
        after_ms: delay_ms(100),
    });
    resolved.topology = vec![relative];
    let plan = eggbench_runner::prepare(&resolved, &prepare_options(root.path())).unwrap();
    assert_eq!(
        plan.specs[0].argv[0],
        relative_fixture.canonicalize().unwrap().to_string_lossy()
    );
    let mut session = LocalSession::prepare(&resolved, options(root.path())).unwrap();
    session.run(&CancellationToken::new()).await.unwrap();
    assert_eq!(session.logs("relative").await.unwrap().stdout.data, b"o");

    for program in ["eggbench-child-fixture", "missing/program", "bin/."] {
        let mut resolved = base_resolved();
        resolved.topology = vec![Service {
            name: name("invalid-program"),
            kind: ServiceKind::Command {
                argv: vec![program.to_owned()],
            },
            lifecycle: Lifecycle::Managed,
            depends_on: Vec::new(),
            config: BTreeMap::new(),
            readiness: None,
            shutdown: None,
            working_directory: None,
            log_limit_bytes: 4096,
        }];
        assert!(
            matches!(
                eggbench_runner::prepare(&resolved, &prepare_options(root.path())),
                Err(RunnerError::InvalidExecutablePath { .. })
            ),
            "program {program} should be rejected during preflight"
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
            service_adapters: ServiceAdapterRegistry::new(),
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
        if cfg!(target_os = "linux") || cfg!(target_os = "macos") {
            PlatformSupport::Supported
        } else if cfg!(unix) {
            PlatformSupport::Unqualified
        } else {
            PlatformSupport::Unsupported
        }
    );
    assert_eq!(UnsupportedPlatform.support(), PlatformSupport::Unsupported);
}

// ---- Named in-process service adapters (generic seam) ----

use eggbench_runner::{
    BoxFuture, ManagedServiceAdapter, ManagedServiceHandle, RuntimeBindings, ServiceStartRequest,
};
use tokio::sync::Mutex as AsyncMutex;

fn named(service: &str, service_type: &str, depends_on: &[&str]) -> Service {
    Service {
        name: name(service),
        kind: ServiceKind::Named {
            service_type: name(service_type),
        },
        lifecycle: Lifecycle::Managed,
        depends_on: depends_on.iter().map(|dep| name(dep)).collect(),
        config: BTreeMap::new(),
        readiness: None,
        shutdown: None,
        working_directory: None,
        log_limit_bytes: 4096,
    }
}

/// Deterministic test adapter recording start/stop order.
#[derive(Debug)]
struct FakeAdapter {
    service_type: &'static str,
    events: Arc<AsyncMutex<Vec<String>>>,
    shutdown_error: Option<String>,
    bindings: BTreeMap<String, String>,
}

impl FakeAdapter {
    fn new(service_type: &'static str, events: Arc<AsyncMutex<Vec<String>>>) -> Self {
        Self {
            service_type,
            events,
            shutdown_error: None,
            bindings: BTreeMap::from([(
                "http_url".to_owned(),
                "http://127.0.0.1:9/bench".to_owned(),
            )]),
        }
    }
}

struct FakeHandle {
    identity: String,
    events: Arc<AsyncMutex<Vec<String>>>,
    shutdown_error: Option<String>,
    bindings: RuntimeBindings,
}

impl ManagedServiceAdapter for FakeAdapter {
    fn service_type(&self) -> &str {
        self.service_type
    }

    fn start(
        &self,
        request: ServiceStartRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<Box<dyn ManagedServiceHandle>, String>> {
        Box::pin(async move {
            if cancel.is_cancelled() {
                return Err("cancelled".to_owned());
            }
            self.events
                .lock()
                .await
                .push(format!("start:{}", request.service));
            let mut bindings = RuntimeBindings::new();
            for (key, value) in &self.bindings {
                bindings
                    .insert(&request.service, key, value.clone())
                    .expect("test bindings are valid");
            }
            Ok(Box::new(FakeHandle {
                identity: request.service,
                events: Arc::clone(&self.events),
                shutdown_error: self.shutdown_error.clone(),
                bindings,
            }) as Box<dyn ManagedServiceHandle>)
        })
    }
}

impl ManagedServiceHandle for FakeHandle {
    fn bindings(&self) -> RuntimeBindings {
        self.bindings.clone()
    }

    fn shutdown(&mut self, _grace: Duration) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(async move {
            self.events
                .lock()
                .await
                .push(format!("stop:{}", self.identity));
            match &self.shutdown_error {
                Some(reason) => Err(reason.clone()),
                None => Ok(()),
            }
        })
    }
}

fn options_with(
    root: &std::path::Path,
    adapter: FakeAdapter,
) -> (RunnerOptions, Arc<AsyncMutex<Vec<String>>>) {
    let events = Arc::clone(&adapter.events);
    let mut registry = ServiceAdapterRegistry::new();
    registry.register(Arc::new(adapter)).expect("test adapter");
    (
        RunnerOptions {
            workspace_root: root.to_path_buf(),
            secrets: Arc::new(MapSecretProvider::empty()),
            probes: ProbeRegistry::with_builtins(),
            platform: Arc::new(UnixPlatform),
            service_adapters: registry,
        },
        events,
    )
}

#[tokio::test]
async fn mixed_services_start_in_dependency_order_and_stop_in_reverse() {
    let root = temp_root();
    let events = Arc::new(AsyncMutex::new(Vec::new()));
    let (runner_options, events) = options_with(
        root.path(),
        FakeAdapter::new("fake-svc", Arc::clone(&events)),
    );
    let mut resolved = base_resolved();
    // Adapter service depends on the command process: unified order applies.
    resolved.topology = vec![
        managed("app", &["sleep", "30000"], &[], 4096),
        named("origin", "fake-svc", &["app"]),
    ];
    let mut session = LocalSession::prepare(&resolved, runner_options).unwrap();
    assert_eq!(session.spawn_order(), vec!["app", "origin"]);
    let report = session.startup(&CancellationToken::new()).await.unwrap();
    assert_eq!(report.started, vec!["app", "origin"]);
    // Adapter bindings are visible after readiness and survive for evidence.
    assert_eq!(
        session.runtime_bindings().get("origin", "http_url"),
        Some("http://127.0.0.1:9/bench")
    );
    // Adapter services own no process logs.
    assert!(session.logs("origin").await.is_none());
    let shutdown = session.shutdown().await;
    assert_eq!(shutdown.stopped_order, vec!["origin", "app"]);
    assert!(shutdown.failures.is_empty());
    assert_eq!(
        events.lock().await.as_slice(),
        ["start:origin", "stop:origin"]
    );
    // Bindings remain available through teardown for final evidence.
    assert_eq!(
        session.runtime_bindings().get("origin", "http_url"),
        Some("http://127.0.0.1:9/bench")
    );
    let topology = session.runtime_topology();
    assert_eq!(topology.services.len(), 2);
    let entry = topology
        .services
        .iter()
        .find(|entry| entry.identity == "origin")
        .expect("origin entry");
    assert_eq!(entry.ownership, eggbench_runner::ServiceOwnership::Adapter);
    assert_eq!(entry.service_type.as_deref(), Some("fake-svc"));
    assert_eq!(
        entry.bindings.get("http_url").map(String::as_str),
        Some("http://127.0.0.1:9/bench")
    );
}

#[tokio::test]
async fn adapter_probe_readiness_is_rejected_and_service_is_torn_down() {
    let root = temp_root();
    let events = Arc::new(AsyncMutex::new(Vec::new()));
    let (runner_options, events) = options_with(
        root.path(),
        FakeAdapter::new("fake-svc", Arc::clone(&events)),
    );
    let mut resolved = base_resolved();
    let mut service = named("origin", "fake-svc", &[]);
    service.readiness = Some(Readiness::Probe {
        probe: name("process-alive"),
        timeout_ms: DurationMs::new(1000).unwrap(),
    });
    resolved.topology = vec![service];
    let mut session = LocalSession::prepare(&resolved, runner_options).unwrap();
    let error = session
        .startup(&CancellationToken::new())
        .await
        .unwrap_err();
    // Plan-level probes on in-process services are rejected: no PID exists.
    assert!(matches!(error, RunnerError::UnsupportedProbe { .. }));
    // The started adapter was still torn down in reverse order.
    assert_eq!(
        events.lock().await.as_slice(),
        ["start:origin", "stop:origin"]
    );
    let shutdown = session.shutdown().await;
    assert!(shutdown.stopped_order.is_empty());
    assert!(shutdown.failures.is_empty());
}

#[tokio::test]
async fn adapter_shutdown_failure_becomes_cleanup_evidence() {
    let root = temp_root();
    let events = Arc::new(AsyncMutex::new(Vec::new()));
    let mut adapter = FakeAdapter::new("fake-svc", Arc::clone(&events));
    adapter.shutdown_error = Some("fake stop failed".to_owned());
    let (runner_options, _) = options_with(root.path(), adapter);
    let mut resolved = base_resolved();
    resolved.topology = vec![named("origin", "fake-svc", &[])];
    let mut session = LocalSession::prepare(&resolved, runner_options).unwrap();
    session.startup(&CancellationToken::new()).await.unwrap();
    let shutdown = session.shutdown().await;
    assert_eq!(shutdown.stopped_order, vec!["origin"]);
    assert_eq!(shutdown.failures.len(), 1);
    assert_eq!(shutdown.failures[0].service, "origin");
    assert_eq!(shutdown.failures[0].reason, "fake stop failed");
}
