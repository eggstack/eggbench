//! Declarative readiness probe dispatch.
//!
//! Core's readiness request names a probe without defining transport
//! semantics. The runner owns a registry of named probes and rejects unknown
//! names with an explicit capability error. HTTP/TCP parameters are never
//! inferred from opaque strings.

use eggbench_core::Name;
use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Name of the deterministic built-in liveness probe.
pub const PROCESS_ALIVE_PROBE: &str = "process-alive";
/// Name of the built-in fake probe that always reports ready.
pub const FAKE_OK_PROBE: &str = "fake-ok";
/// Name of the built-in fake probe that always reports failure.
pub const FAKE_FAIL_PROBE: &str = "fake-fail";
/// Name of the built-in fake probe that never reports ready.
pub const FAKE_NEVER_PROBE: &str = "fake-never";

/// Observation available to a readiness probe.
#[derive(Debug, Clone)]
pub struct ProbeContext {
    /// Service identity under test.
    pub service: String,
    /// Diagnostic process identifier, if the process has started.
    pub pid: Option<u32>,
    /// Whether the observed process is currently alive.
    pub alive: bool,
}

/// Readiness probe failure detail without secret values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeFailure(pub String);

impl fmt::Display for ProbeFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Object-safe readiness probe evaluated under a caller-side timeout.
pub trait ReadinessProbe: Send + Sync + fmt::Debug {
    /// Check readiness once; the caller bounds the wait with the declared timeout.
    fn check<'a>(
        &'a self,
        ctx: &'a ProbeContext,
    ) -> Pin<Box<dyn Future<Output = Result<(), ProbeFailure>> + Send + 'a>>;
}

type ProbeFuture<'a> = Pin<Box<dyn Future<Output = Result<(), ProbeFailure>> + Send + 'a>>;

/// Probe registry with deterministic built-ins and test fakes.
#[derive(Debug, Clone, Default)]
pub struct ProbeRegistry {
    probes: BTreeMap<String, Arc<dyn ReadinessProbe>>,
}

impl ProbeRegistry {
    /// Registry with the deterministic built-in probes registered.
    #[must_use]
    pub fn with_builtins() -> Self {
        let mut registry = Self::default();
        registry.register(PROCESS_ALIVE_PROBE, ProcessAliveProbe);
        registry.register(FAKE_OK_PROBE, FakeOkProbe);
        registry.register(FAKE_FAIL_PROBE, FakeFailProbe);
        registry.register(FAKE_NEVER_PROBE, FakeNeverProbe);
        registry
    }

    /// Register or replace one named probe.
    pub fn register(&mut self, name: impl Into<String>, probe: impl ReadinessProbe + 'static) {
        self.probes.insert(name.into(), Arc::new(probe));
    }

    /// Look up one named probe.
    #[must_use]
    pub fn get(&self, name: &Name) -> Option<Arc<dyn ReadinessProbe>> {
        self.probes.get(name.as_str()).cloned()
    }
}

/// Deterministic liveness probe: ready while the process is alive.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessAliveProbe;

impl ReadinessProbe for ProcessAliveProbe {
    fn check<'a>(&'a self, ctx: &'a ProbeContext) -> ProbeFuture<'a> {
        Box::pin(async move {
            if ctx.alive {
                Ok(())
            } else {
                Err(ProbeFailure(format!(
                    "process {} is not alive",
                    ctx.service
                )))
            }
        })
    }
}

/// Fake probe that always reports ready; for deterministic tests.
#[derive(Debug, Clone, Copy, Default)]
pub struct FakeOkProbe;

impl ReadinessProbe for FakeOkProbe {
    fn check<'a>(&'a self, _ctx: &'a ProbeContext) -> ProbeFuture<'a> {
        Box::pin(async move { Ok(()) })
    }
}

/// Fake probe that always reports failure; for deterministic tests.
#[derive(Debug, Clone, Copy, Default)]
pub struct FakeFailProbe;

impl ReadinessProbe for FakeFailProbe {
    fn check<'a>(&'a self, ctx: &'a ProbeContext) -> ProbeFuture<'a> {
        Box::pin(async move {
            Err(ProbeFailure(format!(
                "fake probe failed for {}",
                ctx.service
            )))
        })
    }
}

/// Fake probe that never reports ready; the caller-side timeout must fire.
#[derive(Debug, Clone, Copy, Default)]
pub struct FakeNeverProbe;

impl ReadinessProbe for FakeNeverProbe {
    fn check<'a>(&'a self, _ctx: &'a ProbeContext) -> ProbeFuture<'a> {
        Box::pin(async move {
            std::future::pending::<()>().await;
            Ok(())
        })
    }
}
