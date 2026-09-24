//! Production driver-catalog ownership.
//!
//! The catalog is the authoritative production driver inventory. With the
//! `eggstack-http` feature it registers the `EggServe` controlled origin
//! (`eggserve-origin`) and the Eggfetch native HTTP workload
//! (`eggfetch-http`); with the `gregg` feature it registers the Gregg
//! host-telemetry driver (`gregg`). Without features it remains empty.
//! Qualification fakes remain test/qualification-only and are never linked
//! through this catalog.

use eggbench_core::{DriverCategory, DriverDescriptor, Name};

/// Authoritative production driver inventory.
///
/// Lookup by canonical driver name; category-specific accessors return the
/// matching descriptor or `None`. The type already exposes service and
/// telemetry accessors so later Eggstack adapters do not require a second
/// registry.
#[derive(Debug, Default, Clone)]
pub struct DriverCatalog {
    descriptors: Vec<DriverDescriptor>,
}

impl DriverCatalog {
    /// Empty catalog: the M001 production state.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            descriptors: Vec::new(),
        }
    }

    /// Production catalog consumed by the CLI.
    ///
    /// With `eggstack-http`, registers the `EggServe` origin service adapter
    /// and the `Eggfetch` workload driver. With `gregg`, registers the Gregg
    /// telemetry driver. Without features the catalog is empty and
    /// production `run` fails closed before managed startup.
    #[must_use]
    pub fn production() -> Self {
        #[allow(unused_mut)]
        let mut descriptors = Vec::new();
        #[cfg(feature = "eggstack-http")]
        descriptors.extend(crate::eggstack::eggstack_descriptors());
        #[cfg(feature = "gregg")]
        descriptors.push(crate::gregg::gregg_telemetry_descriptor());
        Self { descriptors }
    }

    /// Canonical descriptors in stable name order.
    #[must_use]
    pub fn descriptors(&self) -> Vec<DriverDescriptor> {
        let mut out = self.descriptors.clone();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// Look up a workload driver by canonical name.
    #[must_use]
    pub fn workload(&self, name: &Name) -> Option<&DriverDescriptor> {
        self.descriptors
            .iter()
            .find(|d| d.category == DriverCategory::Workload && d.name == *name)
    }

    /// Look up a service adapter by canonical name.
    #[must_use]
    pub fn service(&self, name: &Name) -> Option<&DriverDescriptor> {
        self.descriptors
            .iter()
            .find(|d| d.category == DriverCategory::Service && d.name == *name)
    }

    /// Look up a telemetry adapter by canonical name.
    #[must_use]
    pub fn telemetry(&self, name: &Name) -> Option<&DriverDescriptor> {
        self.descriptors
            .iter()
            .find(|d| d.category == DriverCategory::Telemetry && d.name == *name)
    }

    /// Number of registered production drivers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.descriptors.len()
    }

    /// True when no production driver is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.descriptors.is_empty()
    }
}

/// Production catalog consumed by the CLI.
#[must_use]
pub fn production_catalog() -> DriverCatalog {
    DriverCatalog::production()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_catalog_matches_feature() {
        let catalog = production_catalog();
        let mut expected: Vec<String> = Vec::new();
        #[cfg(feature = "eggstack-http")]
        expected.extend(["eggfetch-http".to_owned(), "eggserve-origin".to_owned()]);
        #[cfg(feature = "gregg")]
        expected.extend(["gregg".to_owned()]);
        let mut names: Vec<String> = catalog
            .descriptors()
            .iter()
            .map(|d| d.name.as_str().to_owned())
            .collect();
        names.sort();
        expected.sort_unstable();
        assert_eq!(names, expected);
        assert_eq!(catalog.len(), expected.len());
        #[cfg(feature = "gregg")]
        {
            let gregg = Name::new("gregg").unwrap();
            let descriptor = catalog.telemetry(&gregg).expect("gregg registered");
            assert!(!descriptor.external_process);
            assert!(descriptor.default);
        }
    }

    #[test]
    fn empty_catalog_lookups_return_none() {
        let catalog = DriverCatalog::empty();
        let name = Name::new("oha").unwrap();
        assert!(catalog.workload(&name).is_none());
        assert!(catalog.service(&name).is_none());
        assert!(catalog.telemetry(&name).is_none());
    }
}
