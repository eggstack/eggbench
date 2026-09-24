//! Gregg host telemetry behind the `gregg` feature.
//!
//! The second half of Eggstack M001: an optional trial-synchronized
//! telemetry source backed by a loopback Gregg daemon. Gregg owns
//! collection and v2 wire semantics; Eggbench owns scheduling,
//! aggregation, and evidence. Transport reuses Eggfetch (no second HTTP
//! client); payloads validate through `gregg-protocol`.
//!
//! Invariants:
//!
//! - Gregg remains optional; required failures prevent measurement,
//!   optional failures disable the collector with explicit warnings.
//! - Only loopback HTTP endpoints are accepted (see `endpoint`).
//! - Telemetry windows stay outside measured workload timing (enforced by
//!   the runner seam, proven by timing tests).
//! - Raw `gregg.ndjson` series are retained per measured trial, bounded.
//! - Host metrics use unambiguous `host_*` names with explicit aggregation.
//! - Minimal/default builds never link this module.

mod collect;
mod endpoint;

pub use collect::{
    GREGG_NDJSON_ARTIFACT, GREGG_PROVENANCE_ARTIFACT, GREGG_SERIES_FORMAT, GreggCollector,
    HOST_CPU_FREQUENCY_HZ, HOST_CPU_PERCENT, HOST_DISK_READ_BYTES_PER_SEC,
    HOST_DISK_WRITE_BYTES_PER_SEC, HOST_MEMORY_PERCENT, HOST_MEMORY_USED_BYTES,
    HOST_NETWORK_RX_BYTES_PER_SEC, HOST_NETWORK_TX_BYTES_PER_SEC, MAX_NDJSON_BYTES,
    MAX_POLL_INTERVAL, MAX_SAMPLES_PER_TRIAL, MIN_POLL_INTERVAL, RequestedHostMetric,
    host_metric_names,
};
pub use endpoint::{GreggEndpoint, HEALTH_PATH, STATUS_PATH, validate_endpoint};

use eggbench_core::{Capability, DriverCategory, DriverDescriptor, Name};
use std::collections::BTreeSet;

/// Canonical telemetry source label served by [`GreggCollector`].
pub const GREGG_SOURCE: &str = "gregg";
/// Eggbench adapter implementation version.
pub const GREGG_ADAPTER_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Exact resolved `gregg-protocol` version from the workspace `Cargo.lock`,
/// emitted by the crate build script. Never hardcode a patch version here.
pub const GREGG_PROTOCOL_VERSION: &str = env!("EGGBENCH_GREGG_PROTOCOL_VERSION");
/// Exact resolved `eggfetch-core` version (transport), shared with M001a.
pub const GREGG_EGGFETCH_VERSION: &str = env!("EGGBENCH_EGGFETCH_CORE_VERSION");

/// Production descriptor for the Gregg host-telemetry driver.
///
/// Advertises one [`Capability::TelemetryField`] per collected `host_*`
/// metric so required-field resolution validates requested fields. The
/// descriptor is the default (and sole) telemetry candidate.
///
/// # Panics
/// Never panics at runtime; the static names are valid by construction.
#[must_use]
pub fn gregg_telemetry_descriptor() -> DriverDescriptor {
    let mut capabilities = BTreeSet::new();
    for name in host_metric_names() {
        capabilities.insert(Capability::TelemetryField {
            field: Name::new(name).expect("static field name"),
        });
    }
    DriverDescriptor {
        name: Name::new(GREGG_SOURCE).expect("static source name"),
        adapter_version: GREGG_ADAPTER_VERSION.to_owned(),
        upstream_name: "gregg-protocol".to_owned(),
        upstream_version: Some(GREGG_PROTOCOL_VERSION.to_owned()),
        category: DriverCategory::Telemetry,
        capabilities,
        supported_platforms: BTreeSet::new(),
        machine_output_schema: None,
        external_process: false,
        default: true,
        compatible_service_types: BTreeSet::new(),
    }
}
