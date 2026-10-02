use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::instrument::WithSubscriber;

const SECRET: &str = "password-and-private-host-canary";

#[test]
fn structured_pool_states_and_untrusted_messages() {
    for (error, expected) in [
        (PoolError::Timeout(TimeoutType::Wait), Cause::WaitTimeout),
        (
            PoolError::Timeout(TimeoutType::Create),
            Cause::CreateTimeout,
        ),
        (
            PoolError::Timeout(TimeoutType::Recycle),
            Cause::RecycleTimeout,
        ),
        (PoolError::Closed, Cause::Closed),
        (PoolError::NoRuntimeSpecified, Cause::Runtime),
    ] {
        assert_eq!(classify(&error), expected);
    }
    let misleading = std::io::Error::other(format!("TLS authentication failed {SECRET}"));
    assert_eq!(sources(&misleading), Cause::Unknown);
    let tls = rustls::Error::General(SECRET.into());
    assert_eq!(sources(&tls), Cause::Tls);
    let wrapped_tls = std::io::Error::other(tls);
    assert_eq!(sources(&wrapped_tls), Cause::Tls);
    let network = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, SECRET);
    assert_eq!(sources(&network), Cause::Network);
}

// Speak the real startup wire protocol to exercise tokio-postgres's structured
// SQLSTATE decoding without manufacturing its private Error representation.
async fn startup_error(code: &str) -> tokio_postgres::Error {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let code = code.to_owned();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let length = socket.read_u32().await.unwrap();
        assert!((8..4096).contains(&length));
        let mut startup = vec![0; length as usize - 4];
        socket.read_exact(&mut startup).await.unwrap();
        let body = format!("SFATAL\0C{code}\0M{SECRET}\0\0");
        socket.write_u8(b'E').await.unwrap();
        socket
            .write_u32(u32::try_from(body.len()).unwrap() + 4)
            .await
            .unwrap();
        socket.write_all(body.as_bytes()).await.unwrap();
        socket.shutdown().await.unwrap();
    });
    let mut config = tokio_postgres::Config::new();
    config
        .host("127.0.0.1")
        .port(port)
        .user(SECRET)
        .password(SECRET);
    let Err(error) = config.connect(tokio_postgres::NoTls).await else {
        panic!("startup must fail");
    };
    server.await.unwrap();
    error
}

#[tokio::test]
async fn startup_sqlstates_are_classified_and_sanitized() {
    for (code, expected) in [
        ("28P01", Cause::Authentication),
        ("28000", Cause::Authentication),
        ("3D000", Cause::Database),
        ("53300", Cause::Capacity),
        ("57P03", Cause::Unavailable),
        ("42501", Cause::Permission),
        ("XX000", Cause::Unknown),
    ] {
        let error = PoolError::Backend(startup_error(code).await);
        assert_eq!(classify(&error), expected);
        let (bytes, dispatcher) = crate::observability::test_support::capture();
        let result = async { diagnostic(error) }
            .with_subscriber(dispatcher)
            .await;
        assert_eq!(result.error.kind, crate::ErrorKind::SharedIndex);
        assert!(result.error.retryable);
        assert_eq!(result.code, 11);
        let serialized = serde_json::to_string(&result).unwrap();
        let trace = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
        assert!(!serialized.contains(SECRET));
        assert!(!trace.contains(SECRET));
        assert!(trace.contains("postgres_pool_failure"));
        assert!(serialized.contains(expected.message()));
    }
}

#[tokio::test]
async fn silent_server_hits_real_creation_timeout_and_closed_pool() {
    use deadpool_postgres::{Manager, Runtime, Timeouts};
    use std::time::Duration;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut config = tokio_postgres::Config::new();
    config
        .host("127.0.0.1")
        .port(listener.local_addr().unwrap().port())
        .user(SECRET);
    let pool = deadpool_postgres::Pool::builder(Manager::new(config, tokio_postgres::NoTls))
        .max_size(1)
        .runtime(Runtime::Tokio1)
        .timeouts(Timeouts {
            create: Some(Duration::from_millis(50)),
            ..Default::default()
        })
        .build()
        .unwrap();
    let error = pool.get().await.err().unwrap();
    assert_eq!(classify(&error), Cause::CreateTimeout);
    pool.close();
    let error = pool.get().await.err().unwrap();
    assert_eq!(classify(&error), Cause::Closed);
}

#[tokio::test]
async fn refused_connection_and_unproven_tls_have_safe_diagnostics() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut config = tokio_postgres::Config::new();
    config.host("127.0.0.1").port(port).user(SECRET);
    let error = config.connect(tokio_postgres::NoTls).await.err().unwrap();
    assert_eq!(backend(&error), Cause::Network);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    // Config::port appends; use a fresh config so this control actually reaches
    // the TLS responder instead of failing host/port-count validation.
    let mut config = tokio_postgres::Config::new();
    config
        .host("127.0.0.1")
        .port(listener.local_addr().unwrap().port())
        .user(SECRET);
    config.connect_timeout(std::time::Duration::from_secs(2));
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 8];
        socket.read_exact(&mut request).await.unwrap();
        assert_eq!(request, [0, 0, 0, 8, 4, 210, 22, 47]);
        socket.write_all(b"N").await.unwrap();
    });
    config.ssl_mode(tokio_postgres::config::SslMode::Require);
    let error = config.connect(tokio_postgres::NoTls).await.err().unwrap();
    // NoTls exposes no typed TLS source; do not infer a cause from its text.
    assert_eq!(backend(&error), Cause::Unknown);
    tokio::time::timeout(std::time::Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL via MASH_TEST_PG_URL"]
async fn real_postgres_pool_success_wait_authentication_and_database_controls() {
    use deadpool_postgres::{Manager, Runtime, Timeouts};
    use std::time::Duration;

    let config: tokio_postgres::Config =
        std::env::var("MASH_TEST_PG_URL").unwrap().parse().unwrap();
    let pool =
        deadpool_postgres::Pool::builder(Manager::new(config.clone(), tokio_postgres::NoTls))
            .max_size(1)
            .runtime(Runtime::Tokio1)
            .timeouts(Timeouts {
                wait: Some(Duration::from_millis(50)),
                ..Default::default()
            })
            .build()
            .unwrap();
    let held = super::super::connection(&pool).await.unwrap();
    assert_eq!(
        held.query_one("SELECT 1::integer", &[])
            .await
            .unwrap()
            .get::<_, i32>(0),
        1
    );
    let error = pool.get().await.err().unwrap();
    assert_eq!(classify(&error), Cause::WaitTimeout);
    drop(held);
    assert!(super::super::connection(&pool).await.is_ok());
    for (mut options, expected) in [
        (config.clone(), Cause::Authentication),
        (config, Cause::Database),
    ] {
        if expected == Cause::Authentication {
            options.password(SECRET);
        } else {
            options.dbname(SECRET);
        }
        let error = options.connect(tokio_postgres::NoTls).await.err().unwrap();
        assert_eq!(backend(&error), expected);
        let output = serde_json::to_string(&diagnostic(PoolError::Backend(error))).unwrap();
        assert!(!output.contains(SECRET));
    }
}
