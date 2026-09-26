//! Security Qualification M002a routine tests (synthetic scope).
//!
//! Exercises the checked-in `qualification/synvoid/v1` profile with the
//! routine-test-only fake subject. No real `SynVoid` checkout is required:
//! live reverse-proxy qualification is a named condition in the M002a
//! closure record, not claimed here.
//!
//! Every test copies the profile workspace to a temporary directory, patches
//! loopback ports to freshly allocated free ports, and drives the real
//! `eggbench` binary with that copy as its working directory.

#![cfg(feature = "eggstack-http")]

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

const EXPECTED_POLICY_ID: &str = "synvoid-qualification-assets.v1";

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_eggbench"))
}

fn profile_source() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../qualification/synvoid/v1")
        .canonicalize()
        .expect("checked-in synvoid v1 profile exists")
}

fn has_python3() -> bool {
    Command::new("python3")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn require_python3() -> bool {
    if has_python3() {
        return true;
    }
    eprintln!("skipping synvoid M002a test: python3 not installed");
    false
}

fn copy_workspace() -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("temporary workspace");
    copy_dir(&profile_source(), temp.path());
    temp
}

fn copy_dir(source: &Path, target: &Path) {
    for entry in std::fs::read_dir(source).expect("read profile dir") {
        let entry = entry.expect("profile dir entry");
        let to = target.join(entry.file_name());
        if entry.file_type().expect("entry type").is_dir() {
            std::fs::create_dir(&to).expect("create workspace subdir");
            copy_dir(&entry.path(), &to);
        } else {
            std::fs::copy(entry.path(), &to).expect("copy profile file");
        }
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("allocate free loopback port")
        .local_addr()
        .expect("listener address")
        .port()
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
/// Corpus content-tree aggregate digest recomputed through normal Eggbench
/// input handling (`qualify expand`), mirroring the plan's negative-proof
/// procedure: mutate the workspace corpus, expand, read the fresh identity.
fn expanded_corpus_digest(workspace: &Path) -> String {
    let expand = qualify(workspace, &["qualify", "expand", "profile.json", "--json"]);
    assert_eq!(
        expand.status.code(),
        Some(0),
        "expand failed: {}",
        String::from_utf8_lossy(&expand.stderr)
    );
    let expansion: Value = serde_json::from_slice(&expand.stdout).expect("expand json");
    expansion["corpus_identity"]["aggregate_sha256"]
        .as_str()
        .expect("corpus aggregate digest")
        .to_owned()
}

/// Point the scenario's fake subject at a free loopback port.
fn patch_subject_port(workspace: &Path, port: u16) {
    let plan_path = workspace.join("scenarios/waf-correctness.json");
    let mut plan = read_json(&plan_path);
    plan["services"][1]["kind"]["argv"] = Value::Array(vec![
        "stubs/fake_synvoid.py".into(),
        "--port".into(),
        port.to_string().into(),
    ]);
    plan["services"][1]["http_url"] = Value::String(format!("http://127.0.0.1:{port}/"));
    write_json(&plan_path, &plan);
    let mut target = read_json(&workspace.join("target-config.json"));
    target["listen"]["port"] = Value::from(port);
    write_json(&workspace.join("target-config.json"), &target);
    let mut provenance = read_json(&workspace.join("materialized/provenance.json"));
    provenance["listen"]["port"] = Value::from(port);
    write_json(&workspace.join("materialized/provenance.json"), &provenance);
}

fn qualify(workspace: &Path, args: &[&str]) -> std::process::Output {
    Command::new(binary())
        .args(args)
        .current_dir(workspace)
        .output()
        .expect("spawn eggbench binary")
}

fn receipt(output_dir: &Path) -> Value {
    read_json(&output_dir.join("qualification-receipt.json"))
}

/// Upstream-manifest verification (plan sections 3, 10) as the routine
/// harness performs it: policy, source pin, digest, and port checks fail
/// closed. Returns `Ok(())` only when every check holds.
fn verify_manifest(
    provenance: &Value,
    plan_http_url: &str,
    corpus_digest: &str,
    expected_source_sha: &str,
) -> Result<(), String> {
    if provenance["policy_id"] != EXPECTED_POLICY_ID {
        return Err("upstream policy identifier mismatch".to_owned());
    }
    if provenance["source_sha"] != expected_source_sha {
        return Err("upstream source SHA mismatch".to_owned());
    }
    if provenance["corpus_digest"] != corpus_digest {
        return Err("exported corpus digest mismatch".to_owned());
    }
    let listen_port = provenance["listen"]["port"]
        .as_u64()
        .ok_or_else(|| "manifest listen port is not a number".to_owned())?;
    let plan_port: u64 = plan_http_url
        .rsplit(':')
        .next()
        .and_then(|port| port.trim_end_matches('/').parse().ok())
        .ok_or_else(|| "scenario http_url has no numeric port".to_owned())?;
    if listen_port != plan_port {
        return Err("static binding port differs from manifest listen port".to_owned());
    }
    Ok(())
}

#[test]
fn profile_validates_and_expands() {
    let workspace = copy_workspace();
    let root = workspace.path();

    let validate = qualify(root, &["qualify", "validate", "profile.json", "--json"]);
    assert_eq!(validate.status.code(), Some(0));
    let value: Value = serde_json::from_slice(&validate.stdout).expect("validate json");
    assert_eq!(value["ok"], true);
    assert_eq!(value["profile_id"], "synvoid-waf-correctness-v1");

    let expand = qualify(root, &["qualify", "expand", "profile.json", "--json"]);
    assert_eq!(expand.status.code(), Some(0));
    let expansion: Value = serde_json::from_slice(&expand.stdout).expect("expand json");
    assert_eq!(expansion["profile_id"], "synvoid-waf-correctness-v1");
    assert_eq!(
        expansion["scenarios"].as_array().expect("scenarios").len(),
        1
    );
    assert_eq!(expansion["scenarios"][0]["id"], "synvoid-waf-correctness");
    assert!(expansion["corpus_identity"]["aggregate_sha256"].is_string());
    assert!(expansion["target_config_identity"]["aggregate_sha256"].is_string());
}

#[test]
fn positive_run_passes_and_leaves_no_subject_behind() {
    if !require_python3() {
        return;
    }
    let workspace = copy_workspace();
    let root = workspace.path();
    let port = free_port();
    patch_subject_port(root, port);

    let output = root.join("suite");
    let run = qualify(
        root,
        &[
            "qualify",
            "run",
            "profile.json",
            "--output",
            "suite",
            "--json",
        ],
    );
    assert_eq!(
        run.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let result = receipt(&output);
    assert_eq!(result["aggregate_verdict"], "pass");
    assert_eq!(result["execution_complete"], true);

    let inspect = qualify(
        root,
        &[
            "qualify",
            "inspect",
            "suite/qualification-receipt.json",
            "--json",
        ],
    );
    assert_eq!(inspect.status.code(), Some(0));

    // Cancellation/teardown left no SynVoid child behind: the fake
    // subject's fixed port is bindable again after the run.
    std::net::TcpListener::bind(format!("127.0.0.1:{port}"))
        .expect("subject port is free after teardown");

    // Portable evidence stays sanitized: no raw attack bytes in the
    // finalized bundle outside bounded service logs.
    let mut leaked = Vec::new();
    let mut stack = vec![output.join("scenarios")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read bundle dir") {
            let entry = entry.expect("bundle entry");
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name != "logs") {
                    stack.push(path);
                }
                continue;
            }
            let bytes = std::fs::read(&path).expect("read bundle file");
            if bytes.windows(8).any(|window| window == b"alert(1)") {
                leaked.push(path);
            }
        }
    }
    assert!(leaked.is_empty(), "raw payload in bundle: {leaked:?}");
}

