//! Generic named in-process service-adapter seam.
//!
//! The runner owns command processes; Eggstack (and future) in-process
//! services participate through this object-safe adapter contract. Core never
//! depends on concrete sibling crates: adapters are registered by name and
//! resolved against the plan's `ServiceKind::Named` service types.
//!
//! Named adapter services participate in dependency ordering and reverse
//! teardown exactly like managed command services. They expose runtime
//! bindings (non-secret, immutable after startup) to workload drivers and
//! are recorded in the runtime-topology evidence artifact.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Boxed sendable future used by the object-safe adapter contracts.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Protocol-neutral runner-owned runtime binding map.
///
/// Values are non-secret connection facts (for example an origin's
/// `http_url`). Bindings are immutable after successful startup; workloads
/// only receive them after readiness, and they remain available through
/// teardown for final evidence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeBindings {
    services: BTreeMap<String, BTreeMap<String, String>>,
}

impl RuntimeBindings {
    /// Empty binding map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// True when no service published any binding.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.services.values().all(BTreeMap::is_empty)
    }

    /// Insert one binding. Service names and keys must be bounded [`Name`]
    /// values; keys additionally reject `/` and interior whitespace. Values
    /// must be non-secret and free of control characters.
    ///
    /// # Errors
    /// Returns a human-readable reason when the binding is not well-formed.
    pub fn insert(&mut self, service: &str, key: &str, value: String) -> Result<(), String> {
        eggbench_core::Name::new(service)
            .map_err(|_| "binding service name is not well-formed".to_owned())?;
        eggbench_core::Name::new(key).map_err(|_| "binding key is not well-formed".to_owned())?;
        if key.chars().any(|c| c.is_whitespace() || c == '/') {
            return Err("binding key is not well-formed".to_owned());
        }
        if value.chars().any(char::is_control) {
            return Err("binding value is not well-formed".to_owned());
        }
        self.services
            .entry(service.to_owned())
            .or_default()
            .insert(key.to_owned(), value);
        Ok(())
    }

    /// Look up one binding.
    #[must_use]
    pub fn get(&self, service: &str, key: &str) -> Option<&str> {
        self.services
            .get(service)
            .and_then(|bindings| bindings.get(key).map(String::as_str))
    }

    /// All bindings for one service.
    #[must_use]
    pub fn service_bindings(&self, service: &str) -> Option<&BTreeMap<String, String>> {
        self.services.get(service)
    }

    /// Iterate `(service, key, value)` triples in stable order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.services.iter().flat_map(|(service, bindings)| {
            bindings
                .iter()
                .map(move |(key, value)| (service.as_str(), key.as_str(), value.as_str()))
        })
    }

    /// Merge another map into this one without overwriting existing entries.
    pub fn merge(&mut self, other: &RuntimeBindings) {
        for (service, key, value) in other.iter() {
            self.services
                .entry(service.to_owned())
                .or_default()
                .entry(key.to_owned())
                .or_insert_with(|| value.to_owned());
        }
    }
}

/// Request passed to one named-service adapter start.
#[derive(Debug, Clone)]
pub struct ServiceStartRequest {
    /// Stable service identity from the plan.
    pub service: String,
    /// Stable service-type label from `ServiceKind::Named`.
    pub service_type: String,
    /// Opaque non-secret config values from `Service.config`.
    pub config: BTreeMap<String, String>,
    /// Graceful shutdown allowance for later teardown.
    pub grace: Duration,
}

/// Object-safe named-service adapter.
///
/// `start` returns only after the adapter's own readiness condition is
/// satisfied. Implementations must honor the cancellation token and perform
/// no measurement.
pub trait ManagedServiceAdapter: Send + Sync {
    /// Stable service-type label this adapter implements.
    fn service_type(&self) -> &str;

    /// Start the service and wait for adapter-owned readiness.
    fn start(
        &self,
        request: ServiceStartRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<Box<dyn ManagedServiceHandle>, String>>;
}

/// Object-safe handle for one started named service.
pub trait ManagedServiceHandle: Send {
    /// Runtime bindings published by this service.
    fn bindings(&self) -> RuntimeBindings;

    /// Graceful shutdown within the given allowance.
    fn shutdown(&mut self, grace: Duration) -> BoxFuture<'_, Result<(), String>>;
}

/// Explicit registry of named-service adapters.
#[derive(Clone, Default)]
pub struct ServiceAdapterRegistry {
    adapters: BTreeMap<String, Arc<dyn ManagedServiceAdapter>>,
}

impl ServiceAdapterRegistry {
    /// Empty registry: no named service type is implemented.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an adapter. Duplicate service types are rejected.
    ///
    /// # Errors
    /// Returns a human-readable reason when the type is already registered.
    pub fn register(&mut self, adapter: Arc<dyn ManagedServiceAdapter>) -> Result<(), String> {
        let service_type = adapter.service_type().to_owned();
        if service_type.is_empty() {
            return Err("adapter service type must not be empty".to_owned());
        }
        if self.adapters.contains_key(&service_type) {
            return Err(format!("duplicate service adapter {service_type}"));
        }
        self.adapters.insert(service_type, adapter);
        Ok(())
    }

    /// Look up the adapter for a named service type.
    #[must_use]
    pub fn get(&self, service_type: &str) -> Option<&Arc<dyn ManagedServiceAdapter>> {
        self.adapters.get(service_type)
    }

    /// True when no adapter is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.adapters.is_empty()
    }

    /// Registered service-type labels in stable order.
    #[must_use]
    pub fn service_types(&self) -> Vec<String> {
        self.adapters.keys().cloned().collect()
    }
}

impl fmt::Debug for ServiceAdapterRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServiceAdapterRegistry")
            .field("service_types", &self.service_types())
            .finish()
    }
}

/// Ownership kind of one runtime-topology entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceOwnership {
    /// Runner-owned OS process.
    Process,
    /// Runner-owned in-process adapter service.
    Adapter,
    /// Observed but never started or stopped by the runner.
    External,
}

/// One service entry in the runtime-topology evidence artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceTopologyEntry {
    /// Stable service identity.
    pub identity: String,
    /// Ownership kind.
    pub ownership: ServiceOwnership,
    /// Named service type, when the service is adapter-owned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_type: Option<String>,
    /// Non-secret runtime bindings published by this service.
    #[serde(default)]
    pub bindings: BTreeMap<String, String>,
}

/// Schema version of the [`RuntimeTopology`] evidence artifact.
pub const RUNTIME_TOPOLOGY_SCHEMA_VERSION: eggbench_core::SchemaVersion =
    eggbench_core::SchemaVersion(1);

/// Versioned runner-owned runtime-topology evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeTopology {
    /// Runtime-topology schema version.
    pub schema_version: eggbench_core::SchemaVersion,
    /// One entry per started or external service, in spawn order.
    pub services: Vec<ServiceTopologyEntry>,
}
