//! Opt-in, application-only diagnostics; never record external payloads.
#[cfg(all(feature = "console", not(tokio_unstable)))]
compile_error!("the console feature requires RUSTFLAGS='--cfg tokio_unstable'");

use std::sync::atomic::{AtomicU64, Ordering};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub(crate) mod http;
pub(crate) mod retrieval;

pub(crate) fn initialize() {
    let trace = std::env::var_os("MASH_TRACE").as_deref() == Some(std::ffi::OsStr::new("1"));
    #[cfg(feature = "console")]
    if std::env::var_os("MASH_CONSOLE").as_deref() == Some(std::ffi::OsStr::new("1")) {
        let console = console_subscriber::ConsoleLayer::builder()
            .server_addr(([127, 0, 0, 1], 6669))
            .spawn();
        let _ = configured_subscriber(std::io::stderr, trace)
            .with(console)
            .try_init();
        return;
    }
    if !trace {
        return;
    }
    let _ = configured_subscriber(std::io::stderr, true).try_init();
}

#[cfg(test)]
fn subscriber<W>(writer: W) -> impl tracing::Subscriber + Send + Sync
where
    W: for<'a> tracing_subscriber::fmt::MakeWriter<'a> + Send + Sync + 'static,
{
    configured_subscriber(writer, true)
}

fn configured_subscriber<W>(
    writer: W,
    enabled: bool,
) -> impl tracing::Subscriber + Send + Sync + for<'a> tracing_subscriber::registry::LookupSpan<'a>
where
    W: for<'a> tracing_subscriber::fmt::MakeWriter<'a> + Send + Sync + 'static,
{
    use tracing_subscriber::Layer;
    tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .json()
            .with_writer(writer)
            .with_ansi(false)
            .with_span_events(
                tracing_subscriber::fmt::format::FmtSpan::NEW
                    | tracing_subscriber::fmt::format::FmtSpan::CLOSE,
            )
            .with_filter(tracing_subscriber::filter::Targets::new().with_target(
                "mash",
                if enabled {
                    tracing::level_filters::LevelFilter::INFO
                } else {
                    tracing::level_filters::LevelFilter::OFF
                },
            )),
    )
}

pub(crate) fn request_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Capture both parts of tracing state before moving work to an OS thread.
/// A span alone does not propagate a task-local subscriber.
pub(crate) struct ThreadContext {
    dispatcher: tracing::Dispatch,
    parent: tracing::Span,
    retrieval: Option<retrieval::Capture>,
}

impl ThreadContext {
    pub(crate) fn capture() -> Self {
        Self {
            dispatcher: tracing::dispatcher::get_default(Clone::clone),
            parent: tracing::Span::current(),
            retrieval: retrieval::current(),
        }
    }

    pub(crate) fn run<T>(&self, work: impl FnOnce() -> T) -> T {
        retrieval::on_blocking_thread(self.retrieval.clone(), || {
            tracing::dispatcher::with_default(&self.dispatcher, || self.parent.in_scope(work))
        })
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) mod test_support;
