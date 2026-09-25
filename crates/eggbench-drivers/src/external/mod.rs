//! External command-driver substrate: trusted resolution, bounded argv
//! execution, version probing, raw artifacts, and a versioned parser contract.
//!
//! Oracles M002 adds the first tool adapters on the substrate: `oha` and
//! `h2load` HTTP load drivers plus the `iperf3` TCP throughput driver.

mod artifact;
mod command;
mod common;
pub mod eggprobe;
pub mod eggreplay;
pub mod eggsec;
mod error;
mod h2load;
mod iperf3;
mod oha;
mod parser;
mod preflight;
mod resolver;
mod version;

pub use artifact::{artifact_candidates, command_metadata_json, workload_output_from_outcome};
pub use command::{
    CapturedStream, ExternalCommandOutcome, ExternalCommandSpec, MAX_STDIN_BYTES, run_command,
};
pub use eggprobe::{
    DIAGNOSTICS_EVIDENCE, EGGPROBE_DRIVER_NAME, EGGPROBE_MACHINE_SCHEMA, EGGPROBE_PARSER_ID,
    EGGPROBE_SUPPORTED_FAMILIES, EGGPROBE_UNSUPPORTED_FAMILIES, EggProbeExecutor, HandshakeProof,
    LoweredTarget, MAX_DIAGNOSTIC_TIMEOUT_MS, ProbeReportParsed, diagnostic_timing_label,
    eggprobe_descriptor, eggprobe_role_label, eggprobe_supported_family_names, handshake_eggprobe,
    lower_target, parse_probe_report, preflight_eggprobe,
};
pub use eggreplay::{
    EGGREPLAY_DRIVER_NAME, EGGREPLAY_PARSER_ID, EggReplayParser, EggReplayWorkload,
    SEMANTIC_REPLAY_EVIDENCE, SemanticReplayEvidence, compute_fixture_identity,
    eggreplay_descriptor, preflight_semantic_replay, semantic_replay_role_label,
};
pub use eggsec::{
    ConfinedTarget, EGGSEC_DRIVER_NAME, EGGSEC_PARSER_ID, EGGSEC_PREFLIGHT_PARSER_ID,
    EGGSEC_SUPPORTED_TEST_TYPES, EGGSEC_UNSUPPORTED_OPERATIONS, EggsecWafExecutor,
    SECURITY_CHECKS_EVIDENCE, confine_target_url, eggsec_descriptor, eggsec_role_label,
    eggsec_supported_test_type_names, generate_scope_manifest, parse_preflight_stdout,
    parse_waf_stdout, preflight_eggsec, run_guarded_preflight, security_timing_label,
    waf_argv_tail,
};
pub use error::{DriverError, ErrorCategory};
pub use h2load::{
    H2LOAD_DRIVER_NAME, H2LOAD_PARSER_ID, H2LOAD_STATUS_ARTIFACT, H2loadParser, H2loadWorkload,
    h2load_descriptor,
};
pub use iperf3::{
    IPERF3_DRIVER_NAME, IPERF3_PARSER_ID, Iperf3Parser, Iperf3Workload, iperf3_descriptor,
};
pub use oha::{
    OHA_DRIVER_NAME, OHA_PARSER_ID, OHA_STATUS_ARTIFACT, OhaParser, OhaWorkload, oha_descriptor,
};
pub use parser::{
    ExternalOutputParser, ExternalParseError, FixtureParsedOutput, FixtureParser,
    ParsedExternalOutput,
};
pub use preflight::{
    executable_path_for, external_binary_present, is_external_workload, probe_external_workload,
};
pub use resolver::{BinaryResolver, ResolvedExecutable};
pub use version::{ToolVersion, VersionProbe, VersionProbeSpec};
