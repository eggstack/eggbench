//! Integration tests for the `eggbench` CLI library API.
//!
//! These tests invoke `eggbench_cli::execute` directly to verify the machine
//! envelope contract and the attached process-exit code without spawning a
//! subprocess for every case. Subprocess coverage lives in
//! `tests/binary_exit_codes.rs`.

use eggbench_cli::{Command, CommandOptions, ExitCode, InputFormat, WorkloadRegistry, execute};
use serde_json::Value;
use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("eggbench-core")
        .join("tests")
        .join("fixtures")
}

#[tokio::test]
async fn validate_accepts_minimal_plan() {
    let plan = fixture_dir().join("minimal.json");
    let presented = execute(
        Command::Validate {
            plan,
            input_format: None,
        },
        CommandOptions::human(),
    )
    .await;
    assert!(presented.envelope.ok);
    assert_eq!(presented.exit_code, ExitCode::Success);
    let body: Value = serde_json::to_value(&presented.envelope).unwrap();
    assert_eq!(body["command"], "validate");
    assert_eq!(body["ok"], true);
    assert_eq!(body["result"]["kind"], "validate");
    assert_eq!(body["result"]["experiment"], "smoke");
    let _ = InputFormat::Json;
}

#[tokio::test]
async fn validate_rejects_unknown_extension() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let bad = tmp.path().join("plan.txt");
    std::fs::write(&bad, "{}").expect("write");
    let presented = execute(
        Command::Validate {
            plan: bad,
            input_format: None,
        },
        CommandOptions::human(),
    )
    .await;
    assert!(!presented.envelope.ok);
    assert_eq!(presented.exit_code, ExitCode::ParseValidation);
    assert_eq!(presented.exit_code.code(), 2);
}

#[tokio::test]
async fn validate_stdin_requires_explicit_format() {
    let presented = execute(
        Command::Validate {
            plan: std::path::PathBuf::from("-"),
            input_format: None,
        },
        CommandOptions::human(),
    )
    .await;
    assert!(!presented.envelope.ok);
    assert_eq!(presented.exit_code, ExitCode::ParseValidation);
    let _ = fixture_dir();
}

#[tokio::test]
async fn validate_rejects_invalid_plan() {
    let plan = fixture_dir().join("invalid-cycle.json");
    let presented = execute(
        Command::Validate {
            plan,
            input_format: None,
        },
        CommandOptions::human(),
    )
    .await;
    assert!(!presented.envelope.ok);
    let failure = presented.envelope.error.as_ref().expect("error");
    assert_eq!(failure.category, "plan_validation");
    assert_eq!(presented.exit_code, ExitCode::ParseValidation);
}

