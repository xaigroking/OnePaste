//! 在独立线程里跑一个隐藏窗口和消息循环，把窗口消息交给回调。

use crate::error::PlatformError;
use std::ffi::c_void;
use std::sync::mpsc;
use std::thread::JoinHandle;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, GetWindowLongPtrW,
    PostMessageW, PostQuitMessage, RegisterClassW, SetWindowLongPtrW, TranslateMessage, GWLP_USERDATA,
    HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_DESTROY, WM_NCDESTROY, WNDCLASSW,
};

type Handler = Box<dyn FnMut(u32, WPARAM, LPARAM) + Send>;

pub(crate) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub(crate) fn os_err(e: windows::core::Error) -> PlatformError {
    PlatformError::Os(i64::from(e.code().0))
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Handler;
    if msg == WM_NCDESTROY {
        if !ptr.is_null() {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(ptr));
        }
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    if !ptr.is_null() {
        (*ptr)(msg, wp, lp);
    }
    match msg {
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// 窗口和线程的持有者；丢弃时关闭窗口并等待线程退出。
pub(crate) struct MessageWindow {
    hwnd: usize,
    thread: Option<JoinHandle<()>>,
}

impl MessageWindow {
    /// `message_only` 为真时创建仅用于收消息的窗口；否则创建隐藏的顶层窗口（能收到系统广播）。
    /// `init` 在窗口创建后、进入消息循环前执行，失败则整体失败。
    pub(crate) fn spawn(
        class_name: &str,
        message_only: bool,
        init: impl FnOnce(HWND) -> Result<(), PlatformError> + Send + 'static,
        handler: impl FnMut(u32, WPARAM, LPARAM) + Send + 'static,
    ) -> Result<Self, PlatformError> {
        let (tx, rx) = mpsc::channel::<Result<usize, PlatformError>>();
        let class = wide(class_name);
        let thread = std::thread::spawn(move || unsafe {
            let hinst = match GetModuleHandleW(PCWSTR::null()) {
                Ok(h) => h,
                Err(e) => {
                    let _ = tx.send(Err(os_err(e)));
                    return;
                }
            };
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wnd_proc),
                hInstance: hinst.into(),
                lpszClassName: PCWSTR(class.as_ptr()),
                ..Default::default()
            };
            RegisterClassW(&wc); // 重复注册会返回 0，可以忽略
            let parent = if message_only { Some(HWND_MESSAGE) } else { None };
            let hwnd = match CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(class.as_ptr()),
                PCWSTR::null(),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                parent,
                None,
                Some(hinst.into()),
                None,
            ) {
                Ok(h) => h,
                Err(e) => {
                    let _ = tx.send(Err(os_err(e)));
                    return;
                }
            };
            let boxed: Box<Handler> = Box::new(Box::new(handler));
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(boxed) as isize);
            if let Err(e) = init(hwnd) {
                let _ = DestroyWindow(hwnd);
                let _ = tx.send(Err(e));
                return;
            }
            let _ = tx.send(Ok(hwnd.0 as usize));
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
        match rx.recv() {
            Ok(Ok(hwnd)) => Ok(MessageWindow { hwnd, thread: Some(thread) }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => Err(PlatformError::Os(-1)),
        }
    }
}

impl Drop for MessageWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = PostMessageW(Some(HWND(self.hwnd as *mut c_void)), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
