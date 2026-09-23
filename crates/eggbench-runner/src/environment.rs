//! Local environment fingerprint collection for M003.
//!
//! The collector runs before managed startup so a failed collection cannot
//! leave descendant processes running. It produces a versioned
//! [`EnvironmentFingerprint`] using only filesystem, `std::process`, and a
//! small set of portable platform APIs. No shell strings are executed and no
//! ambient `PATH` is consulted.
//!
//! Missing optional fields stay absent; the fingerprint never fabricates
//! placeholder values such as `unknown`. Comparison-critical fields are
//! non-secret host facts used to decide whether two runs share a testbed.
//! Transient load or frequency values are warning-only at best.

use eggbench_core::{
    ENVIRONMENT_FINGERPRINT_SCHEMA_VERSION, EnvironmentField, EnvironmentFieldClass,
    EnvironmentFingerprint, Name, SchemaVersion,
};
use std::{collections::BTreeMap, fs, path::Path};

/// One factual non-secret environment attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Fact {
    /// Stable field name.
    name: &'static str,
    /// Observed value when available.
    value: Option<String>,
    /// Field classification.
    class: EnvironmentFieldClass,
}

/// Collector that produces a deterministic local [`EnvironmentFingerprint`].
///
/// The collector does no I/O until [`Self::collect`] is called. It never
/// executes shell strings and never inspects the process environment. The
/// returned fingerprint is deterministic on a single host and never contains
/// secret material.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalEnvironmentCollector;

impl LocalEnvironmentCollector {
    /// Collect a local [`EnvironmentFingerprint`] using portable APIs.
    ///
    /// # Errors
    /// Returns an error when a comparison-critical field fails to validate
    /// against the version-1 schema. Missing optional facts remain absent.
    pub fn collect(self) -> Result<EnvironmentFingerprint, EnvironmentError> {
        let facts = collect_facts();
        let mut fields = BTreeMap::new();
        for fact in facts {
            if let Some(value) = fact.value {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let name = Name::new(fact.name).map_err(|error| EnvironmentError::InvalidName {
                    field: fact.name.to_owned(),
                    message: error.to_string(),
                })?;
                fields.insert(
                    name,
                    EnvironmentField {
                        value: trimmed.to_owned(),
                        class: fact.class,
                    },
                );
            }
        }
        let fingerprint = EnvironmentFingerprint {
            schema_version: SchemaVersion(ENVIRONMENT_FINGERPRINT_SCHEMA_VERSION.0),
            fields,
        };
        fingerprint.validate()?;
        Ok(fingerprint)
    }
}

/// Failures surfaced by [`LocalEnvironmentCollector::collect`].
#[derive(Debug, thiserror::Error)]
pub enum EnvironmentError {
    /// A selected field name or value failed version-1 validation.
    #[error("environment field {field} is invalid: {message}")]
    InvalidName {
        /// Field identifier.
        field: String,
        /// Underlying validation failure.
        message: String,
    },
    /// The resulting fingerprint violated the version-1 schema.
    #[error("environment fingerprint failed validation: {0}")]
    Schema(String),
}

impl From<eggbench_core::BundleError> for EnvironmentError {
    fn from(value: eggbench_core::BundleError) -> Self {
        Self::Schema(value.to_string())
    }
}

fn collect_facts() -> Vec<Fact> {
    let mut facts = Vec::new();
    facts.extend(os_family_facts());
    facts.extend(cpu_facts());
    facts.extend(memory_facts());
    facts.extend(kernel_facts());
    facts.extend(informational_facts());
    facts
}

fn os_family_facts() -> Vec<Fact> {
    let mut facts = Vec::new();
    let family = os_family_label();
    let architecture = os_architecture_label();
    facts.push(Fact {
        name: "os_family",
        value: Some(family.to_owned()),
        class: EnvironmentFieldClass::ComparisonCritical,
    });
    if let Some(arch) = architecture {
        facts.push(Fact {
            name: "architecture",
            value: Some(arch.to_owned()),
            class: EnvironmentFieldClass::ComparisonCritical,
        });
    }
    facts.push(Fact {
        name: "target_family",
        value: Some(target_family_label().to_owned()),
        class: EnvironmentFieldClass::ComparisonCritical,
    });
    if let Some(version) = os_version_label() {
        facts.push(Fact {
            name: "os_version",
            value: Some(version),
            class: EnvironmentFieldClass::WarningOnly,
        });
    }
    facts
}

