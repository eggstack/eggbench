//! `Eggfetch` native HTTP closed-loop workload adapter.
//!
//! One [`EggfetchWorkload`] owns one [`eggfetch_core::Client`] for the whole
//! run: warmups establish pool/connection state and measured trials reflect a
//! warmed run when warmups are configured. Connection reuse is part of the
//! driver method provenance recorded in every invocation's method artifact.
//!
//! Request semantics: HTTP GET against the workload target service's
//! `http_url` runtime binding, full response-body consumption, no
//! Eggbench-level retries, no redirect following (the lean
//! `standard-http1` feature profile omits `Eggfetch` logical retry/redirect
//! support by construction). Only closed-loop load is supported; open-loop
//! requests fail before startup through plan resolution (missing
//! `LoadMode::OpenLoop` capability) and are rejected defensively here.
//!
//! When the `eggstack-path` feature is enabled, the workload can be
//! constructed with a [`crate::eggstack::path::EggstackPathDialer`] so the
//! client reaches its target via a listener-free Eggress route composed with
//! deterministic Eggchaos stream faults. The dialer is queried once per
//! physical connection (the client owns the pool; warmups reuse dialed
//! physical connections naturally).
//!
//! Ownership recap: `Eggfetch` owns outbound HTTP semantics; Eggbench owns
//! scheduling, latency measurement (dispatch to full-body consumption),
//! metric mapping, and evidence.

use super::origin::ORIGIN_HTTP_URL_KEY;
use super::{EGGFETCH_CORE_VERSION, EGGFETCH_HTTP_DRIVER_NAME};
use eggbench_core::{Aggregation, RawHistogramInput, RawMetricObservation, Workload};
#[cfg(feature = "eggstack-path")]
use eggbench_runner::RunEvidenceArtifact;
use eggbench_runner::{
    DrainContext, FailureCategory, InvocationContext, WorkloadArtifact, WorkloadExecutor,
    WorkloadOutput,
};
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

/// Raw latency histogram artifact name, staged per invocation.
pub const LATENCY_HISTOGRAM_ARTIFACT: &str = "latency.hdr";
/// Driver method-evidence artifact name, staged per invocation.
pub const METHOD_ARTIFACT: &str = "eggfetch-method.json";
/// Encoding identifier for the retained histogram bytes.
pub const HISTOGRAM_FORMAT: &str = "hdrhistogram-v2";
/// Logical distribution name referenced by [`RawHistogramInput`].
pub const HISTOGRAM_METRIC: &str = "latency";
/// Unit of the retained latency distribution (integer microseconds).
pub const HISTOGRAM_UNIT: &str = "us";
/// Histogram lower bound in microseconds.
pub const HISTOGRAM_LOW_US: u64 = 1;
/// Histogram upper bound in microseconds (60 s); larger samples saturate.
pub const HISTOGRAM_HIGH_US: u64 = 60_000_000;
/// Histogram significant figures.
pub const HISTOGRAM_SIGFIG: u8 = 3;
/// Per-request timeout in seconds. The runner safety deadline still bounds
/// the whole invocation; this keeps one hung request from consuming it.
pub const REQUEST_TIMEOUT_SECS: u64 = 30;

/// Stable error-category labels. `Eggfetch` error strings never become
/// category identities.
const CATEGORY_TRANSPORT: &str = "transport";
const CATEGORY_TIMEOUT: &str = "timeout";
const CATEGORY_HTTP_3XX: &str = "http_3xx";
const CATEGORY_HTTP_4XX: &str = "http_4xx";
const CATEGORY_HTTP_5XX: &str = "http_5xx";
const CATEGORY_BODY_READ: &str = "body_read";
const CATEGORY_CANCELLED: &str = "cancelled";

/// Native HTTP workload driver for the `eggfetch-http` driver name.
///
/// Holds one `Eggfetch` client for the executor lifetime (one run).
pub struct EggfetchWorkload {
    client: eggfetch_core::Client,
    #[cfg(feature = "eggstack-path")]
    path_dialer: Option<Arc<super::path::EggstackPathDialer>>,
}

