//! `EggServe` H1 controlled-origin adapter.
//!
//! The adapter serves one configured route from an in-process
//! [`eggserve-server`] runtime bound to loopback with an ephemeral port. It
//! implements route discipline: the exact configured path returns the
//! configured status with a deterministic fixed-length body; every other
//! target returns `501 Not Implemented`. No filesystem, timestamp, or random
//! body is involved, so repeated runs against the same config are
//! byte-identical.
//!
//! Ownership recap: `EggServe` owns inbound HTTP/runtime semantics; Eggbench
//! owns configuration, bindings, lifecycle, and evidence. This adapter never
//! binds a public address: the bind address is fixed to `127.0.0.1:0` and is
//! not configurable.

use super::{EGGSERVE_ORIGIN_SERVICE_TYPE, EGGSERVE_SERVER_VERSION};
use eggbench_runner::{
    BoxFuture, ManagedServiceAdapter, ManagedServiceHandle, RuntimeBindings, ServiceStartRequest,
};
use eggserve_primitives::{Response, ResponseBody, StatusCode};
use eggserve_server::{Request, Server, service_fn};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Service config key for the served route path.
pub const ORIGIN_PATH_KEY: &str = "path";
/// Service config key for the deterministic response body size in bytes.
pub const ORIGIN_BODY_BYTES_KEY: &str = "body_bytes";
/// Service config key for the route response status code.
pub const ORIGIN_STATUS_KEY: &str = "status";
/// Runtime binding key for the full origin URL including the route path.
pub const ORIGIN_HTTP_URL_KEY: &str = "http_url";
/// Runtime binding key for the bound loopback address.
pub const ORIGIN_BOUND_ADDR_KEY: &str = "bound_addr";
/// Runtime binding key for the runtime-selected ephemeral port.
pub const ORIGIN_BOUND_PORT_KEY: &str = "bound_port";

/// Default served route path.
pub const DEFAULT_ORIGIN_PATH: &str = "/bench";
/// Default deterministic response body size in bytes.
pub const DEFAULT_ORIGIN_BODY_BYTES: u64 = 1024;
/// Default route response status.
pub const DEFAULT_ORIGIN_STATUS: u16 = 200;
/// Maximum accepted deterministic response body size (1 MiB).
pub const MAX_ORIGIN_BODY_BYTES: u64 = 1024 * 1024;
/// Deterministic body fill byte, repeated `body_bytes` times.
pub const ORIGIN_BODY_FILL: u8 = 0x42;
/// Status returned for every target other than the configured route.
pub const UNCONFIGURED_ROUTE_STATUS: u16 = 501;

/// Controlled-origin adapter for the `eggserve-origin` service type.
#[derive(Debug, Clone, Default)]
pub struct EggServeOriginAdapter {
    _private: (),
}

impl EggServeOriginAdapter {
    /// Stateless adapter; all state is per-start.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl ManagedServiceAdapter for EggServeOriginAdapter {
    fn service_type(&self) -> &str {
        EGGSERVE_ORIGIN_SERVICE_TYPE
    }

    fn start(
        &self,
        request: ServiceStartRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<Box<dyn ManagedServiceHandle>, String>> {
        Box::pin(async move { start_origin(request, cancel).await })
    }
}

/// Handle for one started controlled origin.
struct OriginHandle {
    server: Option<eggserve_server::ServerHandle>,
    bindings: RuntimeBindings,
}

impl ManagedServiceHandle for OriginHandle {
    fn bindings(&self) -> RuntimeBindings {
        self.bindings.clone()
    }

    fn shutdown(&mut self, grace: Duration) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(async move {
            let Some(handle) = self.server.take() else {
                return Ok(());
            };
            handle.shutdown();
            match tokio::time::timeout(grace, handle.wait()).await {
                Ok(()) => Ok(()),
                Err(_) => Err(format!(
                    "eggserve origin did not stop within {}ms (eggserve-server {EGGSERVE_SERVER_VERSION})",
                    grace.as_millis(),
                )),
            }
        })
    }
}

/// Validated origin configuration from opaque service config.
struct OriginConfig {
    path: String,
    body: Vec<u8>,
    status: StatusCode,
}

