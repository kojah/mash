//! Constants the tests of every workspace crate share.
use std::time::Duration;

/// Bound for a wait that only guards against a hung test. Completion or an
/// explicit synchronization signal ends the wait; this is not a latency
/// expectation. Shared build hosts run under heavy load, so a correct run must
/// never race it. Nextest still terminates the complete test after its
/// profile's slow-timeout.
pub const HANG_GUARD: Duration = Duration::from_secs(60);