#[tokio::test]
async fn doctor_production_reports_no_workload_driver() {
    let plan = fixture_dir().join("minimal.json");
    let presented = execute(
        Command::Doctor {
            plan,
            input_format: None,
        },
        CommandOptions::human(),
    )
    .await;
    #[cfg(not(feature = "eggstack-http"))]
    {
        // Production inventory is empty, so resolution fails truthfully while the
        // doctor payload (including has_workload_driver=false) is retained.
        assert!(!presented.envelope.ok);
        assert_eq!(presented.exit_code, ExitCode::CapabilityPreflight);
        let body: Value = serde_json::to_value(&presented.envelope).unwrap();
        assert_eq!(body["result"]["kind"], "doctor");
        assert_eq!(body["result"]["resolved"], false);
        assert_eq!(body["result"]["has_workload_driver"], false);
        assert!(
            !body["result"]["environment_fields"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    #[cfg(feature = "eggstack-http")]
    {
        // With the native drivers compiled in, the closed-loop fixture
        // resolves against the production catalog.
        assert!(presented.envelope.ok);
        assert_eq!(presented.exit_code, ExitCode::Success);
        let body: Value = serde_json::to_value(&presented.envelope).unwrap();
        assert_eq!(body["result"]["resolved"], true);
        assert_eq!(body["result"]["has_workload_driver"], true);
        let drivers = body["result"]["drivers"].as_array().unwrap();
        let fetch = drivers
            .iter()
            .find(|driver| driver["name"] == "eggfetch-http")
            .expect("eggfetch driver is reported");
        assert_eq!(fetch["category"], "Workload");
        assert_eq!(fetch["upstream_name"], "eggfetch-core");
        assert!(
            fetch["upstream_version"]
                .as_str()
                .is_some_and(|version| !version.is_empty()),
            "doctor shows the exact sibling version"
        );
        assert!(
            fetch["capabilities"]
                .as_array()
                .unwrap()
                .iter()
                .any(|capability| capability
                    .as_str()
                    .is_some_and(|text| text.contains("ClosedLoop"))),
            "doctor shows supported load-mode capabilities"
        );
    }
}

#[tokio::test]
async fn doctor_qualification_registry_resolves() {
    let plan = fixture_dir().join("minimal.json");
    let registry = WorkloadRegistry::with_qualification_fake();
    let presented =
        eggbench_cli::commands_doctor_run_with_registry(&plan, None, &registry.inventory())
            .expect("doctor");
    assert!(presented.envelope.ok);
    assert_eq!(presented.exit_code, ExitCode::Success);
    let body: Value = serde_json::to_value(&presented.envelope).unwrap();
    assert_eq!(body["result"]["resolved"], true);
    assert_eq!(body["result"]["has_workload_driver"], true);
}

#[tokio::test]
async fn production_registry_contains_no_fake_driver() {
    let registry = WorkloadRegistry::production();
    #[cfg(not(feature = "eggstack-http"))]
    assert!(!registry.has_workload_driver());
    #[cfg(feature = "eggstack-http")]
    assert!(registry.has_workload_driver());
    assert!(
        !registry
            .inventory()
            .iter()
            .any(|entry| entry.descriptor.name == "fake-load")
    );
    let legacy = WorkloadRegistry::with_builtin();
    assert!(
        !legacy
            .inventory()
            .iter()
            .any(|entry| entry.descriptor.name == "fake-load")
    );
}

#[tokio::test]
async fn production_run_fails_before_startup_without_starting_services() {
    let plan = fixture_dir().join("minimal.json");
    let tmp = tempfile::tempdir().expect("tempdir");
    let bundle = tmp.path().join("run.eggb");
    let presented = execute(
        Command::Run {
            plan,
            input_format: None,
            bundle: bundle.clone(),
        },
        CommandOptions::human(),
    )
    .await;
    #[cfg(not(feature = "eggstack-http"))]
    {
        assert!(!presented.envelope.ok);
        assert_eq!(presented.exit_code, ExitCode::CapabilityPreflight);
        let failure = presented.envelope.error.as_ref().expect("error");
        assert!(
            failure.category == "missing_driver" || failure.category == "unsupported_workload",
            "unexpected category {}",
            failure.category
        );
        // No bundle publication and no managed startup occurred.
        assert!(!bundle.exists());
        assert!(presented.envelope.result.is_none());
    }
    #[cfg(feature = "eggstack-http")]
    {
        // With the native drivers compiled in, the timeout-less fixture
        // resolves but fails orchestration preflight (measurement timeout
        // is required) before any managed startup or bundle publication.
        assert!(!presented.envelope.ok);
        assert_eq!(presented.exit_code, ExitCode::EvidenceIo);
        let failure = presented.envelope.error.as_ref().expect("error");
        assert_eq!(failure.category, "evidence");
        assert!(!bundle.exists());
    }
}

/// Minimal plan plus the M002-required measurement/drain timeouts.
///
/// `minimal.json` intentionally carries empty `timeouts`, which exercises the
/// preflight failure path. Qualification runs need real timeouts.
fn write_qualification_plan(dir: &std::path::Path) -> PathBuf {
    let raw = std::fs::read_to_string(fixture_dir().join("minimal.json")).expect("read");
    let mut value: Value = serde_json::from_str(&raw).expect("parse");
    value["trials"]["timeouts"] = serde_json::json!({"measurement": 5000, "drain": 5000});
    let path = dir.join("qualification.json");
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&value).expect("serialize"),
    )
    .expect("write");
    path
}

#[tokio::test]
async fn qualification_run_success_exits_zero() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let plan = write_qualification_plan(tmp.path());
    let bundle = tmp.path().join("run.eggb");
    let fake = eggbench_runner::test_support::FakeWorkload::default();
    let presented = eggbench_cli::commands_run_with_qualification(
        &plan,
        None,
        &bundle,
        CommandOptions::human(),
        fake,
        std::future::pending(),
    )
    .await
    .expect("qualification run");
    assert!(presented.envelope.ok, "{:?}", presented.envelope.error);
    assert_eq!(presented.exit_code, ExitCode::Success);
    assert!(bundle.exists());
}

