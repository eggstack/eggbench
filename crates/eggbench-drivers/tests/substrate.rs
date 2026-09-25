//! External Oracles M001 substrate qualification: deterministic fixture
//! executable exercises resolver identity, version probing, command
//! lifecycle, bounded capture, parser contract, catalog/CLI regressions,
//! and cleanup semantics.

use eggbench_drivers::external::{
    BinaryResolver, ErrorCategory, ExternalCommandSpec, ExternalOutputParser, FixtureParser,
    VersionProbe, VersionProbeSpec, artifact_candidates, run_command,
};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

fn fixture_exe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_eggbench_fixture"))
}

fn resolved() -> eggbench_drivers::external::ResolvedExecutable {
    BinaryResolver::resolve("eggbench-fixture", Some(&fixture_exe()), None).unwrap()
}

fn command(args: &[&str]) -> ExternalCommandSpec {
    ExternalCommandSpec {
        executable: resolved(),
        args: args.iter().map(OsString::from).collect(),
        cwd: None,
        env: BTreeMap::new(),
        stdin_null: true,
        stdin_bytes: None,
        stdout_limit: 64 * 1024,
        stderr_limit: 64 * 1024,
        timeout: Duration::from_secs(10),
    }
}

fn probe_spec(argv_tail: &[&str], timeout: Duration) -> VersionProbeSpec {
    VersionProbeSpec {
        argv_tail: argv_tail.iter().map(|s| (*s).to_owned()).collect(),
        timeout,
        stdout_limit: 64 * 1024,
        stderr_limit: 64 * 1024,
        parser_id: "eggbench-fixture-version/v1".to_owned(),
    }
}

// ---- Resolver / identity ----

