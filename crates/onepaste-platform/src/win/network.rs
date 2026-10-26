use crate::error::PlatformError;
use crate::network::NetworkState;
use std::ffi::c_void;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::NetworkManagement::IpHelper::{CancelMibChangeNotify2, NotifyNetworkConnectivityHintChange};
use windows::Win32::Networking::WinSock::NL_NETWORK_CONNECTIVITY_HINT;

type Callback = Box<dyn Fn(NetworkState) + Send + Sync>;

/// Connectivity level: 3 is constrained internet access, 4 is full internet access.
const LEVEL_CONSTRAINED_INTERNET: i32 = 3;

unsafe extern "system" fn on_hint(ctx: *const c_void, hint: NL_NETWORK_CONNECTIVITY_HINT) {
    let cb = &*(ctx as *const Callback);
    let online = hint.ConnectivityLevel.0 >= LEVEL_CONSTRAINED_INTERNET;
    cb(if online { NetworkState::Online } else { NetworkState::Offline });
}

pub(crate) struct Watcher {
    handle: HANDLE,
    ctx: *mut Callback,
}

// The handle is only used to unregister; the callback must be Send + Sync.
unsafe impl Send for Watcher {}
unsafe impl Sync for Watcher {}

impl Watcher {
    pub(crate) fn start(cb: Callback) -> Result<Watcher, PlatformError> {
        let ctx = Box::into_raw(Box::new(cb));
        let mut handle = HANDLE::default();
        let err = unsafe {
            NotifyNetworkConnectivityHintChange(Some(on_hint), Some(ctx as *const c_void), true, &mut handle)
        };
        if err.0 != 0 {
            unsafe { drop(Box::from_raw(ctx)) };
            return Err(PlatformError::Os(i64::from(err.0)));
        }
        Ok(Watcher { handle, ctx })
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        unsafe {
            let _ = CancelMibChangeNotify2(self.handle);
            drop(Box::from_raw(self.ctx));
        }
    }
}
