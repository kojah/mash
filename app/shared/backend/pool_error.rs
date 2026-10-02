use crate::AppError;
use deadpool_postgres::{PoolError, TimeoutType};
use std::error::Error;
use tokio_postgres::error::SqlState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cause {
    WaitTimeout,
    CreateTimeout,
    RecycleTimeout,
    Closed,
    Runtime,
    Authentication,
    Database,
    Capacity,
    Unavailable,
    Permission,
    Tls,
    Network,
    Unknown,
}

impl Cause {
    fn message(self) -> &'static str {
        match self {
            Self::WaitTimeout => {
                "PostgreSQL pool wait timed out; check server load and pool capacity"
            }
            Self::CreateTimeout => {
                "PostgreSQL connection creation timed out; check database reachability"
            }
            Self::RecycleTimeout => {
                "PostgreSQL connection recycling timed out; check database health"
            }
            Self::Closed => "PostgreSQL connection pool is closed",
            Self::Runtime => "PostgreSQL pool runtime is not configured",
            Self::Authentication => {
                "PostgreSQL authentication failed; check server database credentials and access rules"
            }
            Self::Database => {
                "PostgreSQL database is unavailable; check the configured database name"
            }
            Self::Capacity => {
                "PostgreSQL connection limit reached; check database connection capacity"
            }
            Self::Unavailable => "PostgreSQL is not accepting connections; check database health",
            Self::Permission => "PostgreSQL access denied; check database permissions",
            Self::Tls => {
                "PostgreSQL TLS negotiation failed; check TLS configuration and certificate trust"
            }
            Self::Network => "PostgreSQL network connection failed; check database reachability",
            Self::Unknown => {
                "PostgreSQL connection pool is unavailable; check database health, credentials and TLS configuration"
            }
        }
    }
}

fn backend(error: &tokio_postgres::Error) -> Cause {
    if let Some(code) = error.code() {
        return match *code {
            SqlState::INVALID_PASSWORD | SqlState::INVALID_AUTHORIZATION_SPECIFICATION => {
                Cause::Authentication
            }
            SqlState::INVALID_CATALOG_NAME => Cause::Database,
            SqlState::TOO_MANY_CONNECTIONS => Cause::Capacity,
            SqlState::CANNOT_CONNECT_NOW => Cause::Unavailable,
            SqlState::INSUFFICIENT_PRIVILEGE => Cause::Permission,
            _ => Cause::Unknown,
        };
    }
    if error.is_closed() {
        return Cause::Closed;
    }
    sources(error)
}

// Inspect types, never Display/Debug or error-message substrings. Bound traversal
// even when a third-party error exposes a cyclic source chain.
fn sources(error: &(dyn Error + 'static)) -> Cause {
    let mut source = Some(error);
    let mut network = false;
    for _ in 0..8 {
        let Some(error) = source else { break };
        if error.is::<rustls::Error>() {
            return Cause::Tls;
        }
        if let Some(io) = error.downcast_ref::<std::io::Error>() {
            network |= matches!(
                io.kind(),
                std::io::ErrorKind::ConnectionRefused
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::NotConnected
                    | std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::AddrNotAvailable
            );
            // io::Error can wrap rustls directly while its source() delegates
            // to the wrapped error's source, skipping that typed boundary.
            if let Some(inner) = io.get_ref() {
                source = Some(inner);
                continue;
            }
        }
        source = error.source();
    }
    if network {
        Cause::Network
    } else {
        Cause::Unknown
    }
}

fn classify(error: &PoolError) -> Cause {
    match error {
        PoolError::Timeout(TimeoutType::Wait) => Cause::WaitTimeout,
        PoolError::Timeout(TimeoutType::Create) => Cause::CreateTimeout,
        PoolError::Timeout(TimeoutType::Recycle) => Cause::RecycleTimeout,
        PoolError::Closed => Cause::Closed,
        PoolError::NoRuntimeSpecified => Cause::Runtime,
        PoolError::Backend(error) => backend(error),
        PoolError::PostCreateHook(_) => Cause::Unknown,
    }
}

pub(super) fn diagnostic(error: PoolError) -> AppError {
    let cause = classify(&error);
    drop(error);
    tracing::warn!(event = "postgres_pool_failure", cause = ?cause, "PostgreSQL pool acquisition failed");
    AppError::shared(cause.message())
}

#[cfg(test)]
mod tests;
