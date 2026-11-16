//! `EggServe` origin and `Eggfetch` workload adapters behind `eggstack-http`.
//!
//! Eggbench owns lifecycle and evidence; the adapters own HTTP semantics. The
//! origin binds loopback on an ephemeral port, which is recorded in evidence.
//! Default builds do not link these adapters.

mod fetch;
mod http_corpus;
mod origin;
#[cfg(feature = "eggstack-path")]
pub mod path;

pub use fetch::EggfetchWorkload;
pub use http_corpus::{HTTP_CORPUS_SEMANTIC_VERSION, HTTP_CORPUS_SOURCE, HttpCorpusExecutor};
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
    capabilities.insert(Capability::HttpCorpus);
    #[cfg(feature = "eggstack-path")]
    capabilities.insert(Capability::NetworkPath);
    let mut compatible = BTreeSet::new();
    compatible.insert(Name::new(EGGSERVE_ORIGIN_SERVICE_TYPE).expect("static service name"));
    // A managed command subject can expose a loopback endpoint through the
    // `http_url` binding. A missing binding still fails closed at execution.
    compatible.insert(Name::new("command").expect("static service kind"));
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

/// Production descriptors in stable name order.
#[must_use]
pub fn eggstack_descriptors() -> Vec<DriverDescriptor> {
    vec![
        eggfetch_http_descriptor(),
        eggserve_origin_descriptor(),
        http_corpus_descriptor(),
    ]
}

/// Production descriptor for fixed-corpus `http_observable` correctness.
#[must_use]
#[allow(clippy::missing_panics_doc)]
pub fn http_corpus_descriptor() -> DriverDescriptor {
    let mut capabilities = BTreeSet::new();
    capabilities.insert(Capability::SecurityCheck {
        family: Name::new("http_observable").expect("static correctness family"),
    });
    DriverDescriptor {
        name: Name::new(HTTP_CORPUS_SOURCE).expect("static correctness source"),
        adapter_version: HTTP_CORPUS_SEMANTIC_VERSION.to_owned(),
        upstream_name: "eggfetch-core".to_owned(),
        upstream_version: Some(EGGFETCH_CORE_VERSION.to_owned()),
        category: DriverCategory::Correctness,
        capabilities,
        supported_platforms: BTreeSet::new(),
        machine_output_schema: Some(eggbench_core::SchemaVersion(1)),
        external_process: false,
        default: false,
        compatible_service_types: BTreeSet::new(),
    }
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