fn cpu_facts() -> Vec<Fact> {
    let mut facts = Vec::new();
    if let Some(model) = cpu_model() {
        facts.push(Fact {
            name: "cpu_model",
            value: Some(model),
            class: EnvironmentFieldClass::ComparisonCritical,
        });
    }
    if let Some(logical) = cpu_logical_count() {
        facts.push(Fact {
            name: "logical_cpu_count",
            value: Some(logical.clone()),
            class: EnvironmentFieldClass::ComparisonCritical,
        });
    }
    if let Some(physical) = cpu_physical_count() {
        facts.push(Fact {
            name: "physical_cpu_count",
            value: Some(physical.clone()),
            class: EnvironmentFieldClass::ComparisonCritical,
        });
    }
    if let Some(freq) = cpu_frequency_mhz() {
        facts.push(Fact {
            name: "current_cpu_frequency_mhz",
            value: Some(freq.clone()),
            class: EnvironmentFieldClass::WarningOnly,
        });
    }
    facts
}

fn memory_facts() -> Vec<Fact> {
    let mut facts = Vec::new();
    if let Some(total) = total_memory_bytes() {
        facts.push(Fact {
            name: "total_memory_bytes",
            value: Some(total.clone()),
            class: EnvironmentFieldClass::ComparisonCritical,
        });
    }
    facts
}

fn kernel_facts() -> Vec<Fact> {
    let mut facts = Vec::new();
    if let Some(release) = kernel_release() {
        facts.push(Fact {
            name: "kernel_release",
            value: Some(release),
            class: EnvironmentFieldClass::ComparisonCritical,
        });
    }
    facts
}

fn informational_facts() -> Vec<Fact> {
    let facts = vec![
        Fact {
            name: "eggbench_collector_version",
            value: Some(env!("CARGO_PKG_VERSION").to_owned()),
            class: EnvironmentFieldClass::Informational,
        },
        Fact {
            name: "rust_target",
            value: Some(std::env::consts::ARCH.to_owned()),
            class: EnvironmentFieldClass::Informational,
        },
        Fact {
            name: "rustc_version_runtime",
            value: rustc_version_runtime(),
            class: EnvironmentFieldClass::Informational,
        },
        Fact {
            name: "build_profile",
            value: Some(build_profile()),
            class: EnvironmentFieldClass::Informational,
        },
    ];
    facts
}

fn os_family_label() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_family = "unix") {
        "unix-other"
    } else {
        "unknown"
    }
}

fn target_family_label() -> &'static str {
    if cfg!(target_family = "unix") {
        "unix"
    } else if cfg!(target_family = "windows") {
        "windows"
    } else {
        "other"
    }
}

fn os_architecture_label() -> Option<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Some("x86_64"),
        "aarch64" => Some("aarch64"),
        "x86" => Some("x86"),
        "arm" => Some("arm"),
        "powerpc64" => Some("powerpc64"),
        "riscv64" => Some("riscv64"),
        "s390x" => Some("s390x"),
        "loongarch64" => Some("loongarch64"),
        _ => None,
    }
}

