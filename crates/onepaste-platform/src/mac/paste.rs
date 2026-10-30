use crate::{paste::{Target, wait_for_target}, PlatformError};
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};
use std::ffi::c_void;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" { fn AXIsProcessTrusted() -> u8; }
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventCreateKeyboardEvent(source: *const c_void, key: u16, down: bool) -> *mut c_void;
    fn CGEventSetFlags(event: *mut c_void, flags: u64);
    fn CGEventPost(tap: u32, event: *mut c_void);
    fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
}
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" { fn CFRelease(value: *const c_void); }

pub(crate) fn permitted() -> bool { unsafe { AXIsProcessTrusted() != 0 } }

pub(crate) fn foreground() -> Option<Target> {
    autoreleasepool(|_| {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        let process_id = app.processIdentifier() as u32;
        if process_id == 0 || process_id == std::process::id() { return None; }
        Some(Target { handle: 0, process_id })
    })
}

pub(crate) fn restore(target: Target) -> Result<(), PlatformError> {
    if foreground() == Some(target) { return Ok(()); }
    let requested = autoreleasepool(|_| {
        let app = NSRunningApplication::runningApplicationWithProcessIdentifier(target.process_id as i32).ok_or(PlatformError::Os(-1))?;
        Ok::<bool, PlatformError>(app.activateWithOptions(NSApplicationActivationOptions::empty()))
    })?;
    if !requested { return Err(PlatformError::Os(-1)); }
    wait_for_target(target)
}

pub(crate) fn invocation_keys_held() -> bool { [54, 55, 56, 60, 58, 61, 59, 62, 36, 76].iter().any(|key| unsafe { CGEventSourceKeyState(0, *key) }) }

pub(crate) fn send() -> Result<(), PlatformError> {
    if !permitted() { return Err(PlatformError::Os(-1)); }
    let mut events: Vec<*mut c_void> = Vec::new();
    // Command down, V down, V up, Command up. Allocate every event before posting any of them.
    for (key, down, flags) in [(55, true, 1 << 20), (9, true, 1 << 20), (9, false, 1 << 20), (55, false, 0)] {
        let event = unsafe { CGEventCreateKeyboardEvent(std::ptr::null(), key, down) };
        if event.is_null() { for event in events { unsafe { CFRelease(event) }; } return Err(PlatformError::Os(-1)); }
        unsafe { CGEventSetFlags(event, flags) }; events.push(event);
    }
    for event in events { unsafe { CGEventPost(0, event); CFRelease(event) }; }
    Ok(())
}
