//! Security Qualification M003 routine tests (synthetic scope).
//!
//! Covers the `M003b` load shapes, the `M003c` subject-telemetry collector, and
//! the `M003d` negative demonstrations with the routine-test-only subject
//! stand-in in `qualification/synvoid/v2`. The subject publishes the
//! owner-authored `synvoid_subject_*` sample names of the closed `SynVoid`
//! telemetry contract `synvoid.eggbench-telemetry.v2`; the mapping consumed
//! here is the owner's own `telemetry-mapping.json`, digest-pinned in every
//! plan. No real `SynVoid` checkout is required: live qualification runs the
//! real minimal binary in `scripts/qualification/synvoid-m003/`.
//!
//! Every assertion below is about Eggbench-owned behavior. The stand-in's
//! verdicts are synthetic and are never presented as `SynVoid` semantics.

#![cfg(all(feature = "eggstack-http", feature = "prometheus-http"))]

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

/// Owner-published mapping file digest from the closed `SynVoid` v2 contract
/// (`telemetry-contract.json` `mapping_sha256` in the owner export).
const OWNER_MAPPING_SHA256: &str =
    "622f6a13c4353cc7465cce39a57ed86fa0db2fe4114258e6f06226c1748d2d99";
/// Eggbench workspace content identity of the same mapping file.
const MAPPING_CONTENT_IDENTITY: &str =
    "dd7d58dd204a691ab0b83b82a83d49f3438b67e3ea6d0e14c7c1fba3832584b0";
/// Owner contract identifier that the checked-in fixture must carry.
const OWNER_CONTRACT_ID: &str = "synvoid.eggbench-telemetry.v2";

const PERF_SCENARIOS: [&str; 8] = [
    "body-gated-c8",
    "body-pooled-c8",
    "body-pooled-c32",
    "mixed-80-20-pooled-c8",
    "mixed-80-20-fresh-c8",
    "control-origin-body-c8",
    "telemetry-pressure-c32",
    "waf-correctness",
];

// The M003 profiles start several concurrent local processes and run
// hundreds of requests each. Executing them from one integration-test binary
// in parallel makes each test perturb the others' same-source measurements.
static QUALIFICATION_TEST_LOCK: Mutex<()> = Mutex::new(());

fn qualification_test_lock() -> MutexGuard<'static, ()> {
    QUALIFICATION_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_eggbench"))
}

fn profile_source() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../qualification/synvoid/v2")
        .canonicalize()
        .expect("checked-in synvoid v2 workspace exists")
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).expect("read json")).expect("parse json")
}

fn write_json(path: &Path, value: &Value) {
    std::fs::write(
        path,
        serde_json::to_vec_pretty(value).expect("serialize json"),
    )
    .expect("write json");
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("allocate free loopback port")
        .local_addr()
        .expect("listener address")
        .port()
}

fn eggbench(workspace: &Path, args: &[&str]) -> std::process::Output {
    Command::new(binary())
        .args(args)
        .current_dir(workspace)
        .output()
        .expect("spawn eggbench binary")
}

fn copy_dir(source: &Path, target: &Path) {
    for entry in std::fs::read_dir(source).expect("read workspace dir") {
        let entry = entry.expect("workspace dir entry");
        let to = target.join(entry.file_name());
        if entry.file_type().expect("entry type").is_dir() {
            std::fs::create_dir(&to).expect("create workspace subdir");
            copy_dir(&entry.path(), &to);
        } else {
            std::fs::copy(entry.path(), &to).expect("copy workspace file");
        }
    }
}

/// A copy of the checked-in workspace with the subject, its telemetry
/// listener, and the controlled origin bound to free loopback ports.
struct Workspace {
    temp: tempfile::TempDir,
    subject_port: u16,
    metrics_port: u16,
    origin_port: u16,
}

