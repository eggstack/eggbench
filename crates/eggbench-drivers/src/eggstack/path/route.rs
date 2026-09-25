//! Listener-free route lowering against `eggress-outbound`.
//!
//! Public surface: build the canonical [`OutboundConnector`] for the plan
//! route request, using only the audited base TCP feature profile. Credentials
//! are rejected at the plan-validation layer; the runtime re-parses for
//! canonical evidence form.

use eggress_outbound::{OutboundConnector, OutboundError};
use eggress_uri::{ProtocolSpec, ProxyChainSpec, RedactedUri};

/// Parse and re-emit the canonical chain. Returns the typed [`ProxyChainSpec`]
/// so callers can construct an [`OutboundConnector::from_chain`] directly.
///
/// # Errors
/// Returns a human-readable reason when the chain cannot be parsed or carries
/// embedded credentials.
pub fn parse_route_chain(chain: &str) -> Result<ProxyChainSpec, String> {
    let spec = eggress_uri::parse_proxy_chain(chain)
        .map_err(|error| format!("proxy chain could not be parsed by eggress-uri: {error}"))?;
    validate_route_spec(&spec)?;
    Ok(spec)
}

fn validate_route_spec(spec: &ProxyChainSpec) -> Result<(), String> {
    if spec.hops.is_empty() {
        return Err("proxy chain must contain at least one hop".to_owned());
    }
    for (index, hop) in spec.hops.iter().enumerate() {
        if hop.credentials.is_some()
            || hop.rule.is_some()
            || hop.local_bind.is_some()
            || hop.tls
            || hop.server_name.is_some()
            || hop.insecure
            || !hop.plugins.is_empty()
            || hop.auth_prefix.is_some()
        {
            return Err(format!(
                "proxy hop {index} uses unsupported credential, rule, bind, TLS, or plugin options"
            ));
        }
        if hop.protocols.len() != 1
            || !matches!(
                hop.protocols.as_slice(),
                [ProtocolSpec::Http | ProtocolSpec::Socks4 | ProtocolSpec::Socks5]
            )
        {
            return Err(format!(
                "proxy hop {index} must use exactly one HTTP, SOCKS4, or SOCKS5 protocol"
            ));
        }
    }
    Ok(())
}

/// Build the listener-free Eggress outbound connector for a route.
///
/// # Errors
/// Returns the underlying [`OutboundError`] when Eggress rejects the chain
/// (typically only invalid hops / unsupported protocol features for the base
/// TCP profile). On success the connector is ready for
/// `connect_tcp_timeout_detailed` calls.
pub fn build_route_chain(
    mode: &eggbench_core::RouteMode,
) -> Result<OutboundConnector, OutboundError> {
    match mode {
        eggbench_core::RouteMode::Direct => Ok(OutboundConnector::direct()),
        eggbench_core::RouteMode::ProxyChain { chain } => {
            let spec = parse_route_chain(chain).map_err(OutboundError::Runtime)?;
            OutboundConnector::from_chain(spec)
        }
    }
}

/// Canonical credential-free chain text suitable for diagnostic evidence.
///
/// # Errors
/// Returns a human-readable reason when the chain is invalid or unsupported.
pub fn redacted_chain_text(chain: &str) -> Result<String, String> {
    Ok(RedactedUri::new(&parse_route_chain(chain)?).to_string())
}

#[cfg(test)]
mod tests {
    use super::redacted_chain_text;

    #[test]
    fn redacted_chain_uses_the_native_canonical_scheme() {
        assert_eq!(
            redacted_chain_text("socks4a://proxy.example:1080").expect("chain"),
            "socks4://proxy.example:1080"
        );
    }
}
