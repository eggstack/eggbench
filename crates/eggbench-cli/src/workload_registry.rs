//! Built-in workload driver registry.
//!
//! M003 does not register a production workload adapter. The CLI exposes a
//! single deterministic `FakeWorkload` adapter used for end-to-end
//! qualification; it is not a real load generator. Production adapters
//! belong to External Oracles / Eggstack Integrations milestones.

use eggbench_core::{DriverCategory, DriverDescriptor, LoadMode, Name};
use eggbench_runner::test_support::FakeWorkload;
use eggbench_runner::{
    DrainContext, FailureCategory, InvocationContext, WorkloadExecutor, WorkloadOutput,
};
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Stable descriptor of one workload driver.
#[derive(Debug, Clone)]
pub struct WorkloadDescriptor {
    /// Canonical driver name.
    pub name: String,
    /// Adapter implementation version.
    pub adapter_version: String,
    /// Upstream name reported to users.
    pub upstream_name: String,
    /// Whether this is the default driver for its category.
    pub default: bool,
}

impl WorkloadDescriptor {
    /// Convert to the canonical [`DriverDescriptor`].
    #[must_use]
    pub fn to_descriptor(&self) -> DriverDescriptor {
        let name = Name::new(self.name.clone()).expect("static descriptor name");
        let mut capabilities = std::collections::BTreeSet::new();
        capabilities.insert(eggbench_core::Capability::LoadMode {
            mode: LoadMode::ClosedLoop,
        });
        DriverDescriptor {
            name,
            adapter_version: self.adapter_version.clone(),
            upstream_name: self.upstream_name.clone(),
            upstream_version: None,
            category: DriverCategory::Workload,
            capabilities,
            supported_platforms: std::collections::BTreeSet::new(),
            machine_output_schema: None,
            external_process: false,
            default: self.default,
            compatible_service_types: std::collections::BTreeSet::new(),
        }
    }
}

/// View of one workload driver exposed by the registry.
#[derive(Debug, Clone)]
pub struct DriverInventoryEntry {
    /// Driver descriptor.
    pub descriptor: WorkloadDescriptor,
}

/// Result of looking up a workload driver.
#[derive(Debug, Clone)]
pub enum NoProductionAdapter {
    /// No production adapter is registered; the requested category is empty.
    Empty,
}

/// In-memory registry of available workload drivers.
#[derive(Default, Clone)]
pub struct WorkloadRegistry {
    drivers: BTreeMap<String, WorkloadDescriptor>,
}

impl WorkloadRegistry {
    /// Create a registry with the M003 default state: a deterministic fake
    /// workload registered for qualification. The fake is not a production
    /// adapter.
    #[must_use]
    pub fn with_builtin() -> Self {
        let mut registry = Self::default();
        registry.register(WorkloadDescriptor {
            name: "fake-load".to_owned(),
            adapter_version: env!("CARGO_PKG_VERSION").to_owned(),
            upstream_name: "fake-load".to_owned(),
            default: true,
        });
        registry
    }

    /// Register or replace a driver descriptor.
    pub fn register(&mut self, descriptor: WorkloadDescriptor) {
        self.drivers.insert(descriptor.name.clone(), descriptor);
    }

    /// Return a stable inventory view of the registered drivers.
    #[must_use]
    pub fn inventory(&self) -> Vec<DriverInventoryEntry> {
        let mut entries: Vec<_> = self
            .drivers
            .values()
            .cloned()
            .map(|descriptor| DriverInventoryEntry { descriptor })
            .collect();
        entries.sort_by(|left, right| left.descriptor.name.cmp(&right.descriptor.name));
        entries
    }

    /// Resolve the default workload driver if one is registered.
    #[must_use]
    pub fn default_workload(&self) -> Option<&WorkloadDescriptor> {
        self.drivers
            .values()
            .find(|descriptor| descriptor.default)
            .or_else(|| self.drivers.values().next())
    }

    /// True if at least one driver is registered.
    #[must_use]
    pub fn has_workload_driver(&self) -> bool {
        !self.drivers.is_empty()
    }
}

/// Wraps a [`FakeWorkload`] in an `Arc<Mutex<...>>` and presents it as a
/// `WorkloadExecutor` for qualification runs.
///
/// The M003 CLI uses this executor only when the caller explicitly selects
/// the deterministic qualification path; production CLI usage reports
/// `unsupported_capability` instead.
#[derive(Clone)]
pub struct BuiltinWorkloadExecutor {
    inner: Arc<Mutex<FakeWorkload>>,
}

impl BuiltinWorkloadExecutor {
    /// Construct a new builtin executor wrapping a [`FakeWorkload`].
    #[must_use]
    pub fn new(inner: FakeWorkload) -> Self {
        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
    }
}

impl WorkloadExecutor for BuiltinWorkloadExecutor {
    fn execute<'a>(
        &'a mut self,
        context: InvocationContext,
    ) -> Pin<Box<dyn Future<Output = Result<WorkloadOutput, FailureCategory>> + Send + 'a>> {
        Box::pin(async move {
            let mut guard = self.inner.lock().await;
            guard.execute(context).await
        })
    }

    fn drain<'a>(
        &'a mut self,
        context: DrainContext,
    ) -> Pin<Box<dyn Future<Output = Result<(), FailureCategory>> + Send + 'a>> {
        Box::pin(async move {
            let mut guard = self.inner.lock().await;
            guard.drain(context).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_registry_contains_fake_load() {
        let registry = WorkloadRegistry::with_builtin();
        assert!(registry.has_workload_driver());
        assert_eq!(registry.default_workload().unwrap().name, "fake-load");
        assert_eq!(registry.inventory().len(), 1);
    }

    #[test]
    fn empty_registry_reports_no_driver() {
        let registry = WorkloadRegistry::default();
        assert!(!registry.has_workload_driver());
        assert!(registry.default_workload().is_none());
    }

    #[test]
    fn descriptor_serializes_into_driver_descriptor() {
        let descriptor = WorkloadDescriptor {
            name: "fake-load".into(),
            adapter_version: "0".into(),
            upstream_name: "fake-load".into(),
            default: true,
        };
        let driver = descriptor.to_descriptor();
        assert_eq!(driver.name.as_str(), "fake-load");
        assert_eq!(driver.category, DriverCategory::Workload);
    }
}
