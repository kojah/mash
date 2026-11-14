//! Resumable WebSocket synchronization with typed, compressed batches.
//!
//! HTTP remains the read API. Mutation transport, durable transfer state and
//! native publication coordination live here; canonical silver stays in the
//! existing server store.
#[cfg(test)]
mod bench;
pub mod catalog;
pub mod client;
pub mod wire;