#[tokio::test]
async fn qualification_run_failed_finalized_run_exits_four_with_bundle() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let plan = write_qualification_plan(tmp.path());
    let bundle = tmp.path().join("run.eggb");
    let mut fake = eggbench_runner::test_support::FakeWorkload::default();
    fake.fail_on = Some(1);
    let presented = eggbench_cli::commands_run_with_qualification(
        &plan,
        None,
        &bundle,
        CommandOptions::human(),
        fake,
        std::future::pending(),
    )
    .await
    .expect("qualification run");
    assert!(!presented.envelope.ok);
    assert_eq!(presented.exit_code, ExitCode::RunCompletedNonSuccess);
    assert_eq!(presented.exit_code.code(), 4);
    // Code 4 retains the finalized bundle result.
    let body: Value = serde_json::to_value(&presented.envelope).unwrap();
    assert_eq!(body["ok"], false);
    assert_eq!(body["result"]["kind"], "run");
    assert!(body["result"]["bundle"].is_string());
    assert_eq!(body["error"]["category"], "run_non_success");
    assert!(bundle.exists());
}

#[tokio::test]
async fn qualification_run_cancellation_exits_four_with_bundle() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let plan = write_qualification_plan(tmp.path());
    let bundle = tmp.path().join("run.eggb");
    let mut fake = eggbench_runner::test_support::FakeWorkload::default();
    fake.pending_on = Some(1);
    // Deterministic trigger: resolve immediately so cancellation fires while
    // the first invocation is pending.
    let presented = eggbench_cli::commands_run_with_qualification(
        &plan,
        None,
        &bundle,
        CommandOptions::human(),
        fake,
        async move {},
    )
    .await
    .expect("qualification run");
    assert!(!presented.envelope.ok);
    assert_eq!(presented.exit_code, ExitCode::RunCompletedNonSuccess);
    let body: Value = serde_json::to_value(&presented.envelope).unwrap();
    assert_eq!(body["result"]["execution_status"], "cancelled");
    assert!(bundle.exists());
}

#[tokio::test]
async fn signal_forwarder_cancels_token() {
    use tokio_util::sync::CancellationToken;
    let cancel = CancellationToken::new();
    assert!(!cancel.is_cancelled());
    eggbench_cli::commands_run_forward_signal(cancel.clone(), async move {}).await;
    assert!(cancel.is_cancelled());
}

#[tokio::test]
async fn inspect_verifies_minimal_bundle() {
    let bundle = fixture_dir().join("current-v2.eggb");
    let presented = execute(
        Command::Inspect {
            bundle,
            emit_manifest_json: false,
        },
        CommandOptions::human(),
    )
    .await;
    assert!(presented.envelope.ok);
    assert_eq!(presented.exit_code, ExitCode::Success);
    let body: Value = serde_json::to_value(&presented.envelope).unwrap();
    assert_eq!(body["result"]["kind"], "inspect");
    assert_eq!(body["result"]["manifest_schema_version"], 2);
}

#[tokio::test]
async fn inspect_rejects_missing_bundle() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let bundle = tmp.path().join("missing.eggb");
    let presented = execute(
        Command::Inspect {
            bundle,
            emit_manifest_json: false,
        },
        CommandOptions::human(),
    )
    .await;
    assert!(!presented.envelope.ok);
    let failure = presented.envelope.error.as_ref().expect("error");
    assert_eq!(failure.category, "bundle");
    assert_eq!(presented.exit_code, ExitCode::EvidenceIo);
    assert_eq!(presented.exit_code.code(), 5);
}

#[tokio::test]
async fn exit_code_matrix_is_locked() {
    assert_eq!(ExitCode::Success.code(), 0);
    assert_eq!(ExitCode::Internal.code(), 1);
    assert_eq!(ExitCode::ParseValidation.code(), 2);
    assert_eq!(ExitCode::CapabilityPreflight.code(), 3);
    assert_eq!(ExitCode::RunCompletedNonSuccess.code(), 4);
    assert_eq!(ExitCode::EvidenceIo.code(), 5);
    assert_eq!(ExitCode::ComparisonFail.code(), 6);
    assert_eq!(ExitCode::ComparisonInconclusive.code(), 7);
    assert_eq!(ExitCode::ComparisonInvalid.code(), 8);
}

#[tokio::test]
async fn internal_failure_maps_to_code_one() {
    let failure = eggbench_cli::CliFailure::new("internal", "boom", ExitCode::Internal);
    let presented = eggbench_cli::PresentedCommandResult::failure("validate", &failure);
    assert_eq!(presented.exit_code.code(), 1);
    assert!(!presented.envelope.ok);
}