impl Workspace {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("temporary workspace");
        copy_dir(&profile_source(), temp.path());
        let workspace = Self {
            subject_port: free_port(),
            metrics_port: free_port(),
            origin_port: free_port(),
            temp,
        };
        workspace.patch_ports();
        workspace
    }

    fn path(&self) -> &Path {
        self.temp.path()
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        eggbench(self.path(), args)
    }

    fn patch_ports(&self) {
        let scenarios = self.path().join("scenarios");
        let mut entries: Vec<_> = std::fs::read_dir(&scenarios)
            .expect("read scenarios")
            .map(|entry| entry.expect("scenario entry").path())
            .collect();
        entries.sort();
        for path in entries {
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            let mut plan = read_json(&path);
            for service in plan
                .get_mut("services")
                .and_then(Value::as_array_mut)
                .expect("services")
            {
                match service["name"].as_str() {
                    Some("synvoid") => {
                        service["kind"]["argv"] = serde_json::json!([
                            "stubs/fake_synvoid_m003.py",
                            "--port",
                            self.subject_port.to_string(),
                            "--metrics-port",
                            self.metrics_port.to_string(),
                            "--metrics-log",
                            "metrics-scrapes.log",
                            "--fault-file",
                            "faults.json",
                        ]);
                        service["http_url"] =
                            Value::String(format!("http://127.0.0.1:{}/", self.subject_port));
                    }
                    Some("subject-metrics") => {
                        service["http_url"] = Value::String(format!(
                            "http://127.0.0.1:{}/metrics",
                            self.metrics_port
                        ));
                    }
                    Some("origin") if service["kind"]["kind"] == "command" => {
                        service["kind"]["argv"] = serde_json::json!([
                            "stubs/controlled_origin_m003.py",
                            "--port",
                            self.origin_port.to_string(),
                            "--routes-json",
                            "routes.json",
                            "--log-file",
                            "origin-requests.log",
                        ]);
                        service["http_url"] = Value::String(format!(
                            "http://127.0.0.1:{}/api/users",
                            self.origin_port
                        ));
                    }
                    _ => {}
                }
            }
            write_json(&path, &plan);
        }
        let target_config = self.path().join("target-config.json");
        let mut document = read_json(&target_config);
        document["listen"]["port"] = Value::from(self.subject_port);
        document["metrics"]["port"] = Value::from(self.metrics_port);
        document["origin"]["url"] = Value::String(format!("http://127.0.0.1:{}", self.origin_port));
        write_json(&target_config, &document);
    }

    /// Install a qualification-only telemetry fault for the subject.
    fn set_faults(&self, faults: &Value) {
        write_json(&self.path().join("faults.json"), faults);
    }

    /// Plan-invisible latency injected by the subject stand-in, used for the
    /// performance-only regression proof. Keyed by listen port exactly like
    /// the M002b convention so the plan identity never changes.
    fn set_delay_ms(&self, delay_ms: u32) {
        std::fs::write(
            format!("/tmp/fake-synvoid-{}.delay_ms", self.subject_port),
            delay_ms.to_string(),
        )
        .expect("write delay sidecar");
    }

    fn clear_delay_ms(&self) {
        let _ = std::fs::remove_file(format!("/tmp/fake-synvoid-{}.delay_ms", self.subject_port));
    }

    /// Per-scrape accounting recorded by the subject stand-in.
    fn scrape_log(&self) -> Vec<(u64, f64)> {
        let path = self.path().join("metrics-scrapes.log");
        let Ok(raw) = std::fs::read_to_string(path) else {
            return Vec::new();
        };
        raw.lines()
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                let count = parts.next()?.parse().ok()?;
                let served_ms = parts.next()?.parse().ok()?;
                Some((count, served_ms))
            })
            .collect()
    }

    fn bundle(&self, suite: &Path, scenario: &str) -> PathBuf {
        let _ = self;
        suite.join("scenarios").join(format!("{scenario}.eggb"))
    }
}

fn receipt(suite: &Path) -> Value {
    read_json(&suite.join("qualification-receipt.json"))
}

fn find_record(suite: &Path, scenario: &str) -> Option<Value> {
    let document = receipt(suite);
    document["scenarios"]
        .as_array()
        .expect("scenario records")
        .iter()
        .find(|record| record["id"] == Value::String(scenario.to_owned()))
        .cloned()
}

fn scenario_record(suite: &Path, scenario: &str) -> Value {
    find_record(suite, scenario).unwrap_or_else(|| panic!("receipt has scenario {scenario}"))
}

fn observations(bundle: &Path, trial: &str) -> Value {
    read_json(&bundle.join("trials").join(trial).join("metrics.json"))
}

fn observation(document: &Value, name: &str) -> Value {
    document["observations"]
        .as_array()
        .expect("observations")
        .iter()
        .find(|observation| observation["name"] == Value::String(name.to_owned()))
        .cloned()
        .unwrap_or_else(|| panic!("trial declares metric {name}"))
}

fn observed_value(document: &Value, name: &str) -> f64 {
    let entry = observation(document, name);
    assert_eq!(
        entry["state"]["state"], "observed",
        "{name} must be observed: {entry}"
    );
    entry["state"]["value"]
        .as_f64()
        .unwrap_or_else(|| panic!("{name} has a numeric observation"))
}

