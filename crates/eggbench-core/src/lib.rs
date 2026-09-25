//! Dependency-light domain contracts for Eggbench.
#![forbid(unsafe_code)]

mod comparison;
mod evidence;
mod metrics;
mod network_path;
mod plan;
mod resolved;
mod types;

pub use comparison::*;
pub use evidence::*;
pub use metrics::*;
pub use network_path::*;
pub use plan::*;
pub use resolved::*;
pub use types::*;

/// Namespace and version for normalized core schemas.
pub const CORE_SCHEMA_NAMESPACE: &str = "org.eggstack.eggbench.core";
/// Current experiment-plan schema version.
pub const EXPERIMENT_PLAN_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);
/// Paired-design experiment-plan schema version (adds `paired`).
pub const EXPERIMENT_PLAN_SCHEMA_VERSION_2: SchemaVersion = SchemaVersion(2);
/// Network-path experiment-plan schema version (adds `network_path`).
pub const EXPERIMENT_PLAN_SCHEMA_VERSION_3: SchemaVersion = SchemaVersion(3);
/// Semantic-replay experiment-plan schema version (adds `SemanticReplay` workload).
pub const EXPERIMENT_PLAN_SCHEMA_VERSION_4: SchemaVersion = SchemaVersion(4);
/// Diagnostic experiment-plan schema version (adds `diagnostics`).
pub const EXPERIMENT_PLAN_SCHEMA_VERSION_5: SchemaVersion = SchemaVersion(5);
