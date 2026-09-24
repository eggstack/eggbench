//! Gregg trial-synchronized telemetry collector.
//!
//! Polls a loopback Gregg daemon's v2 endpoints with Eggfetch transport
//! (no second HTTP client), validates every payload with `gregg-protocol`,
//! and aggregates bounded trial-level host metrics. Gregg owns telemetry
//! collection and wire semantics; Eggbench owns scheduling, aggregation,
//! and evidence.
//!
//! Window discipline: one snapshot immediately after start, cadenced
//! polling at `clamp(daemon_sample_interval_ms, 250ms, 5s)`, one final
//! snapshot at stop when the budget allows. Identical
//! `observed_at_unix_ms` snapshots are deduplicated. At most
//! [`MAX_SAMPLES_PER_TRIAL`] snapshots and [`MAX_NDJSON_BYTES`] raw bytes
//! are retained per trial; the aggregation set always equals the retained
//! set, and overflow is reported as dropped counts plus a warning.

use super::endpoint::{GreggEndpoint, HEALTH_PATH, STATUS_PATH, validate_endpoint};
use super::{GREGG_PROTOCOL_VERSION, GREGG_SOURCE};
use eggbench_core::{Aggregation, MetricWarning, RawMetricObservation};
use eggbench_runner::{
    DrainContext, TelemetryCapability, TelemetryCollector, TelemetryError, TelemetryFuture,
    TelemetryOutput, TelemetryPreflightContext, TelemetryTrialContext, WorkloadArtifact,
};
use gregg_protocol::ReadinessState;
use gregg_protocol::v2::{HealthResponseV2, SCHEMA_VERSION_V2, StatusPayloadV2};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Raw Gregg series artifact staged per measured trial.
pub const GREGG_NDJSON_ARTIFACT: &str = "gregg.ndjson";
/// Endpoint/method provenance artifact staged per measured trial.
pub const GREGG_PROVENANCE_ARTIFACT: &str = "gregg-provenance.json";
/// Format identifier for the retained series.
pub const GREGG_SERIES_FORMAT: &str = "gregg-v2-ndjson";
/// Maximum retained snapshots per trial (aggregation == retained set).
pub const MAX_SAMPLES_PER_TRIAL: usize = 256;
/// Maximum retained raw series bytes per trial.
pub const MAX_NDJSON_BYTES: usize = 262_144;
/// Per-request timeout for health/status fetches.
pub const GREGG_REQUEST_TIMEOUT_SECS: u64 = 10;
/// Minimum polling cadence: never poll faster than the daemon updates.
pub const MIN_POLL_INTERVAL: Duration = Duration::from_millis(250);
/// Maximum polling cadence.
pub const MAX_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Host metric names collected by M001b with their aggregation.
///
/// CPU percent: arithmetic mean of `v2.cpu.usage_pct`.
pub const HOST_CPU_PERCENT: &str = "host_cpu_percent";
/// Memory used bytes: maximum of `v2.memory.used_bytes`.
pub const HOST_MEMORY_USED_BYTES: &str = "host_memory_used_bytes";
/// Memory percent: maximum of `v2.memory.usage_pct`.
pub const HOST_MEMORY_PERCENT: &str = "host_memory_percent";
/// CPU frequency: arithmetic mean of present `v2.cpu_frequency_hz` values.
pub const HOST_CPU_FREQUENCY_HZ: &str = "host_cpu_frequency_hz";
/// Disk read rate: arithmetic mean of `v2.disk_io.aggregate_read_bytes_per_sec`.
pub const HOST_DISK_READ_BYTES_PER_SEC: &str = "host_disk_read_bytes_per_sec";
/// Disk write rate: arithmetic mean of `v2.disk_io.aggregate_write_bytes_per_sec`.
pub const HOST_DISK_WRITE_BYTES_PER_SEC: &str = "host_disk_write_bytes_per_sec";
/// Network receive rate: arithmetic mean of `v2.network.aggregate_rx_bytes_per_sec`.
pub const HOST_NETWORK_RX_BYTES_PER_SEC: &str = "host_network_rx_bytes_per_sec";
/// Network transmit rate: arithmetic mean of `v2.network.aggregate_tx_bytes_per_sec`.
pub const HOST_NETWORK_TX_BYTES_PER_SEC: &str = "host_network_tx_bytes_per_sec";

