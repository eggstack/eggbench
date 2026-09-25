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
    /// Baseline alias file was invalid or failed to resolve.
    #[error("baseline alias failure [{category}]: {detail}")]
    BaselineAlias {
        /// Stable machine-readable category.
        category: String,
        /// Human-readable context.
        detail: String,
    },
    /// Internal/unclassified CLI failure.
    #[error("internal CLI failure: {0}")]
    Internal(String),
}

fn is_network_path_category(category: &str) -> bool {
    matches!(
        category,
        "missing_fault_seed"
            | "unsupported_network_path"
            | "route_credentials_not_supported"
            | "workload_path_incompatible"
            | "paired_network_path_not_supported"
            | "unsupported_route"
            | "invalid_route"
            | "invalid_fault_plan"
    )
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
            Self::PlanValidation(error) => {
                let raw_category = match &error {
                    PlanError::Parse { .. } => "plan_parse",
                    PlanError::UnsupportedVersion(_) => "unsupported_schema_version",
                    PlanError::Validation { category, .. } => *category,
                };
                let network_category = is_network_path_category(raw_category);
                let category = if network_category {
                    raw_category
                } else {
                    "plan_validation"
                };
                let exit_code = if network_category {
                    ExitCode::CapabilityPreflight
                } else {
                    ExitCode::ParseValidation
                };
                CliFailure::new(category, error.to_string(), exit_code)
            }
            Self::Resolution(error) => {
                let category = match &error {
                    ResolveError::InvalidPlan(PlanError::Validation {
                        category: validation_category,
                        ..
                    }) if is_network_path_category(validation_category) => *validation_category,
                    ResolveError::InvalidPlan(_) => "plan_validation",
                    ResolveError::MissingDriver {
                        category: eggbench_core::DriverCategory::Route,
                    } => "missing_route_driver",
                    ResolveError::MissingDriver {
                        category: eggbench_core::DriverCategory::Fault,
                    } => "missing_fault_driver",
                    ResolveError::MissingDriver { .. } => "missing_driver",
                    ResolveError::CategoryMismatch { .. } => "category_mismatch",
                    ResolveError::AmbiguousSelection { .. } => "ambiguous_selection",
                    ResolveError::UnsupportedCapability {
                        capability: eggbench_core::Capability::NetworkPath,
                        ..
                    } => "unsupported_network_path",
                    ResolveError::UnsupportedCapability {
                        capability: eggbench_core::Capability::StreamFaultPlan,
                        ..
                    } => "unsupported_stream_fault_plan",
                    ResolveError::UnsupportedCapability { .. } => "unsupported_capability",
                    ResolveError::UnsupportedPlatform { .. } => "unsupported_platform",
                    ResolveError::MissingExecutablePath(_) => "missing_executable_path",
                    ResolveError::IncompatibleService { .. } => "incompatible_service",
                    ResolveError::DuplicateDriver(_) => "duplicate_driver",
                };
                CliFailure::new(category, error.to_string(), ExitCode::CapabilityPreflight)
            }
            Self::Bundle(_) => CliFailure::new("bundle", self.to_string(), ExitCode::EvidenceIo),
            Self::BaselineAlias { category, detail } => CliFailure::new(
                category,
                format!("baseline alias failure: {detail}"),
                ExitCode::ParseValidation,
            ),
            Self::Internal(_) => CliFailure::new("internal", self.to_string(), ExitCode::Internal),
        }
    }
}
