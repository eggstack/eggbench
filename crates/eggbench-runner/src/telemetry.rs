//! Generic trial-synchronized telemetry-collector seam.
//!
//! Workload adapters measure; telemetry collectors observe alongside.
//! Collectors are object-safe, keyed by source name in a
//! [`TelemetryRegistry`], and run strictly outside measured workload
//! timing: `start_trial` precedes the measurement timer, `stop_trial`
//! follows the captured elapsed time. Core never depends on concrete
//! telemetry backends: the Gregg collector lives in `eggbench-drivers`
//! behind the `gregg` feature.
//!
//! Telemetry output reuses the shared [`WorkloadArtifact`] artifact type and
//! feeds the same post-measurement normalization pipeline as workload
//! metrics. Collectors never write normalized `TrialMetrics` JSON
//! themselves.

use crate::orchestration::{DrainContext, WorkloadArtifact};
use eggbench_core::{MetricWarning, RawMetricObservation, RunId, TrialId};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Boxed sendable future used by the object-safe telemetry contract.
pub type TelemetryFuture<'a, T> =
    std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// Context for collector preflight, before managed startup.
#[derive(Debug, Clone)]
pub struct TelemetryPreflightContext {
    /// Bundle/run identity.
    pub run_id: RunId,
    /// Cancellation token for the preflight probes.
    pub cancellation: CancellationToken,
    /// Safety bound for the whole preflight exchange.
    pub timeout: Duration,
}

/// Context for one measured trial's telemetry window.
#[derive(Debug, Clone)]
pub struct TelemetryTrialContext {
    /// Bundle/run identity.
    pub run_id: RunId,
    /// Stable measured-trial identity.
    pub trial_id: TrialId,
    /// Cancellation token for the trial window.
    pub cancellation: CancellationToken,
    /// Safety bound for start/stop exchanges (polling itself is cadenced).
    pub timeout: Duration,
}

/// Capability established by successful preflight.
#[derive(Debug, Clone)]
pub struct TelemetryCapability {
    /// Bounded polling cadence derived from the backend's native sampling.
    pub poll_interval: Duration,
    /// Backend identity fields (host, system) without credentials.
    pub identity: BTreeMap<String, String>,
}

/// Stable telemetry error category plus bounded human detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetryError {
    /// Stable category (for example `health_unavailable`).
    pub category: &'static str,
    /// Bounded redaction-safe detail.
    pub detail: String,
}

impl TelemetryError {
    /// Build an error with detail truncated to the evidence bound.
    pub fn new(category: &'static str, detail: impl Into<String>) -> Self {
        let detail: String = detail.into();
        Self {
            category,
            detail: detail.chars().take(MAX_TELEMETRY_DETAIL_LEN).collect(),
        }
    }
}

impl fmt::Display for TelemetryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.category, self.detail)
    }
}

/// Maximum characters retained in a telemetry error detail.
pub const MAX_TELEMETRY_DETAIL_LEN: usize = 512;

/// Protocol-neutral telemetry output for one measured trial.
///
/// Artifacts share the [`WorkloadArtifact`] type; metrics feed the same
/// normalization pipeline as workload observations (per-observation
/// producer overrides attribute them to the collector); warnings surface
/// as trial-level `MetricWarning` entries.
#[derive(Debug, Clone, Default)]
pub struct TelemetryOutput {
    /// Optional diagnostic files, bounded during bundle staging.
    pub artifacts: Vec<WorkloadArtifact>,
    /// Raw metric observations for post-measurement normalization.
    pub metrics: Vec<RawMetricObservation>,
    /// Trial-level warnings (for example partial-sample notices).
    pub warnings: Vec<MetricWarning>,
}

/// Object-safe trial-synchronized telemetry collector.
pub trait TelemetryCollector: Send {
    /// Stable source label matching `TelemetryRequest.source` (for example `gregg`).
    fn source(&self) -> &'static str;

    /// Validate the backend before managed startup. No measurement occurs.
    fn preflight(
        &mut self,
        context: TelemetryPreflightContext,
    ) -> TelemetryFuture<'_, Result<TelemetryCapability, TelemetryError>>;

    /// Open the trial observation window. Must precede the workload timer.
    fn start_trial(
        &mut self,
        context: TelemetryTrialContext,
    ) -> TelemetryFuture<'_, Result<(), TelemetryError>>;

    /// Close the window and return captured samples. Must follow the
    /// captured workload elapsed. Returns partial samples with warnings
    /// rather than failing when the final snapshot alone is unavailable.
    fn stop_trial(
        &mut self,
        context: TelemetryTrialContext,
    ) -> TelemetryFuture<'_, Result<TelemetryOutput, TelemetryError>>;

