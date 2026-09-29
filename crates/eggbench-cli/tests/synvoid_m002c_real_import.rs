//! Security Qualification M002 C002 real-import contract tests.
//!
//! Exercises the harness-only owner-export import layer
//! (`scripts/qualification/synvoid-m002/translate-owner-corpus.py` and
//! `assert-origin-log.py`) against inline owner-shaped fixtures. No real
//! `SynVoid` checkout, export, or live process is required: these tests pin
//! the mechanical translation rules (policy pin, owner-declared
//! Detect/Pass mapping, SP wire-encoding, fail-closed rejections) and the
//! blocked-request proof rule. Live reverse-proxy proof runs in the
//! `live-synvoid-linux` harness, not here.

#![cfg(feature = "eggstack-http")]

use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Command;

const POLICY: &str = "synvoid.eggbench-qualification.v1";

fn scripts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/qualification/synvoid-m002")
        .canonicalize()
        .expect("synvoid-m002 qualification scripts exist")
}

fn has_python3() -> bool {
    Command::new("python3")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn owner_corpus(cases: &Value) -> Value {
    json!({
        "schema_version": "eggbench.security_qualification.corpus.v1",
        "policy_id": POLICY,
        "policy_version": "v1",
        "synvoid_package_version": "1.1.0",
        "generated_by": "test-fixture",
        "listen_port": 18080,
        "origin_port": 18081,
        "case_count": cases.as_array().expect("cases").len(),
        "detect_status": 403,
        "pass_status": 200,
        "cases": cases,
    })
}

fn owner_provenance() -> Value {
    json!({
        "schema_version": "eggbench.security_qualification.provenance.v1",
        "policy_id": POLICY,
        "policy_version": "v1",
        "synvoid_package_version": "1.1.0",
        "synvoid_git_sha": "ae045481752b8f750d6e6079b185c526a09c91d5",
        "materializer": "synvoid-eggbench-qualification-materializer@1.0.0",
        "listen_port": 18080,
        "origin_port": 18081,
        "detect_status": 403,
        "pass_status": 200,
        "source_fixtures": [],
        "excluded_fixtures": [],
        "generated_config_sha256": "0".repeat(64),
        "generated_corpus_sha256": "1".repeat(64),
        "site_config_sha256": "2".repeat(64),
    })
}

fn translate(
    corpus: &Value,
    provenance: &Value,
    policy: &str,
    dir: &std::path::Path,
) -> (bool, String, Option<Value>) {
    let corpus_path = dir.join("owner-corpus.json");
    let provenance_path = dir.join("provenance.json");
    let out_path = dir.join("eggbench-corpus.json");
    std::fs::write(
        &corpus_path,
        serde_json::to_vec_pretty(corpus).expect("corpus"),
    )
    .expect("write");
    std::fs::write(
        &provenance_path,
        serde_json::to_vec_pretty(provenance).expect("provenance"),
    )
    .expect("write");
    let output = Command::new("python3")
        .arg(scripts_dir().join("translate-owner-corpus.py"))
        .arg(&corpus_path)
        .arg(&provenance_path)
        .arg(policy)
        .arg("synvoid-waf-owner-v1")
        .arg("test owner label")
        .arg(&out_path)
        .output()
        .expect("run translator");
    let out = if output.status.success() {
        let parsed: Value =
            serde_json::from_slice(&std::fs::read(&out_path).expect("read translated"))
                .expect("parse translated");
        Some(parsed)
    } else {
        None
    };
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        out,
    )
}