fn warning_categories(document: &Value) -> Vec<String> {
    document["warnings"]
        .as_array()
        .map(|warnings| {
            warnings
                .iter()
                .filter_map(|warning| warning["category"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn has_tool(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Stage A of the baseline workflow: materialize every accepted-revision
/// baseline bundle explicitly. There is no automatic baseline discovery.
fn materialize_baselines(workspace: &Workspace) {
    for scenario in PERF_SCENARIOS {
        let output = workspace.run(&[
            "run",
            &format!("scenarios/{scenario}.json"),
            &format!("baselines/{scenario}.eggb"),
            "--json",
        ]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{scenario} baseline failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn checked_in_mapping_fixture_is_the_closed_owner_contract() {
    let workspace = profile_source();
    let mapping_bytes =
        std::fs::read(workspace.join("telemetry/telemetry-mapping.json")).expect("mapping fixture");
    // The fixture is the owner's own artifact, byte-for-byte: no Eggbench
    // translation, no owner metric names hardcoded in production code.
    assert_eq!(
        sha256_hex(&mapping_bytes),
        OWNER_MAPPING_SHA256,
        "checked-in mapping must be the closed owner mapping"
    );
    let mapping: Value = serde_json::from_slice(&mapping_bytes).expect("mapping json");
    assert_eq!(mapping["schema_version"], 1);
    assert_eq!(mapping["source"], "prometheus");
    assert!(mapping.get("contract_id").is_none());
    let fields = mapping["fields"].as_array().expect("mapping fields");
    assert_eq!(fields.len(), 12, "owner inventory is twelve samples");
    let required = fields
        .iter()
        .filter(|field| field["required"] == Value::Bool(true))
        .count();
    assert_eq!(required, 10, "ten required, two optional owner samples");
    for field in fields {
        assert!(
            field["output_name"]
                .as_str()
                .is_some_and(|name| name.starts_with("subject_")),
            "owner mapping output must stay subject-scoped"
        );
        let kind = field["kind"].as_str().expect("kind");
        if kind == "gauge" {
            assert!(field.get("aggregation").is_some());
        } else {
            assert!(field.get("aggregation").is_none());
        }
    }
    let contract = read_json(&workspace.join("telemetry/telemetry-contract.json"));
    assert_eq!(contract["contract_id"], OWNER_CONTRACT_ID);
    assert_eq!(
        contract["schema_version"],
        "synvoid.eggbench-telemetry.contract.v2"
    );
    assert_eq!(contract["mapping_sha256"], OWNER_MAPPING_SHA256);
    assert_eq!(contract["scrape_path"], "/metrics");
    assert!(
        contract["scrape_url"]
            .as_str()
            .is_some_and(|url| url.starts_with("http://127.0.0.1:")),
        "the owner scrape endpoint is loopback only"
    );
    for metric in contract["metrics"].as_array().expect("owner metrics") {
        assert!(metric.get("source_aggregation").is_some());
        assert!(
            metric.get("aggregation").is_none(),
            "owner source aggregation must never masquerade as trial aggregation"
        );
    }
    // Every plan that consumes the mapping pins the resolved content identity
    // of the fixture, not a loose digest.
    for entry in std::fs::read_dir(workspace.join("scenarios")).expect("scenarios") {
        let plan = read_json(&entry.expect("scenario").path());
        for service in plan
            .get("services")
            .and_then(Value::as_array)
            .expect("services")
        {
            if service["kind"]["service_type"] == "prometheus-http" {
                assert_eq!(
                    service["config"]["mapping_ref"],
                    "telemetry/telemetry-mapping.json"
                );
                assert_eq!(
                    service["config"]["mapping_sha256"], MAPPING_CONTENT_IDENTITY,
                    "every telemetry plan pins the mapping content identity"
                );
            }
        }
    }
}

#[test]
#[allow(clippy::too_many_lines)] // One routine smoke profile end to end, asserted in place.
fn smoke_profile_qualifies_with_subject_telemetry_evidence() {
    let _guard = qualification_test_lock();
    let workspace = Workspace::new();
    let suite =
        std::env::temp_dir().join(format!("eggbench-m003-smoke-{}", workspace.subject_port));
    let _ = std::fs::remove_dir_all(&suite);
    let output = workspace.run(&[
        "qualify",
        "run",
        "smoke.profile.json",
        "--output",
        suite.to_str().expect("utf-8 suite path"),
        "--json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "smoke profile failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document = receipt(&suite);
    assert_eq!(document["aggregate_verdict"], "pass");
    assert_eq!(document["execution_complete"], true);
    assert!(
        document["scenarios"]
            .as_array()
            .expect("scenarios")
            .iter()
            .all(|record| record["status"] == "completed"),
        "every M003 scenario must complete: {document}"
    );
    let inspect = workspace.run(&[
        "qualify",
        "inspect",
        suite
            .join("qualification-receipt.json")
            .to_str()
            .expect("utf-8 receipt path"),
        "--json",
    ]);
    assert_eq!(
        inspect.status.code(),
        Some(0),
        "receipt inspection failed: {}",
        String::from_utf8_lossy(&inspect.stderr)
    );

    // Target telemetry evidence for the pressure scenario.
    let bundle = workspace.bundle(&suite, "synvoid-m003-target-telemetry-pressure-c32");
    let provenance: Value =
        read_json(&bundle.join("trials/001/telemetry/00-00-prometheus-provenance.json"));
    assert_eq!(provenance["source"], "prometheus-http");
    assert_eq!(provenance["mapping_sha256"], MAPPING_CONTENT_IDENTITY);
    assert_eq!(
        provenance["mapping_ref"],
        "telemetry/telemetry-mapping.json"
    );
    assert_eq!(provenance["binding_service"], "subject-metrics");
    assert_eq!(provenance["poll_error_count"], 0);
    assert_eq!(provenance["dropped_sample_count"], 0);
    assert_eq!(provenance["missing_field_observation_count"], 0);
    assert!(
        provenance["sample_count"]
            .as_u64()
            .is_some_and(|count| count >= 3),
        "a measured window must contain several bounded polls: {provenance}"
    );
    let trial = observations(&bundle, "001");
    // A required gauge moves plausibly under load, and the mapped owner
    // counter resolves as a non-negative trial delta.
    assert!(observed_value(&trial, "subject_event_loop_lag_ms") > 0.0);
    assert!(observed_value(&trial, "subject_body_buffering_bytes_total") > 0.0);
    assert!(observed_value(&trial, "subject_offload_rejections_total") > 0.0);
    let gauge = observation(&trial, "subject_event_loop_lag_ms");
    assert_eq!(gauge["aggregation"]["kind"], "maximum");
    assert_eq!(gauge["intent"], "diagnostic");
    assert_eq!(
        gauge["provenance"]["source_field"], "synvoid_subject_event_loop_lag_ms",
        "normalized observations must name the owner sample"
    );
    let counter = observation(&trial, "subject_body_buffering_bytes_total");
    assert_eq!(counter["aggregation"]["kind"], "direct");

    // Optional owner samples that the subject does not export stay absent and
    // warned; they are never fabricated as zero.
    let optional_bundle = workspace.bundle(&suite, "synvoid-m003-target-telemetry-optional-c32");
    let optional = observations(&optional_bundle, "001");
    let rss = observation(&optional, "subject_cpu_worker_rss_bytes");
    assert_eq!(rss["state"]["state"], "missing");
    assert_eq!(rss["state"]["reason"], "source_not_provided");
    assert!(rss.get("value").is_none() || rss["value"].is_null());
    assert!(
        warning_categories(&optional).contains(&"prometheus_missing_samples".to_owned()),
        "optional absence must be warned: {optional}"
    );
    let optional_provenance: Value =
        read_json(&optional_bundle.join("trials/001/telemetry/00-00-prometheus-provenance.json"));
    assert!(
        optional_provenance["missing_field_observation_count"]
            .as_u64()
            .is_some_and(|count| count > 0)
    );

    // Correctness evidence for the whole synthetic corpus, including the
    // body-bearing and body-detect cases.
    let correctness = workspace.bundle(&suite, "synvoid-m003-correctness");
    let check = read_json(&correctness.join("security/synvoid-m003-correctness.json"));
    assert_eq!(check["family"], "http_observable");
    assert_eq!(check["source"], "eggbench-http-corpus");
    let cases = check["cases"].as_array().expect("cases");
    assert!(
        cases.iter().all(|case| case["disposition"] == "pass"),
        "every synthetic M003 case must hold its owner-declared expectation: {cases:?}"
    );
    assert!(cases.len() >= 8, "M003 corpus covers body and GET shapes");
    assert!(
        cases
            .iter()
            .any(|case| case["id"] == "benign-json-body" && case["observed_status"] == 200)
    );
    assert!(
        cases
            .iter()
            .any(|case| case["id"] == "detect-ssrf-body" && case["observed_status"] == 403)
    );
}

#[test]
fn perf_profile_baselines_are_explicit_and_repeatable() {
    let _guard = qualification_test_lock();
    let workspace = Workspace::new();
    materialize_baselines(&workspace);
    let suite = std::env::temp_dir().join(format!("eggbench-m003-perf-{}", workspace.subject_port));
    let _ = std::fs::remove_dir_all(&suite);
    let output = workspace.run(&[
        "qualify",
        "run",
        "perf.profile.json",
        "--output",
        suite.to_str().expect("utf-8 suite path"),
        "--json",
    ]);
    assert!(
        matches!(output.status.code(), Some(0 | 7)),
        "perf profile must pass or be inconclusive: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document = receipt(&suite);
    assert!(document["aggregate_verdict"] != "fail", "{document}");
    // Every performance scenario compared against an explicit accepted
    // baseline identity, and every metric stayed honest about its gate.
    for scenario in PERF_SCENARIOS {
        if scenario == "waf-correctness" {
            let record = scenario_record(&suite, "synvoid-m003-correctness");
            assert_eq!(record["correctness_verdict"], "pass");
            continue;
        }
        let record =
            find_record(&suite, &format!("synvoid-m003-{scenario}")).unwrap_or(Value::Null);
        let record = if record.is_null() {
            // The direct-origin control keeps its own scenario id.
            scenario_record(&suite, "origin-m003-body-control-c8")
        } else {
            record
        };
        assert_eq!(record["status"], "completed", "{record}");
        assert!(
            record["baseline_bundle_identity"]["manifest_sha256"].is_string(),
            "{scenario} must compare against a materialized baseline: {record}"
        );
        assert_ne!(record["performance_verdict"], "fail", "{record}");
    }
    // Mixed/body performance metrics remain diagnostic until repeatability
    // justifies a reviewed gate.
    let comparison =
        read_json(&suite.join("scenarios/synvoid-m003-mixed-80-20-pooled-c8.comparison.json"));
    let throughput = comparison["metrics"]
        .as_array()
        .expect("metrics")
        .iter()
        .find(|metric| metric["name"] == "throughput")
        .expect("throughput comparison")
        .clone();
    assert_eq!(throughput["intent"], "diagnostic");
    assert!(throughput.get("gate").is_none() || throughput["gate"].is_null());
    let mismatch = comparison["metrics"]
        .as_array()
        .expect("metrics")
        .iter()
        .find(|metric| metric["name"] == "expected_outcome_mismatch_rate")
        .expect("mismatch comparison")
        .clone();
    assert_eq!(mismatch["disposition"], "pass");
    assert_eq!(mismatch["candidate_estimate"], 0.0);
}

#[test]
fn correctness_only_regression_fails_despite_acceptable_performance() {
    let _guard = qualification_test_lock();
    let workspace = Workspace::new();
    materialize_baselines(&workspace);
    // Mutate only the correctness-family corpus expectation for one body
    // case. The load-shape corpus is a separate input, so the performance
    // family keeps its own honest expectations: correctness fails while
    // performance stays acceptable.
    let corpus_path = workspace.path().join("correctness-corpus.json");
    let mut corpus = read_json(&corpus_path);
    for case in corpus["cases"].as_array_mut().expect("cases") {
        if case["id"] == "benign-json-body" {
            case["expectation"] = serde_json::json!({ "status_exact": 418 });
        }
    }
    write_json(&corpus_path, &corpus);
    let digest = content_identity(workspace.path(), "correctness-corpus.json");
    let correctness_plan = workspace.path().join("scenarios/waf-correctness.json");
    let mut plan = read_json(&correctness_plan);
    for check in plan["http_corpus_checks"]
        .as_array_mut()
        .expect("correctness checks")
    {
        check["corpus_sha256"] = Value::String(digest.clone());
    }
    write_json(&correctness_plan, &plan);
    let suite = std::env::temp_dir().join(format!(
        "eggbench-m003-correctness-regression-{}",
        workspace.subject_port
    ));
    let _ = std::fs::remove_dir_all(&suite);
    let output = workspace.run(&[
        "qualify",
        "run",
        "smoke.profile.json",
        "--output",
        suite.to_str().expect("utf-8 suite path"),
        "--json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(6),
        "a security-correctness regression must fail the suite: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let document = receipt(&suite);
    assert_eq!(
        document["aggregate_verdict"], "fail",
        "a correctness regression must fail the suite: {document}"
    );
    let correctness = scenario_record(&suite, "synvoid-m003-correctness");
    assert_eq!(correctness["correctness_verdict"], "fail", "{correctness}");
    assert_eq!(correctness["combined_verdict"], "fail");
    // The load-shape scenario keeps its own honest verdicts: the suite fails
    // because security correctness failed, not because load got slower.
    let body = scenario_record(&suite, "synvoid-m003-body-pooled-c8");
    assert_eq!(body["performance_verdict"], "pass");
    assert_eq!(body["combined_verdict"], "pass");
}

#[test]
fn performance_only_regression_fails_despite_correct_security_behavior() {
    let _guard = qualification_test_lock();
    let workspace = Workspace::new();
    materialize_baselines(&workspace);
    let suite = std::env::temp_dir().join(format!(
        "eggbench-m003-performance-regression-{}",
        workspace.subject_port
    ));
    let _ = std::fs::remove_dir_all(&suite);
    let output = workspace.run(&[
        "qualify",
        "run",
        "perf.profile.json",
        "--output",
        suite.to_str().expect("utf-8 suite path"),
        "--json",
    ]);
    assert!(
        matches!(output.status.code(), Some(0 | 7)),
        "the accepted-revision perf profile must qualify before the regression: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    // The throttle lives outside the plan, so the comparison identity is
    // unchanged: a performance verdict, never incomparable drift.
    workspace.set_delay_ms(10);
    let regressed = std::env::temp_dir().join(format!(
        "eggbench-m003-performance-regression-delayed-{}",
        workspace.subject_port
    ));
    let _ = std::fs::remove_dir_all(&regressed);
    let output = workspace.run(&[
        "qualify",
        "run",
        "perf.profile.json",
        "--output",
        regressed.to_str().expect("utf-8 suite path"),
        "--json",
    ]);
    workspace.clear_delay_ms();
    assert_eq!(
        output.status.code(),
        Some(6),
        "a performance-only regression must fail the suite: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let document = receipt(&regressed);
    let correctness = scenario_record(&regressed, "synvoid-m003-correctness");
    assert_eq!(
        correctness["correctness_verdict"], "pass",
        "owner security outcomes stay correct under a throttle"
    );
    let gated = scenario_record(&regressed, "synvoid-m003-body-gated-c8");
    assert_eq!(
        gated["performance_verdict"], "fail",
        "the throttled performance gate must fail: {gated}"
    );
    assert_eq!(gated["correctness_verdict"], Value::Null);
    assert_eq!(gated["combined_verdict"], "fail");
    assert_eq!(document["aggregate_verdict"], "fail");
}

/// Assert a run that failed closed on required subject telemetry: no pass, no
/// fabricated observations, and any trial that was staged is explicitly
/// failed rather than silently successful.
fn assert_failed_closed_telemetry(envelope: &Value, bundle: &Path) {
    assert_eq!(envelope["ok"], false, "{envelope}");
    assert_eq!(envelope["result"]["execution_status"], "failed");
    assert_eq!(envelope["result"]["primary_failure"], "TelemetryFailed");
    let manifest = read_json(&bundle.join("manifest.json"));
    assert_eq!(manifest["execution_status"], "failed");
    let trials = bundle.join("trials");
    if !trials.exists() {
        // Rejected before any measured window: nothing may claim success.
        return;
    }
    let mut staged = std::fs::read_dir(&trials)
        .expect("trial directory")
        .map(|entry| entry.expect("trial entry").path())
        .collect::<Vec<_>>();
    staged.sort();
    assert!(!staged.is_empty(), "a failed run published trial evidence");
    for trial in staged {
        let result = read_json(&trial.join("result.json"));
        assert_eq!(
            result["terminal_status"], "failed",
            "every staged trial must be failed: {result}"
        );
        assert_eq!(result["failure_category"], "telemetry_failed");
        let metrics = read_json(&trial.join("metrics.json"));
        for observation in metrics["observations"].as_array().expect("observations") {
            let name = observation["name"].as_str().expect("metric name");
            if name.starts_with("subject_") {
                assert_ne!(
                    observation["state"]["state"], "observed",
                    "a failed telemetry trial must not publish a subject observation: {observation}"
                );
            }
        }
    }
}

#[test]
fn required_telemetry_mapping_drift_fails_closed_at_preflight() {
    let _guard = qualification_test_lock();
    let workspace = Workspace::new();
    // A renamed required owner sample: the pinned mapping still demands it,
    // so the subject no longer satisfies the contract.
    let mapping_path = workspace.path().join("telemetry/telemetry-mapping.json");
    let mut mapping = read_json(&mapping_path);
    for field in mapping["fields"].as_array_mut().expect("fields") {
        if field["output_name"] == "subject_event_loop_lag_ms" {
            field["prometheus_name"] =
                Value::String("synvoid_subject_event_loop_lag_renamed".to_owned());
        }
    }
    write_json(&mapping_path, &mapping);
    let identity = content_identity(workspace.path(), "telemetry/telemetry-mapping.json");
    for entry in std::fs::read_dir(workspace.path().join("scenarios")).expect("scenarios") {
        let path = entry.expect("scenario").path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let mut plan = read_json(&path);
        let mut changed = false;
        for service in plan
            .get_mut("services")
            .and_then(Value::as_array_mut)
            .expect("services")
        {
            if service["kind"]["service_type"] == "prometheus-http" {
                service["config"]["mapping_sha256"] = Value::String(identity.clone());
                changed = true;
            }
        }
        if changed {
            write_json(&path, &plan);
        }
    }
    let bundle = workspace.path().join("telemetry-mapping-drift.eggb");
    let output = workspace.run(&[
        "run",
        "scenarios/telemetry-pressure-c32.json",
        bundle.to_str().expect("utf-8 bundle path"),
        "--json",
    ]);
    assert_ne!(
        output.status.code(),
        Some(0),
        "a renamed required owner metric must fail closed"
    );
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("run envelope");
    assert_eq!(envelope["result"]["measured_trials"], 0);
    let detail = envelope["warnings"]
        .as_array()
        .expect("warnings")
        .iter()
        .find(|warning| warning["category"] == "telemetry_preflight_failed")
        .map(|warning| warning["detail"].as_str().unwrap_or_default().to_owned())
        .expect("preflight failure detail");
    assert!(
        detail.contains("required_metric_missing"),
        "the failure must name the missing required observation: {detail}"
    );
    assert_failed_closed_telemetry(&envelope, &bundle);
}

#[test]
fn required_telemetry_disappearance_fails_closed() {
    let _guard = qualification_test_lock();
    let workspace = Workspace::new();
    // Required owner samples disappear a few scrapes in, i.e. after
    // preflight and inside a measured window.
    workspace.set_faults(&serde_json::json!({ "mode": "omit_required", "after_scrapes": 4 }));
    let bundle = workspace.path().join("telemetry-disappearance.eggb");
    let output = workspace.run(&[
        "run",
        "scenarios/telemetry-pressure-c32.json",
        bundle.to_str().expect("utf-8 bundle path"),
        "--json",
    ]);
    assert_ne!(
        output.status.code(),
        Some(0),
        "a required telemetry disappearance must never pass: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("run envelope");
    assert_failed_closed_telemetry(&envelope, &bundle);
}

#[test]
fn required_telemetry_type_drift_fails_closed() {
    let _guard = qualification_test_lock();
    let workspace = Workspace::new();
    // The subject declares an owner counter as a gauge from the first scrape.
    workspace.set_faults(&serde_json::json!({ "mode": "type_drift" }));
    let bundle = workspace.path().join("telemetry-type-drift.eggb");
    let output = workspace.run(&[
        "run",
        "scenarios/telemetry-pressure-c32.json",
        bundle.to_str().expect("utf-8 bundle path"),
        "--json",
    ]);
    assert_ne!(output.status.code(), Some(0), "TYPE drift must fail closed");
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("run envelope");
    let detail = envelope["warnings"]
        .as_array()
        .expect("warnings")
        .iter()
        .filter_map(|warning| warning["detail"].as_str())
        .collect::<String>();
    assert!(
        detail.contains("sample_type_mismatch"),
        "TYPE drift must be reported as such: {detail}"
    );
    assert_failed_closed_telemetry(&envelope, &bundle);
}

#[test]
fn required_telemetry_counter_reset_fails_closed() {
    let _guard = qualification_test_lock();
    let workspace = Workspace::new();
    // A worker restart mid-trial restarts the bridged owner counter.
    workspace.set_faults(&serde_json::json!({ "mode": "counter_reset", "after_scrapes": 4 }));
    let bundle = workspace.path().join("telemetry-counter-reset.eggb");
    let output = workspace.run(&[
        "run",
        "scenarios/telemetry-pressure-c32.json",
        bundle.to_str().expect("utf-8 bundle path"),
        "--json",
    ]);
    assert_ne!(
        output.status.code(),
        Some(0),
        "a counter reset inside a trial must never yield a negative or zeroed delta"
    );
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("run envelope");
    assert_failed_closed_telemetry(&envelope, &bundle);
}

#[test]
#[allow(clippy::cast_precision_loss)] // A millisecond window sum compared against float scrape times.
fn polling_overhead_is_bounded_by_the_declared_cadence() {
    let _guard = qualification_test_lock();
    let workspace = Workspace::new();
    let bundle = workspace.path().join("telemetry-overhead.eggb");
    let output = workspace.run(&[
        "run",
        "scenarios/telemetry-pressure-c32.json",
        bundle.to_str().expect("utf-8 bundle path"),
        "--json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "overhead measurement run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let interval_ms = 200_u64;
    let mut total_served_ms = 0_u64;
    let mut total_samples = 0_u64;
    for trial in ["001", "002", "003"] {
        let provenance: Value = read_json(
            &bundle
                .join("trials")
                .join(trial)
                .join("telemetry/00-00-prometheus-provenance.json"),
        );
        assert_eq!(provenance["poll_interval_ms"], interval_ms);
        let result = read_json(&bundle.join("trials").join(trial).join("result.json"));
        let window_ms = result["measurement_elapsed_ns"]
            .as_u64()
            .expect("measured window")
            / 1_000_000;
        // Bounded polling: at most one scrape per cadence per window plus the
        // start and stop snapshots, and never a busy loop. Integer arithmetic
        // keeps the bound exact rather than approximately correct.
        let observed = provenance["sample_count"].as_u64().expect("sample count");
        let ceiling = window_ms.div_ceil(interval_ms) + 2;
        assert!(
            observed >= 2 && observed <= ceiling,
            "trial {trial}: {observed} samples exceed the {interval_ms}ms cadence bound for a {window_ms}ms window"
        );
        total_samples += observed;
        total_served_ms += window_ms;
    }
    // The subject recorded how long it spent answering every scrape; polling
    // cost is measured, not assumed.
    let scrapes = workspace.scrape_log();
    assert!(!scrapes.is_empty(), "subject recorded no scrape accounting");
    let served_ms: f64 = scrapes.iter().map(|(_, served)| served).sum();
    assert!(
        served_ms < total_served_ms as f64,
        "aggregate scrape service time {served_ms}ms must stay below the aggregate measured window {total_served_ms}ms"
    );
    // One preflight plus a start and stop snapshot per measured trial sit
    // outside the retained in-window sample list; nothing else may scrape.
    let trials = 3_u64;
    assert!(
        (u64::try_from(scrapes.len()).unwrap_or(u64::MAX) > total_samples)
            && (u64::try_from(scrapes.len()).unwrap_or(0) <= total_samples + 2 * trials + 1),
        "{} scrapes is outside the bounded polling contract for {total_samples} in-window samples over {trials} trials",
        scrapes.len()
    );
    let indices: Vec<u64> = scrapes.iter().map(|(index, _)| *index).collect();
    let dense: Vec<u64> = (1..=u64::try_from(indices.len()).expect("bounded scrape log")).collect();
    assert_eq!(
        indices, dense,
        "scrape accounting must be dense and monotonic"
    );
    eprintln!(
        "polling overhead: {total_samples} in-window samples over {total_served_ms} measured ms, subject-side scrape service total {served_ms}ms"
    );
}

#[test]
fn telemetry_polling_stops_with_the_run_and_leaves_no_listener() {
    let _guard = qualification_test_lock();
    let workspace = Workspace::new();
    let bundle = workspace.path().join("telemetry-drain.eggb");
    let output = workspace.run(&[
        "run",
        "scenarios/telemetry-pressure-c32.json",
        bundle.to_str().expect("utf-8 bundle path"),
        "--json",
    ]);
    assert_eq!(output.status.code(), Some(0));
    // The subject was torn down with the run, so its telemetry listener is
    // gone: no collector task can outlive runner teardown and keep scraping.
    let listener_reachable =
        std::net::TcpStream::connect(("127.0.0.1", workspace.metrics_port)).is_ok();
    assert!(
        !listener_reachable,
        "the subject metrics listener must not survive the run"
    );
    let lifecycle = read_json(&bundle.join("lifecycle").join("lifecycle.json"));
    assert!(
        lifecycle["stopped_order"]
            .as_array()
            .is_some_and(|order| order.iter().any(|identity| identity == "synvoid")),
        "the subject must be stopped inside the run tail: {lifecycle}"
    );
}

#[test]
fn external_oracle_and_eggsec_plans_are_bounded_and_optional() {
    let _guard = qualification_test_lock();
    let workspace = Workspace::new();
    for (scenario, driver) in [
        ("body-oha-c8", "oha"),
        ("eggsec-benign-body-c8", "eggsec-load"),
        ("eggsec-blocked-body-c8", "eggsec-load"),
    ] {
        let plan = read_json(&workspace.path().join(format!("scenarios/{scenario}.json")));
        // One repeated case only: the external adapters never approximate a
        // mixed schedule.
        let workload = plan.get("workload").expect("workload");
        let schedule = workload["schedule"].as_array().expect("schedule");
        assert_eq!(schedule.len(), 1, "{scenario} must repeat one case");
        assert_eq!(workload["connection_policy"], "pooled");
        assert_eq!(workload["target"], "synvoid");
        if !has_tool("oha") && driver == "oha" {
            eprintln!("skipping {scenario} execution: oha not installed");
            continue;
        }
        if !has_tool("eggsec") {
            eprintln!("skipping {scenario} execution: eggsec not installed");
            continue;
        }
        let bundle = workspace.path().join(format!("{scenario}.eggb"));
        let output = workspace.run(&[
            "run",
            &format!("scenarios/{scenario}.json"),
            bundle.to_str().expect("utf-8 bundle path"),
            "--workload-driver",
            driver,
            "--json",
        ]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{scenario} via {driver} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let trial = observations(&bundle, "001");
        assert!(
            observed_value(&trial, "expected_outcome_mismatch_rate").abs() < f64::EPSILON,
            "{scenario} via {driver} recorded a status mismatch"
        );
        let method = read_json(
            &bundle
                .join("trials/001/artifacts")
                .join(if driver == "oha" {
                    "001-oha-method.json"
                } else {
                    "001-eggsec-load-method.json"
                }),
        );
        assert_eq!(method["workload"], "http_corpus");
        if driver == "eggsec-load" {
            // Eggsec's production load path uses Eggfetch, so it is a
            // security-owner execution path, not an independent transport
            // oracle. The evidence must say so.
            assert_eq!(method["transport_independent"], false);
        }
    }
}

/// Eggbench's workspace content identity for one input file, computed the
/// same way the runner computes it (path, byte length, file digest).
fn content_identity(workspace: &Path, requested: &str) -> String {
    let path = workspace.join(requested);
    let raw = std::fs::read(&path).expect("read pinned input");
    let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
    sha2::Digest::update(
        &mut hasher,
        requested.rsplit('/').next().unwrap_or(requested).as_bytes(),
    );
    sha2::Digest::update(&mut hasher, [0u8]);
    sha2::Digest::update(&mut hasher, (raw.len() as u64).to_be_bytes());
    sha2::Digest::update(&mut hasher, [0u8]);
    sha2::Digest::update(&mut hasher, sha256_hex(&raw).as_bytes());
    sha2::Digest::update(&mut hasher, [0u8]);
    format!("{:x}", sha2::Digest::finalize(hasher))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut hasher, bytes);
    format!("{:x}", sha2::Digest::finalize(hasher))
}
