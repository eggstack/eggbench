//! Eggstack-native HTTP drivers behind the `eggstack-http` feature.
//!
//! This module owns the first real Eggstack-native experiment path:
//!
//! ```text
//! ExperimentPlan
//!   | `EggServe` named managed origin (`eggserve-origin`)
//!   | runtime HTTP binding (`http_url`)
//!   | `Eggfetch` `WorkloadExecutor` (`eggfetch-http`)
//!   | Measurement M002 trial lifecycle
//! ```
//!
//! Invariants (from Eggstack Integration M001a):
//!
//! - `Eggfetch` owns outbound HTTP semantics; `EggServe` owns inbound
//!   HTTP/runtime semantics; Eggbench owns lifecycle, configuration, metric
//!   mapping, and evidence only. No Hyper client/server implementation is
//!   created in Eggbench.
//! - The controlled origin binds loopback only (`127.0.0.1` ephemeral).
//! - Runtime-selected ephemeral ports are recorded as evidence.
//! - Minimal/default builds never link these drivers; everything here is
//!   gated behind `eggstack-http`.

mod fetch;
mod origin;
#[cfg(feature = "eggstack-path")]
pub mod path;

pub use fetch::EggfetchWorkload;
pub use origin::EggServeOriginAdapter;

use eggbench_core::{Capability, DriverCategory, DriverDescriptor, HttpVersion, LoadMode, Name};
use eggbench_runner::ServiceAdapterRegistry;
use std::collections::BTreeSet;
use std::sync::Arc;

/// Canonical named service type served by [`EggServeOriginAdapter`].
pub const EGGSERVE_ORIGIN_SERVICE_TYPE: &str = "eggserve-origin";
/// Canonical workload driver name served by [`EggfetchWorkload`].
pub const EGGFETCH_HTTP_DRIVER_NAME: &str = "eggfetch-http";
/// Eggbench adapter implementation version for both drivers.
pub const EGGSTACK_ADAPTER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Exact resolved `eggfetch-core` version from the workspace `Cargo.lock`,
/// emitted by the crate build script. Never hardcode a patch version here.
pub const EGGFETCH_CORE_VERSION: &str = env!("EGGBENCH_EGGFETCH_CORE_VERSION");
/// Exact resolved `eggserve-server` version from the workspace `Cargo.lock`.
pub const EGGSERVE_SERVER_VERSION: &str = env!("EGGBENCH_EGGSERVE_SERVER_VERSION");
/// Exact resolved `eggserve-primitives` version from the workspace `Cargo.lock`.
pub const EGGSERVE_PRIMITIVES_VERSION: &str = env!("EGGBENCH_EGGSERVE_PRIMITIVES_VERSION");

/// Production descriptor for the `EggServe` controlled-origin service adapter.
///
/// # Panics
/// Never panics at runtime; the static names are valid by construction.
#[must_use]
pub fn eggserve_origin_descriptor() -> DriverDescriptor {
    let mut capabilities = BTreeSet::new();
    capabilities.insert(Capability::HttpVersion {
        version: HttpVersion::Http11,
    });
    DriverDescriptor {
        name: Name::new(EGGSERVE_ORIGIN_SERVICE_TYPE).expect("static service name"),
        adapter_version: EGGSTACK_ADAPTER_VERSION.to_owned(),
        upstream_name: "eggserve-server".to_owned(),
        upstream_version: Some(EGGSERVE_SERVER_VERSION.to_owned()),
        category: DriverCategory::Service,
        capabilities,
        supported_platforms: BTreeSet::new(),
        machine_output_schema: None,
        external_process: false,
        default: true,
        compatible_service_types: BTreeSet::new(),
    }
}

/// Production descriptor for the `Eggfetch` native HTTP workload driver.
///
/// Only truthful closed-loop capabilities are advertised: open-loop requests
/// fail plan resolution explicitly through the missing
/// `LoadMode::OpenLoop` capability.
///
/// # Panics
/// Never panics at runtime; the static names are valid by construction.
#[must_use]
pub fn eggfetch_http_descriptor() -> DriverDescriptor {
    let mut capabilities = BTreeSet::new();
    capabilities.insert(Capability::HttpVersion {
        version: HttpVersion::Http11,
    });
    capabilities.insert(Capability::LoadMode {
        mode: LoadMode::ClosedLoop,
    });
    #[cfg(feature = "eggstack-path")]
    capabilities.insert(Capability::NetworkPath);
    let mut compatible = BTreeSet::new();
    compatible.insert(Name::new(EGGSERVE_ORIGIN_SERVICE_TYPE).expect("static service name"));
    DriverDescriptor {
        name: Name::new(EGGFETCH_HTTP_DRIVER_NAME).expect("static driver name"),
        adapter_version: EGGSTACK_ADAPTER_VERSION.to_owned(),
        upstream_name: "eggfetch-core".to_owned(),
        upstream_version: Some(EGGFETCH_CORE_VERSION.to_owned()),
        category: DriverCategory::Workload,
        capabilities,
        supported_platforms: BTreeSet::new(),
        machine_output_schema: None,
        external_process: false,
        default: true,
        compatible_service_types: compatible,
    }
}

/// Both production descriptors in stable name order.
#[must_use]
pub fn eggstack_descriptors() -> Vec<DriverDescriptor> {
    vec![eggfetch_http_descriptor(), eggserve_origin_descriptor()]
}

/// Runner registry with the `EggServe` controlled-origin adapter registered.
///
/// Duplicate registration is rejected by the registry; this constructor
/// starts empty so it cannot duplicate.
///
/// # Panics
/// Never panics at runtime; the static service type is valid by construction.
#[must_use]
pub fn eggstack_service_adapters() -> ServiceAdapterRegistry {
    let mut registry = ServiceAdapterRegistry::new();
    registry
        .register(Arc::new(EggServeOriginAdapter::new()))
        .expect("fresh registry accepts the origin adapter");
    registry
}

/// One `Eggfetch` client bound to a workload executor for a whole run.
///
/// The client is created once per executor (per run), never per request, so
/// warmups establish pool/connection state and measured trials reflect a
/// warmed run when warmups are configured.
#[must_use]
pub fn eggfetch_workload() -> EggfetchWorkload {
    EggfetchWorkload::new()
}