impl EggfetchWorkload {
    /// Create an executor with a fresh default Eggfetch client.
    #[must_use]
    pub fn new() -> Self {
        Self {
            client: eggfetch_core::Client::new(),
            #[cfg(feature = "eggstack-path")]
            path_dialer: None,
        }
    }

    /// Create an executor backed by the resolved Eggstack path dialer.
    #[cfg(feature = "eggstack-path")]
    #[must_use]
    pub fn with_path_dialer(dialer: Arc<super::path::EggstackPathDialer>) -> Self {
        let erased: Arc<dyn eggfetch_core::Dialer> = dialer.clone();
        let client = eggfetch_core::Client::builder().dialer(erased).build();
        Self {
            client,
            path_dialer: Some(dialer),
        }
    }

    /// Underlying client (used by tests that need direct access).
    #[must_use]
    pub fn client(&self) -> &eggfetch_core::Client {
        &self.client
    }

    /// Optional path dialer retained for evidence.
    #[cfg(feature = "eggstack-path")]
    #[must_use]
    pub fn path_dialer(&self) -> Option<&Arc<super::path::EggstackPathDialer>> {
        self.path_dialer.as_ref()
    }
}

impl Default for EggfetchWorkload {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for EggfetchWorkload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EggfetchWorkload")
            .field("driver", &EGGFETCH_HTTP_DRIVER_NAME)
            .field("eggfetch_core", &EGGFETCH_CORE_VERSION)
            .finish()
    }
}

/// One request outcome collected by a worker.
struct RequestOutcome {
    /// Elapsed dispatch-to-full-body time for responses received.
    latency: Option<Duration>,
    /// HTTP status when a response head was received.
    status: Option<u16>,
    /// Body bytes fully consumed.
    bytes: u64,
    /// Failure category when the request did not complete cleanly.
    error: Option<&'static str>,
}

/// Resolved closed-loop run plan for one invocation.
enum RunPlan {
    /// Issue exactly `total` requests with `concurrency` workers.
    Count { total: u64, concurrency: usize },
    /// Issue until `deadline` with `concurrency` workers.
    Deadline {
        deadline: Instant,
        concurrency: usize,
    },
}

impl WorkloadExecutor for EggfetchWorkload {
    fn execute<'a>(
        &'a mut self,
        context: InvocationContext,
    ) -> Pin<Box<dyn Future<Output = Result<WorkloadOutput, FailureCategory>> + Send + 'a>> {
        Box::pin(async move { self.execute_inner(context).await })
    }

    fn drain<'a>(
        &'a mut self,
        _context: DrainContext,
    ) -> Pin<Box<dyn Future<Output = Result<(), FailureCategory>> + Send + 'a>> {
        // The client owns no Eggbench-managed tasks or resources; connection
        // pool state is Eggfetch-owned and needs no explicit close.
        Box::pin(async move { Ok(()) })
    }

    #[cfg(feature = "eggstack-path")]
    fn run_evidence(&mut self) -> Result<Option<RunEvidenceArtifact>, eggbench_core::BundleError> {
        let Some(dialer) = &self.path_dialer else {
            return Ok(None);
        };
        let evidence = dialer.network_path_evidence();
        RunEvidenceArtifact::from_contract(
            "network-path.json",
            super::path::network_path_role_label(),
            "application/json",
            eggbench_core::Sensitivity::Redacted,
            &evidence,
        )
        .map(Some)
    }
}