#[test]
fn owner_translation_is_mechanical_and_pinned() {
    if !has_python3() {
        eprintln!("skipping M002c test: python3 not installed");
        return;
    }
    let temp = tempfile::tempdir().expect("temporary dir");
    let corpus = owner_corpus(&json!([
        {"id": "benign_query_strings", "category": "none", "method": "GET",
         "path": "/search", "query_string": "q=coffee",
         "expected_status": 200},
        {"id": "benign_json_body", "category": "none", "method": "POST",
         "path": "/api/users",
         "headers": [["Content-Type", "application/json"]],
         "body": {"inline": "{}"},
         "expected_status": 200},
        {"id": "query_string_proxy_path", "category": "sqli", "method": "GET",
         "path": "/proxy/http://evil.example.com/search?q=SELECT * FROM users",
         "expected_status": 403},
    ]));
    let (ok, stderr, out) = translate(&corpus, &owner_provenance(), POLICY, temp.path());
    assert!(ok, "translator failed: {stderr}");
    let out = out.expect("translated corpus");
    assert_eq!(out["schema_version"], 1);
    assert_eq!(out["corpus_id"], "synvoid-waf-owner-v1");
    assert_eq!(out["cases"].as_array().expect("cases").len(), 3);
    let by_id = |id: &str| {
        out["cases"]
            .as_array()
            .expect("cases")
            .iter()
            .find(|case| case["id"] == id)
            .unwrap_or_else(|| panic!("missing case {id}"))
            .clone()
    };
    let pass = by_id("benign_query_strings");
    assert_eq!(pass["expectation"], json!({"status_exact": 200}));
    assert_eq!(pass["request"]["path_and_query"], "/search?q=coffee");
    let post = by_id("benign_json_body");
    assert_eq!(post["request"]["method"], "POST");
    assert_eq!(
        post["request"]["body"],
        json!({"kind": "inline_utf8", "value": "{}"})
    );
    let detect = by_id("query_string_proxy_path");
    assert_eq!(detect["expectation"], json!({"status_any_of": [403]}));
    // Owner transport rule: raw SP is %20-encoded on the wire.
    assert_eq!(
        detect["request"]["path_and_query"],
        "/proxy/http://evil.example.com/search?q=SELECT%20*%20FROM%20users"
    );
}

#[test]
fn owner_translation_fails_closed() {
    if !has_python3() {
        eprintln!("skipping M002c test: python3 not installed");
        return;
    }
    let temp = tempfile::tempdir().expect("temporary dir");
    // Wrong policy pin.
    let corpus = owner_corpus(&json!([
        {"id": "a", "category": "none", "method": "GET", "path": "/x", "expected_status": 200},
    ]));
    let (ok, _, _) = translate(&corpus, &owner_provenance(), "someone-else.v9", temp.path());
    assert!(!ok, "translator must reject a foreign policy_id");
    // Unknown wire status (neither owner-declared detect nor pass).
    let corpus = owner_corpus(&json!([
        {"id": "a", "category": "none", "method": "GET", "path": "/x", "expected_status": 418},
    ]));
    let (ok, _, _) = translate(&corpus, &owner_provenance(), POLICY, temp.path());
    assert!(!ok, "translator must reject an undeclared expected_status");
    // File bodies are outside the v1 inline-only contract.
    let corpus = owner_corpus(&json!([
        {"id": "a", "category": "none", "method": "POST", "path": "/x",
         "body": {"file": "payload.bin"}, "expected_status": 200},
    ]));
    let (ok, _, _) = translate(&corpus, &owner_provenance(), POLICY, temp.path());
    assert!(!ok, "translator must reject file bodies");
}

#[test]
fn origin_log_proof_rule() {
    if !has_python3() {
        eprintln!("skipping M002c test: python3 not installed");
        return;
    }
    let temp = tempfile::tempdir().expect("temporary dir");
    let corpus = owner_corpus(&json!([
        {"id": "benign_query_strings", "category": "none", "method": "GET",
         "path": "/search", "query_string": "q=coffee", "expected_status": 200},
        {"id": "xss_percent_encoded", "category": "xss", "method": "GET",
         "path": "/search/%3Cscript%3E", "expected_status": 403},
    ]));
    let (_, _, out) = translate(&corpus, &owner_provenance(), POLICY, temp.path());
    let translated = out.expect("translated corpus");
    let translated_path = temp.path().join("translated.json");
    std::fs::write(
        &translated_path,
        serde_json::to_vec(&translated).expect("write"),
    )
    .expect("write");
    let check = |log: &str| {
        let log_path = temp.path().join("origin.log");
        std::fs::write(&log_path, log).expect("write log");
        Command::new("python3")
            .arg(scripts_dir().join("assert-origin-log.py"))
            .arg(&translated_path)
            .arg("403")
            .arg(&log_path)
            .output()
            .expect("run assert-origin-log")
            .status
            .success()
    };
    assert!(
        check("GET /search?q=coffee 200\n"),
        "pass served must prove"
    );
    assert!(
        !check("GET /search/%3Cscript%3E 403\n"),
        "detect reaching origin must fail"
    );
    assert!(!check(""), "missing pass service must fail");
}
