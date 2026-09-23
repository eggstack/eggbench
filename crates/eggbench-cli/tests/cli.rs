//! Integration tests for the `eggbench` CLI library API.
//!
//! These tests invoke `eggbench_cli::execute` directly to verify the
//! machine envelope contract without spawning a subprocess for every case.

use eggbench_cli::{Command, CommandOptions, ExitCode, InputFormat, execute};
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
    let envelope = execute(
        Command::Validate {
            plan,
            input_format: None,
        },
        CommandOptions::human(),
    )
    .await
    .expect("envelope");
    assert!(envelope.ok);
    let body: Value = serde_json::to_value(&envelope).unwrap();
    assert_eq!(body["command"], "validate");
    assert_eq!(body["ok"], true);
    assert_eq!(body["result"]["kind"], "validate");
    assert_eq!(body["result"]["experiment"], "smoke");
}

#[tokio::test]
async fn validate_rejects_unknown_extension() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let bad = tmp.path().join("plan.txt");
    std::fs::write(&bad, "{}").expect("write");
    let error = execute(
        Command::Validate {
            plan: bad,
            input_format: None,
        },
        CommandOptions::human(),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, eggbench_cli::CliError::PlanFormat(_)));
}

#[tokio::test]
async fn validate_stdin_requires_explicit_format() {
    let error = execute(
        Command::Validate {
            plan: std::path::PathBuf::from("-"),
            input_format: None,
        },
        CommandOptions::human(),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, eggbench_cli::CliError::PlanFormat(_)));
    let _ = fixture_dir();
}

#[tokio::test]
async fn validate_rejects_invalid_plan() {
    let plan = fixture_dir().join("invalid-cycle.json");
    let error = execute(
        Command::Validate {
            plan,
            input_format: None,
        },
        CommandOptions::human(),
    )
    .await
    .unwrap_err();
    let failure = error.into_failure();
    assert_eq!(failure.category, "plan_validation");
    assert_eq!(failure.exit_code, ExitCode::ParseValidation);
}

#[tokio::test]
async fn doctor_emits_environment_fingerprint() {
    let plan = fixture_dir().join("minimal.json");
    let envelope = execute(
        Command::Doctor {
            plan,
            input_format: None,
        },
        CommandOptions::human(),
    )
    .await
    .expect("envelope");
    assert!(envelope.ok);
    let body: Value = serde_json::to_value(&envelope).unwrap();
    assert_eq!(body["result"]["kind"], "doctor");
    assert_eq!(body["result"]["resolved"], true);
    assert!(
        !body["result"]["environment_fields"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn inspect_verifies_minimal_bundle() {
    let bundle = fixture_dir().join("current-v2.eggb");
    let envelope = execute(
        Command::Inspect {
            bundle,
            emit_manifest_json: false,
        },
        CommandOptions::human(),
    )
    .await
    .expect("envelope");
    assert!(envelope.ok);
    let body: Value = serde_json::to_value(&envelope).unwrap();
    assert_eq!(body["result"]["kind"], "inspect");
    assert_eq!(body["result"]["manifest_schema_version"], 2);
}

#[tokio::test]
async fn inspect_rejects_missing_bundle() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let bundle = tmp.path().join("missing.eggb");
    let error = execute(
        Command::Inspect {
            bundle,
            emit_manifest_json: false,
        },
        CommandOptions::human(),
    )
    .await
    .unwrap_err();
    let failure = error.into_failure();
    assert_eq!(failure.category, "bundle");
    assert_eq!(failure.exit_code, ExitCode::EvidenceIo);
}

#[tokio::test]
async fn run_rejects_missing_timeout() {
    let plan = fixture_dir().join("minimal.json");
    let tmp = tempfile::tempdir().expect("tempdir");
    let bundle = tmp.path().join("run.eggb");
    let envelope = execute(
        Command::Run {
            plan,
            input_format: None,
            bundle,
        },
        CommandOptions::human(),
    )
    .await
    .expect("envelope");
    assert!(!envelope.ok);
    let category = envelope.error.as_ref().map(|e| e.category.as_str());
    assert!(
        matches!(category, Some("evidence" | "runner_error")),
        "unexpected category {category:?}"
    );
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
