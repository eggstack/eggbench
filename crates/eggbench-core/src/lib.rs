//! Dependency-light domain contracts for Eggbench.
#![forbid(unsafe_code)]

mod evidence;
mod metrics;
mod plan;
mod resolved;
mod types;

pub use evidence::*;
pub use metrics::*;
pub use plan::*;
pub use resolved::*;
pub use types::*;

/// Namespace and version for normalized core schemas.
pub const CORE_SCHEMA_NAMESPACE: &str = "org.eggstack.eggbench.core";
/// Current experiment-plan schema version.
pub const EXPERIMENT_PLAN_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);
