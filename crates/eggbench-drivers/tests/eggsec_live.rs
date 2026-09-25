//! Eggstack M004a live Eggsec qualification tests.
//!
//! Parser/argv/scope matrices live next to the adapter as unit tests. This
//! suite drives the real `eggsec` binary (when installed) against the
//! deterministic loopback fixtures in `tests/fixtures/`:
//! `eggsec_safe_fixture.py` (fixed body, zero bypasses, Pass) and
//! `eggsec_permissive_fixture.py` (echoing body, at least one bypass, Fail).
//!
//! Live tests skip gracefully (with a stderr note) when the `eggsec`
//! binary is absent; the missing-binary path itself is a first-class
//! capability error exercised through resolution. The qualification
//! harness (`scripts/qualification/m004a-eggsec/`) builds the exact pinned
//! Eggsec source revision and runs this suite with the binary on `PATH`.

use eggbench_core::{RunId, SecurityCheckResultV1};
use eggbench_drivers::{
    EGGSEC_DRIVER_NAME, EggsecWafExecutor, confine_target_url, eggsec_descriptor,
    generate_scope_manifest, preflight_eggsec, run_guarded_preflight,
};
use eggbench_runner::{
    CorrectnessContext, CorrectnessDisposition, CorrectnessExecutor, CorrectnessOutput,
    RuntimeBindings,
};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

fn binary_present(binary: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| {
            let candidate = dir.join(binary);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                candidate.is_file()
                    && candidate
                        .metadata()
                        .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
            }
            #[cfg(not(unix))]
            {
                candidate.is_file()
            }
        })
    })
}

fn require_tool(binary: &str) -> bool {
    if binary_present(binary) {
        true
    } else {
        eprintln!("skipping live {binary} test: binary not installed");
        false
    }
}

fn find_free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("loopback bind")
        .local_addr()
        .expect("local addr")
        .port()
}

fn wait_for_tcp(port: u16) {
    for _ in 0..50 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("server on port {port} never became reachable");
}

