//! Schema-v3 network-path validation, compatibility, and resolution tests.

use eggbench_core::{
    ArtifactBounds, ArtifactPath, ArtifactRole, BaselineReference, BaselineSide, BundleReader,
    BundleWriter, Capability, ComparisonOptions, ComparisonRequest, DefaultDriverPolicy,
    DriverCategory, DriverDescriptor, EnvironmentFingerprint, ExecutionStatus, ExperimentPlan,
    LoadMode, Name, PairedArm, PairedDesign, PlanError, PositiveCount, ResolutionOptions,
    ResolvedPlan, RouteMode, RunId, Sensitivity, Subject, compare, load_comparison_input,
    validate_resolved_plan_bytes,
};
use sha2::Digest;
use std::collections::{BTreeMap, BTreeSet};

fn path_plan() -> ExperimentPlan {
    ExperimentPlan::from_json(include_str!("fixtures/eggstack-path.json")).expect("valid path plan")
}

fn validation_category(error: &PlanError) -> &str {
    match error {
        PlanError::Validation { category, .. } => category,
        _ => "non_validation",
    }
}

fn set_chain(plan: &mut ExperimentPlan, chain: &str) {
    let path = plan.network_path.as_mut().expect("network path");
    path.route.mode = RouteMode::ProxyChain {
        chain: chain.to_owned(),
    };
}

fn descriptor(
    name: &str,
    category: DriverCategory,
    capabilities: BTreeSet<Capability>,
    compatible_service_types: &[&str],
) -> DriverDescriptor {
    DriverDescriptor {
        name: Name::new(name).expect("descriptor name"),
        adapter_version: "adapter-v1".to_owned(),
        upstream_name: match category {
            DriverCategory::Route => "eggress-outbound".to_owned(),
            DriverCategory::Fault => "eggchaos-core".to_owned(),
            DriverCategory::Workload => "eggfetch-core".to_owned(),
            _ => format!("{name}-upstream"),
        },
        upstream_version: Some("1.2.3".to_owned()),
        category,
        capabilities,
        supported_platforms: BTreeSet::new(),
        machine_output_schema: None,
        external_process: false,
        default: true,
        compatible_service_types: compatible_service_types
            .iter()
            .map(|name| Name::new(*name).expect("service type"))
            .collect(),
    }
}

fn path_descriptors(include_fault: bool) -> Vec<DriverDescriptor> {
    let mut descriptors = vec![
        descriptor(
            "eggfetch-http",
            DriverCategory::Workload,
            BTreeSet::from([
                Capability::LoadMode {
                    mode: LoadMode::ClosedLoop,
                },
                Capability::NetworkPath,
            ]),
            &["eggserve-origin"],
        ),
        descriptor(
            "eggress-route",
            DriverCategory::Route,
            BTreeSet::from([Capability::ProxyRouting]),
            &[],
        ),
        descriptor(
            "eggserve-origin",
            DriverCategory::Service,
            BTreeSet::new(),
            &[],
        ),
    ];
    if include_fault {
        descriptors.push(descriptor(
            "eggchaos-stream",
            DriverCategory::Fault,
            BTreeSet::from([Capability::StreamFaultPlan]),
            &[],
        ));
    }

    descriptors
}

fn options() -> ResolutionOptions {
    ResolutionOptions {
        selections: BTreeMap::new(),
        default_policy: DefaultDriverPolicy::Deterministic,
        platform: Name::new("linux-x86_64").expect("linux-x86_64"),
        executable_paths: BTreeMap::new(),
        required_capabilities: BTreeMap::new(),
    }
}

fn canonical_chain_for_evidence(chain: &str) -> String {
    chain
        .split("__")
        .map(|hop| hop.replace("socks4a://", "socks4://"))
        .collect::<Vec<_>>()
        .join("__")
}