/// Write a 7-trial statistical comparison plan with a `latency_p99` gate.
fn write_compare_plan(dir: &std::path::Path, trials: u32) -> PathBuf {
    let raw = std::fs::read_to_string(fixture_dir().join("minimal.json")).expect("read");
    let mut value: Value = serde_json::from_str(&raw).expect("parse");
    value["trials"]["measured"] = serde_json::json!(trials);
    value["trials"]["warmup"] = serde_json::json!(0);
    value["trials"]["cooldown_ms"] = Value::Null;
    value["trials"]["timeouts"] = serde_json::json!({"measurement": 5000, "drain": 5000});
    value["metrics"] = serde_json::json!([{
        "name": "latency_p99",
        "unit": "ms",
        "direction": {"kind": "lower_is_better"},
        "intent": "primary",
        "gate": {"kind": "statistical_relative", "allowance": 500, "min_trials": 1},
    }]);
    let path = dir.join(format!("compare-{trials}.json"));
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&value).expect("serialize"),
    )
    .expect("write");
    path
}

/// Fake workload emitting one `latency_p99` percentile observation per trial.
fn latency_fake(values: &[f64]) -> eggbench_runner::test_support::FakeWorkload {
    let mut fake = eggbench_runner::test_support::FakeWorkload::default();
    fake.metrics_by_invocation = values
        .iter()
        .map(|value| {
            Some(vec![eggbench_core::RawMetricObservation {
                name: "latency_p99".to_owned(),
                unit: "ms".to_owned(),
                value: *value,
                aggregation: eggbench_core::Aggregation::Percentile { basis_points: 9900 },
                source_field: Some("fake.p99".to_owned()),
                raw_artifacts: Vec::new(),
            }])
        })
        .collect();
    fake
}

async fn qualification_bundle(plan: &std::path::Path, bundle: &std::path::Path, values: &[f64]) {
    let presented = eggbench_cli::commands_run_with_qualification(
        plan,
        None,
        bundle,
        CommandOptions::human(),
        latency_fake(values),
        std::future::pending(),
    )
    .await
    .expect("qualification run");
    assert!(presented.envelope.ok, "{:?}", presented.envelope.error);
}

fn manifest_bytes(bundle: &std::path::Path) -> Vec<u8> {
    std::fs::read(bundle.join("manifest.json")).expect("manifest bytes")
}

#[tokio::test]
async fn compare_clear_regression_fails_with_code_six() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let plan = write_compare_plan(tmp.path(), 7);
    let baseline = tmp.path().join("baseline.eggb");
    let candidate = tmp.path().join("candidate.eggb");
    qualification_bundle(&plan, &baseline, &[100.0; 7]).await;
    qualification_bundle(&plan, &candidate, &[130.0; 7]).await;
    let before = (manifest_bytes(&baseline), manifest_bytes(&candidate));

    let presented = execute(
        Command::Compare {
            baseline: Some(baseline.clone()),
            candidate: candidate.clone(),
            alias: None,
            absolute_only: false,
            output: None,
            seed: None,
        },
        CommandOptions::human(),
    )
    .await;
    assert!(!presented.envelope.ok);
    assert_eq!(presented.exit_code, ExitCode::ComparisonFail);
    assert_eq!(presented.exit_code.code(), 6);
    let body: Value = serde_json::to_value(&presented.envelope).unwrap();
    assert_eq!(body["result"]["kind"], "compare");
    assert_eq!(body["result"]["aggregate_verdict"], "fail");
    assert_eq!(body["error"]["category"], "comparison_fail");
    assert_eq!(
        body["result"]["receipt"]["policy_id"],
        "eggbench.trial-bootstrap.v1"
    );
    assert_eq!(body["result"]["receipt"]["metrics"][0]["resamples"], 10000);

    // Neither source bundle was modified.
    assert_eq!(manifest_bytes(&baseline), before.0);
    assert_eq!(manifest_bytes(&candidate), before.1);
}

#[tokio::test]
async fn compare_non_regression_passes_with_code_zero() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let plan = write_compare_plan(tmp.path(), 7);
    let baseline = tmp.path().join("baseline.eggb");
    let candidate = tmp.path().join("candidate.eggb");
    qualification_bundle(&plan, &baseline, &[100.0; 7]).await;
    qualification_bundle(&plan, &candidate, &[101.0; 7]).await;

    let presented = execute(
        Command::Compare {
            baseline: Some(baseline),
            candidate,
            alias: None,
            absolute_only: false,
            output: None,
            seed: Some(42),
        },
        CommandOptions::human(),
    )
    .await;
    assert!(presented.envelope.ok, "{:?}", presented.envelope.error);
    assert_eq!(presented.exit_code, ExitCode::Success);
    let body: Value = serde_json::to_value(&presented.envelope).unwrap();
    assert_eq!(body["result"]["aggregate_verdict"], "pass");
    assert_eq!(body["result"]["comparability_match"], true);
}