impl EggfetchWorkload {
    async fn execute_inner(
        &mut self,
        context: InvocationContext,
    ) -> Result<WorkloadOutput, FailureCategory> {
        let (target, plan) = run_plan(&context.workload)?;
        let url = binding_url(&context, target)?;
        #[cfg(feature = "eggstack-path")]
        let path_start = self
            .path_dialer
            .as_ref()
            .map(|dialer| dialer.begin_invocation());
        let started = Instant::now();
        let (outcomes, max_in_flight) = run_closed_loop(
            &self.client,
            &url,
            &plan,
            &context.cancellation,
            context.timeout,
        )
        .await;
        let elapsed = started.elapsed();
        context.measurement.finish(elapsed);
        #[cfg(feature = "eggstack-path")]
        let network_path = self
            .path_dialer
            .as_ref()
            .zip(path_start)
            .map(|(dialer, before)| {
                let mut value = serde_json::to_value(dialer.invocation_delta(&before))
                    .expect("path diagnostics serialize");
                value
                    .as_object_mut()
                    .expect("path diagnostics are an object")
                    .insert(
                        "faults_active".to_owned(),
                        serde_json::json!(dialer.faults_active()),
                    );
                value
            });
        #[cfg(not(feature = "eggstack-path"))]
        let network_path: Option<serde_json::Value> = None;
        Ok(build_output(
            &outcomes,
            &plan,
            elapsed,
            max_in_flight,
            network_path.as_ref(),
        ))
    }
}

/// Lower the plan workload to a closed-loop run plan.
///
/// Open-loop and open-mode time-bounded workloads are rejected: resolution
/// already fails them through the advertised capability set, and this is the
/// defense-in-depth rejection before any request is issued.
fn run_plan(workload: &Workload) -> Result<(&str, RunPlan), FailureCategory> {
    match workload {
        Workload::ClosedLoop {
            target,
            concurrency,
            requests,
            duration_ms,
        } => {
            let concurrency = concurrency_count(concurrency.get())?;
            match (requests, duration_ms) {
                (Some(requests), None) => Ok((
                    target.as_str(),
                    RunPlan::Count {
                        total: u64::from(requests.get()),
                        concurrency,
                    },
                )),
                (None, Some(duration)) => Ok((
                    target.as_str(),
                    RunPlan::Deadline {
                        deadline: Instant::now() + Duration::from_millis(duration.get()),
                        concurrency,
                    },
                )),
                _ => Err(FailureCategory::WorkloadFailed),
            }
        }
        Workload::FiniteCount {
            target,
            requests,
            concurrency,
        } => Ok((
            target.as_str(),
            RunPlan::Count {
                total: u64::from(requests.get()),
                concurrency: concurrency_count(concurrency.get())?,
            },
        )),
        Workload::TimeBounded {
            target,
            duration_ms,
            mode,
            concurrency,
            ..
        } => {
            if *mode != eggbench_core::LoadMode::ClosedLoop {
                return Err(FailureCategory::WorkloadFailed);
            }
            let concurrency = match concurrency {
                Some(count) => concurrency_count(count.get())?,
                None => 1,
            };
            Ok((
                target.as_str(),
                RunPlan::Deadline {
                    deadline: Instant::now() + Duration::from_millis(duration_ms.get()),
                    concurrency,
                },
            ))
        }
        Workload::OpenLoop { .. } => Err(FailureCategory::WorkloadFailed),
    }
}

fn concurrency_count(raw: u32) -> Result<usize, FailureCategory> {
    usize::try_from(raw).map_err(|_| FailureCategory::WorkloadFailed)
}

/// Read the workload target service's `http_url` runtime binding.
///
/// The binding is startup-established and read-only here; a missing binding
/// fails the invocation without issuing any request.
fn binding_url(context: &InvocationContext, target: &str) -> Result<String, FailureCategory> {
    context
        .bindings
        .service_bindings(target)
        .and_then(|bindings| bindings.get(ORIGIN_HTTP_URL_KEY))
        .cloned()
        .ok_or(FailureCategory::WorkloadFailed)
}