    /// Drain collector-owned tasks/resources after experimental work.
    fn drain(&mut self, context: DrainContext) -> TelemetryFuture<'_, Result<(), TelemetryError>>;
}

/// Explicit registry of telemetry collectors in registration order.
#[derive(Default)]
pub struct TelemetryRegistry {
    collectors: Vec<Box<dyn TelemetryCollector>>,
}

impl TelemetryRegistry {
    /// Empty registry: no telemetry source is collected.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a collector. Duplicate sources are rejected.
    ///
    /// # Errors
    /// Returns a human-readable reason when the source is already registered.
    pub fn register(&mut self, collector: Box<dyn TelemetryCollector>) -> Result<(), String> {
        let source = collector.source();
        if self.collectors.iter().any(|c| c.source() == source) {
            return Err(format!("duplicate telemetry collector {source}"));
        }
        self.collectors.push(collector);
        Ok(())
    }

    /// Look up a collector by source name.
    pub fn get_mut(&mut self, source: &str) -> Option<&mut Box<dyn TelemetryCollector>> {
        self.collectors
            .iter_mut()
            .find(|collector| collector.source() == source)
    }

    /// True when no collector is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.collectors.is_empty()
    }

    /// Registered source labels in registration order.
    #[must_use]
    pub fn sources(&self) -> Vec<&'static str> {
        self.collectors
            .iter()
            .map(|collector| collector.source())
            .collect()
    }
}

impl fmt::Debug for TelemetryRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelemetryRegistry")
            .field("sources", &self.sources())
            .finish()
    }
}

/// Test/debug double recording preflight/start/stop/drain calls.
///
/// Never used in production; qualification tests prove phase ordering and
/// timing exclusion through it. Re-exported through
/// [`crate::test_support`].
#[derive(Debug, Default)]
pub struct FakeTelemetryCollector {
    source_label: &'static str,
    preflights: usize,
    starts: Vec<u32>,
    stops: Vec<u32>,
    drains: usize,
    start_delay: Duration,
    stop_delay: Duration,
    fail_preflight: bool,
    fail_start_on: Option<u32>,
    fail_stop_on: Option<u32>,
    fail_drain: bool,
    output: TelemetryOutput,
}

impl FakeTelemetryCollector {
    /// Collector double with a fixed output returned from every stop.
    #[must_use]
    pub fn new(source_label: &'static str, output: TelemetryOutput) -> Self {
        Self {
            source_label,
            output,
            ..Self::default()
        }
    }

    /// Inject artificial start latency to prove timing exclusion.
    #[must_use]
    pub fn with_start_delay(mut self, delay: Duration) -> Self {
        self.start_delay = delay;
        self
    }

    /// Inject artificial stop latency to prove timing exclusion.
    #[must_use]
    pub fn with_stop_delay(mut self, delay: Duration) -> Self {
        self.stop_delay = delay;
        self
    }

    /// Fail `start_trial` for one trial identity.
    #[must_use]
    pub fn with_start_failure(mut self, trial: u32) -> Self {
        self.fail_start_on = Some(trial);
        self
    }

    /// Fail `stop_trial` for one trial identity.
    #[must_use]
    pub fn with_stop_failure(mut self, trial: u32) -> Self {
        self.fail_stop_on = Some(trial);
        self
    }

    /// Fail `preflight` unconditionally.
    #[must_use]
    pub fn with_preflight_failure(mut self) -> Self {
        self.fail_preflight = true;
        self
    }

    /// Fail `drain` unconditionally.
    #[must_use]
    pub fn with_drain_failure(mut self) -> Self {
        self.fail_drain = true;
        self
    }

    /// Recorded trial identities passed to `start_trial`, in order.
    #[must_use]
    pub fn starts(&self) -> &[u32] {
        &self.starts
    }

    /// Recorded trial identities passed to `stop_trial`, in order.
    #[must_use]
    pub fn stops(&self) -> &[u32] {
        &self.stops
    }

    /// Number of preflight calls.
    #[must_use]
    pub fn preflights(&self) -> usize {
        self.preflights
    }

    /// Number of drain calls.
    #[must_use]
    pub fn drains(&self) -> usize {
        self.drains
    }
}

