//! Production driver-catalog ownership.
//!
//! The catalog is the authoritative production driver inventory. With the
//! `eggstack-http` feature it registers the `EggServe` controlled origin
//! (`eggserve-origin`) and the `Eggfetch` native HTTP workload
//! (`eggfetch-http`); with the `gregg` feature it registers the `Gregg`
//! host-telemetry driver (`gregg`). The external-process oracles (`oha`,
//! `h2load`, `iperf3`), the `EggReplay` semantic workload
//! (`eggreplay-semantic`), the `Eggprobe` pre/post diagnostic driver
//! (`eggprobe`), and the `Eggsec` strict-scope WAF correctness driver
//! (`eggsec-waf`) register unconditionally. Without features and
//! without installed tools only the external descriptors remain.
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
    /// telemetry driver. The external-process workload drivers (`oha`,
    /// `h2load`, `iperf3`) register unconditionally: a missing binary is a
    /// runtime capability error, never a build configuration. Without
    /// features and without installed tools the catalog still lists the
    /// external drivers and production `run` fails closed before managed
    /// startup.
    #[must_use]
    pub fn production() -> Self {
        let mut descriptors = Vec::new();
        #[cfg(feature = "eggstack-http")]
        descriptors.extend(crate::eggstack::eggstack_descriptors());
        #[cfg(feature = "eggstack-path")]
        descriptors.extend(crate::eggstack::path::path_descriptors());
        #[cfg(feature = "gregg")]
        descriptors.push(crate::gregg::gregg_telemetry_descriptor());
        descriptors.extend([
            crate::external::oha_descriptor(),
            crate::external::h2load_descriptor(),
            crate::external::iperf3_descriptor(),
            crate::external::eggreplay_descriptor(),
            crate::external::eggprobe_descriptor(),
            crate::external::eggsec_descriptor(),
        ]);
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

    /// Look up a diagnostic adapter by canonical name.
    #[must_use]
    pub fn diagnostic(&self, name: &Name) -> Option<&DriverDescriptor> {
        self.descriptors
            .iter()
            .find(|d| d.category == DriverCategory::Diagnostic && d.name == *name)
    }

    /// Look up a correctness adapter by canonical name.
    #[must_use]
    pub fn correctness(&self, name: &Name) -> Option<&DriverDescriptor> {
        self.descriptors
            .iter()
            .find(|d| d.category == DriverCategory::Correctness && d.name == *name)
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
        let mut expected: Vec<String> = vec![
            "eggprobe".to_owned(),
            "eggreplay-semantic".to_owned(),
            "eggsec-waf".to_owned(),
            "h2load".to_owned(),
            "iperf3".to_owned(),
            "oha".to_owned(),
        ];
        #[cfg(feature = "eggstack-http")]
        expected.extend([
            "eggbench-http-corpus".to_owned(),
            "eggfetch-http".to_owned(),
            "eggserve-origin".to_owned(),
        ]);
        #[cfg(feature = "eggstack-path")]
        expected.extend(["eggress-route".to_owned(), "eggchaos-stream".to_owned()]);
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
        for tool in ["oha", "h2load", "iperf3", "eggreplay-semantic"] {
            let name = Name::new(tool).unwrap();
            let descriptor = catalog.workload(&name).expect("oracle registered");
            assert!(descriptor.external_process);
            assert!(!descriptor.default);
        }
        let probe = Name::new("eggprobe").unwrap();
        let descriptor = catalog.diagnostic(&probe).expect("eggprobe registered");
        assert!(descriptor.external_process);
        assert!(!descriptor.default);
        assert!(catalog.workload(&probe).is_none());
        let correctness = Name::new("eggsec-waf").unwrap();
        let descriptor = catalog
            .correctness(&correctness)
            .expect("eggsec-waf registered");
        assert!(descriptor.external_process);
        assert!(!descriptor.default);
        assert_eq!(descriptor.category, DriverCategory::Correctness);
        assert!(catalog.workload(&correctness).is_none());
        assert!(catalog.diagnostic(&correctness).is_none());
    }

    #[test]
    fn empty_catalog_lookups_return_none() {
        let catalog = DriverCatalog::empty();
        let name = Name::new("oha").unwrap();
        assert!(catalog.workload(&name).is_none());
        assert!(catalog.service(&name).is_none());
        assert!(catalog.telemetry(&name).is_none());
        assert!(catalog.diagnostic(&name).is_none());
        let correctness = Name::new("eggsec-waf").unwrap();
        assert!(catalog.correctness(&correctness).is_none());
    }
}