/// Run the closed-loop schedule: at most `concurrency` active requests; each
/// worker issues the next request when its previous one finishes.
///
/// Cancellation stops issuance of new requests; in-flight requests race the
/// token and the per-request timeout. All worker tasks are joined before
/// return so no per-request task leaks after the invocation.
async fn run_closed_loop(
    client: &eggfetch_core::Client,
    url: &str,
    plan: &RunPlan,
    cancel: &CancellationToken,
    invocation_timeout: Duration,
) -> (Vec<RequestOutcome>, usize) {
    let (concurrency, remaining, deadline) = match plan {
        RunPlan::Count { total, concurrency } => (*concurrency, *total, None),
        RunPlan::Deadline {
            deadline,
            concurrency,
        } => (*concurrency, u64::MAX, Some(*deadline)),
    };
    // The invocation deadline bounds issuance even if the orchestrator's
    // external timeout is the primary guard.
    let issue_deadline = Instant::now().checked_add(invocation_timeout);
    let shared = Arc::new(Shared {
        client: client.clone(),
        url: url.to_owned(),
        remaining: AtomicU64::new(remaining),
        deadline,
        issue_deadline,
        cancel: cancel.clone(),
        in_flight: AtomicUsize::new(0),
        max_in_flight: AtomicUsize::new(0),
    });
    let mut tasks = JoinSet::new();
    for _ in 0..concurrency.max(1) {
        tasks.spawn(worker(Arc::clone(&shared)));
    }
    let mut outcomes = Vec::new();
    while let Some(joined) = tasks.join_next().await {
        if let Ok(worker_outcomes) = joined {
            outcomes.extend(worker_outcomes);
        }
    }
    let max_in_flight = shared.max_in_flight.load(Ordering::SeqCst);
    (outcomes, max_in_flight)
}

struct Shared {
    client: eggfetch_core::Client,
    url: String,
    remaining: AtomicU64,
    deadline: Option<Instant>,
    issue_deadline: Option<Instant>,
    cancel: CancellationToken,
    in_flight: AtomicUsize,
    max_in_flight: AtomicUsize,
}

async fn worker(shared: Arc<Shared>) -> Vec<RequestOutcome> {
    let mut outcomes = Vec::new();
    loop {
        if shared.cancel.is_cancelled() {
            break;
        }
        if let Some(deadline) = shared.deadline
            && Instant::now() >= deadline
        {
            break;
        }
        if let Some(deadline) = shared.issue_deadline
            && Instant::now() >= deadline
        {
            break;
        }
        // Claim one request. `fetch_update` fails once the counter is
        // exhausted, so exactly the planned count is issued.
        let claimed = shared
            .remaining
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                left.checked_sub(1)
            });
        if !matches!(claimed, Ok(left) if left > 0) {
            break;
        }
        outcomes.push(issue_one(&shared).await);
    }
    outcomes
}

/// Issue one GET and consume the full response body.
///
/// Latency spans request dispatch to full-body consumption or terminal
/// error; post-invocation normalization is never included.
async fn issue_one(shared: &Shared) -> RequestOutcome {
    if shared.cancel.is_cancelled() {
        return RequestOutcome {
            latency: None,
            status: None,
            bytes: 0,
            error: Some(CATEGORY_CANCELLED),
        };
    }
    let current = shared.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
    shared.max_in_flight.fetch_max(current, Ordering::SeqCst);
    let outcome = issue_timed(shared).await;
    shared.in_flight.fetch_sub(1, Ordering::SeqCst);
    outcome
}