/// One plan-requested host metric: name plus plan-declared unit echoed verbatim.
#[derive(Debug, Clone)]
pub struct RequestedHostMetric {
    /// Metric name (one of the `host_*` constants).
    pub name: String,
    /// Plan-declared unit; emitted verbatim so unit mismatches normalize as invalid.
    pub unit: String,
}

/// Gregg telemetry collector for one loopback endpoint.
pub struct GreggCollector {
    client: eggfetch_core::Client,
    endpoint: GreggEndpoint,
    requested: Vec<RequestedHostMetric>,
    poll_interval: Duration,
    identity: BTreeMap<String, String>,
    window: Option<ActiveWindow>,
}

impl std::fmt::Debug for GreggCollector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The Eggfetch client carries pool state without a `Debug` impl;
        // its presence is recorded without introspecting it.
        f.debug_struct("GreggCollector")
            .field("client", &"eggfetch-client")
            .field("endpoint_host", &self.endpoint.host())
            .field("endpoint_port", &self.endpoint.port())
            .field("requested", &self.requested)
            .field("poll_interval", &self.poll_interval)
            .field("identity", &self.identity)
            .field("window_open", &self.window.is_some())
            .finish()
    }
}

struct ActiveWindow {
    samples: Arc<std::sync::Mutex<WindowSamples>>,
    stop: CancellationToken,
    task: JoinHandle<()>,
}

#[derive(Default)]
struct WindowSamples {
    samples: Vec<Sample>,
    raw_bytes: usize,
    dropped: u64,
    poll_errors: u64,
}

#[derive(Clone)]
struct Sample {
    observed_at_unix_ms: u64,
    payload: StatusPayloadV2,
    line: Vec<u8>,
}

impl GreggCollector {
    /// Build a collector for a validated-loopback endpoint and requested metrics.
    ///
    /// Unknown metric names fail closed: only the eight M001b `host_*`
    /// names are collected; anything else is a plan/config error, never a
    /// silent omission.
    ///
    /// # Errors
    /// Returns a human-readable reason for endpoint-policy violations or
    /// unknown metric names.
    pub fn new(endpoint_raw: &str, requested: Vec<RequestedHostMetric>) -> Result<Self, String> {
        let endpoint =
            validate_endpoint(endpoint_raw).map_err(|reason| format!("gregg {reason}"))?;
        for metric in &requested {
            if metric_name_kind(&metric.name).is_none() {
                return Err(format!("unknown gregg metric {}", metric.name));
            }
            if metric.unit.is_empty() || metric.unit.len() > 128 {
                return Err(format!("gregg metric {} unit out of bounds", metric.name));
            }
        }
        Ok(Self {
            client: eggfetch_core::Client::new(),
            endpoint,
            requested,
            poll_interval: Duration::from_secs(1),
            identity: BTreeMap::new(),
            window: None,
        })
    }

    /// Validated endpoint under collection.
    #[must_use]
    pub fn endpoint(&self) -> &GreggEndpoint {
        &self.endpoint
    }
}

/// Aggregation and value extraction per known host metric.
enum HostMetricKind {
    CpuMean,
    MemoryUsedMax,
    MemoryPercentMax,
    FrequencyMean,
    DiskReadMean,
    DiskWriteMean,
    NetworkRxMean,
    NetworkTxMean,
}

fn metric_name_kind(name: &str) -> Option<HostMetricKind> {
    match name {
        HOST_CPU_PERCENT => Some(HostMetricKind::CpuMean),
        HOST_MEMORY_USED_BYTES => Some(HostMetricKind::MemoryUsedMax),
        HOST_MEMORY_PERCENT => Some(HostMetricKind::MemoryPercentMax),
        HOST_CPU_FREQUENCY_HZ => Some(HostMetricKind::FrequencyMean),
        HOST_DISK_READ_BYTES_PER_SEC => Some(HostMetricKind::DiskReadMean),
        HOST_DISK_WRITE_BYTES_PER_SEC => Some(HostMetricKind::DiskWriteMean),
        HOST_NETWORK_RX_BYTES_PER_SEC => Some(HostMetricKind::NetworkRxMean),
        HOST_NETWORK_TX_BYTES_PER_SEC => Some(HostMetricKind::NetworkTxMean),
        _ => None,
    }
}

