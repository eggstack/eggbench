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
use eggbench_core::{
    Aggregation, HttpCaseBodyV1, HttpConnectionPolicy, HttpSecurityCaseV1, RawHistogramInput,
    RawMetricObservation, Workload, load_http_security_corpus,
};
#[cfg(feature = "eggstack-path")]
use eggbench_runner::RunEvidenceArtifact;
use eggbench_runner::{
    DrainContext, FailureCategory, InvocationContext, WorkloadArtifact, WorkloadExecutor,
    WorkloadOutput,
};
use sha2::Digest as _;
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
const MAX_RESPONSE_BODY_BYTES: usize = 8 * 1024 * 1024;

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

#[derive(Clone)]
struct HttpCorpusShared {
    pooled_client: eggfetch_core::Client,
    origin: String,
    cases: Arc<Vec<HttpSecurityCaseV1>>,
    default_headers: Arc<Vec<(String, String)>>,
    bodies: Arc<Vec<Option<Vec<u8>>>>,
    schedule: Arc<Vec<usize>>,
    next: Arc<AtomicUsize>,
    in_flight: Arc<AtomicUsize>,
    max_in_flight: Arc<AtomicUsize>,
    connection_policy: HttpConnectionPolicy,
    cancel: CancellationToken,
    deadline: Instant,
}

struct HttpCorpusOutcome {
    case_id: String,
    status: Option<u16>,
    latency: Option<Duration>,
    bytes: u64,
    transport_error: Option<&'static str>,
    expected_match: Option<bool>,
}

fn http_origin(binding: &str) -> Result<String, FailureCategory> {
    let scheme_end = binding.find("://").ok_or(FailureCategory::WorkloadFailed)?;
    let authority_start = scheme_end + 3;
    let authority_end = binding[authority_start..]
        .find(['/', '?', '#'])
        .map_or(binding.len(), |offset| authority_start + offset);
    let authority = &binding[authority_start..authority_end];
    if authority.is_empty() || authority.contains('@') || authority.contains('#') {
        return Err(FailureCategory::WorkloadFailed);
    }
    Ok(format!("{}://{authority}", &binding[..scheme_end]))
}

fn digest_json<T: serde::Serialize>(value: &T) -> Result<String, FailureCategory> {
    let encoded = serde_json::to_vec(value).map_err(|_| FailureCategory::WorkloadFailed)?;
    Ok(format!("{:x}", sha2::Sha256::digest(encoded)))
}

fn deterministic_shuffle(values: &mut [usize], state: &mut u64) {
    for index in (1..values.len()).rev() {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        let other = usize::try_from(*state).unwrap_or(usize::MAX) % (index + 1);
        values.swap(index, other);
    }
}

#[allow(clippy::too_many_arguments)] // These are the complete immutable schedule and timing inputs.
async fn run_http_corpus_schedule(
    pooled_client: &eggfetch_core::Client,
    origin: &str,
    cases: &[HttpSecurityCaseV1],
    default_headers: &[(String, String)],
    bodies: Vec<Option<Vec<u8>>>,
    schedule: Vec<usize>,
    concurrency: usize,
    connection_policy: HttpConnectionPolicy,
    cancel: &CancellationToken,
    timeout: Duration,
) -> (Vec<HttpCorpusOutcome>, usize) {
    let shared = HttpCorpusShared {
        pooled_client: pooled_client.clone(),
        origin: origin.to_owned(),
        cases: Arc::new(cases.to_vec()),
        default_headers: Arc::new(default_headers.to_vec()),
        bodies: Arc::new(bodies),
        schedule: Arc::new(schedule),
        next: Arc::new(AtomicUsize::new(0)),
        in_flight: Arc::new(AtomicUsize::new(0)),
        max_in_flight: Arc::new(AtomicUsize::new(0)),
        connection_policy,
        cancel: cancel.clone(),
        deadline: Instant::now() + timeout,
    };
    let mut tasks = JoinSet::new();
    for _ in 0..concurrency.max(1) {
        let worker_state = shared.clone();
        tasks.spawn(async move {
            let mut results = Vec::new();
            loop {
                if worker_state.cancel.is_cancelled() || Instant::now() >= worker_state.deadline {
                    break;
                }
                let ordinal = worker_state.next.fetch_add(1, Ordering::SeqCst);
                let Some(&case_index) = worker_state.schedule.get(ordinal) else {
                    break;
                };
                let active = worker_state.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                worker_state
                    .max_in_flight
                    .fetch_max(active, Ordering::SeqCst);
                results.push(issue_http_corpus_case(&worker_state, case_index).await);
                worker_state.in_flight.fetch_sub(1, Ordering::SeqCst);
            }
            results
        });
    }
    let mut outcomes = Vec::new();
    while let Some(joined) = tasks.join_next().await {
        if let Ok(results) = joined {
            outcomes.extend(results);
        }
    }
    (outcomes, shared.max_in_flight.load(Ordering::SeqCst))
}