#[tokio::test]
async fn compare_threshold_crossing_is_inconclusive_with_code_seven() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let plan = write_compare_plan(tmp.path(), 7);
    let baseline = tmp.path().join("baseline.eggb");
    let candidate = tmp.path().join("candidate.eggb");
    qualification_bundle(&plan, &baseline, &[100.0; 7]).await;
    qualification_bundle(
        &plan,
        &candidate,
        &[82.0, 118.0, 85.0, 125.0, 90.0, 122.0, 105.0],
    )
    .await;

    let presented = execute(
        Command::Compare {
            baseline: Some(baseline),
            candidate,
            alias: None,
            absolute_only: false,
            output: None,
            seed: None,
        },
        CommandOptions::human(),
    )
    .await;
    assert_eq!(presented.exit_code, ExitCode::ComparisonInconclusive);
    assert_eq!(presented.exit_code.code(), 7);
    let body: Value = serde_json::to_value(&presented.envelope).unwrap();
    assert_eq!(body["result"]["aggregate_verdict"], "inconclusive");
    assert_eq!(body["error"]["category"], "comparison_inconclusive");
}

#[tokio::test]
async fn compare_insufficient_trials_is_invalid_with_code_eight() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let plan = write_compare_plan(tmp.path(), 3);
    let baseline = tmp.path().join("baseline.eggb");
    let candidate = tmp.path().join("candidate.eggb");
    qualification_bundle(&plan, &baseline, &[100.0; 3]).await;
    qualification_bundle(&plan, &candidate, &[101.0; 3]).await;

    let presented = execute(
        Command::Compare {
            baseline: Some(baseline),
            candidate,
            alias: None,
            absolute_only: false,
            output: None,
            seed: None,
        },
        CommandOptions::human(),
    )
    .await;
    assert_eq!(presented.exit_code, ExitCode::ComparisonInvalid);
    assert_eq!(presented.exit_code.code(), 8);
    let body: Value = serde_json::to_value(&presented.envelope).unwrap();
    assert_eq!(body["result"]["aggregate_verdict"], "invalid");
}

#[tokio::test]
async fn compare_absolute_only_needs_no_baseline() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let raw = std::fs::read_to_string(fixture_dir().join("minimal.json")).expect("read");
    let mut value: Value = serde_json::from_str(&raw).expect("parse");
    value["trials"]["measured"] = serde_json::json!(3);
    value["trials"]["warmup"] = serde_json::json!(0);
    value["trials"]["cooldown_ms"] = Value::Null;
    value["trials"]["timeouts"] = serde_json::json!({"measurement": 5000, "drain": 5000});
    let plan = tmp.path().join("absolute.json");
    std::fs::write(
        &plan,
        serde_json::to_string_pretty(&value).expect("serialize"),
    )
    .expect("write");
    let candidate = tmp.path().join("candidate.eggb");
    qualification_bundle(&plan, &candidate, &[100.0, 100.0, 100.0]).await;
    let receipt_path = tmp.path().join("comparison.json");

    let presented = execute(
        Command::Compare {
            baseline: None,
            candidate,
            alias: None,
            absolute_only: true,
            output: Some(receipt_path.clone()),
            seed: None,
        },
        CommandOptions::human(),
    )
    .await;
    assert!(presented.envelope.ok, "{:?}", presented.envelope.error);
    assert_eq!(presented.exit_code, ExitCode::Success);
    let body: Value = serde_json::to_value(&presented.envelope).unwrap();
    assert_eq!(body["result"]["aggregate_verdict"], "pass");
    assert!(receipt_path.is_file());
    let receipt: Value =
        serde_json::from_str(&std::fs::read_to_string(&receipt_path).expect("read"))
            .expect("parse");
    assert_eq!(receipt["policy_id"], "eggbench.trial-bootstrap.v1");
    assert!(receipt.get("baseline_identity").is_none());
}