/// All eight M001b host metric names, for descriptor capabilities.
#[must_use]
pub fn host_metric_names() -> Vec<String> {
    vec![
        HOST_CPU_PERCENT.to_owned(),
        HOST_MEMORY_USED_BYTES.to_owned(),
        HOST_MEMORY_PERCENT.to_owned(),
        HOST_CPU_FREQUENCY_HZ.to_owned(),
        HOST_DISK_READ_BYTES_PER_SEC.to_owned(),
        HOST_DISK_WRITE_BYTES_PER_SEC.to_owned(),
        HOST_NETWORK_RX_BYTES_PER_SEC.to_owned(),
        HOST_NETWORK_TX_BYTES_PER_SEC.to_owned(),
    ]
}

impl TelemetryCollector for GreggCollector {
    fn source(&self) -> &'static str {
        GREGG_SOURCE
    }

    fn preflight(
        &mut self,
        context: TelemetryPreflightContext,
    ) -> TelemetryFuture<'_, Result<TelemetryCapability, TelemetryError>> {
        Box::pin(async move { self.preflight_inner(context).await })
    }

    fn start_trial(
        &mut self,
        context: TelemetryTrialContext,
    ) -> TelemetryFuture<'_, Result<(), TelemetryError>> {
        Box::pin(async move { self.start_inner(context).await })
    }

    fn stop_trial(
        &mut self,
        context: TelemetryTrialContext,
    ) -> TelemetryFuture<'_, Result<TelemetryOutput, TelemetryError>> {
        Box::pin(async move { self.stop_inner(context).await })
    }

    fn drain(&mut self, _context: DrainContext) -> TelemetryFuture<'_, Result<(), TelemetryError>> {
        Box::pin(async move {
            // Windows are always paired by the runner; drain defensively
            // closes any orphaned window without waiting on the network.
            if let Some(window) = self.window.take() {
                window.stop.cancel();
                window.task.abort();
            }
            Ok(())
        })
    }
}

impl GreggCollector {
    async fn preflight_inner(
        &mut self,
        context: TelemetryPreflightContext,
    ) -> Result<TelemetryCapability, TelemetryError> {
        if context.cancellation.is_cancelled() {
            return Err(TelemetryError::new(
                "collector_cancelled",
                "gregg preflight cancelled",
            ));
        }
        // Health first: readiness gates status interpretation.
        let health = fetch_health(&self.client, &self.endpoint).await?;
        if health.state != ReadinessState::Ready {
            return Err(TelemetryError::new(
                "health_unavailable",
                format!("gregg daemon not ready: {:?}", health.state),
            ));
        }
        if let Some(snapshot) = &health.snapshot {
            snapshot
                .validate()
                .map_err(|_| TelemetryError::new("payload_invalid", "health snapshot invalid"))?;
        }
        let payload = fetch_status(&self.client, &self.endpoint).await?;
        let interval_ms = payload.snapshot.sample_interval_ms;
        let poll_interval =
            Duration::from_millis(interval_ms).clamp(MIN_POLL_INTERVAL, MAX_POLL_INTERVAL);
        let system = &payload.snapshot.system;
        let mut identity = BTreeMap::new();
        for (key, value) in [
            ("hostname", system.hostname.as_str()),
            ("os_name", system.os_name.as_str()),
            ("os_version", system.os_version.as_str()),
            ("architecture", system.architecture.as_str()),
        ] {
            if !value.is_empty() {
                identity.insert(key.to_owned(), value.chars().take(128).collect());
            }
        }
        self.poll_interval = poll_interval;
        self.identity = identity.clone();
        Ok(TelemetryCapability {
            poll_interval,
            identity,
        })
    }

