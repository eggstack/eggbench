//! Typed runner failures with redaction-safe diagnostics.
//!
//! Secret values never appear in these types. Only references, service names,
//! and probe labels are retained.

use std::fmt;

/// One cleanup problem recorded while tearing down an owned process tree.
///
/// The initiating lifecycle failure is always reported separately; cleanup
/// problems never replace it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupFailure {
    /// Service or subject identity that failed to stop.
    pub service: String,
    /// Stable machine-readable reason.
    pub reason: String,
}

impl CleanupFailure {
    /// Create a cleanup failure record from redaction-safe parts.
    #[must_use]
    pub fn new(service: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            reason: reason.into(),
        }
    }
}

impl fmt::Display for CleanupFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cleanup({}): {}", self.service, self.reason)
    }
}

/// Lifecycle failures for the managed-process runner.
///
/// Every post-spawn variant carries the initiating failure plus the cleanup
/// problems observed while tearing down already-started processes, so callers
/// can preserve the primary cause while still reporting teardown trouble.
/// No variant carries a resolved secret value.
#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
    /// Supplied plan/resolved-plan boundary failed preflight validation.
    #[error("invalid plan: {detail}")]
    InvalidPlan {
        /// Human-readable validation context without secret values.
        detail: String,
    },
    /// A managed service requests a kind no runner adapter implements.
    #[error("unsupported service kind for {service}: {detail}")]
    UnsupportedService {
        /// Service identity.
        service: String,
        /// Capability detail.
        detail: String,
    },
    /// A named readiness probe is not registered with the runner.
    #[error("unsupported readiness probe {probe} for {service}")]
    UnsupportedProbe {
        /// Service identity.
        service: String,
        /// Requested probe label.
        probe: String,
    },
    /// The current platform cannot guarantee descendant cleanup.
    #[error("unsupported platform: {detail}")]
    UnsupportedPlatform {
        /// Capability detail.
        detail: String,
    },
    /// A referenced secret has no injected value.
    #[error("missing secret reference {reference} for {service}")]
    MissingSecret {
        /// Service or subject identity.
        service: String,
        /// Requested reference name, never the value.
        reference: String,
    },
    /// A working directory escapes the workspace root or is otherwise unsafe.
    #[error("invalid working directory for {service}: {detail}")]
    InvalidWorkingDirectory {
        /// Service identity.
        service: String,
        /// Policy detail.
        detail: String,
    },
    /// A command program is missing or depends on implicit PATH lookup.
    #[error("invalid executable path for {service}: {detail}")]
    InvalidExecutablePath {
        /// Service identity.
        service: String,
        /// Redaction-safe resolution policy detail.
        detail: String,
    },
    /// A managed command has no program to spawn.
    #[error("empty argv for {service}")]
    EmptyArgv {
        /// Service identity.
        service: String,
    },
    /// Spawning a managed process failed after earlier processes started.
    #[error("spawn failed for {service}: {message}")]
    SpawnFailed {
        /// Failing service identity.
        service: String,
        /// Redaction-safe cause.
        message: String,
        /// Teardown problems for already-started processes.
        cleanup: Vec<CleanupFailure>,
    },
    /// Readiness failed for a managed process.
    #[error("readiness failed for {service}: {message}")]
    ReadinessFailed {
        /// Failing service identity.
        service: String,
        /// Redaction-safe cause.
        message: String,
        /// Teardown problems for already-started processes.
        cleanup: Vec<CleanupFailure>,
    },
    /// Readiness did not complete within its declared timeout.
    #[error("readiness timeout for {service} after {timeout_ms}ms")]
    ReadinessTimeout {
        /// Failing service identity.
        service: String,
        /// Declared timeout in milliseconds.
        timeout_ms: u64,
        /// Teardown problems for already-started processes.
        cleanup: Vec<CleanupFailure>,
    },
    /// A managed process exited before readiness completed.
    #[error("process exited early for {service}: {message}")]
    ProcessExitedEarly {
        /// Failing service identity.
        service: String,
        /// Redaction-safe exit detail.
        message: String,
        /// Teardown problems for already-started processes.
        cleanup: Vec<CleanupFailure>,
    },
    /// Caller cancellation arrived after at least one process started.
    #[error("cancelled during lifecycle")]
    Cancelled {
        /// Teardown problems for already-started processes.
        cleanup: Vec<CleanupFailure>,
    },
    /// Cancellation arrived before any process started.
    #[error("cancelled before spawn")]
    CancelledBeforeSpawn,
}

impl RunnerError {
    /// Teardown problems attached to this failure, if any.
    #[must_use]
    pub fn cleanup(&self) -> &[CleanupFailure] {
        match self {
            Self::SpawnFailed { cleanup, .. }
            | Self::ReadinessFailed { cleanup, .. }
            | Self::ReadinessTimeout { cleanup, .. }
            | Self::ProcessExitedEarly { cleanup, .. }
            | Self::Cancelled { cleanup } => cleanup,
            Self::InvalidPlan { .. }
            | Self::UnsupportedService { .. }
            | Self::UnsupportedProbe { .. }
            | Self::UnsupportedPlatform { .. }
            | Self::MissingSecret { .. }
            | Self::InvalidWorkingDirectory { .. }
            | Self::InvalidExecutablePath { .. }
            | Self::EmptyArgv { .. }
            | Self::CancelledBeforeSpawn => &[],
        }
    }
}
