//! macOS network changes: `NWPathMonitor` (the C interface of Network.framework).

use crate::error::PlatformError;
use crate::network::NetworkState;
use block2::RcBlock;
use std::ffi::{c_char, c_void};

/// `nw_path_status_satisfied`
const PATH_SATISFIED: i32 = 1;

#[link(name = "Network", kind = "framework")]
extern "C" {
    fn nw_path_monitor_create() -> *mut c_void;
    fn nw_path_monitor_set_update_handler(monitor: *mut c_void, handler: &block2::DynBlock<dyn Fn(*mut c_void)>);
    fn nw_path_monitor_set_queue(monitor: *mut c_void, queue: *mut c_void);
    fn nw_path_monitor_start(monitor: *mut c_void);
    fn nw_path_monitor_cancel(monitor: *mut c_void);
    fn nw_path_get_status(path: *mut c_void) -> i32;
}

extern "C" {
    fn dispatch_queue_create(label: *const c_char, attr: *mut c_void) -> *mut c_void;
    fn os_release(object: *mut c_void);
}

pub(crate) struct Watcher {
    monitor: *mut c_void,
    queue: *mut c_void,
}

// The two handles are only cancelled and released in Drop.
unsafe impl Send for Watcher {}
unsafe impl Sync for Watcher {}

impl Watcher {
    pub(crate) fn start(on_change: Box<dyn Fn(NetworkState) + Send + Sync>) -> Result<Watcher, PlatformError> {
        unsafe {
            let monitor = nw_path_monitor_create();
            if monitor.is_null() {
                return Err(PlatformError::Os(-1));
            }
            let queue = dispatch_queue_create(c"dev.onepaste.network".as_ptr(), std::ptr::null_mut());
            if queue.is_null() {
                os_release(monitor);
                return Err(PlatformError::Os(-1));
            }
            let block = RcBlock::new(move |path: *mut c_void| {
                let online = nw_path_get_status(path) == PATH_SATISFIED;
                on_change(if online { NetworkState::Online } else { NetworkState::Offline });
            });
            nw_path_monitor_set_update_handler(monitor, &block);
            nw_path_monitor_set_queue(monitor, queue);
            nw_path_monitor_start(monitor);
            Ok(Watcher { monitor, queue })
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        unsafe {
            nw_path_monitor_cancel(self.monitor);
            os_release(self.monitor);
            os_release(self.queue);
        }
    }
}
