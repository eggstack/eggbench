//! Generic, bounded Prometheus text-format telemetry support.
//!
//! This module contains no subject-specific metric names. A workspace-pinned
//! mapping translates owner-defined exposition samples into Eggbench metric
//! names and explicit trial aggregation semantics.

use eggbench_core::{
    Aggregation, DriverCategory, DriverDescriptor, MetricWarning, Name, RawMetricObservation,
};
use eggbench_runner::{
    DrainContext, TelemetryCapability, TelemetryCollector, TelemetryError, TelemetryFuture,
    TelemetryOutput, TelemetryPreflightContext, TelemetryPreflightTiming, TelemetryTrialContext,
    WorkloadArtifact,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Per-metric output selected by the experiment plan.
#[derive(Debug, Clone)]
pub struct RequestedPrometheusMetric {
    /// Resolved metric name.
    pub name: String,
    /// Resolved metric unit.
    pub unit: String,
}

/// Trial-synchronized generic Prometheus collector.
pub struct PrometheusHttpCollector {
    client: eggfetch_core::Client,
    endpoint: String,
    binding_service: String,
    binding_key: String,
    mapping_ref: String,
    mapping: PrometheusMappingV1,
    mapping_sha256: String,
    requested: Vec<RequestedPrometheusMetric>,
    poll_interval: Duration,
    window: Option<ActiveWindow>,
}

struct ActiveWindow {
    samples: Arc<Mutex<PrometheusWindow>>,
    stop: CancellationToken,
    task: JoinHandle<()>,
}

#[derive(Default)]
struct PrometheusWindow {
    start: Option<BTreeMap<String, f64>>,
    samples: Vec<BTreeMap<String, f64>>,
    poll_errors: u64,
    missing_fields: u64,
    dropped_samples: u64,
}

impl std::fmt::Debug for PrometheusHttpCollector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrometheusHttpCollector")
            .field("endpoint", &self.endpoint)
            .field("mapping_sha256", &self.mapping_sha256)
            .field("requested", &self.requested)
            .field("poll_interval", &self.poll_interval)
            .field("window_open", &self.window.is_some())
            .finish_non_exhaustive()
    }
}