#[tokio::test]
async fn compare_alias_resolves_and_digest_mismatch_fails() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let plan = write_compare_plan(tmp.path(), 7);
    let baseline = tmp.path().join("baseline.eggb");
    let candidate = tmp.path().join("candidate.eggb");
    qualification_bundle(&plan, &baseline, &[100.0; 7]).await;
    qualification_bundle(&plan, &candidate, &[101.0; 7]).await;

    // Derive the pinned digest from a path-based comparison receipt.
    let presented = execute(
        Command::Compare {
            baseline: Some(baseline.clone()),
            candidate: candidate.clone(),
            alias: None,
            absolute_only: false,
            output: None,
            seed: Some(7),
        },
        CommandOptions::human(),
    )
    .await;
    assert!(presented.envelope.ok);
    let body: Value = serde_json::to_value(&presented.envelope).unwrap();
    let digest = body["result"]["receipt"]["baseline_identity"]["manifest_sha256"]
        .as_str()
        .expect("digest")
        .to_owned();

    let alias_path = tmp.path().join("baseline.eggbaseline.json");
    std::fs::write(
        &alias_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": 1,
            "alias": "good-baseline",
            "bundle_path": "baseline.eggb",
            "manifest_sha256": digest,
        }))
        .expect("serialize"),
    )
    .expect("write");
    let presented = execute(
        Command::Compare {
            baseline: None,
            candidate: candidate.clone(),
            alias: Some(alias_path),
            absolute_only: false,
            output: None,
            seed: Some(7),
        },
        CommandOptions::human(),
    )
    .await;
    assert!(presented.envelope.ok, "{:?}", presented.envelope.error);
    let aliased: Value = serde_json::to_value(&presented.envelope).unwrap();
    assert_eq!(aliased["result"]["aggregate_verdict"], "pass");
    assert_eq!(
        aliased["result"]["receipt"]["baseline_reference"]["kind"],
        "alias"
    );
    // Same seed + same inputs resolve identically through path or alias.
    assert_eq!(
        aliased["result"]["receipt"]["metrics"],
        body["result"]["receipt"]["metrics"]
    );

    // Tampered digest fails closed with code 2 and touches nothing.
    let bad_alias = tmp.path().join("bad.eggbaseline.json");
    std::fs::write(
        &bad_alias,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": 1,
            "alias": "tampered",
            "bundle_path": "baseline.eggb",
            "manifest_sha256": "00".repeat(32),
        }))
        .expect("serialize"),
    )
    .expect("write");
    let presented = execute(
        Command::Compare {
            baseline: None,
            candidate,
            alias: Some(bad_alias),
            absolute_only: false,
            output: None,
            seed: None,
        },
        CommandOptions::human(),
    )
    .await;
    assert_eq!(presented.exit_code, ExitCode::ParseValidation);
    assert_eq!(presented.exit_code.code(), 2);
}

#[tokio::test]
async fn compare_missing_bundle_exits_five() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let missing = tmp.path().join("missing.eggb");
    let candidate = tmp.path().join("candidate.eggb");
    let presented = execute(
        Command::Compare {
            baseline: Some(missing),
            candidate,
            alias: None,
            absolute_only: false,
            output: None,
            seed: None,
        },
        CommandOptions::human(),
    )
    .await;
    assert_eq!(presented.exit_code, ExitCode::EvidenceIo);
    assert_eq!(presented.exit_code.code(), 5);
}

#[tokio::test]
async fn envelope_serializes_to_machine_json() {
    let envelope = eggbench_cli::CliEnvelope::ok(
        "validate",
        eggbench_cli::CliOutput::Validate {
            experiment: Some("smoke".into()),
            schema_version: 1,
        },
    );
    let body = envelope.to_pretty_json().expect("serialize");
    let value: Value = serde_json::from_str(&body).expect("parse");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["command"], "validate");
    assert_eq!(value["ok"], true);
    assert_eq!(value["result"]["kind"], "validate");
    let _ = InputFormat::Json;
}

// ---- Eggstack M001a end-to-end loopback (feature-gated) ----

