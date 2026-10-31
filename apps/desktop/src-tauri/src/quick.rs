//! The quick panel: a small window that the global hotkey and the tray menu bring up. It is created hidden at start so
//! that showing it never has to build a window from a menu or hotkey callback.

use onepaste_platform::paste::{self, Target};
use std::sync::{Mutex, atomic::{AtomicBool, Ordering}};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "quick";

struct QuickState { target: Mutex<Option<Target>>, busy: AtomicBool }

/// Creates the hidden window. The page decides what to draw from the window label.
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("OnePaste")
        .inner_size(560.0, 520.0)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .center()
        .visible(false)
        .build()?;
    app.manage(QuickState { target: Mutex::new(None), busy: AtomicBool::new(false) });
    Ok(())
}

/// Shows the panel and tells the page, so it clears its search and focuses the search box.
pub fn show(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        if !w.is_visible().unwrap_or(false) { *app.state::<QuickState>().target.lock().unwrap() = paste::foreground(); }
        let _ = w.center();
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        let _ = app.emit_to(LABEL, "onepaste://quick-opened", ());
    }
}

pub fn hide(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.hide();
    }
}

/// The hotkey: show the panel, or hide it if it is already in front.
pub fn toggle(app: &AppHandle) {
    let visible = app.get_webview_window(LABEL).and_then(|w| w.is_visible().ok()).unwrap_or(false);
    if visible {
        hide(app);
    } else {
        show(app);
    }
}

struct BusyGuard(AppHandle);
impl Drop for BusyGuard {
    fn drop(&mut self) { self.0.state::<QuickState>().busy.store(false, Ordering::Release); }
}

struct NativePaste { app: AppHandle, item: String, target: Option<Target> }
impl paste::Port for NativePaste {
    fn permitted(&mut self) -> bool { paste::permitted() }
    fn copy(&mut self) -> Result<(), paste::Error> {
        let core = self.app.state::<onepaste_app::App>().inner().clone();
        tauri::async_runtime::block_on(core.copy(&self.item)).map_err(|_| paste::Error::Copy)
    }
    fn hide(&mut self) -> Result<(), paste::Error> {
        self.app.get_webview_window(LABEL).ok_or(paste::Error::Hide)?.hide().map_err(|_| paste::Error::Hide)
    }
    fn restore(&mut self) -> Result<(), onepaste_platform::PlatformError> {
        paste::restore(self.target.ok_or(onepaste_platform::PlatformError::Os(-1))?)
    }
    fn paste(&mut self) -> Result<(), onepaste_platform::PlatformError> {
        paste::send(self.target.ok_or(onepaste_platform::PlatformError::Os(-1))?)
    }
}

#[tauri::command]
pub async fn quick_paste(app: AppHandle, item_id: String) -> Result<String, String> {
    let target = {
        let state = app.state::<QuickState>();
        if state.busy.swap(true, Ordering::Acquire) { return Err("quick_busy".into()); }
        let target = *state.target.lock().unwrap(); target
    };
    tauri::async_runtime::spawn_blocking(move || {
        let _busy = BusyGuard(app.clone());
        paste::perform(&mut NativePaste { app, item: item_id, target }).map(|outcome| match outcome {
            paste::Outcome::PasteRequested => "paste_requested",
            paste::Outcome::CopiedNoPermission => "copied_no_permission",
            paste::Outcome::CopiedManual => "copied_manual",
        }.to_string()).map_err(|error| match error { paste::Error::Copy => "quick_copy_failed", paste::Error::Hide => "quick_hide_failed" }.to_string())
    }).await.map_err(|_| "quick_paste_failed".to_string())?
}

#[tauri::command]
pub fn paste_permission() -> bool { paste::permitted() }

#[tauri::command]
pub async fn open_accessibility_settings() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(paste::open_accessibility_settings).await.map_err(|_| "paste_settings_failed".to_string())?.map_err(|_| "paste_settings_failed".to_string())
}