    async fn start_inner(&mut self, context: TelemetryTrialContext) -> Result<(), TelemetryError> {
        if self.window.is_some() {
            return Err(TelemetryError::new(
                "polling_failed",
                "gregg trial window already open",
            ));
        }
        if context.cancellation.is_cancelled() {
            return Err(TelemetryError::new(
                "collector_cancelled",
                "gregg start cancelled",
            ));
        }
        let samples = Arc::new(std::sync::Mutex::new(WindowSamples::default()));
        // Immediate snapshot right after start; failure fails start so the
        // trial never measures against a dead backend.
        let first = fetch_sample(&self.client, &self.endpoint).await?;
        push_sample(&samples, first);
        let task_stop = context.cancellation.clone();
        let task_samples = Arc::clone(&samples);
        let client = self.client.clone();
        let endpoint = self.endpoint.clone();
        let interval = self.poll_interval;
        let task = tokio::spawn(async move {
            poll_loop(client, endpoint, interval, task_samples, task_stop).await;
        });
        self.window = Some(ActiveWindow {
            samples,
            stop: context.cancellation.clone(),
            task,
        });
        Ok(())
    }

    async fn stop_inner(
        &mut self,
        context: TelemetryTrialContext,
    ) -> Result<TelemetryOutput, TelemetryError> {
        let window = self.window.take().ok_or_else(|| {
            TelemetryError::new("polling_failed", "gregg stop without open window")
        })?;
        let deadline = Instant::now() + context.timeout;
        window.stop.cancel();
        // Join the polling task within the stop budget; abort on expiry so
        // no polling task leaks past the trial.
        let remaining = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(remaining, window.task).await {
            Ok(_) => {}
            Err(_) => {
                return Err(TelemetryError::new(
                    "polling_timeout",
                    "gregg polling task did not stop in budget",
                ));
            }
        }
        // Final snapshot when the budget allows; failure degrades to a
        // warning with the captured samples retained.
        let mut warnings: Vec<MetricWarning> = Vec::new();
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.as_millis() > 100 {
            match tokio::time::timeout(remaining, fetch_sample(&self.client, &self.endpoint)).await
            {
                Ok(Ok(sample)) => push_sample(&window.samples, sample),
                Ok(Err(error)) => {
                    warnings.push(warning("gregg_final_snapshot_failed", &error.to_string()));
                }
                Err(_) => warnings.push(warning(
                    "gregg_final_snapshot_failed",
                    "final snapshot exceeded stop budget",
                )),
            }
        } else {
            warnings.push(warning(
                "gregg_final_snapshot_skipped",
                "no stop budget remained for a final snapshot",
            ));
        }
        Ok(build_output(
            &window.samples,
            &self.requested,
            &self.endpoint,
            self.poll_interval,
            &self.identity,
            warnings,
        ))
    }
}