fn path_evidence(resolved: &ResolvedPlan) -> serde_json::Value {
    let path = resolved.network_path.as_ref().expect("resolved path");
    let route = &path.route_driver.descriptor;
    let (redacted_chain, chain_config_digest, configured_hop_count) = match &path.route.mode {
        RouteMode::Direct => (serde_json::Value::Null, serde_json::Value::Null, 0),
        RouteMode::ProxyChain { chain } => {
            let canonical = canonical_chain_for_evidence(chain);
            (
                serde_json::json!(canonical),
                serde_json::json!(format!("{:x}", sha2::Sha256::digest(canonical.as_bytes()))),
                canonical.split("__").count(),
            )
        }
    };
    let (fault_driver, stream_faults) = match &path.stream_faults {
        Some(faults) => {
            let driver = &faults.fault_driver.descriptor;
            let active =
                !faults.request.upstream.is_empty() || !faults.request.downstream.is_empty();
            (
                serde_json::json!({
                    "name": driver.name,
                    "adapter_version": driver.adapter_version,
                    "upstream_name": driver.upstream_name,
                    "upstream_version": driver.upstream_version
                }),
                serde_json::json!({
                    "request": faults.request,
                    "seed_namespace": active.then_some(resolved.seed).flatten(),
                    "rng_version": faults.rng_version
                }),
            )
        }
        None => (serde_json::Value::Null, serde_json::Value::Null),
    };
    serde_json::json!({
        "schema_version": 1,
        "adapter_version": route.adapter_version,
        "route_driver": {
            "name": route.name,
            "adapter_version": route.adapter_version,
            "upstream_name": route.upstream_name,
            "upstream_version": route.upstream_version
        },
        "eggress_outbound_version": route.upstream_version,
        "eggress_uri_version": "1.0.10",
        "fault_driver": fault_driver,
        "semantics": {
            "ordering_version": path.semantics_version,
            "ordering": "route_first_fault_second",
            "fault_layer": "user_space_stream",
            "upstream": "client_to_target",
            "downstream": "target_to_client"
        },
        "route": path.route,
        "redacted_chain": redacted_chain,
        "chain_config_digest": chain_config_digest,
        "configured_hop_count": configured_hop_count,
        "stream_faults": stream_faults,
        "diagnostics": {
            "physical_dial_attempts": 0,
            "successful_dials": 0,
            "fault_wrapped_connections": 0,
            "fault_wrapper_construction_failures": 0,
            "route_failure_buckets_dropped": 0,
            "route_failures": {},
            "hop_count_distribution": {},
            "max_observed_hop_count": 0,
            "connection_ordinal_min": 0,
            "connection_ordinal_max": 0,
            "connection_ordinal_count": 0
        },
        "policy_mode": "static"
    })
}

fn write_path_bundle(
    root: &std::path::Path,
    name: &str,
    plan: &ExperimentPlan,
    descriptors: &[DriverDescriptor],
) -> BundleReader {
    let resolved = eggbench_core::resolve_plan(plan, descriptors, &options()).expect("resolve");
    let evidence = path_evidence(&resolved);
    let bounds = ArtifactBounds {
        artifact_count: PositiveCount::new(16).expect("artifact count"),
        artifact_bytes: 1024 * 1024,
        total_bytes: 4 * 1024 * 1024,
    };
    let destination = root.join(format!("{name}.eggb"));
    let mut writer = BundleWriter::create(&destination, RunId::new(), bounds).expect("writer");
    for (path, role, bytes) in [
        (
            "plan.json",
            ArtifactRole::ExperimentPlan,
            serde_json::to_vec(plan).expect("plan bytes"),
        ),
        (
            "resolved-plan.json",
            ArtifactRole::ResolvedPlan,
            serde_json::to_vec(&resolved).expect("resolved bytes"),
        ),
        (
            "environment.json",
            ArtifactRole::EnvironmentFingerprint,
            serde_json::to_vec(&EnvironmentFingerprint::new(BTreeMap::new()))
                .expect("environment bytes"),
        ),
        (
            "network-path.json",
            ArtifactRole::Other {
                label: Name::new("network-path").expect("role"),
            },
            serde_json::to_vec(&evidence).expect("evidence bytes"),
        ),
    ] {
        writer
            .add_artifact(
                ArtifactPath::new(path).expect("artifact path"),
                role,
                "application/json",
                Sensitivity::Redacted,
                bytes.as_slice(),
            )
            .expect("artifact");
    }
    writer
        .finalize(
            ExecutionStatus::Completed,
            None,
            Subject::Label {
                label: Name::new("path").expect("subject"),
            },
            Vec::new(),
            Vec::new(),
            None,
            None,
        )
        .expect("finalize")
}

