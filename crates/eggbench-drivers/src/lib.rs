//! Shared production driver catalog and external-command substrate.
//!
//! `eggbench-drivers` is the sole production adapter/catalog ownership crate.
//! The CLI owns presentation, not driver implementation or registration.
//!
//! M001 establishes reusable machinery only: trusted executable resolution,
//! binary identity/version probing, argv-only command execution with bounded
//! capture and cancellation, raw-output artifact helpers, and a versioned
//! parser contract. It deliberately ships no oha/h2load/iperf3 adapter and
//! no EggServe/Eggfetch/Gregg integration semantics.

#![forbid(unsafe_code)]

mod catalog;
#[cfg(feature = "eggstack-http")]
pub mod eggstack;
pub mod external;
#[cfg(feature = "gregg")]
pub mod gregg;

pub use catalog::{DriverCatalog, production_catalog};
#[cfg(feature = "eggstack-http")]
pub use eggstack::{
    EGGFETCH_CORE_VERSION, EGGFETCH_HTTP_DRIVER_NAME, EGGSERVE_ORIGIN_SERVICE_TYPE,
    EGGSERVE_PRIMITIVES_VERSION, EGGSERVE_SERVER_VERSION, EGGSTACK_ADAPTER_VERSION,
    EggServeOriginAdapter, EggfetchWorkload, eggfetch_http_descriptor, eggfetch_workload,
    eggserve_origin_descriptor, eggstack_descriptors, eggstack_service_adapters,
};
