//! macOS wake from sleep: `NSWorkspaceDidWakeNotification`. The notification is delivered in the main thread run loop.

use crate::error::PlatformError;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSWorkspace, NSWorkspaceDidWakeNotification};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol};
use std::ptr::NonNull;

pub(crate) struct Watcher {
    center: Retained<NSNotificationCenter>,
    observer: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

// The notification center is thread safe; the observer is only used to unregister in Drop.
unsafe impl Send for Watcher {}
unsafe impl Sync for Watcher {}

impl Watcher {
    pub(crate) fn start(on_wake: Box<dyn Fn() + Send + Sync>) -> Result<Watcher, PlatformError> {
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        let block = RcBlock::new(move |_n: NonNull<NSNotification>| on_wake());
        let observer = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceDidWakeNotification),
                None,
                None,
                &block,
            )
        };
        Ok(Watcher { center, observer })
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        let observer: &objc2::runtime::AnyObject = (*self.observer).as_ref();
        unsafe { self.center.removeObserver(observer) };
    }
}