impl PrometheusHttpCollector {
    /// Build a collector from validated workspace mapping data.
    ///
    /// # Errors
    /// Returns a stable error category for invalid endpoint, mapping, digest,
    /// polling interval, or requested-field configuration.
    #[allow(clippy::too_many_arguments)] // Every field is comparison-critical constructor input.
    pub fn new(
        endpoint: &str,
        mapping: PrometheusMappingV1,
        mapping_sha256: String,
        binding_service: String,
        binding_key: String,
        mapping_ref: String,
        requested: Vec<RequestedPrometheusMetric>,
        poll_interval: Duration,
    ) -> Result<Self, &'static str> {
        validate_private_endpoint(endpoint)?;
        validate_mapping(&mapping)?;
        if mapping_sha256.len() != 64
            || !mapping_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !(MIN_POLL_INTERVAL..=MAX_POLL_INTERVAL).contains(&poll_interval)
        {
            return Err("collector_config_invalid");
        }
        let mapped = mapping
            .fields
            .iter()
            .map(|field| (field.output_name.as_str(), field.unit.as_str()))
            .collect::<BTreeMap<_, _>>();
        if requested.is_empty()
            || requested
                .iter()
                .any(|item| mapped.get(item.name.as_str()).copied() != Some(item.unit.as_str()))
        {
            return Err("collector_fields_invalid");
        }
        let requested_names = requested
            .iter()
            .map(|item| item.name.as_str())
            .collect::<BTreeSet<_>>();
        let mut mapping = mapping;
        mapping
            .fields
            .retain(|field| requested_names.contains(field.output_name.as_str()));
        Ok(Self {
            client: eggfetch_core::Client::new(),
            endpoint: endpoint.to_owned(),
            binding_service,
            binding_key,
            mapping_ref,
            mapping,
            mapping_sha256,
            requested,
            poll_interval,
            window: None,
        })
    }

    async fn scrape(&self) -> Result<BTreeMap<String, f64>, TelemetryError> {
        let mut response = self
            .client
            .get(&self.endpoint)
            .map_err(|_| TelemetryError::new("endpoint_invalid", "scrape URL rejected"))?
            .max_decoded_body_size(MAX_SCRAPE_BYTES)
            .timeout(eggfetch_core::Timeout::from_secs(10))
            .send_detailed()
            .await
            .map_err(|failure| {
                TelemetryError::new(
                    "scrape_unavailable",
                    if failure.is_timeout() {
                        "scrape timeout"
                    } else {
                        "scrape transport failure"
                    },
                )
            })?;
        if !(200..=299).contains(&response.status().as_u16()) {
            return Err(TelemetryError::new(
                "scrape_unavailable",
                "non-success scrape status",
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|_| TelemetryError::new("scrape_unavailable", "scrape body unreadable"))?;
        parse_exposition(&bytes, &self.mapping)
            .map_err(|category| TelemetryError::new(category, "Prometheus exposition rejected"))
    }

    async fn preflight_inner(
        &mut self,
        context: TelemetryPreflightContext,
    ) -> Result<TelemetryCapability, TelemetryError> {
        if context.cancellation.is_cancelled() {
            return Err(TelemetryError::new(
                "collector_cancelled",
                "preflight cancelled",
            ));
        }
        // An owner publishes on its own refresh cadence: a subject can be
        // listening well before its worker series exist. Probing therefore
        // retries on a bounded cadence until the required contract is
        // observable or the run's telemetry bound expires. The first
        // successful sample still carries the contract decision, and the last
        // observed failure is what a permanently absent contract reports.
        let deadline = tokio::time::Instant::now() + context.timeout;
        let mut last_error = None;
        loop {
            if context.cancellation.is_cancelled() {
                return Err(TelemetryError::new(
                    "collector_cancelled",
                    "preflight cancelled",
                ));
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(last_error.unwrap_or_else(|| {
                    TelemetryError::new("required_metric_missing", "no scrape completed in bounds")
                }));
            }
            match tokio::time::timeout(remaining, self.scrape()).await {
                Ok(Ok(sample)) => {
                    let missing_required =
                        self.mapping.fields.iter().any(|field| {
                            field.required && !sample.contains_key(&field.output_name)
                        });
                    if !missing_required {
                        return Ok(self.capability());
                    }
                    last_error = Some(TelemetryError::new(
                        "required_metric_missing",
                        "required mapped field absent",
                    ));
                }
                Ok(Err(error)) => last_error = Some(error),
                Err(_) => {
                    return Err(last_error.unwrap_or_else(|| {
                        TelemetryError::new("preflight_timeout", "scrape exceeded preflight bound")
                    }));
                }
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(last_error.expect("a failed probe records a reason"));
            }
            tokio::time::sleep(remaining.min(self.poll_interval)).await;
        }
    }

    fn capability(&self) -> TelemetryCapability {
        let mut identity = BTreeMap::new();
        identity.insert(
            "endpoint_authority".to_owned(),
            endpoint_authority(&self.endpoint),
        );
        identity.insert("mapping_sha256".to_owned(), self.mapping_sha256.clone());
        identity.insert("binding_service".to_owned(), self.binding_service.clone());
        identity.insert("binding_key".to_owned(), self.binding_key.clone());
        identity.insert(
            "mapping_schema".to_owned(),
            MAPPING_SCHEMA_VERSION.to_string(),
        );
        TelemetryCapability {
            poll_interval: self.poll_interval,
            identity,
        }
    }

    async fn start_inner(&mut self, context: TelemetryTrialContext) -> Result<(), TelemetryError> {
        if context.cancellation.is_cancelled() {
            return Err(TelemetryError::new(
                "collector_cancelled",
                "trial start cancelled",
            ));
        }
        if self.window.is_some() {
            return Err(TelemetryError::new(
                "polling_failed",
                "trial window already open",
            ));
        }
        let start = tokio::time::timeout(context.timeout, self.scrape())
            .await
            .map_err(|_| TelemetryError::new("polling_timeout", "start snapshot timeout"))??;
        let samples = Arc::new(Mutex::new(PrometheusWindow {
            start: Some(start),
            ..PrometheusWindow::default()
        }));
        let client = self.client.clone();
        let endpoint = self.endpoint.clone();
        let mapping = self.mapping.clone();
        let poll_interval = self.poll_interval;
        let stop = context.cancellation.child_token();
        let task_stop = stop.clone();
        let task_samples = Arc::clone(&samples);
        let task = tokio::spawn(async move {
            poll_loop(
                client,
                endpoint,
                mapping,
                poll_interval,
                task_samples,
                task_stop,
            )
            .await;
        });
        self.window = Some(ActiveWindow {
            samples,
            stop,
            task,
        });
        Ok(())
    }

    #[allow(clippy::too_many_lines)] // Final aggregation and evidence share one mapping contract.
    async fn stop_inner(
        &mut self,
        context: TelemetryTrialContext,
    ) -> Result<TelemetryOutput, TelemetryError> {
        let window = self.window.take().ok_or_else(|| {
            TelemetryError::new("polling_failed", "stop without open trial window")
        })?;
        let deadline = tokio::time::Instant::now() + context.timeout;
        window.stop.cancel();
        let mut task = window.task;
        match tokio::time::timeout(context.timeout, &mut task).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => {
                return Err(TelemetryError::new(
                    "polling_failed",
                    "polling task join failed",
                ));
            }
            Err(_) => {
                task.abort();
                return Err(TelemetryError::new(
                    "polling_timeout",
                    "polling task did not drain and was aborted",
                ));
            }
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let final_sample = if remaining.is_zero() || context.cancellation.is_cancelled() {
            None
        } else {
            Some(tokio::time::timeout(remaining, self.scrape()).await)
        };
        let mut warnings = Vec::new();
        if let Some(Ok(Ok(values))) = final_sample {
            push_snapshot(&window.samples, values, &self.mapping);
        } else if context.cancellation.is_cancelled() {
            warnings.push(warning(
                "final_snapshot_cancelled",
                "final scrape skipped during cancellation",
            ));
        } else {
            warnings.push(warning(
                "final_snapshot_failed",
                "final scrape was unavailable",
            ));
        }
        let snapshot = window
            .samples
            .lock()
            .map(|value| {
                (
                    value.start.clone().unwrap_or_default(),
                    value.samples.clone(),
                    value.poll_errors,
                    value.missing_fields,
                    value.dropped_samples,
                )
            })
            .unwrap_or_default();
        if snapshot.2 > 0 {
            warnings.push(warning(
                "poll_errors",
                "one or more in-window scrapes failed",
            ));
        }
        if snapshot.3 > 0 {
            warnings.push(warning(
                "missing_samples",
                "one or more mapped fields were absent",
            ));
        }
        let mut metrics = Vec::new();
        let mut required_invalid = false;
        for requested in &self.requested {
            let Some(field) = self
                .mapping
                .fields
                .iter()
                .find(|field| field.output_name == requested.name)
            else {
                continue;
            };
            let required_gap = field.required
                && (snapshot.2 > 0
                    || snapshot.4 > 0
                    || snapshot
                        .1
                        .iter()
                        .any(|sample| !sample.contains_key(&field.output_name)));
            if required_gap {
                required_invalid = true;
                warnings.push(warning(
                    "required_metric_gap",
                    "required mapped field was absent or a scrape snapshot was lost",
                ));
                continue;
            }
            let values = snapshot
                .1
                .iter()
                .filter_map(|sample| sample.get(&field.output_name).copied())
                .collect::<Vec<_>>();
            let (value, aggregation) = match field.kind {
                PrometheusMetricKind::Gauge => aggregate_gauge(
                    &values,
                    field
                        .aggregation
                        .expect("mapping validation requires gauge aggregation"),
                ),
                PrometheusMetricKind::Counter => {
                    let counter_values = snapshot
                        .1
                        .iter()
                        .filter_map(|sample| sample.get(&field.output_name).copied())
                        .collect::<Vec<_>>();
                    match counter_delta(
                        snapshot.0.get(&field.output_name).copied(),
                        &counter_values,
                    ) {
                        Ok(Some(delta)) => (Some(delta), Some(Aggregation::Direct)),
                        Err(()) => {
                            warnings
                                .push(warning("counter_reset", "counter decreased during trial"));
                            required_invalid |= field.required;
                            (None, None)
                        }
                        Ok(None) => (None, None),
                    }
                }
            };
            if let (Some(value), Some(aggregation)) = (value, aggregation) {
                metrics.push(RawMetricObservation {
                    name: requested.name.clone(),
                    unit: requested.unit.clone(),
                    value,
                    aggregation,
                    source_field: Some(field.prometheus_name.clone()),
                    producer: Some(PROMETHEUS_HTTP_SOURCE.to_owned()),
                    producer_version: Some(env!("CARGO_PKG_VERSION").to_owned()),
                    raw_artifacts: vec!["prometheus-provenance.json".to_owned()],
                });
            } else if field.required {
                warnings.push(warning(
                    "required_metric_missing",
                    "required mapped trial observation is unavailable",
                ));
                required_invalid = true;
            }
        }
        if required_invalid {
            return Err(TelemetryError::new(
                "required_metric_invalid",
                "required mapped metric was missing or reset during the trial",
            ));
        }
        let evidence = serde_json::json!({
            "schema_version": 1,
            "source": PROMETHEUS_HTTP_SOURCE,
            "adapter_version": env!("CARGO_PKG_VERSION"),
            "transport": "eggfetch-core",
            "transport_version": env!("EGGBENCH_EGGFETCH_CORE_VERSION"),
            "exposition_format": "prometheus-text-scalar-v1",
            "endpoint_authority": endpoint_authority(&self.endpoint),
            "binding_service": self.binding_service,
            "binding_key": self.binding_key,
            "mapping_ref": self.mapping_ref,
            "mapping_sha256": self.mapping_sha256,
            "poll_interval_ms": self.poll_interval.as_millis(),
            "sample_count": snapshot.1.len(),
            "poll_error_count": snapshot.2,
            "missing_field_observation_count": snapshot.3,
            "dropped_sample_count": snapshot.4,
            "aggregation": self.mapping.fields.iter().map(|field| (&field.output_name, field.kind, field.aggregation)).collect::<Vec<_>>(),
        });
        let bytes = serde_json::to_vec(&evidence).unwrap_or_default();
        Ok(TelemetryOutput {
            artifacts: vec![WorkloadArtifact {
                name: "prometheus-provenance.json".to_owned(),
                media_type: "application/json".to_owned(),
                bytes,
            }],
            metrics,
            warnings,
        })
    }
}

impl TelemetryCollector for PrometheusHttpCollector {
    fn source(&self) -> &'static str {
        PROMETHEUS_HTTP_SOURCE
    }
    fn preflight_timing(&self) -> TelemetryPreflightTiming {
        // The scrape endpoint is normally a listener of the subject this run
        // manages, so it cannot exist before managed startup. Probing after
        // readiness is still strictly before any warmup or measured trial.
        TelemetryPreflightTiming::AfterReadiness
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
    fn drain(&mut self, context: DrainContext) -> TelemetryFuture<'_, Result<(), TelemetryError>> {
        Box::pin(async move {
            let Some(window) = self.window.take() else {
                return Ok(());
            };
            window.stop.cancel();
            let mut task = window.task;
            match tokio::time::timeout(context.timeout, &mut task).await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(_)) => Err(TelemetryError::new(
                    "polling_failed",
                    "polling task join failed during drain",
                )),
                Err(_) => {
                    task.abort();
                    Err(TelemetryError::new(
                        "polling_timeout",
                        "polling task did not drain before the drain bound",
                    ))
                }
            }
        })
    }
}