async fn issue_timed(shared: &Shared) -> RequestOutcome {
    let start = Instant::now();
    let request = shared
        .client
        .get(shared.url.as_str())
        .map(|builder| builder.timeout(eggfetch_core::Timeout::from_secs(REQUEST_TIMEOUT_SECS)));
    let Ok(builder) = request else {
        return RequestOutcome {
            latency: None,
            status: None,
            bytes: 0,
            error: Some(CATEGORY_TRANSPORT),
        };
    };
    // Race the in-flight request against cancellation so teardown never
    // waits on a hung request beyond the per-request timeout.
    let response = tokio::select! {
        () = shared.cancel.cancelled() => {
            return RequestOutcome {
                latency: None,
                status: None,
                bytes: 0,
                error: Some(CATEGORY_CANCELLED),
            };
        }
        result = builder.send_detailed() => result,
    };
    let mut response = match response {
        Ok(response) => response,
        Err(failure) => {
            return RequestOutcome {
                latency: None,
                status: None,
                bytes: 0,
                error: Some(if is_timeout_failure(&failure) {
                    CATEGORY_TIMEOUT
                } else {
                    CATEGORY_TRANSPORT
                }),
            };
        }
    };
    let status = response.status().as_u16();
    let bytes = tokio::select! {
        () = shared.cancel.cancelled() => {
            return RequestOutcome {
                latency: None,
                status: Some(status),
                bytes: 0,
                error: Some(CATEGORY_CANCELLED),
            };
        }
        body = response.bytes() => match body {
            Ok(bytes) => bytes.len() as u64,
            Err(error) => {
                return RequestOutcome {
                    latency: None,
                    status: Some(status),
                    bytes: 0,
                    error: Some(if is_timeout_error(&error) {
                        CATEGORY_TIMEOUT
                    } else {
                        CATEGORY_BODY_READ
                    }),
                };
            }
        },
    };
    let latency = start.elapsed();
    // Non-2xx responses still completed the round trip: they carry timing
    // and byte evidence while counting as HTTP-status errors.
    let error = match status {
        200..=299 => None,
        300..=399 => Some(CATEGORY_HTTP_3XX),
        400..=499 => Some(CATEGORY_HTTP_4XX),
        _ => Some(CATEGORY_HTTP_5XX),
    };
    RequestOutcome {
        latency: Some(latency),
        status: Some(status),
        bytes,
        error,
    }
}

fn is_timeout_failure(failure: &eggfetch_core::RequestFailure) -> bool {
    failure.is_timeout()
        || failure
            .error()
            .custom_transport_error()
            .is_some_and(|error| error.kind() == eggfetch_core::DialErrorKind::Timeout)
}

fn is_timeout_error(error: &eggfetch_core::Error) -> bool {
    matches!(
        error,
        eggfetch_core::Error::Timeout { .. } | eggfetch_core::Error::TransportIoTimeout { .. }
    )
}

/// Aggregate outcomes into raw metrics, error counts, histogram inputs, and
/// the retained `latency.hdr` plus method-evidence artifacts.
/// Fold request outcomes into histogram, category, and volume totals.
struct Folded {
    histogram: hdrhistogram::Histogram<u64>,
    categories: BTreeMap<&'static str, u64>,
    completed: u64,
    timeouts: u64,
    bytes_received: u64,
}

fn fold_outcomes(outcomes: &[RequestOutcome]) -> Folded {
    let mut folded = Folded {
        histogram: hdrhistogram::Histogram::<u64>::new_with_bounds(
            HISTOGRAM_LOW_US,
            HISTOGRAM_HIGH_US,
            HISTOGRAM_SIGFIG,
        )
        .expect("static histogram bounds are valid"),
        categories: BTreeMap::new(),
        completed: 0,
        timeouts: 0,
        bytes_received: 0,
    };
    for outcome in outcomes {
        if let Some(category) = outcome.error {
            *folded.categories.entry(category).or_default() += 1;
        }
        if outcome.error == Some(CATEGORY_TIMEOUT) {
            folded.timeouts += 1;
        }
        if let Some(latency) = outcome.latency {
            folded.completed += 1;
            folded.bytes_received += outcome.bytes;
            let micros = u64::try_from(latency.as_micros()).unwrap_or(u64::MAX);
            let _ = folded
                .histogram
                .record(micros.clamp(HISTOGRAM_LOW_US, HISTOGRAM_HIGH_US));
        }
    }
    folded
}

/// Lossless-as-practical count conversion for metric values. Request counts
/// are plan-bounded far below `u32::MAX`; larger values saturate instead of
/// losing precision silently.
fn count_f64(count: u64) -> f64 {
    f64::from(u32::try_from(count).unwrap_or(u32::MAX))
}

