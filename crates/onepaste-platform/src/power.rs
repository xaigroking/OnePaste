//! Wake-from-sleep events.

use crate::error::PlatformError;

#[cfg(target_os = "macos")]
use crate::mac::power as imp;
#[cfg(not(any(windows, target_os = "macos")))]
use crate::unsupported::power as imp;
#[cfg(windows)]
use crate::win::power as imp;

/// Wake watcher. Dropping it stops it.
pub struct WakeWatcher {
    _inner: imp::Watcher,
}

impl WakeWatcher {
    pub fn start(on_wake: impl Fn() + Send + Sync + 'static) -> Result<WakeWatcher, PlatformError> {
        Ok(WakeWatcher { _inner: imp::Watcher::start(Box::new(on_wake))? })
    }
}
