//! Shared production driver catalog and external-command substrate.
//!
//! `eggbench-drivers` is the sole production adapter/catalog ownership crate.
//! The CLI owns presentation, not driver implementation or registration.
//!
//! M001 establishes reusable machinery only: trusted executable resolution,
//! binary identity/version probing, argv-only command execution with bounded
//! capture and cancellation, raw-output artifact helpers, and a versioned
//! parser contract. Oracles M002 adds the first tool adapters on that
//! substrate (`oha`, `h2load`, `iperf3`) with no EggServe/Eggfetch/Gregg
//! integration semantics.

#![forbid(unsafe_code)]

mod catalog;
#[cfg(feature = "eggstack-http")]
pub mod eggstack;
pub mod external;
#[cfg(feature = "gregg")]
pub mod gregg;

pub use catalog::{DriverCatalog, production_catalog};
#[cfg(feature = "eggstack-path")]
pub use eggstack::path::{
    EGGCHAOS_CORE_VERSION, EGGRESS_OUTBOUND_VERSION, EGGRESS_URI_VERSION, FAULT_DRIVER_NAME,
    FaultLayer, NETWORK_PATH_EVIDENCE_MAX_BYTES, NETWORK_PATH_EVIDENCE_SCHEMA_VERSION,
    NetworkPathDriverEvidence, NetworkPathEvidence, PathDiagnosticsSnapshot, PathOrdering,
    PathPolicyMode, PathSemantics, ROUTE_DRIVER_NAME, StreamDirection, StreamFaultEvidence,
    build_evidence, build_fault_plan, fault_descriptor, load_network_path_evidence, lower_dialer,
    network_path_role_label, new_path_diagnostics, path_descriptors, redacted_chain_text,
    route_descriptor, validate_request,
};
#[cfg(feature = "eggstack-http")]
pub use eggstack::{
    EGGFETCH_CORE_VERSION, EGGFETCH_HTTP_DRIVER_NAME, EGGSERVE_ORIGIN_SERVICE_TYPE,
    EGGSERVE_PRIMITIVES_VERSION, EGGSERVE_SERVER_VERSION, EGGSTACK_ADAPTER_VERSION,
    EggServeOriginAdapter, EggfetchWorkload, eggfetch_http_descriptor, eggfetch_workload,
    eggserve_origin_descriptor, eggstack_descriptors, eggstack_service_adapters,
};
pub use external::{
    BinaryResolver, ConfinedTarget, DIAGNOSTICS_EVIDENCE, DriverError, EGGPROBE_DRIVER_NAME,
    EGGPROBE_MACHINE_SCHEMA, EGGPROBE_PARSER_ID, EGGPROBE_SUPPORTED_FAMILIES,
    EGGPROBE_UNSUPPORTED_FAMILIES, EGGREPLAY_DRIVER_NAME, EGGREPLAY_PARSER_ID, EGGSEC_DRIVER_NAME,
    EGGSEC_PARSER_ID, EGGSEC_PREFLIGHT_PARSER_ID, EGGSEC_SUPPORTED_TEST_TYPES,
    EGGSEC_UNSUPPORTED_OPERATIONS, EggProbeExecutor, EggReplayParser, EggReplayWorkload,
    EggsecWafExecutor, ErrorCategory, ExternalCommandOutcome, ExternalCommandSpec,
    ExternalOutputParser, H2LOAD_DRIVER_NAME, H2loadParser, H2loadWorkload, HandshakeProof,
    IPERF3_DRIVER_NAME, Iperf3Parser, Iperf3Workload, LoweredTarget, MAX_DIAGNOSTIC_TIMEOUT_MS,
    OHA_DRIVER_NAME, OhaParser, OhaWorkload, ProbeReportParsed, ResolvedExecutable,
    SECURITY_CHECKS_EVIDENCE, SEMANTIC_REPLAY_EVIDENCE, SemanticReplayEvidence, ToolVersion,
    compute_fixture_identity, confine_target_url, diagnostic_timing_label, eggprobe_descriptor,
    eggprobe_role_label, eggprobe_supported_family_names, eggreplay_descriptor, eggsec_descriptor,
    eggsec_role_label, eggsec_supported_test_type_names, executable_path_for,
    external_binary_present, generate_scope_manifest, h2load_descriptor, handshake_eggprobe,
    iperf3_descriptor, is_external_workload, lower_target, oha_descriptor, parse_preflight_stdout,
    parse_probe_report, parse_waf_stdout, preflight_eggprobe, preflight_eggsec,
    preflight_semantic_replay, probe_external_workload, run_guarded_preflight,
    security_timing_label, semantic_replay_role_label, waf_argv_tail,
};
