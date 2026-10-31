//! Quick-panel paste orchestration and platform adapters. No clipboard contents or history are held here.
use crate::PlatformError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target { pub handle: usize, pub process_id: u32 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error { Copy, Hide }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome { PasteRequested, CopiedNoPermission, CopiedManual }

pub trait Port {
    fn permitted(&mut self) -> bool;
    fn copy(&mut self) -> Result<(), Error>;
    fn hide(&mut self) -> Result<(), Error>;
    fn restore(&mut self) -> Result<(), PlatformError>;
    fn paste(&mut self) -> Result<(), PlatformError>;
}

pub fn perform(port: &mut impl Port) -> Result<Outcome, Error> {
    let permitted = port.permitted();
    port.copy()?;
    port.hide()?;
    if !permitted { return Ok(Outcome::CopiedNoPermission); }
    if port.restore().is_err() { return Ok(Outcome::CopiedManual); }
    if port.paste().is_err() { return Ok(Outcome::CopiedManual); }
    Ok(Outcome::PasteRequested)
}

pub fn foreground() -> Option<Target> {
    #[cfg(windows)] { crate::win::paste::foreground() }
    #[cfg(target_os = "macos")] { crate::mac::paste::foreground() }
    #[cfg(not(any(windows, target_os = "macos")))] { None }
}

pub fn permitted() -> bool {
    #[cfg(windows)] { true }
    #[cfg(target_os = "macos")] { crate::mac::paste::permitted() }
    #[cfg(not(any(windows, target_os = "macos")))] { false }
}

pub fn restore(target: Target) -> Result<(), PlatformError> {
    #[cfg(windows)] { crate::win::paste::restore(target) }
    #[cfg(target_os = "macos")] { crate::mac::paste::restore(target) }
    #[cfg(not(any(windows, target_os = "macos")))] { let _ = target; Err(PlatformError::Unsupported) }
}

pub fn send(target: Target) -> Result<(), PlatformError> {
    #[cfg(not(any(windows, target_os = "macos")))] { let _ = target; Err(PlatformError::Unsupported) }
    #[cfg(any(windows, target_os = "macos"))]
    {
        // Wait for the invoking modifiers and Enter to be released, then verify focus before sending any keys.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1200);
        loop {
            #[cfg(windows)] let held = crate::win::paste::invocation_keys_held();
            #[cfg(target_os = "macos")] let held = crate::mac::paste::invocation_keys_held();
            if !held { break; }
            if std::time::Instant::now() >= deadline { return Err(PlatformError::Os(-1)); }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if foreground() != Some(target) { return Err(PlatformError::Os(-1)); }
        #[cfg(windows)] { crate::win::paste::send() }
        #[cfg(target_os = "macos")] { crate::mac::paste::send() }
    }
}

pub fn open_accessibility_settings() -> Result<(), PlatformError> {
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("/usr/bin/open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility").status().map_err(|_| PlatformError::Os(-1))?;
        if status.success() { Ok(()) } else { Err(PlatformError::Os(-1)) }
    }
    #[cfg(not(target_os = "macos"))] { Err(PlatformError::Unsupported) }
}

#[cfg(any(windows, target_os = "macos"))]
pub(crate) fn wait_for_target(target: Target) -> Result<(), PlatformError> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(800);
    loop {
        if foreground() == Some(target) { return Ok(()); }
        if std::time::Instant::now() >= deadline { return Err(PlatformError::Os(-1)); }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake { calls: Vec<&'static str>, permitted: bool, copy_fail: bool, hide_fail: bool, restore_fail: bool, paste_fail: bool }
    impl Default for Fake { fn default() -> Self { Self { calls: vec![], permitted: true, copy_fail: false, hide_fail: false, restore_fail: false, paste_fail: false } } }
    impl Port for Fake {
        fn permitted(&mut self) -> bool { self.calls.push("permission"); self.permitted }
        fn copy(&mut self) -> Result<(), Error> { self.calls.push("copy"); if self.copy_fail { Err(Error::Copy) } else { Ok(()) } }
        fn hide(&mut self) -> Result<(), Error> { self.calls.push("hide"); if self.hide_fail { Err(Error::Hide) } else { Ok(()) } }
        fn restore(&mut self) -> Result<(), PlatformError> { self.calls.push("restore"); if self.restore_fail { Err(PlatformError::Os(-1)) } else { Ok(()) } }
        fn paste(&mut self) -> Result<(), PlatformError> { self.calls.push("paste"); if self.paste_fail { Err(PlatformError::Os(-1)) } else { Ok(()) } }
    }
    #[test]
    fn paste_copies_closes_restores_and_sends_once_in_order() {
        let mut io = Fake::default(); assert_eq!(perform(&mut io), Ok(Outcome::PasteRequested));
        assert_eq!(io.calls, ["permission", "copy", "hide", "restore", "paste"]);
    }
    #[test]
    fn no_permission_only_copies_and_closes_without_activating_or_sending_keys() {
        let mut io = Fake { permitted: false, ..Fake::default() }; assert_eq!(perform(&mut io), Ok(Outcome::CopiedNoPermission));
        assert_eq!(io.calls, ["permission", "copy", "hide"]);
    }
    #[test]
    fn a_failed_copy_or_close_never_restores_focus_or_sends_keys() {
        let mut io = Fake { copy_fail: true, ..Fake::default() }; assert_eq!(perform(&mut io), Err(Error::Copy)); assert_eq!(io.calls, ["permission", "copy"]);
        let mut io = Fake { hide_fail: true, ..Fake::default() }; assert_eq!(perform(&mut io), Err(Error::Hide)); assert_eq!(io.calls, ["permission", "copy", "hide"]);
    }
    #[test]
    fn a_failed_restore_or_key_submission_keeps_the_copy_and_never_retries_the_paste() {
        let mut io = Fake { restore_fail: true, ..Fake::default() }; assert_eq!(perform(&mut io), Ok(Outcome::CopiedManual)); assert_eq!(io.calls, ["permission", "copy", "hide", "restore"]);
        let mut io = Fake { paste_fail: true, ..Fake::default() }; assert_eq!(perform(&mut io), Ok(Outcome::CopiedManual)); assert_eq!(io.calls, ["permission", "copy", "hide", "restore", "paste"]);
    }
}
