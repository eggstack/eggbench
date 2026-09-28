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
use std::net::Ipv4Addr;
use std::pin::Pin;
use std::sync::Arc;

/// Name of the deterministic built-in liveness probe.
pub const PROCESS_ALIVE_PROBE: &str = "process-alive";
/// Name of the deterministic built-in TCP-loopback readiness probe.
pub const TCP_LOOPBACK_PROBE: &str = "tcp-loopback";
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
    /// Static non-secret `http_url` binding from the service declaration,
    /// when the service exposes a known loopback endpoint. Used by the
    /// `tcp-loopback` probe; absent otherwise.
    pub http_url: Option<String>,
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
        registry.register(TCP_LOOPBACK_PROBE, TcpLoopbackProbe);
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

/// Loopback TCP-connect readiness probe.
///
/// Verifies that a managed service exposing a static `http_url` binding has
/// actually accepted a TCP connection on the declared port. Replaces the
/// fixed-delay readiness check for managed command subjects whose bind time
/// varies across platforms (macOS Python startup, Apple Silicon cold cache,
/// etc.); the runner no longer depends on a guessed startup delay.
///
/// Only loopback IPv4 bindings are accepted; non-loopback or non-HTTP
/// bindings are rejected explicitly without any network I/O.
#[derive(Debug, Clone, Copy, Default)]
pub struct TcpLoopbackProbe;

impl ReadinessProbe for TcpLoopbackProbe {
    fn check<'a>(&'a self, ctx: &'a ProbeContext) -> ProbeFuture<'a> {
        Box::pin(async move {
            let url = ctx.http_url.as_deref().ok_or_else(|| {
                ProbeFailure(format!(
                    "service {} has no http_url binding for tcp-loopback probe",
                    ctx.service
                ))
            })?;
            let target = parse_loopback_http_url(url).map_err(|error| {
                ProbeFailure(format!(
                    "service {} http_url is not a loopback http binding: {error}",
                    ctx.service
                ))
            })?;
            let stream = tokio::task::spawn_blocking(move || {
                std::net::TcpStream::connect((target.host, target.port))
            })
            .await
            .map_err(|error| ProbeFailure(format!("tcp-loopback probe task panicked: {error}")))?
            .map_err(|error| {
                ProbeFailure(format!(
                    "tcp-loopback connect to {}:{} failed: {error}",
                    target.host, target.port
                ))
            })?;
            drop(stream);
            Ok(())
        })
    }
}

/// Parsed host/port from a loopback `http://` URL.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LoopbackHttpTarget {
    host: Ipv4Addr,
    port: u16,
}

/// Parse a loopback `http://host:port[/path]` URL into a [`LoopbackHttpTarget`].
///
/// The runner confines this probe to loopback IPv4 because that is the only
/// scope the runner ever issues subject bindings for; non-loopback bindings
/// are rejected explicitly so a future schema change cannot silently broaden
/// the probe's network reach.
fn parse_loopback_http_url(url: &str) -> Result<LoopbackHttpTarget, String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| "binding must use http:// scheme".to_owned())?;
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() {
        return Err("binding has empty authority".to_owned());
    }
    if authority.contains('@') {
        return Err("binding must not contain credentials".to_owned());
    }
    let Some((host_str, port_str)) = authority.rsplit_once(':') else {
        return Err("binding authority must include an explicit port".to_owned());
    };
    let host: Ipv4Addr = host_str
        .parse()
        .map_err(|_| format!("binding host {host_str:?} is not an IPv4 literal"))?;
    if !host.is_loopback() {
        return Err(format!(
            "binding host {host:?} is not loopback; tcp-loopback probe only accepts loopback"
        ));
    }
    let port: u16 = port_str
        .parse()
        .map_err(|_| format!("binding port {port_str:?} is not u16"))?;
    if port == 0 {
        return Err("binding port 0 is reserved".to_owned());
    }
    Ok(LoopbackHttpTarget { host, port })
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
