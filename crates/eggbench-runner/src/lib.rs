//! Managed local process lifecycle for Eggbench.
//!
//! `eggbench-runner` owns runtime side effects behind the runtime-free
//! [`eggbench_core`] contracts: argv-direct spawning with no shell,
//! dependency-ordered startup, readiness bounded by declared timeouts,
//! continuous bounded stdout/stderr spooling, graceful shutdown with forced
//! process-tree cleanup, and reverse-order teardown that preserves the
//! initiating failure.
//!
//! Supported platforms advertise tested descendant cleanup (Linux and
//! qualified macOS via process groups). Other Unix targets remain unqualified;
//! Windows managed execution reports an unsupported-capability error.
//!
//! This milestone performs no load generation, trial scheduling,
//! measurement, or comparison. A lifecycle-only run records no trials and a
//! finalized zero-trial bundle records completed execution and no comparison.
//!
//! [`eggbench_core`]: ../../eggbench-core/index.html

#![forbid(unsafe_code)]

mod bundle;
mod error;
mod platform;
mod probe;
mod secret;
mod session;
mod spec;

pub use bundle::{stage_lifecycle_logs, stage_lifecycle_metadata};
pub use error::{CleanupFailure, RunnerError};
pub use platform::{
    PlatformAdapter, PlatformSupport, UnixPlatform, UnsupportedPlatform, is_process_alive,
};
pub use probe::{
    FAKE_FAIL_PROBE, FAKE_NEVER_PROBE, FAKE_OK_PROBE, FakeFailProbe, FakeNeverProbe, FakeOkProbe,
    PROCESS_ALIVE_PROBE, ProbeContext, ProbeFailure, ProbeRegistry, ProcessAliveProbe,
    ReadinessProbe,
};
pub use secret::{MapSecretProvider, SecretProvider};
pub use session::{
    BoundedOutput, LifecycleEvent, LifecycleEventKind, LifecycleOutcome, LocalSession,
    RunnerOptions, ServiceLogs, ShutdownReport, StartupReport,
};
pub use spec::{
    DEFAULT_GRACE_MS, DEFAULT_SUBJECT_LOG_LIMIT_BYTES, PrepareOptions, ProcessSpec,
    SUBJECT_IDENTITY, SpawnPlan, prepare,
};
