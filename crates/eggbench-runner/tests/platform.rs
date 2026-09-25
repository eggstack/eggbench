//! Platform qualification and unsupported-capability behavior.

#[cfg(windows)]
use eggbench_core::{
    ArtifactBounds, Lifecycle, Name, PositiveCount, ResolvedPlan, Service, ServiceKind,
    TrialPolicy, Workload,
};
#[cfg(windows)]
use eggbench_runner::{
    LocalSession, MapSecretProvider, PlatformAdapter, PlatformSupport, ProbeRegistry, RunnerError,
    RunnerOptions, ServiceAdapterRegistry, UnixPlatform,
};
#[cfg(not(windows))]
use eggbench_runner::{PlatformAdapter, PlatformSupport, UnixPlatform};
#[cfg(windows)]
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

#[cfg(windows)]
fn name(value: &str) -> Name {
    Name::new(value).unwrap()
}

#[cfg(windows)]
fn plan() -> ResolvedPlan {
    ResolvedPlan {
        schema_version: eggbench_core::RESOLVED_PLAN_SCHEMA_VERSION,
        source_plan_schema_version: eggbench_core::EXPERIMENT_PLAN_SCHEMA_VERSION,
        experiment: name("platform"),
        drivers: BTreeMap::new(),
        subject: eggbench_core::Subject::Label {
            label: name("bench"),
        },
        topology: vec![Service {
            name: name("managed"),
            kind: ServiceKind::Command {
                argv: vec!["C:/does-not-exist/fixture".to_owned()],
            },
            lifecycle: Lifecycle::Managed,
            depends_on: Vec::new(),
            config: BTreeMap::new(),
            readiness: None,
            shutdown: None,
            working_directory: None,
            log_limit_bytes: 4096,
        }],
        workload: Workload::FiniteCount {
            target: name("app"),
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
        defaults: eggbench_core::ResolvedDefaults {
            platform: name("platform"),
            warmup_trials: 0,
            measured_trials: 1,
        },
        environment_policy: eggbench_core::EnvironmentPolicy::StrictSameTestbed,
        metrics: Vec::new(),
        artifact_bounds: ArtifactBounds {
            artifact_count: PositiveCount::new(16).unwrap(),
            artifact_bytes: 4096,
            total_bytes: 16 * 1024,
        },
        seed: None,
        paired: None,
        network_path: None,
        diagnostics: Vec::new(),
        warnings: Vec::new(),
    }
}

#[cfg(windows)]
fn options(root: PathBuf) -> RunnerOptions {
    RunnerOptions {
        workspace_root: root,
        secrets: Arc::new(MapSecretProvider::empty()),
        probes: ProbeRegistry::with_builtins(),
        platform: Arc::new(UnixPlatform),
        service_adapters: ServiceAdapterRegistry::new(),
    }
}

#[test]
fn platform_capability_matches_the_qualified_matrix() {
    let support = UnixPlatform.support();
    if cfg!(target_os = "linux") || cfg!(target_os = "macos") {
        assert_eq!(support, PlatformSupport::Supported);
    } else if cfg!(unix) {
        assert_eq!(support, PlatformSupport::Unqualified);
    } else {
        assert_eq!(support, PlatformSupport::Unsupported);
    }
}

#[cfg(windows)]
#[test]
fn windows_managed_execution_is_rejected_before_spawn() {
    let temp = tempfile::tempdir().unwrap();
    let error = LocalSession::prepare(&plan(), options(temp.path().to_path_buf())).unwrap_err();
    assert!(matches!(error, RunnerError::UnsupportedPlatform { .. }));
}

#[cfg(not(windows))]
#[test]
fn current_unix_adapter_reports_its_declared_platform_state() {
    assert_ne!(UnixPlatform.support(), PlatformSupport::Unsupported);
}