async fn poll_loop(
    client: eggfetch_core::Client,
    endpoint: String,
    mapping: PrometheusMappingV1,
    interval: Duration,
    samples: Arc<Mutex<PrometheusWindow>>,
    stop: CancellationToken,
) {
    loop {
        tokio::select! {
            () = stop.cancelled() => break,
            () = tokio::time::sleep(interval) => {
                let result = tokio::select! {
                    () = stop.cancelled() => break,
                    result = scrape_with(&client, &endpoint, &mapping) => result,
                };
                match result {
                    Ok(values) => push_snapshot(&samples, values, &mapping),
                    Err(_) => if let Ok(mut state) = samples.lock() { state.poll_errors = state.poll_errors.saturating_add(1); },
                }
            }
        }
    }
}

async fn scrape_with(
    client: &eggfetch_core::Client,
    endpoint: &str,
    mapping: &PrometheusMappingV1,
) -> Result<BTreeMap<String, f64>, TelemetryError> {
    let mut response = client
        .get(endpoint)
        .map_err(|_| TelemetryError::new("endpoint_invalid", "scrape URL rejected"))?
        .max_decoded_body_size(MAX_SCRAPE_BYTES)
        .timeout(eggfetch_core::Timeout::from_secs(10))
        .send_detailed()
        .await
        .map_err(|_| TelemetryError::new("scrape_unavailable", "scrape transport failed"))?;
    if !(200..=299).contains(&response.status().as_u16()) {
        return Err(TelemetryError::new(
            "scrape_unavailable",
            "non-success scrape status",
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|_| TelemetryError::new("scrape_unavailable", "scrape body unreadable"))?;
    parse_exposition(&bytes, mapping)
        .map_err(|category| TelemetryError::new(category, "Prometheus exposition rejected"))
}

fn push_snapshot(
    window: &Arc<Mutex<PrometheusWindow>>,
    values: BTreeMap<String, f64>,
    mapping: &PrometheusMappingV1,
) {
    if let Ok(mut state) = window.lock() {
        if state.samples.len() >= 512 {
            state.samples.remove(0);
            state.dropped_samples = state.dropped_samples.saturating_add(1);
        }
        state.missing_fields = state.missing_fields.saturating_add(
            mapping
                .fields
                .iter()
                .filter(|field| !values.contains_key(&field.output_name))
                .count() as u64,
        );
        state.samples.push(values);
    }
}

fn aggregate_gauge(
    values: &[f64],
    policy: PrometheusAggregation,
) -> (Option<f64>, Option<Aggregation>) {
    if values.is_empty() {
        return (None, None);
    }
    match policy {
        PrometheusAggregation::Mean => (
            Some(
                values
                    .iter()
                    .map(|value| {
                        *value / f64::from(u32::try_from(values.len()).unwrap_or(u32::MAX))
                    })
                    .sum::<f64>(),
            ),
            Some(Aggregation::Mean),
        ),
        PrometheusAggregation::Max => (
            values.iter().copied().reduce(f64::max),
            Some(Aggregation::Maximum),
        ),
        PrometheusAggregation::Min => (
            values.iter().copied().reduce(f64::min),
            Some(Aggregation::Minimum),
        ),
    }
}

fn counter_delta(start: Option<f64>, observations: &[f64]) -> Result<Option<f64>, ()> {
    let Some(start) = start else {
        return Ok(None);
    };
    let mut previous = start;
    for value in observations {
        if *value < previous {
            return Err(());
        }
        previous = *value;
    }
    Ok(Some(previous - start))
}

fn endpoint_authority(endpoint: &str) -> String {
    let Some((_, rest)) = endpoint.split_once("://") else {
        return String::new();
    };
    rest.split('/').next().unwrap_or_default().to_owned()
}

fn warning(category: &str, detail: &str) -> MetricWarning {
    MetricWarning {
        category: format!("prometheus_{category}"),
        detail: detail.to_owned(),
    }
}

/// Canonical source name for generic Prometheus scraping.
pub const PROMETHEUS_HTTP_SOURCE: &str = "prometheus-http";
/// Mapping contract schema version.
pub const MAPPING_SCHEMA_VERSION: u32 = 1;
/// Maximum mapping contract bytes.
pub const MAX_MAPPING_BYTES: usize = 128 * 1024;
/// Maximum scrape response bytes.
pub const MAX_SCRAPE_BYTES: usize = 1024 * 1024;
/// Maximum exposition lines per scrape.
pub const MAX_EXPOSITION_LINES: usize = 10_000;
/// Maximum metric and label-name bytes.
pub const MAX_PROMETHEUS_NAME_BYTES: usize = 128;
/// Maximum label-value bytes.
pub const MAX_LABEL_VALUE_BYTES: usize = 256;
/// Maximum mapped fields.
pub const MAX_MAPPED_FIELDS: usize = 64;
/// Minimum supported scrape interval.
pub const MIN_POLL_INTERVAL: Duration = Duration::from_millis(100);
/// Maximum supported scrape interval.
pub const MAX_POLL_INTERVAL: Duration = Duration::from_secs(60);

/// Immutable workspace mapping from owner exposition to normalized metrics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrometheusMappingV1 {
    /// Exact supported schema version.
    pub schema_version: u32,
    /// Source vocabulary, currently `prometheus`.
    pub source: String,
    /// Requested output field mappings.
    pub fields: Vec<PrometheusFieldMapping>,
}

/// One explicit Prometheus field mapping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrometheusFieldMapping {
    /// Eggbench output metric name.
    pub output_name: String,
    /// Prometheus exposition sample name.
    pub prometheus_name: String,
    /// Sample semantics.
    pub kind: PrometheusMetricKind,
    /// Output metric unit.
    pub unit: String,
    /// Trial aggregation policy.
    pub aggregation: Option<PrometheusAggregation>,
    /// Exact low-cardinality label selector; omitted means no selector.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    /// Missing fields invalidate required collection when true.
    #[serde(default)]
    pub required: bool,
}

