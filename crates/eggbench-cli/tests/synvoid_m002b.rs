//! Security Qualification M002b routine tests (synthetic scope).
//!
//! Exercises the checked-in `qualification/synvoid/v1` smoke and
//! baseline-relative profiles with the routine-test-only fake subject.
//! No real `SynVoid` checkout is required; live qualification is a named
//! condition in the M002 closure record.
//!
//! External-oracle scenarios run through the documented `run
//! --workload-driver` + `compare` procedure (qualify pins no
//! per-scenario driver; see v1 README deviation D4) and skip gracefully
//! when the tool binary is absent.

#![cfg(feature = "eggstack-http")]

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

const PERF_SCENARIOS: [&str; 8] = [
    "perf-small-c1",
    "perf-small-c8",
    "perf-small-c32",
    "perf-large-c1",
    "perf-large-c8",
    "perf-large-c32",
    "control-small",
    "control-large",
];

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_eggbench"))
}

fn profile_source() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../qualification/synvoid/v1")
        .canonicalize()
        .expect("checked-in synvoid v1 profile exists")
}

fn has_tool(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn require_tool(name: &str) -> bool {
    if has_tool(name) {
        return true;
    }
    eprintln!("skipping synvoid M002b test step: {name} not installed");
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

fn eggbench(workspace: &Path, args: &[&str]) -> std::process::Output {
    Command::new(binary())
        .args(args)
        .current_dir(workspace)
        .output()
        .expect("spawn eggbench binary")
}

/// Point every fake subject in the workspace at one free loopback port.
/// Scenarios execute serially, so one port serves the whole profile.
fn patch_subject_ports(workspace: &Path, port: u16) {
    let scenarios = workspace.join("scenarios");
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
        let mut changed = false;
        for service in plan["services"].as_array_mut().expect("services") {
            if service["name"] == "synvoid" {
                service["kind"]["argv"] = Value::Array(vec![
                    "stubs/fake_synvoid.py".into(),
                    "--port".into(),
                    port.to_string().into(),
                ]);
                let url = service["http_url"].as_str().expect("static http_url");
                let host = url
                    .split('/')
                    .nth(2)
                    .expect("authority")
                    .split(':')
                    .next()
                    .expect("host");
                let route = format!("/{}", url.splitn(4, '/').nth(3).unwrap_or(""));
                service["http_url"] = Value::String(format!("http://{host}:{port}{route}"));
                changed = true;
            }
        }
        if changed {
            write_json(&path, &plan);
        }
    }
    for name in ["target-config.json", "materialized/provenance.json"] {
        let path = workspace.join(name);
        let mut document = read_json(&path);
        document["listen"]["port"] = Value::from(port);
        write_json(&path, &document);
    }
}

/// Stage A of the baseline workflow: run every perf scenario into
/// `baselines/` from the accepted (here: synthetic) revision.
fn materialize_baselines(workspace: &Path) {
    for scenario in PERF_SCENARIOS {
        let output = eggbench(
            workspace,
            &[
                "run",
                &format!("scenarios/{scenario}.json"),
                &format!("baselines/{scenario}.eggb"),
                "--json",
            ],
        );
        assert_eq!(
            output.status.code(),
            Some(0),
            "{scenario} baseline failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn receipt(output_dir: &Path) -> Value {
    read_json(&output_dir.join("qualification-receipt.json"))
}

fn expanded_corpus_digest(workspace: &Path) -> String {
    let expand = eggbench(workspace, &["qualify", "expand", "profile.json", "--json"]);
    assert_eq!(expand.status.code(), Some(0));
    let expansion: Value = serde_json::from_slice(&expand.stdout).expect("expand json");
    expansion["corpus_identity"]["aggregate_sha256"]
        .as_str()
        .expect("corpus aggregate digest")
        .to_owned()
}

#[test]
fn smoke_profile_passes_with_absolute_gates() {
    if !require_tool("python3") {
        return;
    }
    let workspace = copy_workspace();
    let root = workspace.path();
    patch_subject_ports(root, free_port());

    let run = eggbench(
        root,
        &[
            "qualify",
            "run",
            "smoke.profile.json",
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
    let result = receipt(&root.join("suite"));
    assert_eq!(result["aggregate_verdict"], "pass");
    assert_eq!(result["execution_complete"], true);
    assert_eq!(result["scenarios"].as_array().expect("scenarios").len(), 5);

    // Direct-origin controls are distinct subjects: their candidate plans
    // target the adapter service, never the SynVoid proxy.
    for scenario in [
        "origin-benign-small-native-control",
        "origin-large-response-native-control",
    ] {
        let record = result["scenarios"]
            .as_array()
            .expect("scenarios")
            .iter()
            .find(|record| record["id"] == scenario)
            .expect("control record");
        assert_eq!(record["combined_verdict"], "pass");
        let bundle = std::fs::read(
            root.join("suite")
                .join(
                    record["candidate_bundle_path"]
                        .as_str()
                        .expect("bundle path"),
                )
                .join("plan.json"),
        )
        .expect("read control plan");
        let plan: Value = serde_json::from_slice(&bundle).expect("parse control plan");
        assert_eq!(plan["workload"]["target"], "origin");
    }
}

#[test]
fn perf_same_source_pair_never_fails() {
    if !require_tool("python3") {
        return;
    }
    let workspace = copy_workspace();
    let root = workspace.path();
    patch_subject_ports(root, free_port());
    materialize_baselines(root);

    let run = eggbench(
        root,
        &[
            "qualify",
            "run",
            "perf.profile.json",
            "--output",
            "suite",
            "--json",
        ],
    );
    let verdict: Value = serde_json::from_slice(&run.stdout).expect("run json");
    // Same-build noise may yield Inconclusive; it must never yield Fail.
    // (Live-host repeatability remains an M002 closure condition.)
    assert!(
        run.status.code() == Some(0) || run.status.code() == Some(7),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let result = receipt(&root.join("suite"));
    assert_eq!(result["execution_complete"], true);
    assert!(
        verdict["aggregate_verdict"] == "pass" || verdict["aggregate_verdict"] == "inconclusive"
    );
    for record in result["scenarios"].as_array().expect("scenarios") {
        assert_eq!(record["status"], "completed", "scenario {}", record["id"]);
        // Every baseline-relative scenario froze the materialized identity.
        if record["id"] != "synvoid-waf-correctness" {
            assert!(
                record["baseline_bundle_identity"].is_object(),
                "scenario {} has no frozen baseline",
                record["id"]
            );
            assert!(record["comparison_receipt_sha256"].is_string());
        }
        let combined = record["combined_verdict"]
            .as_str()
            .expect("combined verdict");
        assert!(
            combined == "pass" || combined == "inconclusive",
            "scenario {} unexpectedly failed",
            record["id"]
        );
    }
}

#[test]
fn correctness_only_regression_fails_suite_despite_perf_pass() {
    if !require_tool("python3") {
        return;
    }
    let workspace = copy_workspace();
    let root = workspace.path();
    patch_subject_ports(root, free_port());
    materialize_baselines(root);

    // Mutate only the owner-authored corpus expectation, then recompute
    // the temporary corpus identity through normal input handling.
    let corpus_path = root.join("corpus.json");
    let mut corpus = read_json(&corpus_path);
    corpus["cases"][0]["expectation"] = serde_json::json!({"status_exact": 404});
    write_json(&corpus_path, &corpus);
    let digest = expanded_corpus_digest(root);
    let plan_path = root.join("scenarios/waf-correctness.json");
    let mut plan = read_json(&plan_path);
    plan["http_corpus_checks"][0]["corpus_sha256"] = Value::String(digest);
    write_json(&plan_path, &plan);

    let run = eggbench(
        root,
        &[
            "qualify",
            "run",
            "perf.profile.json",
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
    let waf = result["scenarios"]
        .as_array()
        .expect("scenarios")
        .iter()
        .find(|record| record["id"] == "synvoid-waf-correctness")
        .expect("waf record");
    assert_eq!(waf["correctness_verdict"], "fail");
}

#[test]
fn performance_only_regression_fails_suite_despite_correctness_pass() {
    if !require_tool("python3") {
        return;
    }
    let workspace = copy_workspace();
    let root = workspace.path();
    let port = free_port();
    patch_subject_ports(root, port);
    materialize_baselines(root);

    // Qualification-only controlled delay in the subject harness: the
    // scenario plans (and therefore comparison identities) are untouched,
    // and WAF block/pass semantics are identical with any delay.
    let sidecar = std::env::temp_dir().join(format!("fake-synvoid-{port}.delay_ms"));
    std::fs::write(&sidecar, "100").expect("write delay sidecar");
    let run = eggbench(
        root,
        &[
            "qualify",
            "run",
            "perf.profile.json",
            "--output",
            "suite",
            "--json",
        ],
    );
    std::fs::remove_file(&sidecar).ok();
    assert_eq!(
        run.status.code(),
        Some(6),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let result = receipt(&root.join("suite"));
    assert_eq!(result["aggregate_verdict"], "fail");
    let waf = result["scenarios"]
        .as_array()
        .expect("scenarios")
        .iter()
        .find(|record| record["id"] == "synvoid-waf-correctness")
        .expect("waf record");
    assert_eq!(waf["correctness_verdict"], "pass");
    let perf_failed = result["scenarios"]
        .as_array()
        .expect("scenarios")
        .iter()
        .filter(|record| record["performance_verdict"] == "fail")
        .count();
    assert!(perf_failed >= 1, "no performance gate failed");
}

#[test]
fn workload_drift_compares_as_incomparable() {
    if !require_tool("python3") {
        return;
    }
    let workspace = copy_workspace();
    let root = workspace.path();
    patch_subject_ports(root, free_port());
    materialize_baselines(root);

    // Candidate intentionally changes workload semantics: the existing
    // profile must become incomparable rather than a performance result.
    let plan_path = root.join("scenarios/perf-small-c8.json");
    let mut plan = read_json(&plan_path);
    plan["workload"]["concurrency"] = Value::from(16);
    write_json(&plan_path, &plan);

    let run = eggbench(
        root,
        &[
            "qualify",
            "run",
            "perf.profile.json",
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
    let drifted = result["scenarios"]
        .as_array()
        .expect("scenarios")
        .iter()
        .find(|record| record["id"] == "synvoid-benign-small-native-c8")
        .expect("drifted record");
    // An incomparable comparison is trustworthy evidence of drift: the
    // scenario completes with an Invalid combined verdict (only failed
    // runs map to `invalid` status), and the suite aggregates Invalid.
    assert_eq!(drifted["status"], "completed");
    assert_eq!(drifted["combined_verdict"], "invalid");
}

#[test]
fn external_oracle_procedure_observes_proxy() {
    if !require_tool("python3") {
        return;
    }
    for (tool, scenario) in [("oha", "oracle-oha-c8"), ("h2load", "oracle-h2load-c8")] {
        if !require_tool(tool) {
            continue;
        }
        let workspace = copy_workspace();
        let root = workspace.path();
        patch_subject_ports(root, free_port());

        for (plan, bundle) in [
            (
                format!("scenarios/{scenario}.json"),
                "oracle-base.eggb".to_owned(),
            ),
            (
                format!("scenarios/{scenario}.json"),
                "oracle-candidate.eggb".to_owned(),
            ),
        ] {
            let run = eggbench(
                root,
                &["run", &plan, &bundle, "--workload-driver", tool, "--json"],
            );
            assert_eq!(
                run.status.code(),
                Some(0),
                "{tool} run failed: {}",
                String::from_utf8_lossy(&run.stderr)
            );
        }
        let compare = eggbench(
            root,
            &[
                "compare",
                "oracle-base.eggb",
                "oracle-candidate.eggb",
                "--json",
            ],
        );
        // Same-source oracle pairs are Pass under certainty and
        // Inconclusive under statistical uncertainty; both prove the
        // independent driver observes the proxy through its own baseline.
        assert!(
            compare.status.code() == Some(0) || compare.status.code() == Some(7),
            "{tool} compare failed: {}",
            String::from_utf8_lossy(&compare.stderr)
        );
        let envelope: Value = serde_json::from_slice(&compare.stdout).expect("compare json");
        assert!(envelope["result"]["aggregate_verdict"].is_string());
        // The independent driver's trial observations are present,
        // driver-owned, and verdict-independent: the candidate bundle's
        // retained per-trial metrics name the tool as producer.
        let trial_metrics = read_json(
            &root
                .join("oracle-candidate.eggb")
                .join("trials/001/metrics.json"),
        );
        let produced: Vec<(&str, &str)> = trial_metrics["observations"]
            .as_array()
            .expect("trial observations")
            .iter()
            .filter_map(|observation| {
                Some((
                    observation["name"].as_str()?,
                    observation["provenance"]["producer"].as_str()?,
                ))
            })
            .collect();
        assert!(
            produced
                .iter()
                .any(|(name, producer)| *name == "throughput" && *producer == tool),
            "{tool} observations: {produced:?}"
        );
        assert!(
            produced
                .iter()
                .any(|(name, producer)| *name == "error_rate" && *producer == tool),
            "{tool} observations: {produced:?}"
        );
    }
}
