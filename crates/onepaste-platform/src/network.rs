//! Network change events.

use crate::error::PlatformError;

#[cfg(target_os = "macos")]
use crate::mac::network as imp;
#[cfg(not(any(windows, target_os = "macos")))]
use crate::unsupported::network as imp;
#[cfg(windows)]
use crate::win::network as imp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkState {
    Online,
    Offline,
}

/// Network state watcher. The current state is reported once right after registration; dropping it stops it.
pub struct NetworkWatcher {
    _inner: imp::Watcher,
}

impl NetworkWatcher {
    pub fn start(on_change: impl Fn(NetworkState) + Send + Sync + 'static) -> Result<NetworkWatcher, PlatformError> {
        Ok(NetworkWatcher { _inner: imp::Watcher::start(Box::new(on_change))? })
    }
}