/// Prometheus scalar semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrometheusMetricKind {
    /// A value observed throughout the trial.
    Gauge,
    /// A monotonically increasing value whose trial delta is emitted.
    Counter,
}

/// Trial aggregation for gauges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrometheusAggregation {
    /// Arithmetic mean of retained snapshots.
    Mean,
    /// Maximum retained snapshot.
    Max,
    /// Minimum retained snapshot.
    Min,
}

/// Parse or validate an immutable v1 mapping document.
///
/// # Errors
/// Returns a stable mapping syntax, size, or schema error.
pub fn parse_mapping(bytes: &[u8]) -> Result<PrometheusMappingV1, &'static str> {
    if bytes.len() > MAX_MAPPING_BYTES {
        return Err("mapping_too_large");
    }
    let mapping: PrometheusMappingV1 =
        serde_json::from_slice(bytes).map_err(|_| "mapping_invalid")?;
    validate_mapping(&mapping)?;
    Ok(mapping)
}

/// Validate the bounded mapping contract.
///
/// # Errors
/// Returns the first stable mapping validation category.
pub fn validate_mapping(mapping: &PrometheusMappingV1) -> Result<(), &'static str> {
    if mapping.schema_version != MAPPING_SCHEMA_VERSION || mapping.source != "prometheus" {
        return Err("mapping_schema_unsupported");
    }
    if mapping.fields.is_empty() || mapping.fields.len() > MAX_MAPPED_FIELDS {
        return Err("mapping_field_bound");
    }
    let mut outputs = BTreeSet::new();
    for field in &mapping.fields {
        if matches!(field.kind, PrometheusMetricKind::Gauge) != field.aggregation.is_some() {
            return Err("mapping_aggregation_invalid");
        }
        if Name::new(&field.output_name).is_err()
            || !field.output_name.starts_with("subject_")
            || field.output_name.len() > MAX_PROMETHEUS_NAME_BYTES
            || !valid_metric_name(&field.prometheus_name)
            || Name::new(&field.unit).is_err()
            || field.labels.len() > 16
            || !outputs.insert(field.output_name.as_str())
        {
            return Err("mapping_field_invalid");
        }
        for (key, value) in &field.labels {
            if !valid_label_name(key)
                || value.len() > MAX_LABEL_VALUE_BYTES
                || value.chars().any(char::is_control)
            {
                return Err("mapping_selector_invalid");
            }
        }
    }
    Ok(())
}

/// Parse the bounded scalar subset of Prometheus text exposition.
///
/// Comments and metadata lines are ignored. Matching samples must have a
/// finite numeric value, exact declared labels, and no duplicate match.
///
/// # Errors
/// Returns a stable exposition or mapping error category.
pub fn parse_exposition(
    bytes: &[u8],
    mapping: &PrometheusMappingV1,
) -> Result<BTreeMap<String, f64>, &'static str> {
    validate_mapping(mapping)?;
    if bytes.len() > MAX_SCRAPE_BYTES {
        return Err("scrape_too_large");
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "scrape_not_utf8")?;
    if text.lines().count() > MAX_EXPOSITION_LINES {
        return Err("scrape_line_bound");
    }
    let mut values = BTreeMap::new();
    let mut match_counts = BTreeMap::<&str, usize>::new();
    let mut declared_types = BTreeMap::<&str, &str>::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(metadata) = line.strip_prefix("# TYPE ") {
            let mut parts = metadata.split_ascii_whitespace();
            let name = parts.next().ok_or("sample_type_invalid")?;
            let kind = parts.next().ok_or("sample_type_invalid")?;
            if parts.next().is_some() || !valid_metric_name(name) {
                return Err("sample_type_invalid");
            }
            if declared_types.insert(name, kind).is_some() {
                return Err("sample_type_duplicate");
            }
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (sample, value_text) = split_sample_line(line)?;
        let value = value_text
            .parse::<f64>()
            .map_err(|_| "sample_value_invalid")?;
        if !value.is_finite() {
            return Err("sample_value_non_finite");
        }
        let (name, labels) = parse_sample_name_labels(sample)?;
        for field in &mapping.fields {
            if field.prometheus_name == name {
                if field.labels.is_empty() && !labels.is_empty() {
                    return Err("sample_ambiguous_duplicate");
                }
                if field.labels != labels {
                    continue;
                }
                if field.kind == PrometheusMetricKind::Counter && value < 0.0 {
                    return Err("counter_value_negative");
                }
                let count = match_counts.entry(&field.output_name).or_default();
                *count += 1;
                if *count > 1 {
                    return Err("sample_ambiguous_duplicate");
                }
                values.insert(field.output_name.clone(), value);
            }
        }
    }
    for field in &mapping.fields {
        if let Some(declared) = declared_types.get(field.prometheus_name.as_str()) {
            let expected = match field.kind {
                PrometheusMetricKind::Gauge => "gauge",
                PrometheusMetricKind::Counter => "counter",
            };
            if *declared != expected {
                return Err("sample_type_mismatch");
            }
        }
    }
    Ok(values)
}

fn split_sample_line(line: &str) -> Result<(&str, &str), &'static str> {
    let mut quoted = false;
    let mut escaped = false;
    for (index, byte) in line.bytes().enumerate() {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
        } else if byte == b'"' {
            quoted = true;
        } else if byte.is_ascii_whitespace() {
            let sample = &line[..index];
            let mut values = line[index..].split_ascii_whitespace();
            let value = values.next().ok_or("sample_invalid")?;
            if sample.is_empty() || values.next().is_some() {
                return Err("sample_invalid");
            }
            return Ok((sample, value));
        }
    }
    Err("sample_invalid")
}

