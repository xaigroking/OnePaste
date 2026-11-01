use crate::{paste::{Target, wait_for_target}, PlatformError};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId, IsWindow, SetForegroundWindow}};

pub(crate) fn foreground() -> Option<Target> {
    let hwnd = unsafe { GetForegroundWindow() }; if hwnd.0.is_null() { return None; }
    let mut process_id = 0; unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    if process_id == 0 || process_id == std::process::id() { return None; }
    Some(Target { handle: hwnd.0 as usize, process_id })
}

pub(crate) fn restore(target: Target) -> Result<(), PlatformError> {
    let hwnd = HWND(target.handle as *mut _); let mut owner = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut owner)) };
    if owner != target.process_id || !unsafe { IsWindow(Some(hwnd)) }.as_bool() { return Err(PlatformError::Os(-1)); }
    if foreground() != Some(target) && !unsafe { SetForegroundWindow(hwnd) }.as_bool() { return Err(PlatformError::Os(-1)); }
    wait_for_target(target)
}

pub(crate) fn invocation_keys_held() -> bool {
    [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN, VK_RETURN].iter().any(|key| unsafe { GetAsyncKeyState(key.0 as i32) } < 0)
}

fn key(code: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: code, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, ..Default::default() } } }
}

pub(crate) fn send() -> Result<(), PlatformError> {
    let events = [key(VK_CONTROL, false), key(VK_V, false), key(VK_V, true), key(VK_CONTROL, true)];
    let count = unsafe { SendInput(&events, std::mem::size_of::<INPUT>() as i32) };
    if count == events.len() as u32 { return Ok(()); }
    // A partial submission must not leave a synthetic modifier down. Only key-up events are sent for cleanup.
    if count > 0 { unsafe { SendInput(&[key(VK_V, true), key(VK_CONTROL, true)], std::mem::size_of::<INPUT>() as i32) }; }
    Err(PlatformError::Os(-1))
}