/// Full native experiment path: `EggServe` controlled origin, runtime HTTP
/// binding, `Eggfetch` workload, M002 trial lifecycle, immutable bundle.
///
/// Proves `EggServe` starts and becomes ready, the ephemeral URL is recorded,
/// warmup and measured trials execute without errors on loopback, every
/// measured trial retains a raw histogram, `metrics.json` carries observed
/// throughput/latency/error metrics, runtime-topology evidence exists, the
/// server shuts down, and the finalized bundle verifies.
#[cfg(feature = "eggstack-http")]
#[tokio::test]
async fn eggstack_loopback_end_to_end() {
    use eggbench_core::{ArtifactPath, BundleReader};
    use std::io::Read;

    let plan = fixture_dir().join("eggstack-loopback.json");
    let tmp = tempfile::tempdir().expect("tempdir");
    let bundle = tmp.path().join("loopback.eggb");
    let presented = execute(
        Command::Run {
            plan,
            input_format: None,
            bundle: bundle.clone(),
        },
        CommandOptions::human(),
    )
    .await;
    assert!(
        presented.envelope.ok,
        "run failed: {:?}",
        presented.envelope
    );
    assert_eq!(presented.exit_code, ExitCode::Success);
    let body: Value = serde_json::to_value(&presented.envelope).unwrap();
    assert_eq!(body["result"]["execution_status"], "completed");
    assert_eq!(body["result"]["measured_trials"], 3);
    assert!(bundle.exists());

    let reader = BundleReader::open(&bundle).expect("bundle opens");
    reader.verify().expect("bundle verifies");
    assert_eq!(reader.manifest().trials.len(), 3);

    let mut artifact_paths: Vec<String> = reader
        .manifest()
        .artifacts
        .iter()
        .map(|artifact| artifact.path.to_string())
        .collect();
    artifact_paths.sort();

    // One raw histogram plus method evidence per measured trial.
    for trial in 1..=3 {
        assert_loopback_trial(&reader, &artifact_paths, trial);
    }

    // Runtime-topology evidence names the adapter-owned origin and its
    // startup-established loopback binding.
    assert!(
        artifact_paths
            .iter()
            .any(|path| path == "lifecycle/runtime-topology.json"),
        "runtime-topology evidence exists"
    );
    let topology_path = ArtifactPath::new("lifecycle/runtime-topology.json").unwrap();
    let mut topology_file = reader
        .open_artifact(&topology_path)
        .expect("topology opens");
    let mut topology_bytes = Vec::new();
    topology_file
        .read_to_end(&mut topology_bytes)
        .expect("topology reads");
    let topology: Value = serde_json::from_slice(&topology_bytes).expect("topology json");
    let origin = topology["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|service| service["identity"] == "origin")
        .expect("origin entry");
    assert_eq!(origin["ownership"], "adapter");
    assert_eq!(origin["service_type"], "eggserve-origin");
    let http_url = origin["bindings"]["http_url"].as_str().expect("http_url");
    assert!(http_url.starts_with("http://127.0.0.1:"));
    assert!(http_url.ends_with("/bench"));
}

/// Methodological guard: per-trial request volume must not change the
/// trial-level observation count exposed to Measurement M002.
///
/// Two runs differing only in request count (10 vs 50 per trial) must both
/// expose exactly 3 trials with the same requested observation names, while
/// the raw histogram sample counts differ.
#[cfg(feature = "eggstack-http")]
#[tokio::test]
async fn trial_observation_count_is_independent_of_request_volume() {
    use eggbench_core::{BundleReader, TrialId};
    use std::io::Read;

    async fn run_with_requests(requests: u64) -> (Vec<Vec<String>>, Vec<u64>) {
        let plan_text =
            std::fs::read_to_string(fixture_dir().join("eggstack-loopback.json")).expect("fixture");
        let mut plan: Value = serde_json::from_str(&plan_text).expect("fixture json");
        plan["workload"]["requests"] = Value::from(requests);
        let tmp = tempfile::tempdir().expect("tempdir");
        let plan_path = tmp.path().join("plan.json");
        std::fs::write(&plan_path, serde_json::to_vec_pretty(&plan).expect("plan"))
            .expect("write plan");
        let bundle = tmp.path().join("run.eggb");
        let presented = execute(
            Command::Run {
                plan: plan_path,
                input_format: None,
                bundle: bundle.clone(),
            },
            CommandOptions::human(),
        )
        .await;
        assert!(
            presented.envelope.ok,
            "run with {requests} requests failed: {:?}",
            presented.envelope
        );
        let reader = BundleReader::open(&bundle).expect("bundle opens");
        reader.verify().expect("bundle verifies");
        let mut observation_sets = Vec::new();
        let mut samples = Vec::new();
        for trial in 1..=3 {
            let metrics = reader
                .trial_metrics(TrialId::new(trial).unwrap())
                .expect("metrics read")
                .expect("metrics present");
            observation_sets.push(
                metrics
                    .observations
                    .iter()
                    .map(|observation| observation.name.as_str().to_owned())
                    .collect(),
            );
            // Raw sample counts come from the same-trial method evidence.
            let method_path = eggbench_core::ArtifactPath::new(format!(
                "trials/{trial:03}/artifacts/002-eggfetch-method.json"
            ))
            .unwrap();
            let mut file = reader.open_artifact(&method_path).expect("method opens");
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).expect("method reads");
            let method: Value = serde_json::from_slice(&bytes).expect("method json");
            samples.push(method["attempted_requests"].as_u64().expect("samples"));
        }
        (observation_sets, samples)
    }

    let (few_observations, few_samples) = run_with_requests(10).await;
    let (many_observations, many_samples) = run_with_requests(50).await;
    // Both runs expose exactly 3 trial-level observations per trial.
    assert_eq!(few_observations.len(), 3);
    assert_eq!(many_observations.len(), 3);
    for (few, many) in few_observations.iter().zip(many_observations.iter()) {
        assert_eq!(few, &vec!["error_rate", "latency_p99", "throughput"]);
        assert_eq!(few, many);
    }
    // Raw histogram sample counts differ; statistical sample counts do not.
    assert_eq!(few_samples, vec![10, 10, 10]);
    assert_eq!(many_samples, vec![50, 50, 50]);
}