/// Validate an HTTP scrape URL against the private-target policy.
///
/// IP literals are limited to loopback, RFC1918, or IPv6 ULA. DNS names are
/// limited to `localhost` and `.localhost` and must resolve only to loopback.
/// Userinfo, TLS, query, and fragment forms are rejected.
///
/// # Errors
/// Returns a stable error category when the endpoint is malformed, public,
/// or cannot be resolved under the private-target policy.
pub fn validate_private_endpoint(raw: &str) -> Result<(), &'static str> {
    let rest = raw.strip_prefix("http://").ok_or("endpoint_invalid")?;
    if rest.contains(['@', '?', '#']) || raw.len() > 2048 || raw.chars().any(char::is_control) {
        return Err("endpoint_invalid");
    }
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() || authority.contains(char::is_whitespace) {
        return Err("endpoint_invalid");
    }
    let (host, port) = split_authority(authority)?;
    let port = port.parse::<u16>().map_err(|_| "endpoint_invalid")?;
    if port == 0 {
        return Err("endpoint_invalid");
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return if is_private_or_loopback(ip) {
            Ok(())
        } else {
            Err("endpoint_not_private")
        };
    }
    if !(host.eq_ignore_ascii_case("localhost")
        || host.to_ascii_lowercase().ends_with(".localhost"))
    {
        return Err("endpoint_not_private");
    }
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|_| "endpoint_unresolvable")?
        .map(|address| address.ip())
        .collect::<Vec<_>>();
    if addresses.is_empty() || addresses.iter().any(|ip| !ip.is_loopback()) {
        return Err("endpoint_not_private");
    }
    Ok(())
}

fn split_authority(authority: &str) -> Result<(&str, &str), &'static str> {
    if let Some(bracketed) = authority.strip_prefix('[') {
        let end = bracketed.find(']').ok_or("endpoint_invalid")?;
        let host = &bracketed[..end];
        let port = bracketed[end + 1..]
            .strip_prefix(':')
            .ok_or("endpoint_invalid")?;
        if host.is_empty() || port.is_empty() {
            return Err("endpoint_invalid");
        }
        return Ok((host, port));
    }
    let (host, port) = authority.rsplit_once(':').ok_or("endpoint_invalid")?;
    if host.is_empty() || port.is_empty() || host.contains(':') {
        return Err("endpoint_invalid");
    }
    Ok((host, port))
}

fn is_private_or_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_loopback() || ip.is_private(),
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || (ip.segments()[0] & 0xfe00) == 0xfc00
                || ip
                    .to_ipv4_mapped()
                    .is_some_and(|v4| v4.is_loopback() || v4.is_private())
        }
    }
}

fn valid_metric_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_PROMETHEUS_NAME_BYTES
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphabetic()
                || byte == b'_'
                || byte == b':'
                || (index > 0 && byte.is_ascii_digit())
        })
}

fn valid_label_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_PROMETHEUS_NAME_BYTES
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphabetic() || byte == b'_' || (index > 0 && byte.is_ascii_digit())
        })
}

fn parse_sample_name_labels(
    sample: &str,
) -> Result<(&str, BTreeMap<String, String>), &'static str> {
    let Some(open) = sample.find('{') else {
        if !valid_metric_name(sample) {
            return Err("sample_name_invalid");
        }
        return Ok((sample, BTreeMap::new()));
    };
    if !sample.ends_with('}') {
        return Err("sample_labels_invalid");
    }
    let name = &sample[..open];
    if !valid_metric_name(name) {
        return Err("sample_name_invalid");
    }
    let mut labels = BTreeMap::new();
    let inner = &sample[open + 1..sample.len() - 1];
    let mut cursor = 0;
    while cursor < inner.len() {
        let remaining = &inner[cursor..];
        let eq = remaining.find('=').ok_or("sample_labels_invalid")?;
        let key = remaining[..eq].trim();
        if !valid_label_name(key) || !remaining[eq + 1..].starts_with('"') {
            return Err("sample_labels_invalid");
        }
        let (value, consumed) = parse_quoted_label(&remaining[eq + 2..])?;
        if value.len() > MAX_LABEL_VALUE_BYTES || labels.insert(key.to_owned(), value).is_some() {
            return Err("sample_labels_invalid");
        }
        cursor += eq + 2 + consumed;
        if cursor == inner.len() {
            break;
        }
        if inner.as_bytes().get(cursor) != Some(&b',') {
            return Err("sample_labels_invalid");
        }
        cursor += 1;
    }
    Ok((name, labels))
}

fn parse_quoted_label(input: &str) -> Result<(String, usize), &'static str> {
    let mut output = String::new();
    let mut escaped = false;
    for (index, ch) in input.char_indices() {
        if escaped {
            output.push(match ch {
                'n' => '\n',
                '\\' | '"' => ch,
                _ => return Err("sample_labels_invalid"),
            });
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Ok((output, index + ch.len_utf8()));
        } else {
            output.push(ch);
        }
    }
    Err("sample_labels_invalid")
}