/// Fetch and validate the health envelope.
async fn fetch_health(
    client: &eggfetch_core::Client,
    endpoint: &GreggEndpoint,
) -> Result<HealthResponseV2, TelemetryError> {
    let mut response = client
        .get(&endpoint.health_url())
        .map_err(|_| TelemetryError::new("endpoint_invalid", "health URL rejected"))?
        .timeout(eggfetch_core::Timeout::from_secs(
            GREGG_REQUEST_TIMEOUT_SECS,
        ))
        .send_detailed()
        .await
        .map_err(|failure| {
            TelemetryError::new(
                "health_unavailable",
                format!("health request failed: {}", failure_kind(&failure)),
            )
        })?;
    if !(200..=299).contains(&response.status().as_u16()) {
        return Err(TelemetryError::new(
            "health_unavailable",
            format!("health status {}", response.status().as_u16()),
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|_| TelemetryError::new("health_unavailable", "health body unreadable"))?;
    serde_json::from_slice::<HealthResponseV2>(&bytes).map_err(|error| {
        TelemetryError::new(
            "schema_unsupported",
            format!("health envelope rejected: {error}"),
        )
    })
}

/// Fetch, parse, and validate one v2 status payload.
async fn fetch_status(
    client: &eggfetch_core::Client,
    endpoint: &GreggEndpoint,
) -> Result<StatusPayloadV2, TelemetryError> {
    let sample = fetch_sample(client, endpoint).await?;
    Ok(sample.payload)
}

/// Fetch one status snapshot with its exact wire bytes.
async fn fetch_sample(
    client: &eggfetch_core::Client,
    endpoint: &GreggEndpoint,
) -> Result<Sample, TelemetryError> {
    let mut response = client
        .get(&endpoint.status_url())
        .map_err(|_| TelemetryError::new("endpoint_invalid", "status URL rejected"))?
        .timeout(eggfetch_core::Timeout::from_secs(
            GREGG_REQUEST_TIMEOUT_SECS,
        ))
        .send_detailed()
        .await
        .map_err(|failure| {
            TelemetryError::new(
                "status_unavailable",
                format!("status request failed: {}", failure_kind(&failure)),
            )
        })?;
    if !(200..=299).contains(&response.status().as_u16()) {
        return Err(TelemetryError::new(
            "status_unavailable",
            format!("status code {}", response.status().as_u16()),
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|_| TelemetryError::new("status_unavailable", "status body unreadable"))?;
    let payload: StatusPayloadV2 = serde_json::from_slice(&bytes).map_err(|error| {
        TelemetryError::new("payload_invalid", format!("status JSON rejected: {error}"))
    })?;
    if payload.snapshot.schema_version != SCHEMA_VERSION_V2 {
        return Err(TelemetryError::new(
            "schema_unsupported",
            format!(
                "status schema {} (expected {SCHEMA_VERSION_V2})",
                payload.snapshot.schema_version
            ),
        ));
    }
    payload
        .validate()
        .map_err(|_| TelemetryError::new("payload_invalid", "status payload invalid"))?;
    // Retain exact wire bytes when they form one NDJSON line; otherwise
    // fall back to canonical reserialization of the validated payload.
    let line = if bytes.iter().any(|byte| *byte == b'\n' || *byte == b'\r') {
        serde_json::to_vec(&payload)
            .map_err(|_| TelemetryError::new("payload_invalid", "status reserialization failed"))?
    } else {
        bytes.to_vec()
    };
    Ok(Sample {
        observed_at_unix_ms: payload.snapshot.observed_at_unix_ms,
        payload,
        line,
    })
}

/// Stable one-word classification for transport failures (no raw strings).
fn failure_kind(failure: &eggfetch_core::RequestFailure) -> &'static str {
    if failure.is_timeout() {
        "timeout"
    } else {
        "transport"
    }
}

/// Bounded polling loop: fetch, retain, sleep. Errors count; the loop never
/// fails the trial on its own.
async fn poll_loop(
    client: eggfetch_core::Client,
    endpoint: GreggEndpoint,
    interval: Duration,
    samples: Arc<std::sync::Mutex<WindowSamples>>,
    stop: CancellationToken,
) {
    loop {
        tokio::select! {
            () = stop.cancelled() => break,
            () = tokio::time::sleep(interval) => {
                match fetch_sample(&client, &endpoint).await {
                    Ok(sample) => push_sample(&samples, sample),
                    Err(_) => {
                        if let Ok(mut state) = samples.lock() {
                            state.poll_errors = state.poll_errors.saturating_add(1);
                        }
                    }
                }
            }
        }
    }
}

/// Retain one sample within the trial caps; overflow counts as dropped.
/// Aggregation always runs over exactly the retained set.
fn push_sample(samples: &Arc<std::sync::Mutex<WindowSamples>>, sample: Sample) {
    let Ok(mut state) = samples.lock() else {
        return;
    };
    if state.samples.len() >= MAX_SAMPLES_PER_TRIAL
        || state.raw_bytes.saturating_add(sample.line.len()) > MAX_NDJSON_BYTES
    {
        state.dropped = state.dropped.saturating_add(1);
        return;
    }
    state.raw_bytes = state.raw_bytes.saturating_add(sample.line.len() + 1);
    state.samples.push(sample);
}

fn warning(category: &str, detail: &str) -> MetricWarning {
    MetricWarning {
        category: category.to_owned(),
        detail: detail.chars().take(512).collect(),
    }
}

/// Aggregate retained samples into raw observations plus artifacts.
///
/// One lock acquisition clones the retained set; aggregation and
/// serialization run over the clone so polling never blocks on evidence.
fn build_output(
    samples: &Arc<std::sync::Mutex<WindowSamples>>,
    requested: &[RequestedHostMetric],
    endpoint: &GreggEndpoint,
    poll_interval: Duration,
    identity: &BTreeMap<String, String>,
    mut warnings: Vec<MetricWarning>,
) -> TelemetryOutput {
    let snapshot = samples
        .lock()
        .map(|state| WindowView {
            retained: state.samples.clone(),
            dropped: state.dropped,
            poll_errors: state.poll_errors,
        })
        .unwrap_or_default();
    append_collection_warnings(&mut warnings, &snapshot);
    let metrics = aggregate_metrics(requested, &snapshot.retained);
    let ndjson = serialize_series(&snapshot.retained);
    let provenance_bytes = provenance_document(endpoint, poll_interval, identity, &snapshot);
    TelemetryOutput {
        artifacts: vec![
            WorkloadArtifact {
                name: GREGG_NDJSON_ARTIFACT.to_owned(),
                media_type: "application/x-ndjson".to_owned(),
                bytes: ndjson,
            },
            WorkloadArtifact {
                name: GREGG_PROVENANCE_ARTIFACT.to_owned(),
                media_type: "application/json".to_owned(),
                bytes: provenance_bytes,
            },
        ],
        metrics,
        warnings,
    }
}

/// Cloned retention view for lock-free aggregation.
#[derive(Default)]
struct WindowView {
    retained: Vec<Sample>,
    dropped: u64,
    poll_errors: u64,
}

fn append_collection_warnings(warnings: &mut Vec<MetricWarning>, snapshot: &WindowView) {
    let unique = deduplicate(&snapshot.retained);
    let deduplicated = snapshot.retained.len().saturating_sub(unique.len());
    if snapshot.poll_errors > 0 {
        warnings.push(warning(
            "gregg_poll_errors",
            &format!(
                "{} status polls failed during the trial",
                snapshot.poll_errors
            ),
        ));
    }
    if snapshot.dropped > 0 {
        warnings.push(warning(
            "gregg_samples_dropped",
            &format!(
                "{} snapshots exceeded trial retention caps",
                snapshot.dropped
            ),
        ));
    }
    if deduplicated > 0 {
        warnings.push(warning(
            "gregg_timestamps_deduplicated",
            &format!("{deduplicated} repeated daemon timestamps deduplicated"),
        ));
    }
}

/// Deduplicate identical daemon timestamps, keeping the first.
fn deduplicate(retained: &[Sample]) -> Vec<&Sample> {
    let mut seen: Vec<u64> = Vec::new();
    let mut unique = Vec::new();
    for sample in retained {
        if !seen.contains(&sample.observed_at_unix_ms) {
            seen.push(sample.observed_at_unix_ms);
            unique.push(sample);
        }
    }
    unique
}

fn aggregate_metrics(
    requested: &[RequestedHostMetric],
    retained: &[Sample],
) -> Vec<RawMetricObservation> {
    let unique = deduplicate(retained);
    let mut metrics = Vec::new();
    for metric in requested {
        let Some(kind) = metric_name_kind(&metric.name) else {
            continue;
        };
        let values: Vec<f64> = unique
            .iter()
            .filter_map(|sample| extract(&sample.payload, &kind))
            .collect();
        if values.is_empty() {
            // Absent optional fields stay missing; never substitute zero.
            continue;
        }
        let (value, aggregation) = match kind {
            HostMetricKind::CpuMean
            | HostMetricKind::FrequencyMean
            | HostMetricKind::DiskReadMean
            | HostMetricKind::DiskWriteMean
            | HostMetricKind::NetworkRxMean
            | HostMetricKind::NetworkTxMean => (mean(&values), Aggregation::Mean),
            HostMetricKind::MemoryUsedMax | HostMetricKind::MemoryPercentMax => {
                (max(&values), Aggregation::Maximum)
            }
        };
        metrics.push(RawMetricObservation {
            name: metric.name.clone(),
            unit: metric.unit.clone(),
            value,
            aggregation,
            source_field: Some(source_field(&kind).to_owned()),
            producer: Some(GREGG_SOURCE.to_owned()),
            producer_version: Some(GREGG_PROTOCOL_VERSION.to_owned()),
            raw_artifacts: vec![GREGG_NDJSON_ARTIFACT.to_owned()],
        });
    }
    metrics
}

/// NDJSON series: exact retained lines in arrival order.
fn serialize_series(retained: &[Sample]) -> Vec<u8> {
    let mut ndjson = Vec::new();
    for sample in retained {
        ndjson.extend_from_slice(&sample.line);
        ndjson.push(b'\n');
    }
    ndjson
}

fn provenance_document(
    endpoint: &GreggEndpoint,
    poll_interval: Duration,
    identity: &BTreeMap<String, String>,
    snapshot: &WindowView,
) -> Vec<u8> {
    let provenance = serde_json::json!({
        "source": GREGG_SOURCE,
        "endpoint_host": endpoint.host(),
        "endpoint_port": endpoint.port(),
        "health_path": HEALTH_PATH,
        "status_path": STATUS_PATH,
        "wire_schema_version": SCHEMA_VERSION_V2,
        "protocol_crate": "gregg-protocol",
        "protocol_crate_version": GREGG_PROTOCOL_VERSION,
        "adapter_version": super::GREGG_ADAPTER_VERSION,
        "system": identity,
        "poll_interval_ms": poll_interval.as_millis(),
        "sample_count": snapshot.retained.len(),
        "dropped_samples": snapshot.dropped,
        "poll_errors": snapshot.poll_errors,
    });
    serde_json::to_vec_pretty(&provenance).expect("provenance serializes")
}

/// Lossless-as-practical byte-count conversion. Host quantities stay far
/// below 2^53, where `u64 as f64` is exact; larger values saturate instead
/// of losing precision silently.
fn quantity_f64(value: u64) -> f64 {
    const EXACT_TOP: u64 = (1 << 53) - 1;
    #[allow(clippy::cast_precision_loss)]
    let converted = value as f64;
    if value > EXACT_TOP {
        f64::from(u32::MAX)
    } else {
        converted
    }
}

/// Extract one metric value from a payload; `None` when the optional field
/// is absent on this snapshot.
fn extract(payload: &StatusPayloadV2, kind: &HostMetricKind) -> Option<f64> {
    match kind {
        HostMetricKind::CpuMean => Some(f64::from(payload.snapshot.cpu.usage_pct)),
        HostMetricKind::MemoryUsedMax => Some(quantity_f64(payload.snapshot.memory.used_bytes)),
        HostMetricKind::MemoryPercentMax => Some(f64::from(payload.snapshot.memory.usage_pct)),
        HostMetricKind::FrequencyMean => payload.cpu_frequency_hz.map(quantity_f64),
        HostMetricKind::DiskReadMean => payload
            .disk_io
            .as_ref()
            .map(|disk| quantity_f64(disk.aggregate_read_bytes_per_sec)),
        HostMetricKind::DiskWriteMean => payload
            .disk_io
            .as_ref()
            .map(|disk| quantity_f64(disk.aggregate_write_bytes_per_sec)),
        HostMetricKind::NetworkRxMean => payload
            .network
            .as_ref()
            .map(|network| quantity_f64(network.aggregate_rx_bytes_per_sec)),
        HostMetricKind::NetworkTxMean => payload
            .network
            .as_ref()
            .map(|network| quantity_f64(network.aggregate_tx_bytes_per_sec)),
    }
}

/// Wire source-field label for provenance.
fn source_field(kind: &HostMetricKind) -> &'static str {
    match kind {
        HostMetricKind::CpuMean => "v2.cpu.usage_pct",
        HostMetricKind::MemoryUsedMax => "v2.memory.used_bytes",
        HostMetricKind::MemoryPercentMax => "v2.memory.usage_pct",
        HostMetricKind::FrequencyMean => "v2.cpu_frequency_hz",
        HostMetricKind::DiskReadMean => "v2.disk_io.aggregate_read_bytes_per_sec",
        HostMetricKind::DiskWriteMean => "v2.disk_io.aggregate_write_bytes_per_sec",
        HostMetricKind::NetworkRxMean => "v2.network.aggregate_rx_bytes_per_sec",
        HostMetricKind::NetworkTxMean => "v2.network.aggregate_tx_bytes_per_sec",
    }
}

fn mean(values: &[f64]) -> f64 {
    let count = u64::try_from(values.len()).unwrap_or(u64::MAX);
    values.iter().sum::<f64>() / quantity_f64(count)
}

fn max(values: &[f64]) -> f64 {
    values.iter().copied().fold(f64::NEG_INFINITY, f64::max)
}