#[test]
fn mutated_expectation_yields_qualification_fail() {
    if !require_python3() {
        return;
    }
    let workspace = copy_workspace();
    let root = workspace.path();
    patch_subject_port(root, free_port());

    // Alter one expected observable status only, then recompute the
    // temporary corpus identity through normal Eggbench input handling.
    let corpus_path = root.join("corpus.json");
    let mut corpus = read_json(&corpus_path);
    corpus["cases"][0]["expectation"] = serde_json::json!({"status_exact": 404});
    write_json(&corpus_path, &corpus);
    let digest = expanded_corpus_digest(root);
    let plan_path = root.join("scenarios/waf-correctness.json");
    let mut plan = read_json(&plan_path);
    plan["http_corpus_checks"][0]["corpus_sha256"] = Value::String(digest);
    write_json(&plan_path, &plan);

    let run = qualify(
        root,
        &[
            "qualify",
            "run",
            "profile.json",
            "--output",
            "suite",
            "--json",
        ],
    );
    assert_eq!(
        run.status.code(),
        Some(6),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let result = receipt(&root.join("suite"));
    assert_eq!(result["aggregate_verdict"], "fail");
    assert_eq!(result["execution_complete"], true);
}

#[test]
fn unreachable_subject_yields_invalid() {
    if !require_python3() {
        return;
    }
    let workspace = copy_workspace();
    let root = workspace.path();
    // The stub listens on a free port while the static binding points at a
    // different closed port: corpus execution meets a transport failure,
    // which must be Invalid (never a correctness Fail).
    let plan_path = root.join("scenarios/waf-correctness.json");
    let mut plan = read_json(&plan_path);
    let listen = free_port();
    let closed = free_port();
    assert_ne!(listen, closed);
    plan["services"][1]["kind"]["argv"] = Value::Array(vec![
        "stubs/fake_synvoid.py".into(),
        "--port".into(),
        listen.to_string().into(),
    ]);
    plan["services"][1]["http_url"] = Value::String(format!("http://127.0.0.1:{closed}/"));
    write_json(&plan_path, &plan);

    let run = qualify(
        root,
        &[
            "qualify",
            "run",
            "profile.json",
            "--output",
            "suite",
            "--json",
        ],
    );
    assert_eq!(
        run.status.code(),
        Some(8),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let result = receipt(&root.join("suite"));
    assert_eq!(result["aggregate_verdict"], "invalid");
    assert_eq!(result["execution_complete"], false);
}

#[test]
fn tampered_corpus_fails_closed() {
    if !require_python3() {
        return;
    }
    let workspace = copy_workspace();
    let root = workspace.path();
    patch_subject_port(root, free_port());

    // Corrupt the workspace corpus without updating the declared identity:
    // expansion preflight must refuse the input (Invalid, never Pass).
    let corpus_path = root.join("corpus.json");
    let mut raw = std::fs::read(&corpus_path).expect("read corpus");
    raw.extend_from_slice(b" ");
    std::fs::write(&corpus_path, raw).expect("tamper corpus");

    let run = qualify(
        root,
        &[
            "qualify",
            "run",
            "profile.json",
            "--output",
            "suite",
            "--json",
        ],
    );
    assert_eq!(
        run.status.code(),
        Some(8),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let result = receipt(&root.join("suite"));
    assert_eq!(result["aggregate_verdict"], "invalid");
}

#[test]
fn manifest_verification_rejects_mismatch() {
    let workspace = copy_workspace();
    let root = workspace.path();
    let port = free_port();
    patch_subject_port(root, port);

    // Simulate harness post-materialization: bind the recomputed corpus
    // digest into the manifest, then every check must hold.
    let digest = expanded_corpus_digest(root);
    let provenance_path = root.join("materialized/provenance.json");
    let mut provenance = read_json(&provenance_path);
    provenance["corpus_digest"] = Value::String(digest.clone());
    let plan = read_json(&root.join("scenarios/waf-correctness.json"));
    let http_url = plan["services"][1]["http_url"]
        .as_str()
        .expect("static http_url");
    let source_sha = provenance["source_sha"]
        .as_str()
        .expect("source sha")
        .to_owned();

    assert!(verify_manifest(&provenance, http_url, &digest, &source_sha).is_ok());

    let mut wrong_policy = provenance.clone();
    wrong_policy["policy_id"] = Value::String("other-policy".into());
    assert!(verify_manifest(&wrong_policy, http_url, &digest, &source_sha).is_err());

    let mut wrong_digest = provenance.clone();
    wrong_digest["corpus_digest"] = Value::String("0".repeat(64));
    assert!(verify_manifest(&wrong_digest, http_url, &digest, &source_sha).is_err());

    let mut wrong_port = provenance.clone();
    wrong_port["listen"]["port"] = Value::from(port.wrapping_add(1));
    assert!(verify_manifest(&wrong_port, http_url, &digest, &source_sha).is_err());

    assert!(verify_manifest(&provenance, http_url, &digest, "deadbeef").is_err());
}