/// Stable production descriptor with dynamically mapped output fields.
///
/// # Panics
/// The static source name is valid by construction.
#[must_use]
pub fn prometheus_http_descriptor() -> DriverDescriptor {
    DriverDescriptor {
        name: Name::new(PROMETHEUS_HTTP_SOURCE).expect("static source name"),
        adapter_version: env!("CARGO_PKG_VERSION").to_owned(),
        upstream_name: "prometheus-text-exposition".to_owned(),
        upstream_version: Some("bounded-v1".to_owned()),
        category: DriverCategory::Telemetry,
        capabilities: BTreeSet::new(),
        supported_platforms: BTreeSet::new(),
        machine_output_schema: None,
        external_process: false,
        default: !cfg!(feature = "gregg"),
        compatible_service_types: BTreeSet::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eggbench_core::{RunId, TrialId};
    use eggbench_runner::{
        DrainContext, TelemetryCollector, TelemetryPreflightContext, TelemetryTrialContext,
    };
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio_util::sync::CancellationToken;

    fn mapping() -> PrometheusMappingV1 {
        PrometheusMappingV1 {
            schema_version: 1,
            source: "prometheus".to_owned(),
            fields: vec![PrometheusFieldMapping {
                output_name: "subject_cpu_percent".to_owned(),
                prometheus_name: "synvoid_cpu_percent".to_owned(),
                kind: PrometheusMetricKind::Gauge,
                unit: "percent".to_owned(),
                aggregation: Some(PrometheusAggregation::Mean),
                labels: BTreeMap::new(),
                required: true,
            }],
        }
    }

    async fn local_metrics_origin() -> (String, Arc<AtomicUsize>, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let server_requests = Arc::clone(&requests);
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut request = [0_u8; 4096];
                let _ = stream.read(&mut request).await;
                let value = server_requests.fetch_add(1, Ordering::SeqCst) + 1;
                let body = format!(
                    "subject_cpu_percent {}\nsubject_requests_total {value}\n",
                    f64::from(u32::try_from(value).expect("test request count is bounded"))
                );
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                if stream.write_all(response.as_bytes()).await.is_err() {
                    break;
                }
            }
        });
        (format!("http://{address}/metrics"), requests, task)
    }

    async fn fixed_metrics_origin(body: &'static str) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut request = [0_u8; 4096];
                let _ = stream.read(&mut request).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                if stream.write_all(response.as_bytes()).await.is_err() {
                    break;
                }
            }
        });
        (format!("http://{address}/metrics"), task)
    }

    fn collector_mapping() -> PrometheusMappingV1 {
        PrometheusMappingV1 {
            schema_version: 1,
            source: "prometheus".to_owned(),
            fields: vec![
                PrometheusFieldMapping {
                    output_name: "subject_cpu_percent".to_owned(),
                    prometheus_name: "subject_cpu_percent".to_owned(),
                    kind: PrometheusMetricKind::Gauge,
                    unit: "percent".to_owned(),
                    aggregation: Some(PrometheusAggregation::Max),
                    labels: BTreeMap::new(),
                    required: true,
                },
                PrometheusFieldMapping {
                    output_name: "subject_requests_total".to_owned(),
                    prometheus_name: "subject_requests_total".to_owned(),
                    kind: PrometheusMetricKind::Counter,
                    unit: "count".to_owned(),
                    aggregation: None,
                    labels: BTreeMap::new(),
                    required: true,
                },
            ],
        }
    }

    /// A live loopback exposition whose body a test can rewrite between
    /// scrapes, counting scrapes and their served bytes.
    async fn scripted_metrics_origin(
        body: Arc<Mutex<String>>,
    ) -> (String, Arc<AtomicUsize>, Arc<AtomicU64>, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let scrapes = Arc::new(AtomicUsize::new(0));
        let scrapes_for_task = Arc::clone(&scrapes);
        let served_bytes = Arc::new(AtomicU64::new(0));
        let served_for_task = Arc::clone(&served_bytes);
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut request = [0_u8; 4096];
                let _ = stream.read(&mut request).await;
                let body = body.lock().expect("scripted body lock").clone();
                scrapes_for_task.fetch_add(1, Ordering::SeqCst);
                served_for_task.fetch_add(body.len() as u64, Ordering::SeqCst);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                if stream.write_all(response.as_bytes()).await.is_err() {
                    break;
                }
            }
        });
        (
            format!("http://{address}/metrics"),
            scrapes,
            served_bytes,
            task,
        )
    }

    fn gauge_and_counter_body(gauge: &str, counter: u64) -> String {
        format!(
            "# HELP subject_cpu_percent gauge\n# TYPE subject_cpu_percent gauge\n# TYPE subject_requests_total counter\nsubject_cpu_percent {gauge}\nsubject_requests_total {counter}\n"
        )
    }

    fn scripted_collector(
        endpoint: &str,
        digest_byte: char,
    ) -> Result<PrometheusHttpCollector, &'static str> {
        PrometheusHttpCollector::new(
            endpoint,
            collector_mapping(),
            std::iter::repeat_n(digest_byte, 64).collect(),
            "subject".to_owned(),
            "metrics_url".to_owned(),
            "telemetry/telemetry-mapping.json".to_owned(),
            vec![
                RequestedPrometheusMetric {
                    name: "subject_cpu_percent".to_owned(),
                    unit: "percent".to_owned(),
                },
                RequestedPrometheusMetric {
                    name: "subject_requests_total".to_owned(),
                    unit: "count".to_owned(),
                },
            ],
            MIN_POLL_INTERVAL,
        )
    }

    fn preflight_context() -> TelemetryPreflightContext {
        TelemetryPreflightContext {
            run_id: RunId::new(),
            cancellation: CancellationToken::new(),
            timeout: Duration::from_secs(2),
        }
    }

    fn trial_context(trial_id: u32) -> TelemetryTrialContext {
        TelemetryTrialContext {
            run_id: RunId::new(),
            trial_id: TrialId::new(trial_id).unwrap(),
            cancellation: CancellationToken::new(),
            timeout: Duration::from_secs(2),
        }
    }

    #[test]
    fn exposition_accepts_scalar_samples_and_ignores_comments() {
        let values = parse_exposition(
            b"# HELP synvoid_cpu_percent CPU\n# TYPE synvoid_cpu_percent gauge\nsynvoid_cpu_percent 12.5\n",
            &mapping(),
        )
        .unwrap();
        assert_eq!(values.get("subject_cpu_percent"), Some(&12.5));
    }

    #[test]
    fn exposition_rejects_non_finite_and_duplicate_ambiguous_samples() {
        assert_eq!(
            parse_exposition(b"synvoid_cpu_percent NaN\n", &mapping()),
            Err("sample_value_non_finite")
        );
        assert_eq!(
            parse_exposition(
                b"synvoid_cpu_percent{worker=\"a\"} 1\nsynvoid_cpu_percent{worker=\"b\"} 2\n",
                &mapping()
            ),
            Err("sample_ambiguous_duplicate")
        );
    }

    #[test]
    fn exact_low_cardinality_selector_accepts_only_matching_label() {
        let mut mapping = mapping();
        mapping.fields[0]
            .labels
            .insert("worker".to_owned(), "cpu".to_owned());
        let values = parse_exposition(
            b"synvoid_cpu_percent{worker=\"other\"} 99\nsynvoid_cpu_percent{worker=\"cpu\"} 12.5\n",
            &mapping,
        )
        .unwrap();
        assert_eq!(values.get("subject_cpu_percent"), Some(&12.5));
    }

    #[test]
    fn exposition_parser_handles_escaped_and_spaced_label_values() {
        let mut mapping = mapping();
        mapping.fields[0]
            .labels
            .insert("worker".to_owned(), "cpu worker".to_owned());
        let values = parse_exposition(
            b"synvoid_cpu_percent{worker=\"cpu worker\"} 12.5\n",
            &mapping,
        )
        .unwrap();
        assert_eq!(values.get("subject_cpu_percent"), Some(&12.5));
    }

    #[test]
    fn endpoint_requires_private_dns_results() {
        assert!(validate_private_endpoint("http://127.0.0.1:9100/metrics").is_ok());
        assert_eq!(
            validate_private_endpoint("https://127.0.0.1:9100/metrics"),
            Err("endpoint_invalid")
        );
        assert_eq!(
            validate_private_endpoint("http://8.8.8.8:9100/metrics"),
            Err("endpoint_not_private")
        );
    }

    #[test]
    fn gauges_use_explicit_mean_max_or_min_aggregation() {
        assert_eq!(
            aggregate_gauge(&[1.0, 3.0, 2.0], PrometheusAggregation::Mean),
            (Some(2.0), Some(Aggregation::Mean))
        );
        assert_eq!(
            aggregate_gauge(&[1.0, 3.0, 2.0], PrometheusAggregation::Max),
            (Some(3.0), Some(Aggregation::Maximum))
        );
        assert_eq!(
            aggregate_gauge(&[1.0, 3.0, 2.0], PrometheusAggregation::Min),
            (Some(1.0), Some(Aggregation::Minimum))
        );
    }

    #[test]
    fn counter_delta_is_nonnegative_and_reset_is_invalid() {
        assert_eq!(counter_delta(Some(5.0), &[6.0, 9.0]), Ok(Some(4.0)));
        assert_eq!(counter_delta(Some(5.0), &[6.0, 4.0, 9.0]), Err(()));
        assert_eq!(counter_delta(Some(5.0), &[]), Ok(Some(0.0)));
        assert_eq!(counter_delta(None, &[9.0]), Ok(None));
    }

    #[test]
    fn exposition_rejects_timestamp_and_bounds_large_samples() {
        assert_eq!(
            parse_exposition(b"synvoid_cpu_percent 1 1700000000\n", &mapping()),
            Err("sample_invalid")
        );
        assert_eq!(
            parse_exposition(&vec![b' '; MAX_SCRAPE_BYTES + 1], &mapping()),
            Err("scrape_too_large")
        );
    }

    #[test]
    fn exposition_type_metadata_must_match_the_mapping() {
        assert_eq!(
            parse_exposition(
                b"# TYPE synvoid_cpu_percent counter\nsynvoid_cpu_percent 3\n",
                &mapping()
            ),
            Err("sample_type_mismatch")
        );
    }

    #[tokio::test]
    async fn collector_synchronizes_trial_and_emits_gauge_and_counter_delta() {
        let (endpoint, requests, server) = local_metrics_origin().await;
        let mut collector = PrometheusHttpCollector::new(
            &endpoint,
            collector_mapping(),
            "ab".repeat(32),
            "subject".to_owned(),
            "metrics_url".to_owned(),
            "mapping.json".to_owned(),
            vec![
                RequestedPrometheusMetric {
                    name: "subject_cpu_percent".to_owned(),
                    unit: "percent".to_owned(),
                },
                RequestedPrometheusMetric {
                    name: "subject_requests_total".to_owned(),
                    unit: "count".to_owned(),
                },
            ],
            MIN_POLL_INTERVAL,
        )
        .unwrap();
        let preflight = TelemetryPreflightContext {
            run_id: RunId::new(),
            cancellation: CancellationToken::new(),
            timeout: Duration::from_secs(2),
        };
        let capability = collector.preflight(preflight).await.unwrap();
        assert_eq!(capability.poll_interval, MIN_POLL_INTERVAL);
        let trial = |trial_id| TelemetryTrialContext {
            run_id: RunId::new(),
            trial_id: TrialId::new(trial_id).unwrap(),
            cancellation: CancellationToken::new(),
            timeout: Duration::from_secs(2),
        };
        collector.start_trial(trial(1)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(120)).await;
        let output = collector.stop_trial(trial(1)).await.unwrap();
        assert!(requests.load(Ordering::SeqCst) >= 4);
        assert!(
            output
                .artifacts
                .iter()
                .any(|artifact| artifact.name == "prometheus-provenance.json")
        );
        assert!(
            output
                .metrics
                .iter()
                .any(|metric| metric.name == "subject_cpu_percent"
                    && metric.aggregation == Aggregation::Maximum)
        );
        assert!(
            output
                .metrics
                .iter()
                .any(|metric| metric.name == "subject_requests_total"
                    && metric.value > 0.0
                    && metric.aggregation == Aggregation::Direct)
        );
        assert!(output.artifacts.iter().all(|artifact| {
            !String::from_utf8_lossy(&artifact.bytes).contains("subject_cpu_percent 1")
        }));
        collector.start_trial(trial(2)).await.unwrap();
        collector
            .drain(DrainContext {
                run_id: RunId::new(),
                cancellation: CancellationToken::new(),
                timeout: Duration::from_secs(1),
            })
            .await
            .unwrap();
        assert!(collector.window.is_none());
        server.abort();
    }

    #[tokio::test]
    async fn required_missing_field_fails_preflight_and_optional_field_stays_missing() {
        let (endpoint, server) = fixed_metrics_origin("unrelated_metric 1\n").await;
        let requested = vec![RequestedPrometheusMetric {
            name: "subject_cpu_percent".to_owned(),
            unit: "percent".to_owned(),
        }];
        let mut required = PrometheusHttpCollector::new(
            &endpoint,
            mapping(),
            "ab".repeat(32),
            "subject".to_owned(),
            "metrics_url".to_owned(),
            "mapping.json".to_owned(),
            requested.clone(),
            MIN_POLL_INTERVAL,
        )
        .unwrap();
        let preflight = TelemetryPreflightContext {
            run_id: RunId::new(),
            cancellation: CancellationToken::new(),
            timeout: Duration::from_secs(2),
        };
        assert_eq!(
            required.preflight(preflight).await.unwrap_err().category,
            "required_metric_missing"
        );

        let mut optional_mapping = mapping();
        optional_mapping.fields[0].required = false;
        let mut optional = PrometheusHttpCollector::new(
            &endpoint,
            optional_mapping,
            "cd".repeat(32),
            "subject".to_owned(),
            "metrics_url".to_owned(),
            "mapping.json".to_owned(),
            requested,
            MIN_POLL_INTERVAL,
        )
        .unwrap();
        let preflight = TelemetryPreflightContext {
            run_id: RunId::new(),
            cancellation: CancellationToken::new(),
            timeout: Duration::from_secs(2),
        };
        optional.preflight(preflight).await.unwrap();
        let trial = TelemetryTrialContext {
            run_id: RunId::new(),
            trial_id: TrialId::new(1).unwrap(),
            cancellation: CancellationToken::new(),
            timeout: Duration::from_secs(2),
        };
        optional.start_trial(trial.clone()).await.unwrap();
        let output = optional.stop_trial(trial).await.unwrap();
        assert!(output.metrics.is_empty());
        assert!(
            output
                .warnings
                .iter()
                .any(|warning| warning.category == "prometheus_missing_samples")
        );
        server.abort();
    }

    #[tokio::test]
    async fn drain_cancels_polling_and_the_task_never_outlives_the_window() {
        let body = Arc::new(Mutex::new(gauge_and_counter_body("1", 1)));
        let (endpoint, scrapes, _bytes, server) = scripted_metrics_origin(body).await;
        let mut collector = scripted_collector(&endpoint, 'a').unwrap();
        collector.preflight(preflight_context()).await.unwrap();
        let trial = trial_context(1);
        collector.start_trial(trial.clone()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;
        let before_drain = scrapes.load(Ordering::SeqCst);
        assert!(before_drain >= 2, "polling must run inside the window");
        // Cancellation alone must not be relied on for teardown: drain owns
        // joining the polling task.
        trial.cancellation.cancel();
        collector
            .drain(DrainContext {
                run_id: RunId::new(),
                cancellation: CancellationToken::new(),
                timeout: Duration::from_secs(2),
            })
            .await
            .unwrap();
        assert!(collector.window.is_none());
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert_eq!(
            scrapes.load(Ordering::SeqCst),
            before_drain,
            "no scrape may be issued after drain"
        );
        // A drained collector cannot be stopped; the window is gone.
        assert_eq!(
            collector
                .stop_trial(trial_context(1))
                .await
                .unwrap_err()
                .category,
            "polling_failed"
        );
        server.abort();
    }

    #[tokio::test]
    async fn type_drift_on_a_required_sample_fails_preflight_closed() {
        let body = Arc::new(Mutex::new(
            "# TYPE subject_cpu_percent counter\nsubject_cpu_percent 4\nsubject_requests_total 7\n"
                .to_owned(),
        ));
        let (endpoint, _scrapes, _bytes, server) = scripted_metrics_origin(body).await;
        let mut collector = scripted_collector(&endpoint, 'a').unwrap();
        assert_eq!(
            collector
                .preflight(preflight_context())
                .await
                .unwrap_err()
                .category,
            "sample_type_mismatch"
        );
        server.abort();
    }

    #[tokio::test]
    async fn required_counter_reset_during_the_trial_fails_closed() {
        let body = Arc::new(Mutex::new(gauge_and_counter_body("1", 1)));
        let (endpoint, _scrapes, _bytes, server) = scripted_metrics_origin(Arc::clone(&body)).await;
        let mut collector = scripted_collector(&endpoint, 'a').unwrap();
        collector.preflight(preflight_context()).await.unwrap();
        let trial = trial_context(1);
        collector.start_trial(trial.clone()).await.unwrap();
        *body.lock().unwrap() = gauge_and_counter_body("1", 9);
        tokio::time::sleep(Duration::from_millis(150)).await;
        // A reset is a restart of the owner counter, never a negative delta.
        *body.lock().unwrap() = gauge_and_counter_body("1", 2);
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(
            collector.stop_trial(trial).await.unwrap_err().category,
            "required_metric_invalid"
        );
        server.abort();
    }

    #[tokio::test]
    async fn required_sample_disappearing_mid_trial_fails_closed() {
        let body = Arc::new(Mutex::new(gauge_and_counter_body("1", 1)));
        let (endpoint, _scrapes, _bytes, server) = scripted_metrics_origin(Arc::clone(&body)).await;
        let mut collector = scripted_collector(&endpoint, 'a').unwrap();
        collector.preflight(preflight_context()).await.unwrap();
        let trial = trial_context(1);
        collector.start_trial(trial.clone()).await.unwrap();
        // Required gauge disappears while the trial is still measured.
        *body.lock().unwrap() =
            "# TYPE subject_requests_total counter\nsubject_requests_total 4\n".to_owned();
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(
            collector.stop_trial(trial).await.unwrap_err().category,
            "required_metric_invalid"
        );
        server.abort();
    }

    #[tokio::test]
    async fn polling_is_bounded_by_the_declared_cadence() {
        let body = Arc::new(Mutex::new(gauge_and_counter_body("1", 1)));
        let (endpoint, scrapes, served_bytes, server) = scripted_metrics_origin(body).await;
        let mut collector = scripted_collector(&endpoint, 'a').unwrap();
        collector.preflight(preflight_context()).await.unwrap();
        let trial = trial_context(1);
        collector.start_trial(trial).await.unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        let in_window = scrapes.load(Ordering::SeqCst);
        // One cadence start snapshot, at most one in-flight scrape at a time,
        // plus the bounded stop snapshot: never a busy loop.
        assert!(
            (3..=8).contains(&in_window),
            "unexpected scrape count {in_window} at {MIN_POLL_INTERVAL:?} cadence"
        );
        assert!(served_bytes.load(Ordering::SeqCst) > 0);
        let stopped = collector.stop_trial(trial_context(1)).await.unwrap();
        assert!(
            stopped
                .metrics
                .iter()
                .any(|metric| metric.name == "subject_requests_total")
        );
        server.abort();
    }

    #[tokio::test]
    async fn mapping_digest_is_capability_and_provenance_identity() {
        let body = Arc::new(Mutex::new(gauge_and_counter_body("1", 1)));
        let (endpoint, _scrapes, _bytes, server) = scripted_metrics_origin(body).await;
        let mut first = scripted_collector(&endpoint, 'a').unwrap();
        let mut second = scripted_collector(&endpoint, 'b').unwrap();
        let first_identity = first.preflight(preflight_context()).await.unwrap().identity;
        let second_identity = second
            .preflight(preflight_context())
            .await
            .unwrap()
            .identity;
        assert_eq!(first_identity.get("mapping_sha256"), Some(&"a".repeat(64)));
        assert_eq!(second_identity.get("mapping_sha256"), Some(&"b".repeat(64)));
        assert_ne!(first_identity, second_identity);
        for collector in [&mut first, &mut second] {
            let trial = trial_context(1);
            collector.start_trial(trial.clone()).await.unwrap();
            let output = collector.stop_trial(trial).await.unwrap();
            let provenance = output
                .artifacts
                .iter()
                .find(|artifact| artifact.name == "prometheus-provenance.json")
                .expect("provenance artifact");
            let document: serde_json::Value =
                serde_json::from_slice(&provenance.bytes).expect("provenance json");
            assert_eq!(
                document["mapping_sha256"],
                collector.mapping_sha256.as_str(),
                "provenance must carry the pinned mapping identity"
            );
            assert_eq!(document["source"], PROMETHEUS_HTTP_SOURCE);
            assert_eq!(document["poll_interval_ms"], 100);
        }
        server.abort();
    }

    #[test]
    fn mapping_cannot_publish_a_host_named_output() {
        // Gregg host metrics keep the `host_` prefix; the generic subject
        // collector may not shadow or rename them.
        let mut collision = collector_mapping();
        collision.fields[0].output_name = "host_cpu_percent".to_owned();
        assert_eq!(validate_mapping(&collision), Err("mapping_field_invalid"));
        let mut owner_metric_name = collector_mapping();
        owner_metric_name.fields[0].output_name = "subject_synvoid_cpu_percent".to_owned();
        assert_eq!(validate_mapping(&owner_metric_name), Ok(()));
    }

    #[test]
    fn exposition_line_and_sample_ring_bounds_are_enforced() {
        let lines = b"subject_cpu_percent 1\n".repeat(MAX_EXPOSITION_LINES + 1);
        assert_eq!(
            parse_exposition(&lines, &collector_mapping()),
            Err("scrape_line_bound")
        );
        // The in-window ring buffer is bounded; overflow is counted, never
        // silently forgotten.
        let window = Arc::new(Mutex::new(PrometheusWindow::default()));
        let mapping = collector_mapping();
        let mut values = BTreeMap::new();
        values.insert("subject_cpu_percent".to_owned(), 1.0);
        values.insert("subject_requests_total".to_owned(), 1.0);
        for _ in 0..600 {
            push_snapshot(&window, values.clone(), &mapping);
        }
        let state = window.lock().expect("window lock");
        assert_eq!(state.samples.len(), 512);
        assert_eq!(state.dropped_samples, 88);
    }

    #[test]
    fn endpoint_policy_rejects_credentials_queries_and_public_names() {
        for invalid in [
            "http://user@127.0.0.1:9100/metrics",
            "http://127.0.0.1:9100/metrics?x=1",
            "http://127.0.0.1:0/metrics",
            "http://example.com:80/metrics",
            "http://172.32.0.1:9100/metrics",
        ] {
            assert!(
                validate_private_endpoint(invalid).is_err(),
                "{invalid} must be rejected"
            );
        }
        assert!(validate_private_endpoint("http://[::1]:9100/metrics").is_ok());
        assert!(validate_private_endpoint("http://192.168.1.10:9100/metrics").is_ok());
    }
}