/// Push volume/rate metrics for one invocation.
fn push_volume_metrics(
    metrics: &mut Vec<RawMetricObservation>,
    attempted: u64,
    folded: &Folded,
    elapsed: Duration,
) {
    let errors: u64 = folded.categories.values().sum();
    let elapsed_secs = elapsed.as_secs_f64().max(f64::MIN_POSITIVE);
    let rate = |count: u64| count_f64(count) / count_f64(attempted);
    metrics.push(raw(
        "throughput",
        "rps",
        count_f64(folded.completed) / elapsed_secs,
        Aggregation::Rate,
        "eggfetch.responses_received",
        &[],
    ));
    metrics.push(raw(
        "error_rate",
        "ratio",
        rate(errors),
        Aggregation::Ratio,
        "eggfetch.error_counts",
        &[],
    ));
    metrics.push(raw(
        "timeout_rate",
        "ratio",
        rate(folded.timeouts),
        Aggregation::Ratio,
        "eggfetch.timeouts",
        &[],
    ));
    metrics.push(raw(
        "bytes_received",
        "bytes",
        count_f64(folded.bytes_received),
        Aggregation::Sum,
        "eggfetch.bytes_received",
        &[],
    ));
}

/// Push latency distribution metrics derived from the retained histogram.
fn push_latency_metrics(
    metrics: &mut Vec<RawMetricObservation>,
    histogram: &hdrhistogram::Histogram<u64>,
) {
    let micros_to_ms = |value: u64| count_f64(value) / 1000.0;
    let with_histogram = [LATENCY_HISTOGRAM_ARTIFACT.to_owned()];
    metrics.push(raw(
        "latency_min",
        "ms",
        micros_to_ms(histogram.min()),
        Aggregation::Minimum,
        "eggfetch.latency_histogram",
        &with_histogram,
    ));
    metrics.push(raw(
        "latency_mean",
        "ms",
        histogram.mean() / 1000.0,
        Aggregation::Mean,
        "eggfetch.latency_histogram",
        &with_histogram,
    ));
    for (name, quantile, basis_points) in [
        ("latency_p50", 0.50, 5_000),
        ("latency_p90", 0.90, 9_000),
        ("latency_p95", 0.95, 9_500),
        ("latency_p99", 0.99, 9_900),
        ("latency_p999", 0.999, 9_990),
    ] {
        metrics.push(raw(
            name,
            "ms",
            micros_to_ms(histogram.value_at_quantile(quantile)),
            Aggregation::Percentile { basis_points },
            "eggfetch.latency_histogram",
            &with_histogram,
        ));
    }
}

/// Aggregate outcomes into raw metrics, error counts, histogram inputs, and
/// the retained `latency.hdr` plus method-evidence artifacts.
fn build_output(
    outcomes: &[RequestOutcome],
    plan: &RunPlan,
    elapsed: Duration,
    max_in_flight: usize,
    network_path: Option<&serde_json::Value>,
) -> WorkloadOutput {
    let attempted = outcomes.len() as u64;
    let folded = fold_outcomes(outcomes);
    let mut metrics = Vec::new();
    if attempted > 0 {
        push_volume_metrics(&mut metrics, attempted, &folded, elapsed);
    }
    if !folded.histogram.is_empty() {
        push_latency_metrics(&mut metrics, &folded.histogram);
    }

    let histograms = vec![RawHistogramInput {
        metric: HISTOGRAM_METRIC.to_owned(),
        artifact_name: LATENCY_HISTOGRAM_ARTIFACT.to_owned(),
        format: HISTOGRAM_FORMAT.to_owned(),
        unit: HISTOGRAM_UNIT.to_owned(),
        method: Some(
            "closed-loop dispatch-to-full-body micros, saturating, no coordinated-omission correction"
                .to_owned(),
        ),
    }];
    let error_counts: Vec<(String, u64)> = folded
        .categories
        .iter()
        .map(|(category, count)| ((*category).to_owned(), *count))
        .collect();

    let hdr_bytes = serialize_histogram(&folded.histogram);
    let method = method_evidence(
        plan,
        attempted,
        folded.completed,
        &folded.categories,
        elapsed,
        max_in_flight,
        outcomes,
        network_path,
    );
    let method_bytes = serde_json::to_vec_pretty(&method).expect("method evidence serializes");
    WorkloadOutput {
        artifacts: vec![
            WorkloadArtifact {
                name: LATENCY_HISTOGRAM_ARTIFACT.to_owned(),
                media_type: "application/x-hdrhistogram-v2".to_owned(),
                bytes: hdr_bytes,
            },
            WorkloadArtifact {
                name: METHOD_ARTIFACT.to_owned(),
                media_type: "application/json".to_owned(),
                bytes: method_bytes,
            },
        ],
        metrics,
        histograms,
        error_counts,
        measurement_elapsed: Some(elapsed),
    }
}

