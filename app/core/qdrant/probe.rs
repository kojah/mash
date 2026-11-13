//! Bounded checks of what answers on the configured loopback ports.
use crate::AppError;
use std::time::Duration;

/// What already listens on the configured ports.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Ports {
    /// Nothing; Qdrant can bind both.
    Free,
    /// A Qdrant Mash can use already serves both ports.
    Qdrant { version: String },
}

#[derive(serde::Deserialize)]
struct Instance {
    title: String,
    version: String,
}

/// Classify the ports, or explain why neither Mash's Qdrant nor the one
/// already there can serve them.
pub(super) async fn ports(ports: super::Ports) -> Result<Ports, AppError> {
    let http = listening(ports.http).await;
    let grpc = listening(ports.grpc).await;
    if !http && !grpc {
        return Ok(Ports::Free);
    }
    if !http {
        return Err(conflict(format!(
            "port {} (Qdrant's gRPC port in the Mash configuration) is in use by another program, but nothing answers on HTTP port {}; stop that program, or set other ports in [qdrant] url and grpc_url",
            ports.grpc, ports.http
        )));
    }
    match identify(ports.http).await {
        Some(version) if !compatible(&version) => Err(conflict(format!(
            "Qdrant {version} already answers on 127.0.0.1:{}, and Mash needs Qdrant 1.20 or later; upgrade or stop that service, then rerun `mash qdrant install`",
            ports.http
        ))),
        Some(version) if grpc => Ok(Ports::Qdrant { version }),
        Some(version) => Err(conflict(format!(
            "Qdrant {version} answers on 127.0.0.1:{} but not on gRPC port {}; Mash needs both on the same instance. Fix that service's ports, or stop it and rerun `mash qdrant install`",
            ports.http, ports.grpc
        ))),
        None => Err(conflict(format!(
            "port {} (Qdrant's HTTP port in the Mash configuration) is in use by a program that is not Qdrant; stop it, or set other ports in [qdrant] url and grpc_url",
            ports.http
        ))),
    }
}

/// Wait until the started Qdrant is ready on both ports; returns its version.
pub(super) async fn ready(ports: super::Ports, within: Duration) -> Result<String, AppError> {
    let deadline = tokio::time::Instant::now() + within;
    let client = client(Duration::from_secs(3))?;
    loop {
        let ready = client
            .get(format!("http://127.0.0.1:{}/readyz", ports.http))
            .send()
            .await
            .is_ok_and(|response| response.status().is_success());
        if ready
            && listening(ports.grpc).await
            && let Some(version) = identify(ports.http).await
        {
            return Ok(version);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(super::install_error(format!(
                "Qdrant did not become ready on 127.0.0.1:{} and :{} within {} s",
                ports.http,
                ports.grpc,
                within.as_secs()
            )));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

fn compatible(version: &str) -> bool {
    let mut parts = version.split('.').map(str::parse::<u32>);
    matches!((parts.next(), parts.next()), (Some(Ok(1)), Some(Ok(minor))) if minor >= 20)
}

async fn listening(port: u16) -> bool {
    tokio::time::timeout(
        Duration::from_secs(2),
        tokio::net::TcpStream::connect(("127.0.0.1", port)),
    )
    .await
    .is_ok_and(|connected| connected.is_ok())
}

/// The version of the Qdrant answering on `port`, if one does.
async fn identify(port: u16) -> Option<String> {
    let response = client(Duration::from_secs(3))
        .ok()?
        .get(format!("http://127.0.0.1:{port}/"))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    if response
        .content_length()
        .is_some_and(|length| length > 64 * 1024)
    {
        return None;
    }
    let body = response.bytes().await.ok()?;
    let instance: Instance = serde_json::from_slice(&body).ok()?;
    instance
        .title
        .starts_with("qdrant")
        .then_some(instance.version)
}

fn client(timeout: Duration) -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .map_err(|error| AppError::internal(format!("HTTP client: {error}")))
}

fn conflict(message: String) -> AppError {
    AppError::new(12, crate::ErrorKind::Conflict, message, false)
}

#[cfg(test)]
pub(super) fn is_compatible(version: &str) -> bool {
    compatible(version)
}
