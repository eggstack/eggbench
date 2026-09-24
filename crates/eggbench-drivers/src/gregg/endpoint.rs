//! Gregg loopback endpoint policy.
//!
//! Gregg serves unauthenticated telemetry on a private network. M001b
//! accepts loopback HTTP endpoints only: `127.0.0.0/8`, `::1`, or
//! `localhost` when deterministic resolution stays loopback. Public
//! addresses, private-LAN addresses, HTTPS, embedded credentials, and
//! query/fragment endpoint forms are rejected.

/// Health probe path served by the daemon.
pub const HEALTH_PATH: &str = "/v2/healthz";
/// Status snapshot path served by the daemon.
pub const STATUS_PATH: &str = "/v2/status";

/// Validated Gregg endpoint: loopback HTTP base URL with explicit port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GreggEndpoint {
    base_url: String,
    host: String,
    port: u16,
}

impl GreggEndpoint {
    /// Health probe URL.
    #[must_use]
    pub fn health_url(&self) -> String {
        format!("{}{HEALTH_PATH}", self.base_url)
    }

    /// Status snapshot URL.
    #[must_use]
    pub fn status_url(&self) -> String {
        format!("{}{STATUS_PATH}", self.base_url)
    }

    /// Endpoint host label for evidence (no credentials exist by policy).
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Endpoint port for evidence.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }
}

/// Validate a configured Gregg endpoint string.
///
/// Returns the normalized base URL (no trailing slash) with host/port, or
/// a stable machine-readable reason: `endpoint_invalid` for malformed
/// values, `endpoint_not_loopback` for non-loopback hosts.
///
/// # Errors
/// Returns the rejection reason when the endpoint violates M001b policy.
pub fn validate_endpoint(raw: &str) -> Result<GreggEndpoint, &'static str> {
    let rest = raw.strip_prefix("http://").ok_or("endpoint_invalid")?;
    if raw.starts_with("https://") || rest.contains('@') {
        return Err("endpoint_invalid");
    }
    // Reject query/fragment forms; the base carries paths only.
    if rest.contains(['?', '#']) {
        return Err("endpoint_invalid");
    }
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    // Split authority from an optional path; only empty or `/` paths allowed.
    let (authority, path) = match rest.find('/') {
        Some(index) => rest.split_at(index),
        None => (rest, ""),
    };
    if !path.is_empty() {
        return Err("endpoint_invalid");
    }
    if authority.is_empty() {
        return Err("endpoint_invalid");
    }
    let (host, port) = split_authority(authority)?;
    check_loopback(&host)?;
    Ok(GreggEndpoint {
        base_url: format!("http://{authority}"),
        host,
        port,
    })
}

/// Split `host:port`, requiring an explicit numeric port.
fn split_authority(authority: &str) -> Result<(String, u16), &'static str> {
    if let Some(bracketed) = authority.strip_prefix('[') {
        // IPv6 literal: `[::1]:port`.
        let end = bracketed.find(']').ok_or("endpoint_invalid")?;
        let host = &bracketed[..end];
        let port = bracketed[end + 1..]
            .strip_prefix(':')
            .ok_or("endpoint_invalid")?;
        if host.is_empty() {
            return Err("endpoint_invalid");
        }
        let port: u16 = port.parse().map_err(|_| "endpoint_invalid")?;
        return Ok((host.to_owned(), port));
    }
    // IPv4/hostname: exactly one colon separating a non-empty port.
    let (host, port) = authority.rsplit_once(':').ok_or("endpoint_invalid")?;
    if host.is_empty() || port.is_empty() || host.contains(':') {
        return Err("endpoint_invalid");
    }
    // Reject userinfo-looking hosts and whitespace/controls.
    if host.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("endpoint_invalid");
    }
    let port: u16 = port.parse().map_err(|_| "endpoint_invalid")?;
    Ok((host.to_owned(), port))
}

/// Enforce loopback-only hosts: `127.0.0.0/8`, `::1`, or `localhost` with
/// loopback-only deterministic resolution.
fn check_loopback(host: &str) -> Result<(), &'static str> {
    if host.eq_ignore_ascii_case("localhost") {
        // Deterministic resolution must remain loopback: every resolved
        // address must be a loopback address, else the label is rejected.
        let resolved = format!("{host}:1")
            .parse::<std::net::SocketAddr>()
            .ok()
            .map(|addr| vec![addr.ip()])
            .or_else(|| {
                use std::net::ToSocketAddrs;
                format!("{host}:1")
                    .to_socket_addrs()
                    .ok()
                    .map(|addrs| addrs.map(|addr| addr.ip()).collect::<Vec<_>>())
            })
            .unwrap_or_default();
        if resolved.is_empty() || !resolved.iter().all(std::net::IpAddr::is_loopback) {
            return Err("endpoint_not_loopback");
        }
        return Ok(());
    }
    let addr: std::net::IpAddr = host.parse().map_err(|_| "endpoint_not_loopback")?;
    if !addr.is_loopback() {
        return Err("endpoint_not_loopback");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_forms_accept() {
        for raw in [
            "http://127.0.0.1:11310",
            "http://127.0.0.1:11310/",
            "http://127.12.34.56:8080",
            "http://[::1]:11310",
            "http://localhost:11310",
        ] {
            assert!(validate_endpoint(raw).is_ok(), "accept {raw}");
        }
        let endpoint = validate_endpoint("http://127.0.0.1:11310").unwrap();
        assert_eq!(endpoint.host(), "127.0.0.1");
        assert_eq!(endpoint.port(), 11310);
        assert_eq!(endpoint.health_url(), "http://127.0.0.1:11310/v2/healthz");
        assert_eq!(endpoint.status_url(), "http://127.0.0.1:11310/v2/status");
    }

    #[test]
    fn non_loopback_rejected() {
        for raw in [
            "http://10.0.0.5:11310",
            "http://192.168.1.2:11310",
            "http://8.8.8.8:11310",
            "http://example.com:11310",
            "http://[::2]:11310",
        ] {
            assert_eq!(validate_endpoint(raw), Err("endpoint_not_loopback"));
        }
    }

    #[test]
    fn malformed_rejected() {
        for raw in [
            "https://127.0.0.1:11310",
            "http://user:pass@127.0.0.1:11310",
            "http://127.0.0.1:11310/v2/status?x=1",
            "http://127.0.0.1:11310#frag",
            "http://127.0.0.1",
            "http://127.0.0.1:notaport",
            "http://:11310",
            "http:///v2/status",
            "http://127.0.0.1:11310/base",
            "127.0.0.1:11310",
            "",
        ] {
            assert_eq!(
                validate_endpoint(raw),
                Err("endpoint_invalid"),
                "reject {raw}"
            );
        }
    }
}
