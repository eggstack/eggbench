//! Injected secret resolution at the process boundary.
//!
//! Plans carry [`eggbench_core::SecretRef`] references only. Values are
//! supplied by the caller through this trait and are set as process
//! environment entries at spawn time. Values are never stored in specs,
//! events, errors, or bundle artifacts beyond the live process environment.

use eggbench_core::Name;
use std::collections::BTreeMap;
use std::fmt;

/// Caller-supplied secret values keyed by reference name.
///
/// Implementations must not log resolved values.
pub trait SecretProvider: Send + Sync + fmt::Debug {
    /// Resolve one reference to its value, or `None` when unknown.
    fn resolve(&self, reference: &Name) -> Option<String>;
}

/// Map-backed secret provider for tests and simple embeddings.
///
/// `Debug` reports reference names only; values are always redacted.
#[derive(Clone, Default)]
pub struct MapSecretProvider {
    /// Reference text to secret value.
    values: BTreeMap<String, String>,
}

impl fmt::Debug for MapSecretProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let references: Vec<&str> = self.values.keys().map(String::as_str).collect();
        f.debug_struct("MapSecretProvider")
            .field("references", &references)
            .field("values", &"[REDACTED]")
            .finish()
    }
}

impl MapSecretProvider {
    /// Create an empty provider that resolves nothing.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Insert one reference-to-value mapping.
    pub fn insert(&mut self, reference: &Name, value: impl Into<String>) {
        self.values
            .insert(reference.as_str().to_owned(), value.into());
    }

    /// Build a provider from one mapping.
    #[must_use]
    pub fn with(reference: &Name, value: impl Into<String>) -> Self {
        let mut provider = Self::default();
        provider.insert(reference, value);
        provider
    }
}

impl SecretProvider for MapSecretProvider {
    fn resolve(&self, reference: &Name) -> Option<String> {
        self.values.get(reference.as_str()).cloned()
    }
}