/// `EggServe` startup failure is reported truthfully without hanging: an
/// invalid origin config passes preparation (adapters validate at start)
/// and fails managed startup with the adapter error preserved.
#[cfg(feature = "eggstack-http")]
#[tokio::test]
async fn eggserve_startup_failure_is_reported_without_hanging() {
    let plan_text =
        std::fs::read_to_string(fixture_dir().join("eggstack-loopback.json")).expect("fixture");
    let mut plan: Value = serde_json::from_str(&plan_text).expect("fixture json");
    plan["services"][0]["config"]["status"] = Value::from("600");
    let tmp = tempfile::tempdir().expect("tempdir");
    let plan_path = tmp.path().join("plan.json");
    std::fs::write(&plan_path, serde_json::to_vec_pretty(&plan).expect("plan"))
        .expect("write plan");
    let bundle = tmp.path().join("run.eggb");
    let presented = execute(
        Command::Run {
            plan: plan_path,
            input_format: None,
            bundle: bundle.clone(),
        },
        CommandOptions::human(),
    )
    .await;
    assert!(!presented.envelope.ok);
    assert_eq!(presented.exit_code, ExitCode::RunCompletedNonSuccess);
    let body: Value = serde_json::to_value(&presented.envelope).unwrap();
    assert_eq!(body["result"]["primary_failure"], "StartupFailed");
    assert!(bundle.exists(), "failed run still finalizes evidence");
}

/// Assert one loopback trial retained its histogram, method evidence, and
/// normalized throughput/latency/error observations.
#[cfg(feature = "eggstack-http")]
fn assert_loopback_trial(
    reader: &eggbench_core::BundleReader,
    artifact_paths: &[String],
    trial: u32,
) {
    use eggbench_core::TrialId;

    let prefix = format!("trials/{trial:03}/artifacts/");
    assert!(
        artifact_paths
            .iter()
            .any(|path| path == &format!("{prefix}001-latency.hdr")),
        "trial {trial} retains latency.hdr"
    );
    assert!(
        artifact_paths
            .iter()
            .any(|path| path == &format!("{prefix}002-eggfetch-method.json")),
        "trial {trial} retains method evidence"
    );

    // Normalized metrics carry observed throughput/latency/error values.
    let metrics = reader
        .trial_metrics(TrialId::new(trial).unwrap())
        .expect("metrics read")
        .expect("metrics present");
    let observed: Vec<&str> = metrics
        .observations
        .iter()
        .map(|observation| observation.name.as_str())
        .collect();
    assert_eq!(observed, vec!["error_rate", "latency_p99", "throughput"]);
    let value = |name: &str| {
        let observation = metrics
            .observations
            .iter()
            .find(|observation| observation.name.as_str() == name)
            .expect("metric");
        match observation.state {
            eggbench_core::ObservationState::Observed { value } => value,
            _ => panic!("metric {name} is not observed"),
        }
    };
    assert!(
        value("error_rate") < f64::EPSILON,
        "loopback run is error-free"
    );
    assert!(value("throughput") > 1.0, "loopback serves real traffic");
    assert!(value("latency_p99") >= 0.0);
    // The histogram reference points at the retained same-trial artifact.
    assert_eq!(metrics.histograms.len(), 1);
    assert_eq!(metrics.histograms[0].metric.as_str(), "latency");
    assert_eq!(metrics.histograms[0].format, "hdrhistogram-v2");
    assert!(
        metrics.histograms[0]
            .path
            .to_string()
            .ends_with("001-latency.hdr")
    );
}

/// With the native drivers, a production run whose workload target has no
/// runtime binding fails truthfully at invocation: the run finalizes with
/// `WorkloadFailed` (exit code 4) and retains its evidence bundle.
#[cfg(feature = "eggstack-http")]
#[tokio::test]
async fn production_run_without_target_binding_fails_at_workload() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let plan = write_qualification_plan(tmp.path());
    let bundle = tmp.path().join("run.eggb");
    let presented = execute(
        Command::Run {
            plan,
            input_format: None,
            bundle: bundle.clone(),
        },
        CommandOptions::human(),
    )
    .await;
    assert!(!presented.envelope.ok);
    assert_eq!(presented.exit_code, ExitCode::RunCompletedNonSuccess);
    let body: Value = serde_json::to_value(&presented.envelope).unwrap();
    assert_eq!(body["result"]["primary_failure"], "WorkloadFailed");
    assert!(bundle.exists());
}
