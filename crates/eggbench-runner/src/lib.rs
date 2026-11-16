//! Local process lifecycle and trial orchestration over `eggbench-core` contracts.
//!
//! Managed startup has one cleanup boundary: workload drain and service
//! teardown are attempted, with cleanup failures recorded separately from the
//! primary error. Failed evidence publication never yields a finalized bundle.

#![forbid(unsafe_code)]

mod bundle;
mod correctness;
mod diagnostics;
mod environment;
mod error;
pub(crate) mod orchestration;
mod platform;
mod prepare;
mod probe;
mod secret;
mod service;
mod session;
mod spec;
mod subject;
mod telemetry;

pub use bundle::{
    stage_lifecycle_logs, stage_lifecycle_metadata, stage_run_evidence, stage_runtime_topology,
};
pub use correctness::{
    CorrectnessContext, CorrectnessDisposition, CorrectnessExecutionRecord, CorrectnessExecutor,
    CorrectnessOutput, CorrectnessRegistry, FakeCorrectnessExecutor, correctness_timing_label,
    security_operation_label, security_role_label,
};
pub use diagnostics::{
    DiagnosticContext, DiagnosticDisposition, DiagnosticExecutionRecord, DiagnosticExecutor,
    DiagnosticOutput, DiagnosticRegistry, DiagnosticsIndex, FakeDiagnosticExecutor,
    diagnostics_role_label,
};
pub use environment::{EnvironmentError, LocalEnvironmentCollector};
pub use error::{CleanupFailure, RunnerError};
pub use orchestration::{
    DrainContext, FailureCategory, InvocationContext, InvocationKind, MeasurementSignal,
    OrchestrationError, PhaseEvent, PhaseKind, PhaseOutcome, ResetContext, ResetHook,
    ResetRegistry, RunEvidenceArtifact, RunEvidenceContract, RunOutcome, WorkloadArtifact,
    WorkloadExecutor, WorkloadOutput, execute_run, execute_run_with_diagnostics,
};
pub use platform::{
    PlatformAdapter, PlatformSupport, UnixPlatform, UnsupportedPlatform, is_process_alive,
};
pub use probe::{
    FAKE_FAIL_PROBE, FAKE_NEVER_PROBE, FAKE_OK_PROBE, FakeFailProbe, FakeNeverProbe, FakeOkProbe,
    PROCESS_ALIVE_PROBE, ProbeContext, ProbeFailure, ProbeRegistry, ProcessAliveProbe,
    ReadinessProbe, TCP_LOOPBACK_PROBE, TcpLoopbackProbe,
};
pub use secret::{MapSecretProvider, SecretProvider};
pub use service::{
    BindingConsumption, BoxFuture, ManagedServiceAdapter, ManagedServiceHandle,
    RUNTIME_TOPOLOGY_SCHEMA_VERSION, RuntimeBindings, RuntimeTopology, ServiceAdapterRegistry,
    ServiceOwnership, ServiceStartRequest, ServiceTopologyEntry,
};
pub use telemetry::{
    FakeTelemetryCollector, FakeTelemetryHandle, MAX_TELEMETRY_DETAIL_LEN, TelemetryCapability,
    TelemetryCollector, TelemetryError, TelemetryFuture, TelemetryOutput,
    TelemetryPreflightContext, TelemetryPreflightTiming, TelemetryRegistry, TelemetryTrialContext,
};

/// Deterministic adapters for runner integration tests and qualification.
pub mod test_support {
    /// Fake one-shot correctness executor with canned pass/fail outcomes.
    pub use crate::correctness::FakeCorrectnessExecutor;
    /// Fake one-shot diagnostic executor with canned outcomes.
    pub use crate::diagnostics::FakeDiagnosticExecutor;
    /// Fake workload with configurable delay, failure, timeout, and drain behavior.
    pub use crate::orchestration::FakeWorkload;
    /// Fake telemetry collector recording preflight/start/stop/drain calls.
    pub use crate::telemetry::{FakeTelemetryCollector, FakeTelemetryHandle};
}
pub use prepare::{
    BundlePreparation, PrepareError, SubjectSnapshotError, build_subject_snapshot,
    collect_local_environment, prepare_bundle,
};
pub use session::{
    BoundedOutput, LifecycleEvent, LifecycleEventKind, LifecycleOutcome, LocalSession,
    RunnerOptions, ServiceLogs, ShutdownReport, StartupReport,
};
pub use spec::{
    AdapterSpec, DEFAULT_GRACE_MS, DEFAULT_SUBJECT_LOG_LIMIT_BYTES, LaunchEntry, LaunchKind,
    PrepareOptions, ProcessSpec, SUBJECT_IDENTITY, SpawnPlan, prepare,
};
pub use subject::{MAX_SUBJECT_SNAPSHOT_BYTES, SUBJECT_SNAPSHOT_SCHEMA_VERSION, SubjectSnapshot};
