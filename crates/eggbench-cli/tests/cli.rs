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
    assert!(!registry.has_workload_driver());
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
}

#[tokio::test]
async fn internal_failure_maps_to_code_one() {
    let failure = eggbench_cli::CliFailure::new("internal", "boom", ExitCode::Internal);
    let presented = eggbench_cli::PresentedCommandResult::failure("validate", &failure);
    assert_eq!(presented.exit_code.code(), 1);
    assert!(!presented.envelope.ok);
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