fn assert_path_bundle_mismatch(
    name: &str,
    baseline_plan: &ExperimentPlan,
    baseline_descriptors: &[DriverDescriptor],
    candidate_plan: &ExperimentPlan,
    candidate_descriptors: &[DriverDescriptor],
) {
    let temp = tempfile::tempdir().expect("tempdir");
    let baseline = load_comparison_input(&write_path_bundle(
        temp.path(),
        "baseline",
        baseline_plan,
        baseline_descriptors,
    ))
    .expect("baseline input");
    let candidate = load_comparison_input(&write_path_bundle(
        temp.path(),
        "candidate",
        candidate_plan,
        candidate_descriptors,
    ))
    .expect("candidate input");
    let receipt = compare(
        &ComparisonRequest {
            candidate: &candidate,
            baseline: Some(BaselineSide {
                reference: BaselineReference::Bundle {
                    identity: baseline.identity.clone(),
                    path: "baseline.eggb".to_owned(),
                },
                input: &baseline,
            }),
        },
        &ComparisonOptions::default(),
    );
    assert!(receipt.comparability.critical_mismatch, "{name}");
    assert!(candidate.network_path_evidence.is_some(), "{name}");
    assert!(baseline.network_path_evidence.is_some(), "{name}");
}

#[test]
fn schema_v3_path_with_explicit_seed_and_native_chain_is_valid() {
    path_plan().validate().expect("schema-v3 path validates");
}

#[test]
fn credential_and_ambiguous_route_syntax_is_rejected() {
    for chain in [
        "trojan://secret@proxy.example:443",
        "http://user%3Apass@proxy.example:8080",
        "http://proxy.example:8080?token=secret",
        "http://proxy.example:8080#token",
    ] {
        let mut plan = path_plan();
        set_chain(&mut plan, chain);
        let error = plan.validate().expect_err("credential route must fail");
        assert_eq!(
            validation_category(&error),
            "route_credentials_not_supported"
        );
    }
}

#[test]
fn route_debug_never_emits_chain_text() {
    let mode = RouteMode::ProxyChain {
        chain: "trojan://secret@proxy.example:443".to_owned(),
    };
    let debug = format!("{mode:?}");
    assert!(!debug.contains("secret"));
    assert!(debug.contains("REDACTED"));
}

#[test]
fn bracketed_ipv6_proxy_routes_are_accepted_and_lowered_later() {
    let mut plan = path_plan();
    set_chain(&mut plan, "http://[::1]:8080");
    plan.validate().expect("IPv6 proxy route validates");
}

#[test]
fn credential_bearing_service_config_is_rejected_for_path_plans() {
    for (key, value) in [
        ("proxy_url", "http://user:password@proxy.example:8080"),
        ("x", "a2Vycl9zZWNyZXQ="),
        ("api_token", "secret-marker"),
    ] {
        let mut plan = path_plan();
        plan.services[0].config.insert(
            Name::new(key).expect("config name").to_string(),
            value.to_owned(),
        );
        let error = plan
            .validate()
            .expect_err("service config credentials must fail");
        assert_eq!(
            validation_category(&error),
            "route_credentials_not_supported"
        );
        assert!(!format!("{error:?}").contains("secret-marker"));
    }
}

#[test]
fn malformed_and_extended_route_protocols_are_rejected() {
    for (chain, category) in [
        ("proxy.example:8080", "invalid_route"),
        ("ssh://proxy.example:22", "unsupported_route"),
        ("quic+http://proxy.example:443", "unsupported_route"),
        ("http://proxy.example:0", "invalid_route"),
    ] {
        let mut plan = path_plan();
        set_chain(&mut plan, chain);
        let error = plan.validate().expect_err("unsupported route must fail");
        assert_eq!(validation_category(&error), category);
    }
}

#[test]
fn invalid_fault_plan_has_stable_preflight_category() {
    let mut plan = path_plan();
    plan.network_path
        .as_mut()
        .expect("path")
        .stream_faults
        .as_mut()
        .expect("faults")
        .upstream[0]
        .kind = eggbench_core::StreamFaultKind::Slice {
        average_size: PositiveCount::new(8).expect("average"),
        variation: 8,
        delay_ms: eggbench_core::DurationMs::new(1).expect("delay"),
    };
    let error = plan.validate().expect_err("slice bound must fail");
    assert_eq!(validation_category(&error), "invalid_fault_plan");
}

