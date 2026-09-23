//! CLI failure types and conversion to [`CliFailure`].

pub use crate::envelope::CliFailure;
use crate::envelope::ExitCode;
use eggbench_core::{BundleError, PlanError, ResolveError};
use thiserror::Error;

/// Top-level CLI error type.
#[derive(Debug, Error)]
pub enum CliError {
    /// Plan input was missing or unreadable.
    #[error("plan input I/O failure: {0}")]
    PlanIo(String),
    /// Plan input format was unsupported or unrecognized.
    #[error("unsupported plan input format: {0}")]
    PlanFormat(String),
    /// Plan validation failed.
    #[error("plan validation failure: {0}")]
    PlanValidation(#[from] PlanError),
    /// Plan resolution failed.
    #[error("plan resolution failure: {0}")]
    Resolution(#[from] ResolveError),
    /// Bundle I/O or verification failed.
    #[error("bundle failure: {0}")]
    Bundle(#[from] BundleError),
    /// Internal/unclassified CLI failure.
    #[error("internal CLI failure: {0}")]
    Internal(String),
}

impl CliError {
    /// Convert into a stable [`CliFailure`].
    #[must_use]
    pub fn into_failure(self) -> CliFailure {
        match self {
            Self::PlanIo(_) => {
                CliFailure::new("plan_io", self.to_string(), ExitCode::ParseValidation)
            }
            Self::PlanFormat(_) => {
                CliFailure::new("plan_format", self.to_string(), ExitCode::ParseValidation)
            }
            Self::PlanValidation(_) => CliFailure::new(
                "plan_validation",
                self.to_string(),
                ExitCode::ParseValidation,
            ),
            Self::Resolution(error) => {
                let category = match &error {
                    ResolveError::InvalidPlan(_) => "plan_validation",
                    ResolveError::MissingDriver { .. } => "missing_driver",
                    ResolveError::CategoryMismatch { .. } => "category_mismatch",
                    ResolveError::AmbiguousSelection { .. } => "ambiguous_selection",
                    ResolveError::UnsupportedCapability { .. } => "unsupported_capability",
                    ResolveError::UnsupportedPlatform { .. } => "unsupported_platform",
                    ResolveError::MissingExecutablePath(_) => "missing_executable_path",
                    ResolveError::IncompatibleService { .. } => "incompatible_service",
                    ResolveError::DuplicateDriver(_) => "duplicate_driver",
                };
                CliFailure::new(category, error.to_string(), ExitCode::CapabilityPreflight)
            }
            Self::Bundle(_) => CliFailure::new("bundle", self.to_string(), ExitCode::EvidenceIo),
            Self::Internal(_) => CliFailure::new("internal", self.to_string(), ExitCode::Internal),
        }
    }
}
