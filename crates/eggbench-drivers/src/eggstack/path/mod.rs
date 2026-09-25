//! Eggstack M002 listener-free network path.
//!
//! Eggress + Eggchaos adapter module behind the `eggstack-path` cargo
//! feature. Produces:
//! - `EggstackPathDialer` (route-first, then fault), implementing
//!   `eggfetch_core::Dialer`.
//! - Bounded run and invocation path diagnostics.
//! - Versioned `network-path.json` evidence.

#![allow(clippy::missing_errors_doc)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_panics_doc)]

mod dialer;
mod evidence;
mod fault;
mod route;

pub use dialer::{AsyncStreamDialStream, EggstackPathDialer, lower_dialer, new_path_diagnostics};
pub use evidence::{
    FaultLayer, NETWORK_PATH_EVIDENCE_MAX_BYTES, NETWORK_PATH_EVIDENCE_SCHEMA_VERSION,
    NetworkPathDriverEvidence, NetworkPathEvidence, PathDiagnosticsSnapshot, PathOrdering,
    PathPolicyMode, PathSemantics, SharedPathDiagnostics, StreamDirection, StreamFaultEvidence,
    build_evidence, load_network_path_evidence, network_path_role_label,
};
pub use fault::build_fault_plan;
pub use route::{build_route_chain, parse_route_chain, redacted_chain_text};

use eggbench_core::{Capability, DriverCategory, DriverDescriptor, Name};
use std::collections::BTreeSet;

/// Canonical M002 route driver name exposed to plan and resolved plans.
pub const ROUTE_DRIVER_NAME: &str = "eggress-route";
/// Canonical M002 stream-fault driver name exposed to plan and resolved plans.
pub const FAULT_DRIVER_NAME: &str = "eggchaos-stream";
/// Adapter implementation version for both route and fault descriptors.
pub const EGGSTACK_PATH_ADAPTER_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Exact resolved `eggress-outbound` version from the workspace `Cargo.lock`.
pub const EGGRESS_OUTBOUND_VERSION: &str = env!("EGGBENCH_EGGRESS_OUTBOUND_VERSION");
/// Exact resolved `eggress-uri` version from the workspace `Cargo.lock`.
pub const EGGRESS_URI_VERSION: &str = env!("EGGBENCH_EGGRESS_URI_VERSION");
/// Exact resolved `eggchaos-core` version from the workspace `Cargo.lock`.
pub const EGGCHAOS_CORE_VERSION: &str = env!("EGGBENCH_EGGCHAOS_CORE_VERSION");

/// Diagnostic label for transport-level connect failures surfaced by the
/// eggfetch DialError but with no preserved Eggress category.
pub const ROUTE_FAILURE_OTHER: &str = "transport_other";
/// Maximum bytes retained per retry-cached route-failure map.
pub const MAX_ROUTE_FAILURE_KIND_BUCKETS: usize = 16;
/// Maximum bytes retained for configured chain text in evidence.
pub const MAX_RETAINED_CHAIN_BYTES: usize = 1024;
/// Connection-attribute dial timeout (defaulting to the M001a 30 s).
pub const DEFAULT_DIAL_TIMEOUT_MS: u64 = 30_000;

fn route_capability_set() -> BTreeSet<Capability> {
    let mut set = BTreeSet::new();
    set.insert(Capability::ProxyRouting);
    set
}

fn fault_capability_set() -> BTreeSet<Capability> {
    let mut set = BTreeSet::new();
    set.insert(Capability::StreamFaultPlan);
    set
}

/// Production descriptor for the listener-free Eggress route driver.
#[must_use]
pub fn route_descriptor() -> DriverDescriptor {
    DriverDescriptor {
        name: Name::new(ROUTE_DRIVER_NAME).expect("static driver name"),
        adapter_version: EGGSTACK_PATH_ADAPTER_VERSION.to_owned(),
        upstream_name: "eggress-outbound".to_owned(),
        upstream_version: Some(EGGRESS_OUTBOUND_VERSION.to_owned()),
        category: DriverCategory::Route,
        capabilities: route_capability_set(),
        supported_platforms: BTreeSet::new(),
        machine_output_schema: None,
        external_process: false,
        default: true,
        compatible_service_types: BTreeSet::new(),
    }
}

/// Production descriptor for the deterministic Eggchaos stream-fault driver.
#[must_use]
pub fn fault_descriptor() -> DriverDescriptor {
    DriverDescriptor {
        name: Name::new(FAULT_DRIVER_NAME).expect("static driver name"),
        adapter_version: EGGSTACK_PATH_ADAPTER_VERSION.to_owned(),
        upstream_name: "eggchaos-core".to_owned(),
        upstream_version: Some(EGGCHAOS_CORE_VERSION.to_owned()),
        category: DriverCategory::Fault,
        capabilities: fault_capability_set(),
        supported_platforms: BTreeSet::new(),
        machine_output_schema: None,
        external_process: false,
        default: true,
        compatible_service_types: BTreeSet::new(),
    }
}

/// Both M002 path descriptors in stable name order.
#[must_use]
pub fn path_descriptors() -> Vec<DriverDescriptor> {
    let mut descriptors = vec![route_descriptor(), fault_descriptor()];
    descriptors.sort_by(|left, right| left.name.cmp(&right.name));
    descriptors
}

/// Validate that a request lowers through the audited native route profile.
///
/// # Errors
/// Returns a bounded reason when route syntax, protocol, or secret policy fails.
pub fn validate_request(request: &eggbench_core::NetworkPathRequest) -> Result<(), String> {
    match &request.route.mode {
        eggbench_core::RouteMode::Direct => Ok(()),
        eggbench_core::RouteMode::ProxyChain { chain } => {
            let canonical = redacted_chain_text(chain)?;
            if canonical.contains(['@', '%', '?', '#']) {
                return Err("route_credentials_not_supported".to_owned());
            }
            Ok(())
        }
    }
}
