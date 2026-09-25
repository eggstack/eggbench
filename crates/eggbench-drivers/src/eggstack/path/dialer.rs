//! Route-first, fault-second Eggfetch dialer.

#![allow(clippy::missing_errors_doc)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_panics_doc)]

use super::evidence::{
    NetworkPathEvidence, PathDiagnosticsSnapshot, SharedPathDiagnostics, build_evidence,
};
use crate::eggstack::path::route::{build_route_chain, parse_route_chain, redacted_chain_text};
use eggbench_core::ResolvedNetworkPath;
use eggchaos_core::{BidirectionalChaosStream, LivePolicy};
use eggfetch_core::transport::dialer::DialStreamTrait;
use eggfetch_core::{
    DialError, DialErrorKind, DialFuture, DialStream, DialTarget, Dialer as EggfetchDialer,
};
use eggress_core::BoxStream;
use eggress_outbound::{OutboundConnectError, OutboundConnectErrorKind, OutboundConnector};
use sha2::Digest;
use std::io::IoSlice;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Safe adapter from an Eggress boxed stream to Eggfetch's stream trait object.
pub struct AsyncStreamDialStream {
    inner: BoxStream,
}

impl AsyncStreamDialStream {
    /// Wrap one Eggress stream without a raw-pointer cast.
    #[must_use]
    pub fn new(inner: BoxStream) -> Self {
        Self { inner }
    }
}

impl AsyncRead for AsyncStreamDialStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut *self.get_mut().inner).poll_read(cx, buffer)
    }
}

impl AsyncWrite for AsyncStreamDialStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut *self.get_mut().inner).poll_write(cx, bytes)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffers: &[IoSlice<'_>],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut *self.get_mut().inner).poll_write_vectored(cx, buffers)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut *self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut *self.get_mut().inner).poll_shutdown(cx)
    }
}

/// Construct a shared diagnostics slot for one workload executor.
#[must_use]
pub fn new_path_diagnostics() -> Arc<SharedPathDiagnostics> {
    Arc::new(SharedPathDiagnostics::new())
}

fn validate_resolved_provenance(resolved: &ResolvedNetworkPath) -> Result<(), String> {
    let route = &resolved.route_driver;
    if route.executable_path.is_some()
        || route.descriptor.external_process
        || route.descriptor.category != eggbench_core::DriverCategory::Route
        || route.descriptor.name.as_str() != super::ROUTE_DRIVER_NAME
        || route.descriptor.upstream_name != "eggress-outbound"
        || route.descriptor.upstream_version.as_deref() != Some(super::EGGRESS_OUTBOUND_VERSION)
        || !route
            .descriptor
            .capabilities
            .contains(&eggbench_core::Capability::ProxyRouting)
    {
        return Err(
            "resolved route provenance does not match the pinned Eggress adapter".to_owned(),
        );
    }
    if resolved.semantics_version != eggbench_core::NETWORK_PATH_SEMANTICS_VERSION {
        return Err("resolved route semantics version is unsupported".to_owned());
    }
    if let Some(faults) = &resolved.stream_faults {
        let fault = &faults.fault_driver;
        if fault.executable_path.is_some()
            || fault.descriptor.external_process
            || fault.descriptor.category != eggbench_core::DriverCategory::Fault
            || fault.descriptor.name.as_str() != super::FAULT_DRIVER_NAME
            || fault.descriptor.upstream_name != "eggchaos-core"
            || fault.descriptor.upstream_version.as_deref() != Some(super::EGGCHAOS_CORE_VERSION)
            || !fault
                .descriptor
                .capabilities
                .contains(&eggbench_core::Capability::StreamFaultPlan)
            || faults.rng_version != eggbench_core::NETWORK_PATH_RNG_VERSION
        {
            return Err(
                "resolved fault provenance does not match the pinned Eggchaos adapter".to_owned(),
            );
        }
    }
    Ok(())
}