impl TelemetryCollector for FakeTelemetryCollector {
    fn source(&self) -> &'static str {
        self.source_label
    }

    fn preflight(
        &mut self,
        _context: TelemetryPreflightContext,
    ) -> TelemetryFuture<'_, Result<TelemetryCapability, TelemetryError>> {
        Box::pin(async move {
            self.preflights += 1;
            if self.fail_preflight {
                return Err(TelemetryError::new(
                    "status_unavailable",
                    "injected preflight failure",
                ));
            }
            Ok(TelemetryCapability {
                poll_interval: Duration::from_secs(1),
                identity: BTreeMap::new(),
            })
        })
    }

    fn start_trial(
        &mut self,
        context: TelemetryTrialContext,
    ) -> TelemetryFuture<'_, Result<(), TelemetryError>> {
        Box::pin(async move {
            if !self.start_delay.is_zero() {
                tokio::time::sleep(self.start_delay).await;
            }
            let trial = context.trial_id.get();
            self.starts.push(trial);
            if self.fail_start_on == Some(trial) {
                return Err(TelemetryError::new(
                    "polling_failed",
                    "injected start failure",
                ));
            }
            Ok(())
        })
    }

    fn stop_trial(
        &mut self,
        context: TelemetryTrialContext,
    ) -> TelemetryFuture<'_, Result<TelemetryOutput, TelemetryError>> {
        Box::pin(async move {
            if !self.stop_delay.is_zero() {
                tokio::time::sleep(self.stop_delay).await;
            }
            let trial = context.trial_id.get();
            self.stops.push(trial);
            if self.fail_stop_on == Some(trial) {
                return Err(TelemetryError::new(
                    "polling_failed",
                    "injected stop failure",
                ));
            }
            Ok(self.output.clone())
        })
    }

    fn drain(&mut self, _context: DrainContext) -> TelemetryFuture<'_, Result<(), TelemetryError>> {
        Box::pin(async move {
            self.drains += 1;
            if self.fail_drain {
                return Err(TelemetryError::new(
                    "polling_failed",
                    "injected drain failure",
                ));
            }
            Ok(())
        })
    }
}

/// Shared handle for asserting on a registered fake collector.
#[derive(Debug, Clone)]
pub struct FakeTelemetryHandle {
    inner: Arc<tokio::sync::Mutex<FakeTelemetryCollector>>,
}

impl FakeTelemetryHandle {
    /// Wrap a fake collector for registry registration with later inspection.
    #[must_use]
    pub fn wrap(collector: FakeTelemetryCollector) -> (Self, Box<dyn TelemetryCollector>) {
        let source_label = collector.source_label;
        let handle = Self {
            inner: Arc::new(tokio::sync::Mutex::new(collector)),
        };
        let proxy = HandleProxy::wrap(handle.clone(), source_label);
        (handle, proxy)
    }

    /// Snapshot recorded starts.
    pub async fn starts(&self) -> Vec<u32> {
        self.inner.lock().await.starts().to_vec()
    }

    /// Snapshot recorded stops.
    pub async fn stops(&self) -> Vec<u32> {
        self.inner.lock().await.stops().to_vec()
    }

    /// Snapshot preflight count.
    pub async fn preflights(&self) -> usize {
        self.inner.lock().await.preflights()
    }

    /// Snapshot drain count.
    pub async fn drains(&self) -> usize {
        self.inner.lock().await.drains()
    }
}

#[derive(Debug)]
struct HandleProxy {
    handle: FakeTelemetryHandle,
    source_label: &'static str,
}

impl HandleProxy {
    fn wrap(
        handle: FakeTelemetryHandle,
        source_label: &'static str,
    ) -> Box<dyn TelemetryCollector> {
        Box::new(Self {
            handle,
            source_label,
        })
    }
}

impl TelemetryCollector for HandleProxy {
    fn source(&self) -> &'static str {
        self.source_label
    }

    fn preflight(
        &mut self,
        context: TelemetryPreflightContext,
    ) -> TelemetryFuture<'_, Result<TelemetryCapability, TelemetryError>> {
        Box::pin(async move { self.handle.inner.lock().await.preflight(context).await })
    }

    fn start_trial(
        &mut self,
        context: TelemetryTrialContext,
    ) -> TelemetryFuture<'_, Result<(), TelemetryError>> {
        Box::pin(async move { self.handle.inner.lock().await.start_trial(context).await })
    }

    fn stop_trial(
        &mut self,
        context: TelemetryTrialContext,
    ) -> TelemetryFuture<'_, Result<TelemetryOutput, TelemetryError>> {
        Box::pin(async move { self.handle.inner.lock().await.stop_trial(context).await })
    }

    fn drain(&mut self, context: DrainContext) -> TelemetryFuture<'_, Result<(), TelemetryError>> {
        Box::pin(async move { self.handle.inner.lock().await.drain(context).await })
    }
}