#[test]
fn nonempty_faults_require_an_explicit_seed() {
    let mut plan = path_plan();
    plan.seed = None;
    let error = plan.validate().expect_err("faults require seed");
    assert_eq!(validation_category(&error), "missing_fault_seed");
}

#[test]
fn external_subject_and_paired_path_are_rejected() {
    let mut external = path_plan();
    external.subject = Subject::External {
        target: Name::new("origin").expect("target"),
        revision: None,
        digest: None,
    };
    let error = external.validate().expect_err("external path must fail");
    assert_eq!(validation_category(&error), "workload_path_incompatible");

    let mut paired = path_plan();
    paired.paired = Some(PairedDesign {
        baseline: PairedArm {
            service: Name::new("origin").expect("service"),
            subject: Subject::Label {
                label: Name::new("baseline").expect("label"),
            },
        },
        candidate: PairedArm {
            service: Name::new("candidate").expect("service"),
            subject: Subject::Label {
                label: Name::new("candidate").expect("label"),
            },
        },
    });
    let error = paired.validate().expect_err("paired path must fail");
    assert_eq!(
        validation_category(&error),
        "paired_network_path_not_supported"
    );
}

#[test]
fn schema_v3_preserves_paired_without_network_path() {
    let mut plan = path_plan();
    plan.network_path = None;
    let mut candidate = plan.services[0].clone();
    candidate.name = Name::new("candidate").expect("service");
    plan.services.push(candidate);
    plan.paired = Some(PairedDesign {
        baseline: PairedArm {
            service: Name::new("origin").expect("service"),
            subject: Subject::Label {
                label: Name::new("baseline").expect("label"),
            },
        },
        candidate: PairedArm {
            service: Name::new("candidate").expect("service"),
            subject: Subject::Label {
                label: Name::new("candidate").expect("label"),
            },
        },
    });
    plan.trials.measured = PositiveCount::new(2).expect("trials");
    plan.validate()
        .expect("v3 paired without path remains valid");
}

#[test]
fn legacy_source_plans_reject_network_path() {
    for version in [1, 2] {
        let mut value = serde_json::to_value(path_plan()).expect("path plan JSON");
        value["schema_version"] = serde_json::json!(version);
        let plan: ExperimentPlan =
            serde_json::from_value(value).unwrap_or_else(|error| panic!("v{version}: {error}"));
        let error = plan
            .validate()
            .expect_err("legacy network_path must be rejected");
        assert_eq!(validation_category(&error), "unsupported_option");
    }

    let mut value = serde_json::to_value(path_plan()).expect("path plan JSON");
    value["schema_version"] = serde_json::json!(1);
    value["network_path"] = serde_json::Value::Null;
    assert!(
        serde_json::from_value::<ExperimentPlan>(value).is_err(),
        "legacy network_path must be omitted rather than null"
    );
}

#[test]
fn duplicate_and_oversized_fault_sets_are_rejected() {
    let mut duplicate = path_plan();
    let faults = duplicate
        .network_path
        .as_mut()
        .expect("path")
        .stream_faults
        .as_mut()
        .expect("faults");
    let repeated = faults.upstream[0].clone();
    faults.upstream.push(repeated);
    let error = duplicate
        .validate()
        .expect_err("duplicate fault identity must fail");
    assert_eq!(validation_category(&error), "invalid_fault_plan");
    assert!(error.to_string().contains("is duplicated"));

    let mut oversized = path_plan();
    let faults = oversized
        .network_path
        .as_mut()
        .expect("path")
        .stream_faults
        .as_mut()
        .expect("faults");
    let repeated = faults.upstream[0].clone();
    faults.upstream.extend(std::iter::repeat_n(repeated, 128));
    let error = oversized
        .validate()
        .expect_err("129 upstream faults must fail");
    assert_eq!(validation_category(&error), "invalid_fault_plan");
}

#[test]
fn v1_and_v2_serialization_omit_absent_network_path() {
    for version in [1, 2] {
        let mut plan = path_plan();
        plan.network_path = None;
        plan.schema_version = eggbench_core::SchemaVersion(version);
        let json = plan.to_json().expect("legacy plan serializes");
        assert!(!json.contains("network_path"), "v{version}: {json}");
    }
}