/// Lower one resolved network path into an immutable run-scoped dialer.
///
/// # Errors
/// Returns a bounded reason when route, fault, or deterministic seed
/// configuration cannot be lowered safely.
pub fn lower_dialer(
    resolved: &ResolvedNetworkPath,
    seed_namespace: Option<u64>,
    diagnostics: Arc<SharedPathDiagnostics>,
    connect_timeout: Duration,
) -> Result<EggstackPathDialer, String> {
    validate_resolved_provenance(resolved)?;
    let request = eggbench_core::NetworkPathRequest {
        route: resolved.route.clone(),
        stream_faults: resolved
            .stream_faults
            .as_ref()
            .map(|faults| faults.request.clone()),
    };
    eggbench_core::validate_network_path_contract(&request).map_err(|error| error.to_string())?;
    let fault_requests = resolved
        .stream_faults
        .as_ref()
        .map(|faults| &faults.request);
    let has_faults = fault_requests
        .is_some_and(|faults| !faults.upstream.is_empty() || !faults.downstream.is_empty());
    if has_faults && seed_namespace.is_none() {
        return Err("missing_fault_seed".to_owned());
    }

    let connector =
        Arc::new(build_route_chain(&resolved.route.mode).map_err(|error| error.to_string())?);
    let (redacted_chain, chain_config_digest, configured_hop_count) = match &resolved.route.mode {
        eggbench_core::RouteMode::Direct => (None, None, 0),
        eggbench_core::RouteMode::ProxyChain { chain } => {
            let canonical = redacted_chain_text(chain)?;
            let digest = format!("{:x}", sha2::Sha256::digest(canonical.as_bytes()));
            let hop_count = u16::try_from(parse_route_chain(chain)?.hops.len())
                .map_err(|_| "route hop count exceeds evidence bound".to_owned())?;
            (Some(canonical), Some(digest), hop_count)
        }
    };

    let seed = seed_namespace.unwrap_or(0);
    let (upstream_policy, downstream_policy) = match fault_requests {
        Some(requests) => (
            LivePolicy::new(build_plan(&requests.upstream)?, seed),
            LivePolicy::new(build_plan(&requests.downstream)?, seed),
        ),
        None => (
            LivePolicy::new(eggchaos_core::FaultPlan::empty(), seed),
            LivePolicy::new(eggchaos_core::FaultPlan::empty(), seed),
        ),
    };

    Ok(EggstackPathDialer {
        resolved: resolved.clone(),
        seed_namespace,
        upstream_policy,
        downstream_policy,
        connector,
        diagnostics,
        connection_ordinal: AtomicU64::new(0),
        connect_timeout,
        has_faults,
        redacted_chain,
        chain_config_digest,
        configured_hop_count,
    })
}

fn build_plan(
    requests: &[eggbench_core::StreamFaultRequest],
) -> Result<eggchaos_core::FaultPlan, String> {
    super::fault::build_fault_plan(requests).map_err(|error| error.to_string())
}

/// Immutable route-first/fault-second dialer for one Eggfetch client.
pub struct EggstackPathDialer {
    resolved: ResolvedNetworkPath,
    seed_namespace: Option<u64>,
    upstream_policy: LivePolicy,
    downstream_policy: LivePolicy,
    connector: Arc<OutboundConnector>,
    diagnostics: Arc<SharedPathDiagnostics>,
    connection_ordinal: AtomicU64,
    connect_timeout: Duration,
    has_faults: bool,
    redacted_chain: Option<String>,
    chain_config_digest: Option<String>,
    configured_hop_count: u16,
}

#[allow(clippy::missing_fields_in_debug)]
impl std::fmt::Debug for EggstackPathDialer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EggstackPathDialer")
            .field("route_driver", &self.resolved.route_driver.descriptor.name)
            .field(
                "fault_driver",
                &self
                    .resolved
                    .stream_faults
                    .as_ref()
                    .map(|faults| &faults.fault_driver.descriptor.name),
            )
            .field("seed_namespace", &self.seed_namespace)
            .field("configured_hop_count", &self.configured_hop_count)
            .field("has_faults", &self.has_faults)
            .finish()
    }
}

impl EggstackPathDialer {
    /// Snapshot run-level path counters.
    #[must_use]
    pub fn diagnostics_snapshot(&self) -> PathDiagnosticsSnapshot {
        self.diagnostics.snapshot()
    }

    /// Whether this dialer has a non-empty static fault policy.
    #[must_use]
    pub const fn faults_active(&self) -> bool {
        self.has_faults
    }