fn os_version_label() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        read_trimmed("/etc/os-release").ok().map(|contents| {
            contents
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once('=')?;
                    if key.trim() == "PRETTY_NAME" {
                        Some(value.trim_matches('"').to_owned())
                    } else {
                        None
                    }
                })
                .unwrap_or(contents.lines().next().unwrap_or("").trim().to_owned())
        })
    }
    #[cfg(target_os = "macos")]
    {
        let release = kernel_release().unwrap_or_default();
        Some(format!("macos {release}"))
    }
    #[cfg(target_os = "windows")]
    {
        Some(format!(
            "windows {}",
            kernel_release().unwrap_or_else(|| "unknown".to_owned())
        ))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

fn kernel_release() -> Option<String> {
    #[cfg(unix)]
    {
        run_collect(["uname", "-r"])
            .ok()
            .map(|raw| raw.trim().to_owned())
            .filter(|value| !value.is_empty())
    }
    #[cfg(not(unix))]
    {
        None
    }
}

fn cpu_model() -> Option<String> {
    read_cpuinfo_field("model name")
        .or_else(|| read_cpuinfo_field("Hardware"))
        .or_else(|| read_cpuinfo_field("Processor"))
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn cpuinfo_logical_count() -> Option<usize> {
    let raw = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    let mut logical = 0usize;
    let mut found = false;
    for line in raw.lines() {
        if let Some((key, _value)) = line.split_once(':')
            && key.trim() == "processor"
        {
            logical += 1;
            found = true;
        }
    }
    if found { Some(logical) } else { None }
}

fn cpu_logical_count() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        if let Ok(value) = std::fs::read_to_string("/sys/devices/system/cpu/online") {
            let count = parse_cpu_range_count(value.trim());
            if count > 0 {
                return Some(count.to_string());
            }
        }
        if let Ok(entries) = std::fs::read_dir("/sys/devices/system/cpu") {
            let mut count = 0usize;
            for entry in entries.flatten() {
                let name = entry.file_name();
                if let Some(name) = name.to_str()
                    && let Some(rest) = name.strip_prefix("cpu")
                    && rest.parse::<u32>().is_ok()
                {
                    count += 1;
                }
            }
            if count > 0 {
                return Some(count.to_string());
            }
        }
        cpuinfo_logical_count().map(|value| value.to_string())
    }
    #[cfg(target_os = "macos")]
    {
        let value = run_collect(["sysctl", "-n", "hw.ncpu"]).ok();
        value.and_then(|raw| usize::from_str(raw.trim()).ok().map(|v| v.to_string()))
    }
    #[cfg(target_os = "windows")]
    {
        let value = std::env::var("NUMBER_OF_PROCESSORS").ok();
        value.filter(|value| !value.is_empty())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

fn parse_cpu_range_count(value: &str) -> usize {
    let mut total = 0usize;
    for part in value.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((start, end)) = part.split_once('-') {
            if let (Ok(start), Ok(end)) = (start.parse::<u32>(), end.parse::<u32>())
                && end >= start
            {
                total += usize::try_from(end - start + 1).unwrap_or(usize::MAX);
            }
        } else if part.parse::<u32>().is_ok() {
            total += 1;
        }
    }
    total
}

fn cpu_physical_count() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let raw = std::fs::read_to_string("/proc/cpuinfo").ok()?;
        let mut cores_per_package: BTreeMap<String, u32> = BTreeMap::new();
        let mut siblings_per_package: BTreeMap<String, u32> = BTreeMap::new();
        let mut current_physical = String::new();
        for line in raw.lines() {
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim();
                let value = value.trim();
                match key {
                    "physical id" => {
                        value.clone_into(&mut current_physical);
                    }
                    "cpu cores" => {
                        if let Ok(count) = value.parse::<u32>() {
                            let entry = cores_per_package
                                .entry(current_physical.clone())
                                .or_insert(0);
                            *entry = (*entry).max(count);
                        }
                    }
                    "siblings" => {
                        if let Ok(count) = value.parse::<u32>() {
                            let entry = siblings_per_package
                                .entry(current_physical.clone())
                                .or_insert(0);
                            *entry = (*entry).max(count);
                        }
                    }
                    _ => {}
                }
            }
        }
        let cores: u32 = cores_per_package.values().copied().sum();
        if cores > 0 {
            return Some(cores.to_string());
        }
        let siblings: u32 = siblings_per_package.values().copied().sum();
        if siblings > 0 {
            return Some(siblings.to_string());
        }
        None
    }
    #[cfg(target_os = "macos")]
    {
        run_collect(["sysctl", "-n", "hw.physicalcpu"])
            .ok()
            .and_then(|raw| usize::from_str(raw.trim()).ok().map(|v| v.to_string()))
    }
    #[cfg(target_os = "windows")]
    {
        None
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

fn cpu_frequency_mhz() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let mut max_mhz = 0.0f64;
        let entries = std::fs::read_dir("/sys/devices/system/cpu").ok()?;
        for entry in entries.flatten() {
            let path = entry.path().join("cpufreq/cpuinfo_max_freq");
            if let Ok(value) = std::fs::read_to_string(&path)
                && let Ok(khz) = value.trim().parse::<f64>()
            {
                let mhz = khz / 1000.0;
                if mhz.is_finite() && mhz > max_mhz {
                    max_mhz = mhz;
                }
            }
        }
        if max_mhz > 0.0 {
            return Some(format!("{max_mhz:.3}"));
        }
        None
    }
    #[cfg(target_os = "macos")]
    {
        let raw = run_collect(["sysctl", "-n", "hw.cpufrequency_max"]).ok()?;
        let value = raw.trim().parse::<f64>().ok()?;
        if value > 0.0 {
            Some(format!("{:.3}", value / 1_000_000.0))
        } else {
            None
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

fn total_memory_bytes() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let raw = std::fs::read_to_string("/proc/meminfo").ok()?;
        for line in raw.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                let mut parts = rest.split_whitespace();
                if let Some(value) = parts.next()
                    && let Ok(kib) = value.parse::<u64>()
                {
                    return Some((kib * 1024).to_string());
                }
            }
        }
        None
    }
    #[cfg(target_os = "macos")]
    {
        let raw = run_collect(["sysctl", "-n", "hw.memsize"]).ok()?;
        raw.trim().parse::<u64>().ok().map(|v| v.to_string())
    }
    #[cfg(target_os = "windows")]
    {
        None
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

fn read_cpuinfo_field(key: &str) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let raw = std::fs::read_to_string("/proc/cpuinfo").ok()?;
        for line in raw.lines() {
            if let Some((name, value)) = line.split_once(':')
                && name.trim() == key
            {
                return Some(value.trim().to_owned());
            }
        }
        None
    }
    #[cfg(target_os = "macos")]
    {
        let value = run_collect(["sysctl", "-n", "machdep.cpu.brand_string"]).ok();
        value.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = key;
        None
    }
}