#[test]
fn resolved_plan_schema_v1_v2_and_v3_remain_readable() {
    let current = include_str!("fixtures/sample-resolved-plan.json");
    for version in [1, 2, 3] {
        let json = current.replacen(
            "\"schema_version\": 3",
            &format!("\"schema_version\": {version}"),
            1,
        );
        validate_resolved_plan_bytes(json.as_bytes())
            .unwrap_or_else(|error| panic!("resolved v{version} must remain readable: {error}"));
    }
}

#[test]
fn legacy_resolved_versions_reject_network_path_and_malformed_contract() {
    let resolved = eggbench_core::resolve_plan(&path_plan(), &path_descriptors(true), &options())
        .expect("path resolves");
    let mut value = serde_json::to_value(&resolved).expect("resolved JSON");
    value["network_path"]["route"]["mode"] = serde_json::json!({
        "kind": "proxy_chain",
        "chain": "trojan://secret@proxy.example:443"
    });
    let bytes = serde_json::to_vec(&value).expect("resolved JSON");
    assert!(validate_resolved_plan_bytes(&bytes).is_err());

    let resolved = eggbench_core::resolve_plan(&path_plan(), &path_descriptors(true), &options())
        .expect("path resolves");
    for version in [1, 2] {
        let mut value = serde_json::to_value(&resolved).expect("resolved JSON");
        value["schema_version"] = serde_json::json!(version);
        value["source_plan_schema_version"] = serde_json::json!(version);
        let bytes = serde_json::to_vec(&value).expect("legacy JSON");
        assert!(validate_resolved_plan_bytes(&bytes).is_err());
    }
}

#[test]
fn resolution_selects_exact_route_and_fault_drivers() {
    let resolved = eggbench_core::resolve_plan(&path_plan(), &path_descriptors(true), &options())
        .expect("path resolves");
    let path = resolved.network_path.expect("resolved path");
    assert_eq!(path.route_driver.descriptor.name.as_str(), "eggress-route");
    let faults = path.stream_faults.expect("resolved faults");
    assert_eq!(
        faults.fault_driver.descriptor.name.as_str(),
        "eggchaos-stream"
    );
    assert_eq!(faults.rng_version, "splitmix64-v1");
    assert_eq!(path.semantics_version, "route-first-fault-second-v1");
}

#[test]
fn resolution_fails_closed_without_path_capability_or_fault_driver() {
    let mut descriptors = path_descriptors(true);
    descriptors[0].capabilities.remove(&Capability::NetworkPath);
    let error = eggbench_core::resolve_plan(&path_plan(), &descriptors, &options())
        .expect_err("workload must advertise NetworkPath");
    assert!(error.to_string().contains("NetworkPath"));

    let error = eggbench_core::resolve_plan(&path_plan(), &path_descriptors(false), &options())
        .expect_err("fault driver is required");
    assert!(matches!(
        error,
        eggbench_core::ResolveError::MissingDriver {
            category: DriverCategory::Fault
        }
    ));
}

#[test]
fn exact_upstream_versions_survive_resolved_plan_serialization() {
    let resolved = eggbench_core::resolve_plan(&path_plan(), &path_descriptors(true), &options())
        .expect("path resolves");
    let bytes = serde_json::to_vec(&resolved).expect("resolved plan JSON");
    validate_resolved_plan_bytes(&bytes).expect("resolved plan remains valid");
    let value = serde_json::to_value(&resolved).expect("resolved plan value");
    assert_eq!(
        value["network_path"]["route_driver"]["descriptor"]["upstream_version"],
        "1.2.3"
    );
    assert_eq!(
        value["network_path"]["stream_faults"]["fault_driver"]["descriptor"]["upstream_version"],
        "1.2.3"
    );
}

#[test]
fn resolution_rejects_missing_route_and_unsupported_route_capability() {
    let without_route = path_descriptors(true)
        .into_iter()
        .filter(|descriptor| descriptor.category != DriverCategory::Route)
        .collect::<Vec<_>>();
    let error = eggbench_core::resolve_plan(&path_plan(), &without_route, &options())
        .expect_err("route driver is required");
    assert!(matches!(
        error,
        eggbench_core::ResolveError::MissingDriver {
            category: DriverCategory::Route
        }
    ));

    let mut without_capability = path_descriptors(true);
    without_capability
        .iter_mut()
        .find(|descriptor| descriptor.category == DriverCategory::Route)
        .expect("route descriptor")
        .capabilities
        .remove(&Capability::ProxyRouting);
    let error = eggbench_core::resolve_plan(&path_plan(), &without_capability, &options())
        .expect_err("ProxyRouting capability is required");
    assert!(matches!(
        error,
        eggbench_core::ResolveError::UnsupportedCapability {
            capability: Capability::ProxyRouting,
            ..
        }
    ));
}