    /// Begin one invocation and capture its pre-invocation counters.
    #[must_use]
    pub fn begin_invocation(&self) -> PathDiagnosticsSnapshot {
        self.diagnostics.begin_invocation(self.current_ordinal())
    }

    /// Return one invocation's coherent diagnostics delta.
    #[must_use]
    pub fn invocation_delta(&self, before: &PathDiagnosticsSnapshot) -> PathDiagnosticsSnapshot {
        self.diagnostics.invocation_delta(before)
    }

    /// First ordinal that a new invocation may receive.
    #[must_use]
    pub fn current_ordinal(&self) -> u64 {
        self.connection_ordinal.load(Ordering::SeqCst)
    }

    /// Build final run evidence from retained, credential-free configuration.
    #[must_use]
    pub fn network_path_evidence(&self) -> NetworkPathEvidence {
        build_evidence(
            &self.resolved,
            self.redacted_chain.clone(),
            self.chain_config_digest.clone(),
            self.configured_hop_count,
            self.seed_namespace,
            self.diagnostics.snapshot(),
        )
    }

    fn next_ordinal(&self) -> u64 {
        self.connection_ordinal.fetch_add(1, Ordering::SeqCst)
    }
}

impl EggfetchDialer for EggstackPathDialer {
    fn dial(&self, target: DialTarget) -> DialFuture<'_> {
        let ordinal = self.next_ordinal();
        self.diagnostics.record_dial_attempt();
        let connector = Arc::clone(&self.connector);
        let upstream = self.upstream_policy.clone();
        let downstream = self.downstream_policy.clone();
        let diagnostics = Arc::clone(&self.diagnostics);
        let timeout = self.connect_timeout;
        let has_faults = self.has_faults;
        Box::pin(async move {
            let (stream, info) = connector
                .connect_tcp_timeout_detailed(target.host(), target.port(), timeout)
                .await
                .map_err(|failure| {
                    diagnostics.record_route_failure(route_failure_label(&failure));
                    dial_error(failure)
                })?;
            diagnostics
                .record_successful_dial(u64::try_from(info.hop_count).unwrap_or(u64::MAX), ordinal);
            let stream: DialStream = Box::new(AsyncStreamDialStream::new(stream));
            if !has_faults {
                return Ok(stream);
            }
            BidirectionalChaosStream::new_live(stream, upstream, downstream, "eggress", ordinal)
                .map(|wrapped| Box::new(wrapped) as DialStream)
                .map_err(|error| {
                    diagnostics.record_fault_wrapper_construction_failure();
                    DialError::new(
                        DialErrorKind::Other,
                        format!("eggchaos stream construction failed: {error}"),
                    )
                })
                .inspect(|_| diagnostics.record_fault_wrapped_connection())
        })
    }
}

fn route_failure_label(failure: &OutboundConnectError) -> String {
    let mut label = format!("{}:{}", failure.kind(), failure.stage());
    if let Some(hop) = failure.hop_index() {
        label.push_str(":hop=");
        label.push_str(&hop.to_string());
    }
    if let Some(protocol) = failure.protocol() {
        label.push_str(":protocol=");
        label.push_str(protocol);
    }
    label
}

fn dial_error(failure: OutboundConnectError) -> DialError {
    let kind = match failure.kind() {
        OutboundConnectErrorKind::Timeout => DialErrorKind::Timeout,
        OutboundConnectErrorKind::Authentication => DialErrorKind::Authentication,
        OutboundConnectErrorKind::Policy => DialErrorKind::Rejected,
        _ => DialErrorKind::Connection,
    };
    DialError::with_source(kind, failure.to_string(), failure)
}

#[allow(dead_code)]
fn assert_send_unpin<T: Send + Unpin + DialStreamTrait>() {}
#[allow(dead_code)]
fn assert_stream_adapter<T: Send + Unpin + DialStreamTrait>() {}

#[allow(dead_code)]
fn assert_adapter_types(
    _: Option<AsyncStreamDialStream>,
    _: Option<BidirectionalChaosStream<DialStream>>,
) {
    assert_send_unpin::<AsyncStreamDialStream>();
    assert_send_unpin::<BidirectionalChaosStream<DialStream>>();
    assert_stream_adapter::<AsyncStreamDialStream>();
    assert_stream_adapter::<BidirectionalChaosStream<DialStream>>();
}