async fn issue_http_corpus_case(shared: &HttpCorpusShared, case_index: usize) -> HttpCorpusOutcome {
    let case = &shared.cases[case_index];
    let failed = |error| HttpCorpusOutcome {
        case_id: case.id.clone(),
        status: None,
        latency: None,
        bytes: 0,
        transport_error: Some(error),
        expected_match: None,
    };
    let Ok(method) = eggfetch_core::Method::from_bytes(case.request.method.as_bytes()) else {
        return failed(CATEGORY_TRANSPORT);
    };
    let url = format!("{}{}", shared.origin, case.request.path_and_query);
    let client = match shared.connection_policy {
        HttpConnectionPolicy::Pooled => shared.pooled_client.clone(),
        HttpConnectionPolicy::FreshPerRequest => eggfetch_core::Client::new(),
    };
    let timeout = shared.deadline.saturating_duration_since(Instant::now());
    if timeout.is_zero() {
        return failed(CATEGORY_TIMEOUT);
    }
    let mut builder = match client.request(method, &url) {
        Ok(builder) => builder.timeout(eggfetch_core::Timeout {
            pool: Some(timeout),
            connect: Some(timeout),
            write: Some(timeout),
            read: Some(timeout),
            total: Some(timeout),
        }),
        Err(_) => return failed(CATEGORY_TRANSPORT),
    }
    .max_decoded_body_size(MAX_RESPONSE_BODY_BYTES);
    for (name, value) in shared.default_headers.iter() {
        if !case
            .request
            .headers
            .iter()
            .any(|(case_name, _)| case_name.eq_ignore_ascii_case(name))
        {
            builder = builder.header(name, value);
        }
    }
    for (name, value) in &case.request.headers {
        builder = builder.header(name, value);
    }
    let body = shared.bodies[case_index].clone();
    if let Some(body) = body {
        builder = builder.bytes(body);
    }
    let started = Instant::now();
    let response = tokio::select! {
        () = shared.cancel.cancelled() => return failed(CATEGORY_CANCELLED),
        response = builder.send_detailed() => response,
    };
    let mut response = match response {
        Ok(response) => response,
        Err(failure) => {
            return failed(if is_timeout_failure(&failure) {
                CATEGORY_TIMEOUT
            } else {
                CATEGORY_TRANSPORT
            });
        }
    };
    let status = response.status().as_u16();
    let bytes = tokio::select! {
        () = shared.cancel.cancelled() => return HttpCorpusOutcome { case_id: case.id.clone(), status: Some(status), latency: None, bytes: 0, transport_error: Some(CATEGORY_CANCELLED), expected_match: None },
        body = response.bytes() => match body {
            Ok(body) => body.len() as u64,
            Err(error) => return HttpCorpusOutcome { case_id: case.id.clone(), status: Some(status), latency: None, bytes: 0, transport_error: Some(if is_timeout_error(&error) { CATEGORY_TIMEOUT } else { CATEGORY_BODY_READ }), expected_match: None },
        },
    };
    HttpCorpusOutcome {
        case_id: case.id.clone(),
        status: Some(status),
        latency: Some(started.elapsed()),
        bytes,
        transport_error: None,
        expected_match: Some(case.expectation.matches(status)),
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)] // Builds the bounded, payload-free method and metric evidence together.
fn build_http_corpus_output(
    cases: &[HttpSecurityCaseV1],
    default_headers: &[(String, String)],
    outcomes: &[HttpCorpusOutcome],
    planned_count: u64,
    elapsed: Duration,
    max_in_flight: usize,
    connection_policy: HttpConnectionPolicy,
    seed: u64,
    corpus_sha256: &str,
    planned_schedule_sha256: &str,
    realized_schedule_sha256: &str,
) -> WorkloadOutput {
    use sha2::Digest as _;
    let mut histogram = hdrhistogram::Histogram::<u64>::new_with_bounds(
        HISTOGRAM_LOW_US,
        HISTOGRAM_HIGH_US,
        HISTOGRAM_SIGFIG,
    )
    .expect("histogram bounds valid");
    let mut statuses = BTreeMap::<u16, u64>::new();
    let mut dispatches = BTreeMap::<String, u64>::new();
    let mut errors = BTreeMap::<&'static str, u64>::new();
    let mut completed = 0_u64;
    let mut mismatches = 0_u64;
    let mut bytes = 0_u64;
    for outcome in outcomes {
        *dispatches.entry(outcome.case_id.clone()).or_default() += 1;
        if let Some(status) = outcome.status {
            *statuses.entry(status).or_default() += 1;
        }
        if let Some(error) = outcome.transport_error {
            *errors.entry(error).or_default() += 1;
        }
        if outcome.expected_match == Some(false) {
            mismatches += 1;
        }
        if let Some(latency) = outcome.latency {
            completed += 1;
            bytes += outcome.bytes;
            let us = u64::try_from(latency.as_micros())
                .unwrap_or(u64::MAX)
                .clamp(HISTOGRAM_LOW_US, HISTOGRAM_HIGH_US);
            let _ = histogram.record(us);
        }
    }
    let attempted = outcomes.len() as u64;
    let denominator = count_f64(planned_count.max(1));
    let mut metrics = vec![
        raw(
            "throughput",
            "rps",
            count_f64(completed) / elapsed.as_secs_f64().max(f64::MIN_POSITIVE),
            Aggregation::Rate,
            "eggfetch.responses_received",
            &[],
        ),
        raw(
            "transport_error_rate",
            "ratio",
            count_f64(errors.values().sum()) / denominator,
            Aggregation::Ratio,
            "eggfetch.transport_errors",
            &[],
        ),
        raw(
            "expected_outcome_mismatch_rate",
            "ratio",
            count_f64(mismatches) / denominator,
            Aggregation::Ratio,
            "eggfetch.expected_outcome_mismatches",
            &[],
        ),
        raw(
            "expected_outcome_mismatches",
            "count",
            count_f64(mismatches),
            Aggregation::Sum,
            "eggfetch.expected_outcome_mismatches",
            &[],
        ),
        raw(
            "bytes_received",
            "bytes",
            count_f64(bytes),
            Aggregation::Sum,
            "eggfetch.bytes_received",
            &[],
        ),
    ];
    if !histogram.is_empty() {
        push_latency_metrics(&mut metrics, &histogram);
    }
    let method = serde_json::json!({
        "driver": EGGFETCH_HTTP_DRIVER_NAME,
        "eggfetch_core_version": EGGFETCH_CORE_VERSION,
        "method_evidence_schema": 2,
        "workload": "http_corpus",
        "corpus_sha256": corpus_sha256,
        "planned_schedule_sha256": planned_schedule_sha256,
        "realized_schedule_sha256": realized_schedule_sha256,
        "seed": seed,
        "connection_policy": connection_policy,
        "physical_connection_contract": match connection_policy { HttpConnectionPolicy::Pooled => "one client pool per invocation", HttpConnectionPolicy::FreshPerRequest => "new client per request; each request uses a fresh physical connection" },
        "default_headers": default_headers,
        "planned_requests": planned_count,
        "attempted_requests": attempted,
        "completed_responses": completed,
        "transport_errors": errors,
        "expected_outcome_mismatches": mismatches,
        "case_dispatch_counts": dispatches,
        "status_counts": statuses,
        "maximum_observed_in_flight": max_in_flight,
        "elapsed_ms": elapsed.as_millis(),
        "latency_sample_policy": "dispatch to full response-body consumption",
        "request_payloads_retained": false,
    });
    let artifact = serde_json::to_vec_pretty(&method).expect("bounded method evidence serializes");
    let mut artifacts = vec![WorkloadArtifact {
        name: "eggfetch-method.json".to_owned(),
        media_type: "application/json".to_owned(),
        bytes: artifact,
    }];
    if !histogram.is_empty() {
        artifacts.push(WorkloadArtifact {
            name: LATENCY_HISTOGRAM_ARTIFACT.to_owned(),
            media_type: "application/x-hdrhistogram-v2".to_owned(),
            bytes: serialize_histogram(&histogram),
        });
    }
    let _case_ids_digest = format!(
        "{:x}",
        sha2::Sha256::digest(
            cases
                .iter()
                .map(|case| case.id.as_str())
                .collect::<Vec<_>>()
                .join("\0")
        )
    );
    WorkloadOutput {
        artifacts,
        metrics,
        histograms: if histogram.is_empty() {
            Vec::new()
        } else {
            vec![RawHistogramInput {
                metric: HISTOGRAM_METRIC.to_owned(),
                artifact_name: LATENCY_HISTOGRAM_ARTIFACT.to_owned(),
                format: HISTOGRAM_FORMAT.to_owned(),
                unit: HISTOGRAM_UNIT.to_owned(),
                method: Some("HTTP corpus dispatch-to-full-body microseconds".to_owned()),
            }]
        },
        error_counts: errors
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
        measurement_elapsed: Some(elapsed),
    }
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
        if matches!(context.workload, Workload::HttpCorpus { .. }) {
            return self.execute_http_corpus(context).await;
        }
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

    async fn execute_http_corpus(
        &mut self,
        context: InvocationContext,
    ) -> Result<WorkloadOutput, FailureCategory> {
        let Workload::HttpCorpus {
            target,
            corpus_ref,
            corpus_sha256,
            schedule,
            concurrency,
            connection_policy,
            default_headers,
        } = &context.workload
        else {
            return Err(FailureCategory::WorkloadFailed);
        };
        let binding = binding_url(&context, target.as_str())?;
        let _confined = crate::external::confine_target_url(&binding)
            .map_err(|_| FailureCategory::WorkloadFailed)?;
        let origin = http_origin(&binding)?;
        let workspace = std::env::current_dir().map_err(|_| FailureCategory::WorkloadFailed)?;
        let (corpus, body_root) = load_http_security_corpus(&workspace, corpus_ref, corpus_sha256)
            .map_err(|_| FailureCategory::WorkloadFailed)?;
        let bodies = corpus
            .cases
            .iter()
            .map(|case| match &case.request.body {
                HttpCaseBodyV1::None => Ok(None),
                HttpCaseBodyV1::InlineUtf8(value) => Ok(Some(value.as_bytes().to_vec())),
                HttpCaseBodyV1::File(path) => std::fs::read(body_root.join(path))
                    .map(Some)
                    .map_err(|_| FailureCategory::WorkloadFailed),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut selected = Vec::new();
        for entry in schedule {
            let Some((index, case)) = corpus
                .cases
                .iter()
                .enumerate()
                .find(|(_, case)| case.id == entry.case_id)
            else {
                return Err(FailureCategory::WorkloadFailed);
            };
            if default_headers.iter().any(|(name, value)| {
                case.request.headers.iter().any(|(case_name, case_value)| {
                    case_name.eq_ignore_ascii_case(name) && case_value != value
                })
            }) {
                return Err(FailureCategory::WorkloadFailed);
            }
            let count =
                usize::try_from(entry.count.get()).map_err(|_| FailureCategory::WorkloadFailed)?;
            selected
                .try_reserve(count)
                .map_err(|_| FailureCategory::WorkloadFailed)?;
            selected.extend(std::iter::repeat_n(index, count));
        }
        let seed = context
            .schedule_seed
            .ok_or(FailureCategory::WorkloadFailed)?;
        let planned_count = selected.len() as u64;
        let planned_schedule_sha256 = digest_json(schedule)?;
        let mut schedule_rng = seed ^ 0x9e37_79b9_7f4a_7c15;
        deterministic_shuffle(&mut selected, &mut schedule_rng);
        let mut order_hasher = sha2::Sha256::new();
        for &index in &selected {
            order_hasher.update(corpus.cases[index].id.as_bytes());
            order_hasher.update([0]);
        }
        let realized_schedule_sha256 = format!("{:x}", order_hasher.finalize());
        let started = Instant::now();
        let (outcomes, max_in_flight) = run_http_corpus_schedule(
            &self.client,
            &origin,
            &corpus.cases,
            default_headers,
            bodies,
            selected,
            concurrency.get() as usize,
            *connection_policy,
            &context.cancellation,
            context.timeout,
        )
        .await;
        let elapsed = started.elapsed();
        context.measurement.finish(elapsed);
        Ok(build_http_corpus_output(
            &corpus.cases,
            default_headers,
            &outcomes,
            planned_count,
            elapsed,
            max_in_flight,
            *connection_policy,
            seed,
            corpus_sha256,
            &planned_schedule_sha256,
            &realized_schedule_sha256,
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
        Workload::OpenLoop { .. } | Workload::SemanticReplay { .. } => {
            Err(FailureCategory::WorkloadFailed)
        }
        Workload::HttpCorpus { .. } => Err(FailureCategory::WorkloadFailed),
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
        // exhausted, so exactly the planned count is issued. Rust 1.99 renamed
        // it `try_update`, but that spelling is stable only from 1.95 and this
        // workspace's MSRV is 1.89, so the deprecated spelling is kept until
        // the MSRV moves.
        #[allow(deprecated)]
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

#[cfg(test)]
mod http_corpus_load_tests {
    use super::*;
    use eggbench_core::{HttpCaseRequestV1, HttpObservableExpectationV1};
    use std::sync::atomic::AtomicUsize;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn local_origin(
        status: u16,
    ) -> (
        String,
        Arc<AtomicUsize>,
        Arc<tokio::sync::Mutex<Vec<(String, Vec<u8>)>>>,
        CancellationToken,
    ) {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind test origin");
        let address = listener.local_addr().expect("origin address");
        let connections = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let cancel = CancellationToken::new();
        let server_cancel = cancel.clone();
        let server_connections = Arc::clone(&connections);
        let server_requests = Arc::clone(&requests);
        tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    () = server_cancel.cancelled() => break,
                    accepted = listener.accept() => accepted,
                };
                let Ok((mut stream, _)) = accepted else { break };
                server_connections.fetch_add(1, Ordering::SeqCst);
                let records = Arc::clone(&server_requests);
                tokio::spawn(async move {
                    loop {
                        let mut request = Vec::new();
                        let mut byte = [0_u8; 1];
                        while !request.ends_with(b"\r\n\r\n") {
                            if stream.read_exact(&mut byte).await.is_err() {
                                return;
                            }
                            request.push(byte[0]);
                            if request.len() > 32 * 1024 {
                                return;
                            }
                        }
                        let head = String::from_utf8_lossy(&request);
                        let first = head.lines().next().unwrap_or_default().to_owned();
                        let content_length = head
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().ok())
                                    .flatten()
                            })
                            .unwrap_or(0);
                        let mut body = vec![0_u8; content_length];
                        if stream.read_exact(&mut body).await.is_err() {
                            return;
                        }
                        records.lock().await.push((first, body));
                        let response = format!(
                            "HTTP/1.1 {status} Test\r\nContent-Length: 2\r\nConnection: keep-alive\r\n\r\nok"
                        );
                        if stream.write_all(response.as_bytes()).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        (format!("http://{address}"), connections, requests, cancel)
    }

    fn post_case(expected: u16) -> HttpSecurityCaseV1 {
        HttpSecurityCaseV1 {
            id: "post_json".to_owned(),
            category: None,
            request: HttpCaseRequestV1 {
                method: "POST".to_owned(),
                path_and_query: "/submit?q=1".to_owned(),
                headers: vec![("content-type".to_owned(), "application/json".to_owned())],
                body: HttpCaseBodyV1::InlineUtf8("{\"ok\":true}".to_owned()),
            },
            expectation: HttpObservableExpectationV1::Exact {
                status_exact: expected,
            },
        }
    }

    async fn run_case(
        policy: HttpConnectionPolicy,
        response_status: u16,
        expected: u16,
    ) -> (
        Vec<HttpCorpusOutcome>,
        usize,
        Arc<AtomicUsize>,
        Arc<tokio::sync::Mutex<Vec<(String, Vec<u8>)>>>,
        CancellationToken,
    ) {
        let (origin, connections, requests, cancel) = local_origin(response_status).await;
        let case = post_case(expected);
        let outcomes = run_http_corpus_schedule(
            &eggfetch_core::Client::new(),
            &origin,
            std::slice::from_ref(&case),
            &[],
            vec![Some(b"{\"ok\":true}".to_vec())],
            vec![0, 0],
            1,
            policy,
            &CancellationToken::new(),
            Duration::from_secs(3),
        )
        .await;
        (outcomes.0, outcomes.1, connections, requests, cancel)
    }

    #[tokio::test]
    async fn pooled_post_preserves_body_and_expected_403_is_not_transport_error() {
        let (outcomes, max_in_flight, connections, requests, cancel) =
            run_case(HttpConnectionPolicy::Pooled, 403, 403).await;
        assert_eq!(outcomes.len(), 2);
        assert!(
            outcomes
                .iter()
                .all(|outcome| outcome.transport_error.is_none())
        );
        assert!(
            outcomes
                .iter()
                .all(|outcome| outcome.expected_match == Some(true))
        );
        assert_eq!(max_in_flight, 1);
        assert_eq!(connections.load(Ordering::SeqCst), 1);
        let records = requests.lock().await;
        assert_eq!(records.len(), 2);
        assert!(
            records
                .iter()
                .all(|(line, _)| line == "POST /submit?q=1 HTTP/1.1")
        );
        assert!(records.iter().all(|(_, body)| body == b"{\"ok\":true}"));
        cancel.cancel();
    }

    #[tokio::test]
    #[allow(clippy::float_cmp)] // Rates are exact integer ratios in this fixture.
    async fn fresh_per_request_uses_distinct_connections_and_mismatch_is_separate() {
        let (outcomes, _, connections, requests, cancel) =
            run_case(HttpConnectionPolicy::FreshPerRequest, 200, 403).await;
        assert_eq!(outcomes.len(), 2);
        assert!(
            outcomes
                .iter()
                .all(|outcome| outcome.transport_error.is_none())
        );
        assert!(
            outcomes
                .iter()
                .all(|outcome| outcome.expected_match == Some(false))
        );
        assert_eq!(connections.load(Ordering::SeqCst), 2);
        assert_eq!(requests.lock().await.len(), 2);
        let output = build_http_corpus_output(
            &[post_case(403)],
            &[],
            &outcomes,
            2,
            Duration::from_millis(2),
            1,
            HttpConnectionPolicy::FreshPerRequest,
            7,
            &"ab".repeat(32),
            &"cd".repeat(32),
            &"ef".repeat(32),
        );
        let transport = output
            .metrics
            .iter()
            .find(|metric| metric.name == "transport_error_rate")
            .unwrap();
        let mismatch = output
            .metrics
            .iter()
            .find(|metric| metric.name == "expected_outcome_mismatch_rate")
            .unwrap();
        assert_eq!(transport.value, 0.0);
        assert_eq!(mismatch.value, 1.0);
        let evidence = output
            .artifacts
            .iter()
            .find(|artifact| artifact.name == METHOD_ARTIFACT)
            .unwrap();
        assert!(!String::from_utf8_lossy(&evidence.bytes).contains("{\\\"ok\\\":true}"));
        cancel.cancel();
    }

    #[test]
    fn deterministic_shuffle_is_stable_and_trial_seed_sensitive() {
        let original = vec![0, 1, 2, 3, 4, 5, 6, 7];
        let mut same_a = original.clone();
        let mut same_b = original.clone();
        let mut seed_a = 41;
        let mut seed_b = 41;
        deterministic_shuffle(&mut same_a, &mut seed_a);
        deterministic_shuffle(&mut same_b, &mut seed_b);
        assert_eq!(same_a, same_b);
        let mut other_trial = original;
        let mut other_seed = 42;
        deterministic_shuffle(&mut other_trial, &mut other_seed);
        assert_ne!(same_a, other_trial);
    }
}