#[test]
fn symlink_canonical_identity_is_recorded() {
    let exe = fixture_exe();
    let dir = tempfile::tempdir().unwrap();
    let link = dir.path().join("tool-link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&exe, &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&exe, &link).unwrap();
    let resolved = BinaryResolver::resolve("tool", Some(&link), None).unwrap();
    assert_eq!(resolved.selected_path, link);
    assert_eq!(resolved.sha256_hex.len(), 64);
    assert!(resolved.file_size > 0);
}

// ---- Version probe ----

#[tokio::test]
async fn version_probe_succeeds() {
    let exe = resolved();
    let version = VersionProbe::run(
        &exe,
        &probe_spec(&["version"], Duration::from_secs(10)),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(version.version, "1.2.3");
    assert_eq!(version.exit_code, Some(0));
    assert_eq!(version.tool, "eggbench-fixture");
}

#[tokio::test]
async fn version_probe_nonzero_exit_fails() {
    let exe = resolved();
    let err = VersionProbe::run(
        &exe,
        &probe_spec(&["exit", "3"], Duration::from_secs(10)),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(err.category(), ErrorCategory::VersionProbeFailed);
}

#[tokio::test]
async fn version_probe_timeout_is_typed() {
    let exe = resolved();
    let err = VersionProbe::run(
        &exe,
        &probe_spec(&["sleep", "5000"], Duration::from_millis(100)),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(err.category(), ErrorCategory::VersionProbeTimeout);
}

// ---- Command lifecycle ----

#[tokio::test]
async fn successful_exit_reports_typed_outcome() {
    let outcome = run_command(&command(&["version"]), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(outcome.exit_code, Some(0));
    assert!(!outcome.stdout.retained().is_empty());
    assert!(!outcome.cancelled && !outcome.timed_out);
}

#[tokio::test]
async fn nonzero_exit_outcome_is_typed() {
    let outcome = run_command(&command(&["exit", "3"]), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(outcome.exit_code, Some(3));
}

#[tokio::test]
async fn timeout_is_typed() {
    let mut spec = command(&["sleep", "5000"]);
    spec.timeout = Duration::from_millis(100);
    let err = run_command(&spec, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(err.category(), ErrorCategory::TimedOut);
}

#[tokio::test]
async fn cancellation_is_typed() {
    let cancel = CancellationToken::new();
    let clone = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        clone.cancel();
    });
    let err = run_command(&command(&["sleep", "5000"]), &cancel)
        .await
        .unwrap_err();
    assert_eq!(err.category(), ErrorCategory::Cancelled);
}

#[tokio::test]
async fn output_larger_than_cap_does_not_deadlock() {
    let mut spec = command(&["stdout-bytes", "200000"]);
    spec.stdout_limit = 4096;
    let outcome = run_command(&spec, &CancellationToken::new()).await.unwrap();
    assert!(outcome.stdout.truncated());
    assert_eq!(outcome.stdout.retained_bytes(), 4096);
    assert!(outcome.stdout.total_bytes() >= 200_000);
    assert!(outcome.stdout.dropped_bytes() > 0);
}

#[tokio::test]
async fn concurrent_stdout_stderr_are_both_drained() {
    let outcome = run_command(
        &command(&["both-bytes", "5000", "5000"]),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.exit_code, Some(0));
    assert_eq!(outcome.stdout.total_bytes(), 5000);
    assert_eq!(outcome.stderr.total_bytes(), 5000);
}

#[cfg(unix)]
#[tokio::test]
async fn unix_descendant_cleanup_through_process_group() {
    let token = CancellationToken::new();
    let clone = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        clone.cancel();
    });
    let err = run_command(&command(&["spawn-child"]), &token)
        .await
        .unwrap_err();
    assert_eq!(err.category(), ErrorCategory::Cancelled);
}

// ---- Parser contract ----

#[tokio::test]
async fn fixture_parser_qualifies_end_to_end() {
    let outcome = run_command(&command(&["version"]), &CancellationToken::new())
        .await
        .unwrap();
    let parsed = FixtureParser.parse(&outcome).unwrap();
    assert_eq!(parsed.tool_version, "1.2.3");

    let malformed = run_command(&command(&["malformed"]), &CancellationToken::new())
        .await
        .unwrap();
    assert!(FixtureParser.parse(&malformed).is_err());
}

// ---- Raw artifacts ----

#[tokio::test]
async fn raw_artifacts_are_deterministic_and_bounded() {
    let outcome = run_command(&command(&["version"]), &CancellationToken::new())
        .await
        .unwrap();
    let artifacts = artifact_candidates(&outcome, Some("eggbench-fixture/v1"));
    assert_eq!(artifacts.len(), 3);
    assert_eq!(artifacts[0].name, "stdout.raw");
    assert_eq!(artifacts[1].name, "stderr.raw");
    assert_eq!(artifacts[2].name, "command-metadata.json");
    for artifact in &artifacts {
        assert!(!artifact.name.contains('/'));
    }
}

// ---- Catalog / CLI regressions ----

#[test]
fn production_catalog_registers_oracles_unconditionally() {
    let catalog = eggbench_drivers::production_catalog();
    // The external-process drivers (oracles plus EggReplay/Eggprobe/Eggsec)
    // register in every build; only the Eggstack-native drivers are
    // feature-gated.
    let mut expected: Vec<String> = Vec::new();
    expected.extend([
        "eggprobe".to_owned(),
        "eggreplay-semantic".to_owned(),
        "eggsec-waf".to_owned(),
        "h2load".to_owned(),
        "iperf3".to_owned(),
        "oha".to_owned(),
    ]);
    #[cfg(feature = "eggstack-http")]
    expected.extend(["eggfetch-http".to_owned(), "eggserve-origin".to_owned()]);
    #[cfg(feature = "eggstack-path")]
    expected.extend(["eggress-route".to_owned(), "eggchaos-stream".to_owned()]);
    #[cfg(feature = "gregg")]
    expected.push("gregg".to_owned());
    expected.sort_unstable();
    let names: Vec<String> = catalog
        .descriptors()
        .iter()
        .map(|d| d.name.as_str().to_owned())
        .collect();
    assert_eq!(names, expected);
    assert_eq!(catalog.len(), expected.len());
}