fn raw(
    name: &str,
    unit: &str,
    value: f64,
    aggregation: Aggregation,
    source_field: &str,
    raw_artifacts: &[String],
) -> RawMetricObservation {
    RawMetricObservation {
        name: name.to_owned(),
        unit: unit.to_owned(),
        value,
        aggregation,
        source_field: Some(source_field.to_owned()),
        producer: None,
        producer_version: None,
        raw_artifacts: raw_artifacts.to_vec(),
    }
}

/// Serialize the histogram in the documented V2 binary encoding.
fn serialize_histogram(histogram: &hdrhistogram::Histogram<u64>) -> Vec<u8> {
    use hdrhistogram::serialization::Serializer;
    let mut bytes = Vec::new();
    let mut serializer = hdrhistogram::serialization::V2Serializer::new();
    serializer
        .serialize(histogram, &mut bytes)
        .expect("histogram serializes into memory");
    bytes
}

/// Diagnostic method evidence retained per invocation.
#[allow(clippy::too_many_arguments)] // Keep the method-evidence inputs explicit at the call site.
fn method_evidence(
    plan: &RunPlan,
    attempted: u64,
    completed: u64,
    categories: &BTreeMap<&'static str, u64>,
    elapsed: Duration,
    max_in_flight: usize,
    outcomes: &[RequestOutcome],
    network_path: Option<&serde_json::Value>,
) -> serde_json::Value {
    let (requested_concurrency, mode) = match plan {
        RunPlan::Count { concurrency, .. } => (*concurrency, "count"),
        RunPlan::Deadline { concurrency, .. } => (*concurrency, "deadline"),
    };
    let mut status_counts: BTreeMap<u16, u64> = BTreeMap::new();
    for outcome in outcomes {
        if let Some(status) = outcome.status {
            *status_counts.entry(status).or_default() += 1;
        }
    }
    let mut evidence = serde_json::json!({
        "driver": EGGFETCH_HTTP_DRIVER_NAME,
        "eggfetch_core_version": EGGFETCH_CORE_VERSION,
        "http_method": "GET",
        "http_version": "1.1",
        "client_reuse_policy": "one client per executor for the whole run",
        "schedule_mode": mode,
        "requested_concurrency": requested_concurrency,
        "attempted_requests": attempted,
        "completed_requests": completed,
        "error_counts": categories,
        "status_counts": status_counts,
        "elapsed_ms": elapsed.as_millis(),
        "maximum_observed_in_flight": max_in_flight,
        "per_request_timeout_secs": REQUEST_TIMEOUT_SECS,
        "latency_sample_policy": "response-received-only dispatch-to-full-body micros",
        "histogram": {
            "format": HISTOGRAM_FORMAT,
            "low_us": HISTOGRAM_LOW_US,
            "high_us": HISTOGRAM_HIGH_US,
            "significant_figures": HISTOGRAM_SIGFIG,
            "saturation": "clamped",
            "coordinated_omission_correction": "none (closed-loop only)",
        },
    });
    if let Some(network_path) = network_path {
        evidence
            .as_object_mut()
            .expect("method evidence is an object")
            .insert("network_path".to_owned(), network_path.clone());
    }
    evidence
}
