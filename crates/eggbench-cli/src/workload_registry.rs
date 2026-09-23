//! Workload driver registry with an explicit production/qualification split.
//!
//! Production `eggbench` registers no workload adapter at this milestone:
//! [`WorkloadRegistry::production`] (and the legacy [`WorkloadRegistry::with_builtin`]
//! alias) return an empty registry, so `run` fails before managed startup
//! with a stable `missing_driver`/`unsupported_workload` category.
//!
//! Deterministic qualification uses [`WorkloadRegistry::with_qualification_fake`]
//! (or [`QualificationRuntime`]) to inject the `FakeWorkload` descriptor and
//! executor explicitly. That path is never used by `main.rs` and never
//! appears in production help or the default driver inventory. Production
//! adapters belong to External Oracles / Eggstack Integrations milestones.

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
#[derive(Debug, Default, Clone)]
pub struct WorkloadRegistry {
    drivers: BTreeMap<String, WorkloadDescriptor>,
}

/// Minimal workload-runtime seam separating production and qualification.
///
/// Production returns only real compiled/registered adapters (empty at this
/// milestone). Qualification registers the deterministic fake descriptor and
/// can build its executor. A smaller injection seam with the same invariant
/// would also satisfy the corrective.
pub trait WorkloadRuntime {
    /// Stable inventory view of registered drivers.
    fn inventory(&self) -> Vec<DriverInventoryEntry>;
    /// Canonical descriptors used for plan resolution.
    fn driver_descriptors(&self) -> Vec<eggbench_core::DriverDescriptor>;
    /// True when at least one workload driver is registered.
    fn has_workload_driver(&self) -> bool;
}

/// Production runtime: no synthetic driver unless a real adapter is compiled
/// in (none at this milestone).
#[derive(Debug, Default, Clone)]
pub struct ProductionRuntime {
    registry: WorkloadRegistry,
}

impl ProductionRuntime {
    /// Empty production registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            registry: WorkloadRegistry::production(),
        }
    }
}

impl WorkloadRuntime for ProductionRuntime {
    fn inventory(&self) -> Vec<DriverInventoryEntry> {
        self.registry.inventory()
    }

    fn driver_descriptors(&self) -> Vec<eggbench_core::DriverDescriptor> {
        self.registry.descriptors()
    }

    fn has_workload_driver(&self) -> bool {
        self.registry.has_workload_driver()
    }
}

/// Qualification/test runtime: deterministic fake descriptor plus executor.
///
/// Never used by `main.rs`. Test and qualification harnesses inject this
/// explicitly; no public production flag selects it.
#[derive(Debug, Default, Clone)]
pub struct QualificationRuntime {
    registry: WorkloadRegistry,
}

impl QualificationRuntime {
    /// Registry with the deterministic `fake-load` descriptor.
    #[must_use]
    pub fn new() -> Self {
        Self {
            registry: WorkloadRegistry::with_qualification_fake(),
        }
    }

    /// Build the qualification executor wrapping the given fake workload.
    #[must_use]
    pub fn workload_executor(inner: FakeWorkload) -> BuiltinWorkloadExecutor {
        BuiltinWorkloadExecutor::new(inner)
    }
}

impl WorkloadRuntime for QualificationRuntime {
    fn inventory(&self) -> Vec<DriverInventoryEntry> {
        self.registry.inventory()
    }

    fn driver_descriptors(&self) -> Vec<eggbench_core::DriverDescriptor> {
        self.registry.descriptors()
    }

    fn has_workload_driver(&self) -> bool {
        self.registry.has_workload_driver()
    }
}

impl WorkloadRegistry {
    /// Production registry state: no synthetic workload driver.
    ///
    /// At this milestone the set of real compiled production adapters is
    /// empty, so `doctor` reports `has_workload_driver=false` and `run`
    /// fails before managed startup.
    #[must_use]
    pub fn production() -> Self {
        Self::default()
    }

    /// Legacy constructor preserved for call-site compatibility.
    ///
    /// Returns the production (empty) registry. Qualification harnesses must
    /// use [`Self::with_qualification_fake`] explicitly.
    #[must_use]
    pub fn with_builtin() -> Self {
        Self::production()
    }

    /// Qualification-only registry with the deterministic `fake-load`
    /// descriptor registered.
    ///
    /// Not used by the production binary. Injected explicitly by tests and
    /// qualification harnesses; it never appears in production help or the
    /// default driver inventory.
    #[must_use]
    pub fn with_qualification_fake() -> Self {
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

    /// Canonical descriptors used for plan resolution, in inventory order.
    #[must_use]
    pub fn descriptors(&self) -> Vec<eggbench_core::DriverDescriptor> {
        self.inventory()
            .iter()
            .map(|entry| entry.descriptor.to_descriptor())
            .collect()
    }

    /// Reset the registry to the empty production state.
    pub fn reset_registry(&mut self) {
        self.drivers.clear();
    }
}

/// Qualification-only executor wrapping a [`FakeWorkload`].
///
/// Never constructed by the production binary. Qualification harnesses inject
/// it explicitly through [`QualificationRuntime::workload_executor`]; the
/// production `run` path fails before it could reach this executor.
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
    fn production_registry_contains_no_fake_driver() {
        let registry = WorkloadRegistry::production();
        assert!(!registry.has_workload_driver());
        assert!(registry.default_workload().is_none());
        assert!(registry.inventory().is_empty());
        assert!(
            !registry
                .inventory()
                .iter()
                .any(|entry| entry.descriptor.name == "fake-load")
        );
    }

    #[test]
    fn legacy_builtin_constructor_is_production_empty() {
        let registry = WorkloadRegistry::with_builtin();
        assert!(!registry.has_workload_driver());
        assert!(
            !registry
                .inventory()
                .iter()
                .any(|entry| entry.descriptor.name == "fake-load")
        );
    }

    #[test]
    fn qualification_registry_injects_fake_load() {
        let registry = WorkloadRegistry::with_qualification_fake();
        assert!(registry.has_workload_driver());
        assert_eq!(registry.default_workload().unwrap().name, "fake-load");
        assert_eq!(registry.inventory().len(), 1);
    }

    #[test]
    fn production_runtime_reports_no_driver() {
        let runtime = ProductionRuntime::new();
        assert!(!runtime.has_workload_driver());
        assert!(runtime.inventory().is_empty());
        assert!(runtime.driver_descriptors().is_empty());
    }

    #[test]
    fn qualification_runtime_reports_fake_driver() {
        let runtime = QualificationRuntime::new();
        assert!(runtime.has_workload_driver());
        assert_eq!(runtime.inventory().len(), 1);
        assert_eq!(runtime.driver_descriptors().len(), 1);
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