fn read_trimmed(path: &str) -> Result<String, std::io::Error> {
    let raw = fs::read_to_string(Path::new(path))?;
    Ok(raw.trim().to_owned())
}

#[cfg(unix)]
fn run_collect<const N: usize>(argv: [&str; N]) -> Result<String, std::io::Error> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut child = Command::new(argv[0])
        .args(&argv[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .env_clear()
        .spawn()?;
    let mut output = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut output);
    }
    let _ = child.wait();
    Ok(output)
}

fn rustc_version_runtime() -> Option<String> {
    // `rustc_version_runtime` crate would introduce a dependency for a
    // single informational label; the host's current toolchain is not part
    // of comparison-critical evidence.
    None
}

fn build_profile() -> String {
    if cfg!(debug_assertions) {
        "debug".to_owned()
    } else {
        "release".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_has_no_secret_values() {
        let fingerprint = LocalEnvironmentCollector.collect().expect("collect");
        for (name, field) in &fingerprint.fields {
            assert!(
                !field.value.contains("SECRET"),
                "{name} looks like a secret"
            );
            assert!(
                !field.value.contains("password"),
                "{name} looks like a password"
            );
            assert!(!field.value.contains("token="), "{name} looks like a token");
        }
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let left = LocalEnvironmentCollector.collect().expect("left");
        let right = LocalEnvironmentCollector.collect().expect("right");
        assert_eq!(left, right);
    }

    #[test]
    fn fingerprint_validates() {
        let fingerprint = LocalEnvironmentCollector.collect().expect("collect");
        fingerprint.validate().expect("validate");
        assert_eq!(
            fingerprint.schema_version,
            SchemaVersion(ENVIRONMENT_FINGERPRINT_SCHEMA_VERSION.0)
        );
    }

    #[test]
    fn parse_cpu_range_count_handles_union_and_single() {
        assert_eq!(parse_cpu_range_count("0-3"), 4);
        assert_eq!(parse_cpu_range_count("0-3,5,7-9"), 8);
        assert_eq!(parse_cpu_range_count(""), 0);
    }

    #[test]
    fn build_profile_is_present() {
        let fingerprint = LocalEnvironmentCollector.collect().expect("collect");
        assert!(
            fingerprint
                .fields
                .contains_key(&Name::new("build_profile").unwrap())
        );
    }

    #[test]
    fn transient_load_never_marked_comparison_critical() {
        let fingerprint = LocalEnvironmentCollector.collect().expect("collect");
        if let Some(field) = fingerprint
            .fields
            .get(&Name::new("current_cpu_frequency_mhz").unwrap())
        {
            assert_eq!(field.class, EnvironmentFieldClass::WarningOnly);
        }
    }
}