/// Guard that kills the child fixture server on drop.
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn spawn_fixture(name: &str, port: u16) -> ChildGuard {
    let child = Command::new("python3")
        .arg(fixture_path(name))
        .arg(port.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn qualification fixture");
    wait_for_tcp(port);
    ChildGuard(child)
}

fn bindings_for(url: &str) -> RuntimeBindings {
    let mut bindings = RuntimeBindings::new();
    bindings
        .insert("origin", "http_url", url.to_owned())
        .expect("test binding");
    bindings
}

fn check_context(url: &str, check_id: &str) -> CorrectnessContext {
    CorrectnessContext {
        run_id: RunId::new(),
        check_id: check_id.to_owned(),
        source: EGGSEC_DRIVER_NAME.to_owned(),
        target: "origin".to_owned(),
        test_type: "sqli".to_owned(),
        max_successful_bypasses: 0,
        concurrency: 2,
        timeout_ms: 60_000,
        bindings: bindings_for(url),
        cancellation: CancellationToken::new(),
        timeout: Duration::from_secs(90),
    }
}

fn staged_result(output: &CorrectnessOutput) -> SecurityCheckResultV1 {
    let result: SecurityCheckResultV1 =
        serde_json::from_slice(&output.sanitized_result).expect("sanitized result parses");
    result
        .validate_contract()
        .expect("sanitized result satisfies the contract");
    result
}

#[test]
fn eggsec_descriptor_is_correctness_external() {
    let descriptor = eggsec_descriptor();
    assert_eq!(descriptor.name.as_str(), EGGSEC_DRIVER_NAME);
    assert_eq!(
        descriptor.category,
        eggbench_core::DriverCategory::Correctness
    );
    assert!(descriptor.external_process);
    assert!(!descriptor.default);
}

#[test]
fn public_targets_never_reach_the_tool() {
    // No binary is needed: confinement fails before any spawn.
    assert!(confine_target_url("http://8.8.8.8/").is_err());
    assert!(confine_target_url("http://example.com/").is_err());
    assert!(confine_target_url("http://127.0.0.1:8080/bench").is_ok());
}

#[tokio::test]
async fn live_safe_fixture_passes_with_zero_bypasses() {
    if !require_tool("eggsec") {
        return;
    }
    let cancel = CancellationToken::new();
    let (executable, probed) = preflight_eggsec(&cancel)
        .await
        .expect("resolve and probe the live eggsec binary");
    assert!(!probed.version.is_empty());
    let port = find_free_port();
    let url = format!("http://127.0.0.1:{port}/");
    let _server = spawn_fixture("eggsec_safe_fixture.py", port);

    let temp = tempfile::tempdir().expect("tempdir");
    let (scope_bytes, scope_sha) =
        generate_scope_manifest("127.0.0.1").expect("generate loopback scope");
    let scope_path = temp.path().join("live.scope.toml");
    std::fs::write(&scope_path, &scope_bytes).expect("stage scope");
    run_guarded_preflight(&executable, &scope_path, &url, &cancel)
        .await
        .expect("strict guarded preflight allows the in-scope local target");

    let mut executor = EggsecWafExecutor::from_resolved(executable, temp.path().to_path_buf());
    let output = executor
        .execute(check_context(&url, "live-safe"))
        .await
        .expect("safe fixture executes");
    assert_eq!(output.disposition, CorrectnessDisposition::Pass);
    let result = staged_result(&output);
    assert_eq!(result.successful_bypasses, 0);
    assert!(result.evaluated_cases >= 1);
    assert_eq!(result.scope_sha256, scope_sha);
    // The generated scope manifest is executor-managed temporary state:
    // no manifest survives the check.
    let leftovers: Vec<_> = std::fs::read_dir(temp.path())
        .expect("read scope dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "toml"))
        .collect();
    assert!(
        leftovers.iter().all(|entry| entry.path() == scope_path),
        "executor manifests are removed after the check: {leftovers:?}"
    );
}

#[tokio::test]
async fn live_permissive_fixture_fails_with_bypasses() {
    if !require_tool("eggsec") {
        return;
    }
    let cancel = CancellationToken::new();
    let (executable, _probed) = preflight_eggsec(&cancel)
        .await
        .expect("resolve and probe the live eggsec binary");
    let port = find_free_port();
    let url = format!("http://127.0.0.1:{port}/");
    let _server = spawn_fixture("eggsec_permissive_fixture.py", port);

    let temp = tempfile::tempdir().expect("tempdir");
    let (scope_bytes, _) = generate_scope_manifest("127.0.0.1").expect("generate loopback scope");
    let scope_path = temp.path().join("live.scope.toml");
    std::fs::write(&scope_path, &scope_bytes).expect("stage scope");
    run_guarded_preflight(&executable, &scope_path, &url, &cancel)
        .await
        .expect("strict guarded preflight allows the in-scope local target");

    let mut executor = EggsecWafExecutor::from_resolved(executable, temp.path().to_path_buf());
    let output = executor
        .execute(check_context(&url, "live-fail"))
        .await
        .expect("permissive fixture executes");
    // A deliberately permissive fixture yields at least one Eggsec-declared
    // successful bypass: a valid observation with a Fail disposition.
    assert_eq!(output.disposition, CorrectnessDisposition::Fail);
    let result = staged_result(&output);
    assert!(result.successful_bypasses >= 1);
    assert_eq!(result.disposition, eggbench_core::SecurityDisposition::Fail);
    // Sanitized evidence carries digests, never payload bytes.
    let raw = String::from_utf8_lossy(&output.sanitized_result);
    assert!(!raw.contains("echo:"), "no fixture echo leaks");
    for case in &result.sanitized_cases {
        assert_eq!(case.payload_sha256.len(), 64);
    }
}

#[tokio::test]
async fn live_out_of_scope_preflight_denies_without_traffic() {
    if !require_tool("eggsec") {
        return;
    }
    let cancel = CancellationToken::new();
    let (executable, _probed) = preflight_eggsec(&cancel)
        .await
        .expect("resolve and probe the live eggsec binary");
    // Scope allows only loopback; the public target must be denied by the
    // no-network policy preview (no security traffic is ever sent).
    let temp = tempfile::tempdir().expect("tempdir");
    let (scope_bytes, _) = generate_scope_manifest("127.0.0.1").expect("generate loopback scope");
    let scope_path = temp.path().join("live.scope.toml");
    std::fs::write(&scope_path, &scope_bytes).expect("stage scope");
    let denied = run_guarded_preflight(&executable, &scope_path, "http://8.8.8.8/", &cancel).await;
    assert!(denied.is_err(), "out-of-scope target is denied");
    assert!(
        denied
            .unwrap_err()
            .to_string()
            .contains("security_scope_denied"),
        "stable denial category"
    );
}
