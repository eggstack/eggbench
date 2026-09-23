//! Subprocess exit-code tests against the actual `eggbench` binary.
//!
//! Covers the exact matrix in both JSON and human modes:
//! 0 success, 2 parse/validation, 3 capability/preflight (production run
//! with no adapter), 5 evidence/bundle failure. Code 4 (finalized
//! non-success run retaining its bundle) cannot be produced by the
//! production binary — which correctly has no executable fake path — so it
//! is locked through the injected qualification harness in `tests/cli.rs`
//! exercising the same presentation/exit-code path.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_eggbench"))
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("eggbench-core")
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(binary())
        .args(args)
        .output()
        .expect("spawn eggbench binary")
}

fn arg_strings(values: &[String]) -> Vec<&str> {
    values.iter().map(String::as_str).collect()
}

#[test]
fn validate_success_exits_zero_in_both_modes() {
    let plan = fixture("minimal.json");
    let plan = plan.to_str().unwrap();
    for json in [false, true] {
        let mut args = vec!["validate", plan];
        if json {
            args.push("--json");
        }
        let output = run(&args);
        assert_eq!(output.status.code(), Some(0), "args {args:?}");
        if json {
            let value: Value = serde_json::from_slice(&output.stdout).expect("one JSON doc");
            assert_eq!(value["ok"], true);
            assert_eq!(value["command"], "validate");
        }
    }
}

#[test]
fn invalid_plan_exits_two_in_both_modes() {
    let plan = fixture("invalid-cycle.json");
    let plan = plan.to_str().unwrap();
    for json in [false, true] {
        let mut args = vec!["validate", plan];
        if json {
            args.push("--json");
        }
        let output = run(&args);
        assert_eq!(output.status.code(), Some(2), "args {args:?}");
        if json {
            let value: Value = serde_json::from_slice(&output.stdout).expect("one JSON doc");
            assert_eq!(value["ok"], false);
            assert!(value.get("error").is_some());
        }
    }
}

#[test]
fn production_run_without_adapter_exits_three_in_both_modes() {
    let plan = fixture("minimal.json");
    let plan_str = plan.to_str().unwrap().to_owned();
    for json in [false, true] {
        let tmp = tempfile::tempdir().expect("tempdir");
        let bundle = tmp.path().join("run.eggb");
        let bundle_str = bundle.to_str().unwrap().to_owned();
        let owned = if json {
            vec![
                "run".to_owned(),
                plan_str.clone(),
                bundle_str.clone(),
                "--json".to_owned(),
            ]
        } else {
            vec!["run".to_owned(), plan_str.clone(), bundle_str.clone()]
        };
        let args = arg_strings(&owned);
        let output = run(&args);
        assert_eq!(output.status.code(), Some(3), "json={json}");
        assert!(!bundle.exists(), "no bundle may be published");
        if json {
            let value: Value = serde_json::from_slice(&output.stdout).expect("one JSON doc");
            assert_eq!(value["ok"], false);
            let category = value["error"]["category"].as_str().unwrap_or("");
            assert!(
                category == "missing_driver" || category == "unsupported_workload",
                "unexpected category {category}"
            );
        }
    }
}

#[test]
fn doctor_production_reports_missing_driver_with_code_three() {
    let plan = fixture("minimal.json");
    let plan_str = plan.to_str().unwrap().to_owned();
    for json in [false, true] {
        let owned = if json {
            vec!["doctor".to_owned(), plan_str.clone(), "--json".to_owned()]
        } else {
            vec!["doctor".to_owned(), plan_str.clone()]
        };
        let args = arg_strings(&owned);
        let output = run(&args);
        assert_eq!(output.status.code(), Some(3), "json={json}");
        if json {
            let value: Value = serde_json::from_slice(&output.stdout).expect("one JSON doc");
            assert_eq!(value["ok"], false);
            assert_eq!(value["result"]["has_workload_driver"], false);
        }
    }
}

#[test]
fn corrupt_bundle_inspect_exits_five_in_both_modes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let bundle = tmp.path().join("corrupt.eggb");
    std::fs::create_dir_all(&bundle).expect("mkdir");
    std::fs::write(bundle.join("manifest.json"), "{not json").expect("write");
    let bundle_str = bundle.to_str().unwrap().to_owned();
    for json in [false, true] {
        let owned = if json {
            vec![
                "inspect".to_owned(),
                bundle_str.clone(),
                "--json".to_owned(),
            ]
        } else {
            vec!["inspect".to_owned(), bundle_str.clone()]
        };
        let args = arg_strings(&owned);
        let output = run(&args);
        assert_eq!(output.status.code(), Some(5), "json={json}");
        if json {
            let value: Value = serde_json::from_slice(&output.stdout).expect("one JSON doc");
            assert_eq!(value["ok"], false);
        }
    }
}

#[test]
fn missing_bundle_inspect_exits_five_in_both_modes() {
    for json in [false, true] {
        let tmp = tempfile::tempdir().expect("tempdir");
        let bundle = tmp.path().join("missing.eggb");
        let bundle_str = bundle.to_str().unwrap().to_owned();
        let owned = if json {
            vec![
                "inspect".to_owned(),
                bundle_str.clone(),
                "--json".to_owned(),
            ]
        } else {
            vec!["inspect".to_owned(), bundle_str.clone()]
        };
        let args = arg_strings(&owned);
        let output = run(&args);
        assert_eq!(output.status.code(), Some(5), "json={json}");
        if json {
            let value: Value = serde_json::from_slice(&output.stdout).expect("one JSON doc");
            assert_eq!(value["ok"], false);
        }
    }
}

#[test]
fn compare_missing_bundle_exits_five_in_both_modes() {
    for json in [false, true] {
        let tmp = tempfile::tempdir().expect("tempdir");
        let missing = tmp.path().join("missing.eggb");
        let candidate = tmp.path().join("candidate.eggb");
        let owned = if json {
            vec![
                "compare".to_owned(),
                missing.to_str().unwrap().to_owned(),
                candidate.to_str().unwrap().to_owned(),
                "--json".to_owned(),
            ]
        } else {
            vec![
                "compare".to_owned(),
                missing.to_str().unwrap().to_owned(),
                candidate.to_str().unwrap().to_owned(),
            ]
        };
        let args = arg_strings(&owned);
        let output = run(&args);
        assert_eq!(output.status.code(), Some(5), "json={json}");
        if json {
            let value: Value = serde_json::from_slice(&output.stdout).expect("one JSON doc");
            assert_eq!(value["ok"], false);
        }
    }
}

#[test]
fn compare_without_roles_exits_two_in_both_modes() {
    for json in [false, true] {
        let mut args = vec!["compare"];
        if json {
            args.push("--json");
        }
        let output = run(&args);
        assert_eq!(output.status.code(), Some(2), "json={json}");
    }
}

#[test]
fn json_failures_emit_exactly_one_document() {
    // Invalid plan in JSON mode: stdout must be exactly one envelope.
    let plan = fixture("invalid-cycle.json");
    let output = Command::new(binary())
        .args(["validate", plan.to_str().unwrap(), "--json"])
        .output()
        .expect("spawn");
    assert_eq!(output.status.code(), Some(2));
    let text = String::from_utf8(output.stdout).expect("utf8");
    let value: Value = serde_json::from_str(text.trim()).expect("single JSON doc");
    assert_eq!(value["ok"], false);
    // No second document trailing.
    assert_eq!(
        text.trim().matches('\n').count(),
        text.trim().lines().count() - 1
    );
    let _ = Path::new(".");
}
