//! Application interaction time includes suspend; it never reads global input.
use std::time::{Duration, SystemTime, UNIX_EPOCH};
pub trait AutoLockClock: Send + Sync { fn now(&self) -> Duration; }
pub(crate) struct WallClock;
impl AutoLockClock for WallClock {
    fn now(&self) -> Duration { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default() }
}