#[test]
fn resolution_rejects_external_network_path_drivers_before_evidence() {
    let mut descriptors = path_descriptors(true);
    let route = descriptors
        .iter_mut()
        .find(|descriptor| descriptor.category == DriverCategory::Route)
        .expect("route descriptor");
    route.external_process = true;
    route.capabilities.insert(Capability::ExternalBinary);
    let mut resolution = options();
    resolution.executable_paths.insert(
        Name::new("eggress-route").expect("route"),
        "/tmp/route".to_owned(),
    );
    let error = eggbench_core::resolve_plan(&path_plan(), &descriptors, &resolution)
        .expect_err("external route must fail");
    assert!(error.to_string().contains("native Eggress"));
}

#[test]
fn requested_driver_names_must_match_resolved_descriptors() {
    let mut plan = path_plan();
    plan.network_path.as_mut().expect("path").route.driver =
        Name::new("other-route").expect("name");
    let error = eggbench_core::resolve_plan(&plan, &path_descriptors(true), &options())
        .expect_err("requested route must exist");
    assert!(matches!(
        error,
        eggbench_core::ResolveError::MissingDriver {
            category: DriverCategory::Route
        }
    ));
}

fn direct_path_evidence(route_version: &str) -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "adapter_version": "adapter-v1",
        "route_driver": {
            "name": "eggress-route",
            "adapter_version": "adapter-v1",
            "upstream_name": "eggress-outbound",
            "upstream_version": route_version
        },
        "eggress_outbound_version": route_version,
        "eggress_uri_version": "1.0.10",
        "fault_driver": null,
        "semantics": {
            "ordering_version": "route-first-fault-second-v1",
            "ordering": "route_first_fault_second",
            "fault_layer": "user_space_stream",
            "upstream": "client_to_target",
            "downstream": "target_to_client"
        },
        "route": {
            "driver": "eggress-route",
            "mode": { "kind": "direct" }
        },
        "redacted_chain": null,
        "chain_config_digest": null,
        "configured_hop_count": 0,
        "stream_faults": null,
        "diagnostics": {
            "physical_dial_attempts": 0,
            "successful_dials": 0,
            "fault_wrapped_connections": 0,
            "fault_wrapper_construction_failures": 0,
            "route_failure_buckets_dropped": 0,
            "route_failures": {},
            "hop_count_distribution": {},
            "max_observed_hop_count": 0,
            "connection_ordinal_min": 0,
            "connection_ordinal_max": 0,
            "connection_ordinal_count": 0
        },
        "policy_mode": "static"
    })
}

#[test]
fn full_bundle_comparison_matrix_rejects_each_path_identity_change() {
    let base = path_plan();
    let mut direct = base.clone();
    direct.network_path.as_mut().expect("path").route.mode = RouteMode::Direct;
    assert_path_bundle_mismatch(
        "route-mode",
        &base,
        &path_descriptors(true),
        &direct,
        &path_descriptors(true),
    );

    let mut other_chain = base.clone();
    other_chain.network_path.as_mut().expect("path").route.mode = RouteMode::ProxyChain {
        chain: "http://other-proxy.example:8080".to_owned(),
    };
    assert_path_bundle_mismatch(
        "route-chain",
        &base,
        &path_descriptors(true),
        &other_chain,
        &path_descriptors(true),
    );

    let mut old_eggress = path_descriptors(true);
    old_eggress
        .iter_mut()
        .find(|driver| driver.category == DriverCategory::Route)
        .expect("route")
        .upstream_version = Some("1.0.9".to_owned());
    assert_path_bundle_mismatch(
        "eggress-version",
        &base,
        &path_descriptors(true),
        &base,
        &old_eggress,
    );

    let mut no_faults = base.clone();
    no_faults.network_path.as_mut().expect("path").stream_faults = None;
    assert_path_bundle_mismatch(
        "fault-presence",
        &base,
        &path_descriptors(true),
        &no_faults,
        &path_descriptors(false),
    );

    let mut old_eggchaos = path_descriptors(true);
    old_eggchaos
        .iter_mut()
        .find(|driver| driver.category == DriverCategory::Fault)
        .expect("fault")
        .upstream_version = Some("0.1.1".to_owned());
    assert_path_bundle_mismatch(
        "eggchaos-version",
        &base,
        &path_descriptors(true),
        &base,
        &old_eggchaos,
    );

    let mut other_seed = base.clone();
    other_seed.seed = Some(23);
    assert_path_bundle_mismatch(
        "seed",
        &base,
        &path_descriptors(true),
        &other_seed,
        &path_descriptors(true),
    );

    let mut other_upstream = base.clone();
    other_upstream
        .network_path
        .as_mut()
        .expect("path")
        .stream_faults
        .as_mut()
        .expect("faults")
        .upstream[0]
        .id = Name::new("other-upstream").expect("id");
    assert_path_bundle_mismatch(
        "upstream-plan",
        &base,
        &path_descriptors(true),
        &other_upstream,
        &path_descriptors(true),
    );

    let mut other_downstream = base.clone();
    other_downstream
        .network_path
        .as_mut()
        .expect("path")
        .stream_faults
        .as_mut()
        .expect("faults")
        .downstream[0]
        .id = Name::new("other-downstream").expect("id");
    assert_path_bundle_mismatch(
        "downstream-plan",
        &base,
        &path_descriptors(true),
        &other_downstream,
        &path_descriptors(true),
    );
}

