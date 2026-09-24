//! Stable redaction-safe driver error taxonomy.

use thiserror::Error;

/// Stable machine-readable error category.
///
/// Categories form the compatibility surface; human detail may change but
/// category identities must remain stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCategory {
    /// Requested binary was not found.
    BinaryNotFound,
    /// Search path contained an untrusted component.
    UntrustedSearchPath,
    /// Selected file is not executable.
    NotExecutable,
    /// Executable identity (canonicalization/hash) failed.
    ExecutableIdentityFailed,
    /// Version probe timed out.
    VersionProbeTimeout,
    /// Version probe failed.
    VersionProbeFailed,
    /// Tool version is unsupported by the adapter policy.
    UnsupportedVersion,
    /// Plan workload or option has no honest mapping to tool flags.
    UnsupportedOption,
    /// Process spawn failed.
    SpawnFailed,
    /// Execution was cancelled.
    Cancelled,
    /// Execution timed out.
    TimedOut,
    /// Tool exited nonzero.
    NonzeroExit,
    /// Output exceeded the configured bound.
    OutputTruncated,
    /// Output parsing failed.
    ParseFailed,
    /// Cleanup after execution failed.
    CleanupFailed,
}

impl ErrorCategory {
    /// Stable `snake_case` label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BinaryNotFound => "binary_not_found",
            Self::UntrustedSearchPath => "untrusted_search_path",
            Self::NotExecutable => "not_executable",
            Self::ExecutableIdentityFailed => "executable_identity_failed",
            Self::VersionProbeTimeout => "version_probe_timeout",
            Self::VersionProbeFailed => "version_probe_failed",
            Self::UnsupportedVersion => "unsupported_version",
            Self::UnsupportedOption => "unsupported_option",
            Self::SpawnFailed => "spawn_failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::NonzeroExit => "nonzero_exit",
            Self::OutputTruncated => "output_truncated",
            Self::ParseFailed => "parse_failed",
            Self::CleanupFailed => "cleanup_failed",
        }
    }
}

impl std::fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Typed driver-substrate error.
///
/// Detail strings are human-readable and redaction-safe: they never contain
/// secret-bearing argv or environment values.
#[derive(Debug, Error)]
pub enum DriverError {
    /// Binary resolution or identity failure.
    #[error("{category}: {detail}")]
    Resolution {
        /// Stable category.
        category: ErrorCategory,
        /// Redaction-safe context.
        detail: String,
    },
    /// Version probe failure.
    #[error("{category}: {detail}")]
    Probe {
        /// Stable category.
        category: ErrorCategory,
        /// Redaction-safe context.
        detail: String,
    },
    /// Command execution failure.
    #[error("{category}: {detail}")]
    Execution {
        /// Stable category.
        category: ErrorCategory,
        /// Redaction-safe context.
        detail: String,
    },
    /// Output parsing failure.
    #[error("{category}: {detail}")]
    Parse {
        /// Stable category.
        category: ErrorCategory,
        /// Redaction-safe context.
        detail: String,
    },
}

impl DriverError {
    /// Stable machine category for this error.
    #[must_use]
    pub const fn category(&self) -> ErrorCategory {
        match self {
            Self::Resolution { category, .. }
            | Self::Probe { category, .. }
            | Self::Execution { category, .. }
            | Self::Parse { category, .. } => *category,
        }
    }

    /// Construct a resolution error.
    pub fn resolution(category: ErrorCategory, detail: impl Into<String>) -> Self {
        Self::Resolution {
            category,
            detail: detail.into(),
        }
    }

    /// Construct a probe error.
    pub fn probe(category: ErrorCategory, detail: impl Into<String>) -> Self {
        Self::Probe {
            category,
            detail: detail.into(),
        }
    }

    /// Construct an execution error.
    pub fn execution(category: ErrorCategory, detail: impl Into<String>) -> Self {
        Self::Execution {
            category,
            detail: detail.into(),
        }
    }

    /// Construct a parse error.
    pub fn parse(category: ErrorCategory, detail: impl Into<String>) -> Self {
        Self::Parse {
            category,
            detail: detail.into(),
        }
    }
}
