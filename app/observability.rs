//! Opt-in, application-only diagnostics; never record external payloads.
use std::sync::atomic::{AtomicU64, Ordering};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub(crate) fn initialize() {
    if std::env::var_os("MASH_TRACE").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    let _ = subscriber(std::io::stderr).try_init();
}

fn subscriber<W>(writer: W) -> impl tracing::Subscriber + Send + Sync
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
            .with_filter(
                tracing_subscriber::filter::Targets::new()
                    .with_target("mash", tracing::Level::INFO),
            ),
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
}

impl ThreadContext {
    pub(crate) fn capture() -> Self {
        Self {
            dispatcher: tracing::dispatcher::get_default(Clone::clone),
            parent: tracing::Span::current(),
        }
    }

    pub(crate) fn run<T>(&self, work: impl FnOnce() -> T) -> T {
        tracing::dispatcher::with_default(&self.dispatcher, || self.parent.in_scope(work))
    }
}

#[cfg(test)]
mod tests;