#[test]
fn comparison_loader_accepts_only_complete_consistent_path_evidence() {
    let mut plan = path_plan();
    let path = plan.network_path.as_mut().expect("path");
    path.route.mode = RouteMode::Direct;
    path.stream_faults = None;
    let resolved = eggbench_core::resolve_plan(&plan, &path_descriptors(false), &options())
        .expect("path resolves");
    let evidence = direct_path_evidence("1.2.3");
    let resolved_path = resolved.network_path.as_ref().expect("resolved path");
    assert_eq!(resolved_path.route.driver.as_str(), "eggress-route");
    assert_eq!(
        resolved_path.route_driver.descriptor.name.as_str(),
        "eggress-route"
    );
    assert_eq!(
        resolved_path.route_driver.descriptor.upstream_name,
        "eggress-outbound"
    );
    assert_eq!(
        resolved_path
            .route_driver
            .descriptor
            .upstream_version
            .as_deref(),
        Some("1.2.3")
    );
    assert_eq!(
        resolved_path.semantics_version,
        "route-first-fault-second-v1"
    );
    assert_eq!(evidence["adapter_version"], "adapter-v1");
    let bounds = ArtifactBounds {
        artifact_count: PositiveCount::new(16).expect("artifact count"),
        artifact_bytes: 1024 * 1024,
        total_bytes: 4 * 1024 * 1024,
    };
    let temp = tempfile::tempdir().expect("tempdir");
    let destination = temp.path().join("path.eggb");
    let mut writer = BundleWriter::create(&destination, RunId::new(), bounds).expect("writer");
    for (path, role, bytes) in [
        (
            "plan.json",
            ArtifactRole::ExperimentPlan,
            serde_json::to_vec(&plan).expect("plan bytes"),
        ),
        (
            "resolved-plan.json",
            ArtifactRole::ResolvedPlan,
            serde_json::to_vec(&resolved).expect("resolved bytes"),
        ),
        (
            "environment.json",
            ArtifactRole::EnvironmentFingerprint,
            serde_json::to_vec(&EnvironmentFingerprint::new(BTreeMap::new()))
                .expect("environment bytes"),
        ),
        (
            "network-path.json",
            ArtifactRole::Other {
                label: Name::new("network-path").expect("role"),
            },
            serde_json::to_vec(&evidence).expect("evidence bytes"),
        ),
    ] {
        writer
            .add_artifact(
                ArtifactPath::new(path).expect("artifact path"),
                role,
                "application/json",
                Sensitivity::Redacted,
                bytes.as_slice(),
            )
            .expect("artifact");
    }
    let reader = writer
        .finalize(
            ExecutionStatus::Completed,
            None,
            Subject::Label {
                label: Name::new("path").expect("subject"),
            },
            Vec::new(),
            Vec::new(),
            None,
            None,
        )
        .expect("finalize");
    let input = load_comparison_input(&reader).expect("comparison input");
    assert!(input.network_path_evidence.is_some());
}