fn parse_config(
    config: &std::collections::BTreeMap<String, String>,
) -> Result<OriginConfig, String> {
    let path = config
        .get(ORIGIN_PATH_KEY)
        .map_or(DEFAULT_ORIGIN_PATH, String::as_str);
    validate_path(path)?;
    let body_len = match config.get(ORIGIN_BODY_BYTES_KEY) {
        None => DEFAULT_ORIGIN_BODY_BYTES,
        Some(raw) => raw.parse::<u64>().map_err(|_| {
            format!("origin {ORIGIN_BODY_BYTES_KEY} must be an integer 0..={MAX_ORIGIN_BODY_BYTES}")
        })?,
    };
    if body_len > MAX_ORIGIN_BODY_BYTES {
        return Err(format!(
            "origin {ORIGIN_BODY_BYTES_KEY} must be an integer 0..={MAX_ORIGIN_BODY_BYTES}"
        ));
    }
    let status_raw = match config.get(ORIGIN_STATUS_KEY) {
        None => DEFAULT_ORIGIN_STATUS,
        Some(raw) => raw
            .parse::<u16>()
            .map_err(|_| "origin status must be an integer 200..=599".to_owned())?,
    };
    if !(200..=599).contains(&status_raw) {
        return Err("origin status must be an integer 200..=599".to_owned());
    }
    let status = StatusCode::new(status_raw)
        .map_err(|error| format!("origin status is not constructible: {error}"))?;
    let body_len = usize::try_from(body_len)
        .map_err(|_| format!("origin {ORIGIN_BODY_BYTES_KEY} exceeds addressable memory"))?;
    Ok(OriginConfig {
        path: path.to_owned(),
        body: vec![ORIGIN_BODY_FILL; body_len],
        status,
    })
}

fn validate_path(path: &str) -> Result<(), String> {
    if !path.starts_with('/') {
        return Err("origin path must start with '/'".to_owned());
    }
    if path.len() > 256 {
        return Err("origin path must be at most 256 bytes".to_owned());
    }
    if path.contains(['?', '#'])
        || path
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || !c.is_ascii())
    {
        return Err(
            "origin path must be visible ASCII without query, fragment, or whitespace".to_owned(),
        );
    }
    Ok(())
}

async fn start_origin(
    request: ServiceStartRequest,
    cancel: CancellationToken,
) -> Result<Box<dyn ManagedServiceHandle>, String> {
    let config = parse_config(&request.config)?;
    let body = Arc::new(config.body);
    let route_path = config.path;
    let route = Arc::new(route_path.clone());
    let status = config.status;
    let unconfigured = StatusCode::new(UNCONFIGURED_ROUTE_STATUS)
        .map_err(|error| format!("unconfigured-route status is not constructible: {error}"))?;

    let service = service_fn(move |http: Request| {
        let route = Arc::clone(&route);
        let body = Arc::clone(&body);
        async move {
            if http.head().target().path() == route.as_str() {
                Response::builder()
                    .status(status)
                    .body(ResponseBody::Bytes((*body).clone()))
                    .map_err(|error| {
                        eggserve_server::ServiceError::internal(format!(
                            "origin response construction failed: {error}"
                        ))
                    })
            } else {
                Response::builder()
                    .status(unconfigured)
                    .body(ResponseBody::Empty)
                    .map_err(|error| {
                        eggserve_server::ServiceError::internal(format!(
                            "origin response construction failed: {error}"
                        ))
                    })
            }
        }
    });

    // Loopback-only by construction: the bind address is fixed and never
    // sourced from plan config.
    let bind = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
    let server = Server::builder()
        .bind(bind)
        .build()
        .map_err(|error| format!("eggserve origin build failed: {error}"))?;
    let handle = tokio::select! {
        () = cancel.cancelled() => {
            return Err("eggserve origin start cancelled before bind".to_owned());
        }
        started = server.start_with_service(service) => {
            started.map_err(|error| format!("eggserve origin start failed: {error}"))?
        }
    };
    // The bound socket is listening, so adapter-owned readiness holds once
    // the handle exists; the accept loop drains the backlog independently.
    let addr = handle.local_addr();
    if !addr.ip().is_loopback() {
        handle.shutdown();
        handle.wait().await;
        return Err("eggserve origin bound a non-loopback address".to_owned());
    }
    if cancel.is_cancelled() {
        handle.shutdown();
        handle.wait().await;
        return Err("eggserve origin start cancelled before readiness".to_owned());
    }
    let mut bindings = RuntimeBindings::new();
    let identity = request.service.as_str();
    bindings.insert(
        identity,
        ORIGIN_HTTP_URL_KEY,
        format!("http://{addr}{route_path}"),
    )?;
    bindings.insert(identity, ORIGIN_BOUND_ADDR_KEY, addr.ip().to_string())?;
    bindings.insert(identity, ORIGIN_BOUND_PORT_KEY, addr.port().to_string())?;
    // Provenance (exact sibling versions) is reported through the driver
    // descriptors, not the bindings map, which carries connection facts only.
    Ok(Box::new(OriginHandle {
        server: Some(handle),
        bindings,
    }) as Box<dyn ManagedServiceHandle>)
}
